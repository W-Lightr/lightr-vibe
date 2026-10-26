// git worktree 隔离层（task 10-26-worktree-backend / design §2）：
// 仓库信息探测、工作树创建、未提交改动的自动提交、工作树删除。仅用
// std::process::Command 调 git，不引入新 crate、不加 tauri capability。
// **全仓唯一调用 git 的地方**（PAC2）。
//
// 为什么删除前要自动提交（task 10-26-worktree-autocommit）：
// `git worktree remove --force` 删除的是**工作目录**，目录里的未提交改动不在 git 对象库，
// 删掉即永久丢失——保留分支也救不回来，因为分支上根本没有那些内容。所以终态清理必须
// **先 commit 再 remove**（顺序不能反：反了目录已消失，`git add` 无从谈起）。
//
// 规格见 .trellis/spec/frontend/tauri-bridge.md，三条不可妥协的安全约束：
// 1. `base_sha` 必须在 `git worktree add` **之前**解析（创建期间有人切分支会让基准漂移）；
// 2. 交给 git 的路径必须是**绝对路径**（git 先按后缀匹配 worktree，相对路径能命中磁盘
//    别处的同名 worktree 并被 `--force` 删掉）；盘符相对路径（`C:foo`）与非 UTF-8 路径
//    直接拒绝，且**不执行任何 git 命令**；
// 3. 创建后必须用 `symbolic-ref --quiet HEAD` 校验 HEAD 真的挂在 work_branch 上——
//    `worktree add <path> <name>` 在 `<name>` 不是本地分支（如同名 tag）时会静默
//    detached checkout，任务提交会挂到没人指向的提交上。
//
// 错误串：`worktree-not-repo` / `path-not-absolute` / `path-not-utf8` /
// `worktree-path-exists` / `worktree-add: <stderr>` / `worktree-detached` /
// `worktree-missing` / `git status: <stderr>` / `git add: <stderr>` /
// `git commit: <stderr>` / `worktree-remove: <stderr>` / `git: <stderr>`（其余 git 失败）。

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Windows CREATE_NO_WINDOW：git 子进程不弹控制台窗口（与 agent.rs 既有做法一致）
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 只读 git 命令（rev-parse / for-each-ref 等）超时：10s
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// 写 git 命令（worktree add / remove / branch -D）超时：120s（大仓库 checkout 可能很慢）
const GIT_WRITE_TIMEOUT: Duration = Duration::from_secs(120);
/// init 命令超时：10 分钟（`pnpm install` 这类初始化允许较久）
const INIT_TIMEOUT: Duration = Duration::from_secs(600);
/// stderr 拼进错误串时保留的尾部字节数（避免把整段编译日志塞进 UI）
const STDERR_TAIL: usize = 800;

// ---------- 返回结构（camelCase 与前端 worktreeService.ts 逐字对齐） ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    /// false → 前端禁用 worktree 开关
    pub is_repo: bool,
    /// 当前分支名；detached HEAD 时为 None
    pub current_branch: Option<String>,
    /// 本地分支（不含远端）
    pub branches: Vec<String>,
    /// 非仓库 / 无提交 / git 缺失时的可读原因
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorktreeRequest {
    pub root_path: String,
    /// 空 = 用当前分支
    #[serde(default)]
    pub base_branch: Option<String>,
    pub task_id: String,
    /// 可选初始化命令；空则不执行（策略 A：默认不预装、不构建）
    #[serde(default)]
    pub init_command: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedWorktree {
    pub worktree_path: String,
    pub work_branch: String,
    /// 创建**之前**解析的基线提交
    pub base_sha: String,
    /// 实际使用的基线分支（空请求解析为当前分支后的结果）
    pub base_branch: String,
    /// init 命令失败时的输出尾部；成功为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub init_error: Option<String>,
}

// ---------- 路径安全 ----------

/// 盘符相对路径（`C:foo`：有盘符但**没有**根）。它的含义依赖每个盘符各自的当前目录，
/// 父进程与 git 子进程不共享，两边解析结果可能不同——而其中一边是删除。
fn is_drive_relative(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return false;
    }
    // `C:` 结尾或 `C:foo` 是盘符相对；`C:\` / `C:/` 是绝对路径
    match bytes.get(2) {
        None => true,
        Some(b'\\') | Some(b'/') => false,
        Some(_) => true,
    }
}

/// 校验并返回**绝对**路径：拒绝盘符相对路径与非 UTF-8 路径，不做有损转换。
/// 失败时调用方必须在执行任何 git 命令**之前**返回。
fn require_absolute(raw: &str) -> Result<PathBuf, String> {
    if raw.trim().is_empty() {
        return Err("path-not-absolute".into());
    }
    // `C:foo` 在 Windows 上 Path::is_absolute() 也为 false，但两条错误串要区分开
    if is_drive_relative(raw) {
        return Err("path-not-absolute".into());
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("path-not-absolute".into());
    }
    // 非 UTF-8 路径无法无损交给 git 参数，直接拒绝（不用 to_string_lossy）
    if path.to_str().is_none() {
        return Err("path-not-utf8".into());
    }
    Ok(normalize_path(path))
}

/// 词法规范化：折叠 `.` 与可回退的 `..`（mkdir / 比较用，不触文件系统）
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 绝对路径 → 交给 git 前的字符串（已确保是 UTF-8 绝对路径）
fn abs_str(path: &Path) -> Result<String, String> {
    path.to_str().map(str::to_string).ok_or_else(|| "path-not-utf8".to_string())
}

/// worktree 路径：`<root 的父目录>/<root 目录名>-task-<taskId>`
fn worktree_path_for(root: &Path, task_id: &str) -> Result<PathBuf, String> {
    let parent = root.parent().ok_or_else(|| "path-not-absolute".to_string())?;
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "path-not-absolute".to_string())?;
    Ok(parent.join(format!("{}-task-{}", name, task_id)))
}

// ---------- git 调用 ----------

/// 命令输出（超时 / 启动失败统一为 Err）
struct CmdOutput {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl CmdOutput {
    /// stderr 尾部（多行错误在 UI 上只保留最后几百字节）
    fn tail(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            self.stdout.trim().to_string()
        } else {
            self.stderr.trim().to_string()
        };
        let chars: Vec<char> = text.chars().collect();
        if chars.len() <= STDERR_TAIL {
            return text;
        }
        format!("…{}", chars[chars.len() - STDERR_TAIL..].iter().collect::<String>())
    }
}

/// 带超时执行命令：独立线程读管道避免输出撑满管道死锁；超时 kill 子进程。
fn run_command(mut cmd: Command, timeout: Duration) -> Result<CmdOutput, String> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("git: 启动失败（是否已安装 git？）: {}", e))?;
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || read_all(stdout_handle));
    let stderr_reader = std::thread::spawn(move || read_all(stderr_handle));

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(CmdOutput {
                    ok: status.success(),
                    stdout: stdout_reader.join().unwrap_or_default(),
                    stderr: stderr_reader.join().unwrap_or_default(),
                })
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("git: 执行超时（{}s）", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("git: 等待子进程失败: {}", e));
            }
        }
    }
}

fn read_all(mut handle: Option<impl Read>) -> String {
    let mut buf = Vec::new();
    if let Some(h) = handle.as_mut() {
        let _ = h.read_to_end(&mut buf);
    }
    // git 输出在 Windows 上是 UTF-8/本地码页混合，有损转换只为展示
    String::from_utf8_lossy(&buf).into_owned()
}

/// 只读 git 命令：失败返回 `Err("git: <stderr 尾部>")`
fn run_git(cwd: &Path, args: &[&str]) -> Result<CmdOutput, String> {
    run_git_with_timeout(cwd, args, GIT_TIMEOUT)
}

/// 写 git 命令（worktree add/remove 等）：同样返回 Ok(out)，由调用方按子命令拼错误串
fn run_git_write(cwd: &Path, args: &[&str]) -> Result<CmdOutput, String> {
    run_git_with_timeout(cwd, args, GIT_WRITE_TIMEOUT)
}

fn run_git_with_timeout(cwd: &Path, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
    let mut cmd = Command::new("git");
    // why: -C 而不是 current_dir —— 路径含空格/非 UTF-8 时行为一致，且 git 统一在
    // 该目录下解析；cwd 已由调用方校验为绝对路径。
    cmd.arg("-C").arg(cwd).args(args);
    run_command(cmd, timeout)
}

/// 只读命令，要求成功：返回 stdout（已 trim）
fn git_stdout(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = run_git(cwd, args)?;
    if !out.ok {
        return Err(format!("git {}: {}", args.first().copied().unwrap_or(""), out.tail()));
    }
    Ok(out.stdout.trim().to_string())
}

// ---------- 仓库信息 ----------

/// 读仓库信息（不抛错：失败编码进 isRepo/detail）
fn repo_info_inner(root: &str) -> RepoInfo {
    let empty = |detail: String| RepoInfo {
        is_repo: false,
        current_branch: None,
        branches: Vec::new(),
        detail: Some(detail),
    };
    let root_path = match require_absolute(root) {
        Ok(p) => p,
        Err(e) => return empty(e),
    };
    if !root_path.is_dir() {
        return empty(format!("目录不存在: {}", root_path.display()));
    }
    // 1) 是否 git 仓库（非仓库 / git 缺失 / 权限问题都落到这里）
    match run_git(&root_path, &["rev-parse", "--git-dir"]) {
        Ok(out) if out.ok => {}
        Ok(out) => return empty(format!("不是 git 仓库: {}", out.tail())),
        Err(e) => return empty(e),
    }

    // 2) 当前分支：detached HEAD 时 `--abbrev-ref HEAD` 返回 "HEAD"
    let current_branch = match git_stdout(&root_path, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(name) if name != "HEAD" && !name.is_empty() => Some(name),
        // 空仓库（还没有提交）也走这里：rev-parse 报错，视作无当前分支
        _ => None,
    };

    // 3) 本地分支列表（refs/heads）
    let branches: Vec<String> = match git_stdout(
        &root_path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    ) {
        Ok(text) => text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    };

    // 没有提交的仓库无法创建 worktree：给出可读原因（仍 isRepo=true，UI 亦可禁用）
    let detail = if branches.is_empty() && current_branch.is_none() {
        Some("仓库还没有任何提交，无法创建 worktree".to_string())
    } else {
        None
    };

    RepoInfo {
        is_repo: true,
        current_branch,
        branches,
        detail,
    }
}

// ---------- 创建 ----------

/// 创建流程（design §2.2 严格 10 步；拆分出来便于单测路径逻辑）
fn create_inner(req: &CreateWorktreeRequest) -> Result<CreatedWorktree, String> {
    // 0) 前后都校验一次：盘符相对 / 非 UTF-8 在这里就返回，不执行任何 git 命令
    let root = require_absolute(&req.root_path)?;

    let info = repo_info_inner(&req.root_path);
    if !info.is_repo {
        return Err(format!(
            "worktree-not-repo: {}",
            info.detail.unwrap_or_else(|| "不是 git 仓库".to_string())
        ));
    }

    // 1) base_branch：请求为空 → 当前分支（detached 时无法推断，要求显式选择）
    let requested = req
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let base_branch = match (requested, info.current_branch) {
        (Some(b), _) => b.to_string(),
        (None, Some(b)) => b,
        (None, None) => {
            return Err("worktree-base-branch: 当前处于游离 HEAD，请显式选择基线分支".into())
        }
    };
    // 请求的分支必须是本地分支（远端 / 幽灵分支会在第 7 步产出 detached）
    if !info.branches.iter().any(|b| b == &base_branch) {
        return Err(format!("worktree-base-branch: 本地不存在分支 {}", base_branch));
    }

    // 2) base_sha —— 必须在 worktree add **之前**解析
    let base_sha = git_stdout(&root, &["rev-parse", &base_branch])
        .map_err(|e| format!("worktree-base-sha: {}", e))?;

    // 3) 任务分支
    let work_branch = format!("task/{}", req.task_id);

    // 4) 绝对路径
    let worktree_path = worktree_path_for(&root, &req.task_id)?;
    let worktree_str = abs_str(&worktree_path)?;

    // 5) 目标已存在 → 明确报错，不覆盖
    if worktree_path.exists() {
        return Err(format!("worktree-path-exists: {}", worktree_str));
    }

    // 6) 清理失效注册（上一轮被手工删掉的目录）
    let pruned = run_git_write(&root, &["worktree", "prune"])?;
    if !pruned.ok {
        return Err(format!("worktree-prune: {}", pruned.tail()));
    }

    // 7) 添加：任务分支**从 base_branch 新建**（`-b`）。
    //    why `-b` 而不是 `add <path> <name>`：后者要求 `<name>` 已存在，否则
    //    `fatal: invalid reference`；若恰好同名 tag 则以 tag 为起点静默 detached
    //    （实测：`worktree add <path> task/<id>` + 同名 tag → `Preparing worktree
    //    (detached HEAD <sha>)`，symbolic-ref 为空）。用 `-b` 显式指定起点，再由第 8
    //    步校验兜底。（design §2.2 第 7 步的命令形式在此修正。）
    //    分支已残留（上一轮失败留下）时 `-b` 同样失败 —— 首版不自动重试，直接报错
    //    让用户手动清理。
    let created_branch = !work_branch_exists(&root, &work_branch);
    let added = run_git_write(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            work_branch.as_str(),
            worktree_str.as_str(),
            base_branch.as_str(),
        ],
    )?;
    if !added.ok {
        // `-b` 会**先建分支再建目录**：目录创建失败（权限 / 被占用 / 路径被 git 自己
        // 规范化后判定已存在）时分支已经落地，不清理就留一条谁都不需要的分支，
        // 且下次重试必然撞 `a branch named '<work_branch>' already exists`。
        // 只清理**本次刚建出来**的分支（调用前已存在的是上一轮残留，留给用户处理）。
        if created_branch {
            rollback_work_branch(&root, &work_branch, &base_sha, &worktree_str);
        }
        return Err(format!("worktree-add: {}", added.tail()));
    }

    // 8) symbolic-ref 校验：必须是 refs/heads/<work_branch>（完整 ref 比对，不用 --short：
    //    同名 tag 存在时 --short 会消歧成 heads/<name>）。`-b` 已保证正常情况挂对，
    //    这里是最后一道兜底——校验失败说明 HEAD 漂移，绝不能让任务在 detached 上跑。
    let expected = format!("refs/heads/{}", work_branch);
    let head = git_stdout(&worktree_path, &["symbolic-ref", "--quiet", "HEAD"]);
    let head_ok = matches!(&head, Ok(ref v) if v == &expected);
    if !head_ok {
        // 回滚：先删工作树（否则删分支会被 checkout 保护拒绝），再删刚建的分支，
        // 避免留下「无人指向的提交」与残留分支两处垃圾。
        let _ = run_git_write(&root, &["worktree", "remove", "--force", worktree_str.as_str()]);
        let _ = run_git_write(&root, &["worktree", "prune"]);
        rollback_work_branch(&root, &work_branch, &base_sha, &worktree_str);
        return Err("worktree-detached".into());
    }

    // 9) init 命令（可选）：失败只记 init_error，**不回滚**（目录与分支都已在正确位置）
    let init_error = match req
        .init_command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(cmd_line) => run_init_command(&worktree_path, cmd_line).err(),
        None => None,
    };

    // 10)
    Ok(CreatedWorktree {
        worktree_path: worktree_str,
        work_branch,
        base_sha,
        base_branch,
        init_error,
    })
}

/// 在工作树里执行一次初始化命令（Windows 走 `cmd /C`，其他平台走 `sh -c`）。
/// 用 shell 是因为用户填的是命令行（含 `&&`、变量等）。失败返回输出尾部。
/// 注意：Windows 上 `cmd /C` 对首尾引号的剥离规则很怪，`cmd /S /C` 才能把整串当一条
/// 命令原样交给它（否则 `"pnpm install" && ...` 这类写法会被截断）。
fn run_init_command(cwd: &Path, command_line: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.arg("/S").arg("/C").arg(command_line);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command_line);
        c
    };
    cmd.current_dir(cwd);
    let out = run_command(cmd, INIT_TIMEOUT)?;
    if out.ok {
        Ok(())
    } else {
        Err(out.tail())
    }
}

// ---------- 自动提交 ----------

/// `worktree_commit_all` 的入参。与 `worktree_remove` 一样用结构体：Tauri 对裸函数参数
/// 只支持内置属性，serde 相关的东西必须落在 `Deserialize` 结构体上。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitWorktreeRequest {
    pub worktree_path: String,
    /// 提交信息（调用方给，建议带任务标题）。trim 后为空则用默认文案。
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitWorktreeResult {
    /// 是否真的产生了提交（工作区本来就干净 → false，不产生空提交）
    pub committed: bool,
    /// 新提交的 sha；`committed: false` 时省略
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    /// 提交落在哪条本地分支上（`git symbolic-ref --short HEAD`）。前端据此确认
    /// **只落在 `task/*` 上**，绝不碰项目主分支（prd 约束）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// message 的兜底文案：调用方漏传时不至于让 `git commit -m ""` 失败
const DEFAULT_COMMIT_MESSAGE: &str = "chore(task): 自动提交未保存改动";

/// 把工作区里的未提交改动提交到当前分支（detail 见 design §2.2 的六步）。
///
/// **必须在 `worktree remove` 之前调用**：目录删掉后工作区改动就不在磁盘上了，
/// `git add` 无从谈起。分支是这次任务的产品，提交落上去才能被用户 `git log` 找回。
///
/// 不做的事（设计约束）：不加 `--no-verify`（用户配的 hook 该生效）、不 `git config`
/// 任何东西（缺身份就是失败，交给用户）、不 `--amend`、不 push、绝不碰主分支
/// （`git -C <worktree>` 的 HEAD 就是 `task/<id>`）。
fn commit_all_inner(req: &CommitWorktreeRequest) -> Result<CommitWorktreeResult, String> {
    // 1) 路径安全：与 remove 同一套校验，非法路径在**任何 git 命令之前**返回
    let abs = require_absolute(&req.worktree_path)?;
    let abs = abs_str(&abs)?;

    // 2) 目录必须存在（目录已删就没什么可提交的）
    if !Path::new(&abs).is_dir() {
        return Err("worktree-missing".into());
    }
    let root = Path::new(&abs);

    // 3) 必须挂在本地分支上（detached HEAD 时提交会变成一个**没有任何 ref 指向**的
    //    游离提交：目录一删就永久丢失，正是本任务要消灭的那种数据丢失）。
    //    `symbolic-ref --quiet HEAD` 在 detached 时返回非 0——与 `create_inner` 的
    //    `worktree-detached` 校验同一套判定，但这里在 `add -A` 之前就拒绝，不改工作区。
    let branch = match git_stdout(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(b) if !b.is_empty() => b,
        _ => return Err("worktree-detached".into()),
    };

    // 4) 探测是否干净：`--porcelain` 为空 = 没有改动 → 直接返回，绝不产生空提交
    let status = run_git(root, &["status", "--porcelain"])?;
    if !status.ok {
        return Err(format!("git status: {}", status.tail()));
    }
    if status.stdout.trim().is_empty() {
        return Ok(CommitWorktreeResult {
            committed: false,
            sha: None,
            branch: Some(branch),
        });
    }

    // 5) 全部加入（遵守 .gitignore：node_modules 这类不会进提交）
    let added = run_git_write(root, &["add", "-A"])?;
    if !added.ok {
        return Err(format!("git add: {}", added.tail()));
    }

    // 6) 提交（典型失败：缺 user.name / email）
    let message = {
        let trimmed = req.message.trim();
        if trimmed.is_empty() {
            DEFAULT_COMMIT_MESSAGE
        } else {
            trimmed
        }
    };
    let committed = run_git_write(root, &["commit", "-m", message])?;
    if !committed.ok {
        return Err(format!("git commit: {}", committed.tail()));
    }

    // 7) 新提交的 sha（只读命令）
    let sha = git_stdout(root, &["rev-parse", "HEAD"])
        .map_err(|e| format!("git rev-parse: {}", e))?;
    Ok(CommitWorktreeResult {
        committed: true,
        sha: Some(sha),
        branch: Some(branch),
    })
}

// ---------- 删除 ----------

/// 分支是否存在（完整 ref 判定，不受同名 tag 干扰）
fn work_branch_exists(root: &Path, work_branch: &str) -> bool {
    git_stdout(
        root,
        &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{}", work_branch)],
    )
    .map(|s| !s.is_empty())
    .unwrap_or(false)
}

/// 创建工作树失败后的分支回滚：删目录（若已建出）→ prune → 分支无未落地工作时才删。
/// why 保守：删除不可逆，`branch_holds_unlanded_work` 探针失败一律读作「有工作」，
/// 宁可留一条分支让用户手工处理，也不删掉可能含提交的分支。
fn rollback_work_branch(root: &Path, work_branch: &str, base_sha: &str, worktree_str: &str) {
    let wt = Path::new(worktree_str);
    if wt.exists() {
        let _ = run_git_write(root, &["worktree", "remove", "--force", worktree_str]);
    }
    let _ = run_git_write(root, &["worktree", "prune"]);
    if work_branch_exists(root, work_branch)
        && !branch_holds_unlanded_work(root, work_branch, base_sha)
    {
        let _ = run_git_write(root, &["branch", "-D", work_branch]);
    }
}

/// 分支是否还有未落地的提交：tip **不是** base 的祖先，且 tree 与 base 也不同。
/// 两种探针任一失败一律读作「有工作」（删除不可逆，失败关闭）。
fn branch_holds_unlanded_work(root: &Path, work_branch: &str, base: &str) -> bool {
    // 分支已不存在 → 没有可删的，交回调用方按「不保留」处理
    if !work_branch_exists(root, work_branch) {
        return false;
    }
    // tip 是 base 的祖先 → 没有新提交
    let ancestor = run_git(root, &["merge-base", "--is-ancestor", work_branch, base]);
    if let Ok(out) = &ancestor {
        if out.ok {
            return false;
        }
        // 退出码 1 = 不是祖先；其它非零（如 base 不存在）继续走 tree 比较
    }
    // tree 相同（改了又改回 / 只改了工作区）→ 没有需要保留的内容
    let tip_tree = git_stdout(root, &["rev-parse", &format!("{}^{{tree}}", work_branch)]);
    let base_tree = git_stdout(root, &["rev-parse", &format!("{}^{{tree}}", base)]);
    matches!((tip_tree, base_tree), (Ok(a), Ok(b)) if a == b)
        .then_some(false)
        // 探针不可用或有差异 → 保守：保留分支
        .unwrap_or(true)
}

/// 删除工作树。`keep_branch` 为 true 时**只**删目录与注册（任务终态：分支是这次工作的
/// 产品，要留给用户 `git log` / `git merge`）；false 时在「分支没有未落地提交」的前提下
/// 尽力删掉分支（启动失败回滚：那次尝试没有价值）。
fn remove_inner(
    root_path: &str,
    worktree_path: &str,
    work_branch: &str,
    base_branch: Option<String>,
    base_sha: Option<String>,
    keep_branch: bool,
) -> Result<(), String> {
    // 1) 路径安全：必须先转绝对路径再交给 git，否则相对路径能命中磁盘别处的 worktree
    let root = require_absolute(root_path)?;
    let abs = require_absolute(worktree_path)?;
    let abs = abs_str(&abs)?;

    // 2) 删除工作树；目录已不存在视为成功（注册项由 prune 清掉）
    let removed = run_git_write(&root, &["worktree", "remove", "--force", abs.as_str()])?;
    if !removed.ok && Path::new(&abs).exists() {
        return Err(format!("worktree-remove: {}", removed.tail()));
    }

    // 3) 清理失效注册
    let pruned = run_git_write(&root, &["worktree", "prune"])?;
    if !pruned.ok {
        // prune 失败不致命（目录已删），但不静默吞掉：报出来让用户看到
        return Err(format!("worktree-prune: {}", pruned.tail()));
    }

    // 4) 只删目录（任务终态）：**必须短路在分支守卫之前**，否则守卫一放行分支照删
    if keep_branch {
        return Ok(());
    }

    // 5) 分支守卫：base 优先用创建时记录的 sha（分支可能已被移动），退化到分支名
    let base = base_sha
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            base_branch
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        });
    let Some(base) = base else {
        // 没有基线可比 → 保守保留分支
        return Ok(());
    };
    if branch_holds_unlanded_work(&root, work_branch, &base) {
        return Ok(());
    }

    // 6) 删除分支；不存在（已被手工删掉）视为成功
    let deleted = run_git_write(&root, &["branch", "-D", work_branch])?;
    if !deleted.ok {
        let tail = deleted.tail();
        if tail.contains("not found") || tail.contains("not a valid") {
            return Ok(());
        }
        return Err(format!("worktree-branch-delete: {}", tail));
    }
    Ok(())
}

// ---------- Tauri commands ----------

/// 仓库信息：是否 git 仓库 + 当前分支 + 本地分支列表。**永不抛错**。
#[tauri::command]
pub fn worktree_repo_info(root_path: String) -> Result<RepoInfo, String> {
    Ok(repo_info_inner(&root_path))
}

/// 创建工作树（严格按 design §2.2）；`initCommand` 失败只记 `initError`。
#[tauri::command]
pub fn worktree_create(req: CreateWorktreeRequest) -> Result<CreatedWorktree, String> {
    create_inner(&req)
}

/// `worktree_remove` 的入参。多字段命令一律用结构体：Tauri 对**函数参数**只支持
/// 内置属性，`#[serde(default)]` 写在参数上是编译错误；放进 `Deserialize` 结构体才生效
/// （这也是 `keep_branch` 能省略、从而满足 AC4「旧调用行为不变」的关键）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveWorktreeRequest {
    pub root_path: String,
    pub worktree_path: String,
    pub work_branch: String,
    pub base_branch: Option<String>,
    pub base_sha: Option<String>,
    /// 只删工作树、保留分支（任务终态清理用）。省略 = false（启动失败回滚用）
    #[serde(default)]
    pub keep_branch: bool,
}

/// 把工作树里的未提交改动提交到任务分支。**终态清理必须在 `worktree_remove` 之前调用**，
/// 否则目录一删改动就永久丢失（即使分支保留也救不回来）。
/// 干净工作区不产生空提交（`committed: false`）。
#[tauri::command]
pub fn worktree_commit_all(req: CommitWorktreeRequest) -> Result<CommitWorktreeResult, String> {
    commit_all_inner(&req)
}

/// 删除工作树：默认（`keepBranch` 省略 / false）在分支无未落地提交时顺带删掉任务分支。
///
/// 细节抽屉「删除分支」（R3 / AC5）会在目录**已删**后再次调用本命令来删分支，此时
/// `worktreePath` 会退化成空串 → `require_absolute("")` 报 `path-not-absolute`，
/// 分支永远删不掉。所以：空路径 = 「没有目录要删」，跳过第 2 步直接走 prune + 分支
/// （`remove_inner_allow_absent_dir`）；非空路径行为与改动前完全一致。
#[tauri::command]
pub fn worktree_remove(req: RemoveWorktreeRequest) -> Result<(), String> {
    remove_inner_allow_absent_dir(
        &req.root_path,
        &req.worktree_path,
        &req.work_branch,
        req.base_branch,
        req.base_sha,
        req.keep_branch,
    )
}

/// 目录已清理、只剩分支时的删分支入口：`worktree_remove` 的 `worktreePath` 允许为空
/// （空 = 没有目录要删），非空时行为与改动前完全一致。
fn remove_inner_allow_absent_dir(
    root_path: &str,
    worktree_path: &str,
    work_branch: &str,
    base_branch: Option<String>,
    base_sha: Option<String>,
    keep_branch: bool,
) -> Result<(), String> {
    if !worktree_path.trim().is_empty() {
        return remove_inner(
            root_path,
            worktree_path,
            work_branch,
            base_branch,
            base_sha,
            keep_branch,
        );
    }
    let root = require_absolute(root_path)?;
    let pruned = run_git_write(&root, &["worktree", "prune"])?;
    if !pruned.ok {
        return Err(format!("worktree-prune: {}", pruned.tail()));
    }
    if keep_branch {
        return Ok(());
    }
    let base = base_sha
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            base_branch
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        });
    let Some(base) = base else {
        return Ok(());
    };
    if branch_holds_unlanded_work(&root, work_branch, &base) {
        return Ok(());
    }
    let deleted = run_git_write(&root, &["branch", "-D", work_branch])?;
    if deleted.ok {
        return Ok(());
    }
    let tail = deleted.tail();
    if tail.contains("not found") || tail.contains("not a valid") {
        return Ok(());
    }
    Err(format!("worktree-branch-delete: {}", tail))
}

// ---------- 单元测试 ----------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("lightr-worktree-test-{}-{}", tag, nanos));
        fs::create_dir_all(&dir).expect("创建测试目录失败");
        dir
    }

    #[test]
    fn drive_relative_paths_are_detected() {
        assert!(is_drive_relative("C:foo"));
        assert!(is_drive_relative("c:foo\\bar"));
        assert!(is_drive_relative("C:"));
        assert!(!is_drive_relative("C:\\foo"));
        assert!(!is_drive_relative("C:/foo"));
        assert!(!is_drive_relative("/foo/bar"));
        assert!(!is_drive_relative("relative/path"));
        assert!(!is_drive_relative(""));
    }

    #[test]
    fn require_absolute_rejects_relative_and_drive_relative() {
        // 盘符相对：明确拒绝（不执行任何 git 命令由调用顺序保证：这里先于所有 git 调用）
        assert_eq!(require_absolute("C:foo"), Err("path-not-absolute".to_string()));
        assert_eq!(require_absolute("C:"), Err("path-not-absolute".to_string()));
        // 普通相对路径
        assert_eq!(require_absolute("foo/bar"), Err("path-not-absolute".to_string()));
        assert_eq!(require_absolute("  "), Err("path-not-absolute".to_string()));
        // 绝对路径通过，且折叠 `.`
        let abs = require_absolute("C:/a/./b").unwrap();
        assert!(abs.is_absolute());
    }

    #[test]
    fn worktree_path_is_sibling_of_repo() {
        let p = worktree_path_for(Path::new("C:/work/my-repo"), "task-abc").unwrap();
        assert_eq!(p, PathBuf::from("C:/work/my-repo-task-task-abc"));
    }

    #[test]
    fn repo_info_on_non_repo_does_not_throw() {
        let dir = tmp_dir("notrepo");
        let info = worktree_repo_info(dir.to_string_lossy().into_owned()).unwrap();
        // 临时目录可能是仓库内的父路径的子目录：只有确实不是仓库时断言
        if !info.is_repo {
            assert!(info.detail.is_some(), "非仓库必须给出原因");
            assert!(info.branches.is_empty());
        }
        // 不存在的目录同样不抛错
        let missing = worktree_repo_info(
            dir.join("missing-dir").to_string_lossy().into_owned(),
        )
        .unwrap();
        assert!(!missing.is_repo);
        assert!(missing.detail.is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_rejects_drive_relative_root_without_touching_git() {
        let err = create_inner(&CreateWorktreeRequest {
            root_path: "C:foo".into(),
            base_branch: None,
            task_id: "task-1".into(),
            init_command: None,
        })
        .unwrap_err();
        assert_eq!(err, "path-not-absolute");
    }

    #[test]
    fn remove_rejects_relative_worktree_path() {
        let err = remove_inner("C:/work/repo", "relative/wt", "task/task-1", None, None, false)
            .unwrap_err();
        assert_eq!(err, "path-not-absolute");
        // 盘符相对同样拒绝
        let err =
            remove_inner("C:/work/repo", "C:wt", "task/task-1", None, None, false).unwrap_err();
        assert_eq!(err, "path-not-absolute");
        // root 本身非法时也在执行 git 前返回
        let err = remove_inner("repo", "C:/work/wt", "task/task-1", None, None, false).unwrap_err();
        assert_eq!(err, "path-not-absolute");
    }

    /// 真 git 集成：初始化临时仓库 → 创建 → 校验 HEAD → 删除 → 分支清理。
    /// git 不在 PATH 时跳过（CI 之外不应因环境失败而红）。
    #[test]
    fn create_and_remove_roundtrip_on_real_repo() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let base = tmp_dir("repo-parent");
        let root = base.join("repo");
        fs::create_dir_all(&root).unwrap();
        let root_str = root.to_string_lossy().into_owned();

        let ok = |args: &[&str]| {
            let out = run_git(&root, args).expect("git 调用失败");
            assert!(out.ok, "git {:?} 失败: {}", args, out.tail());
        };
        ok(&["init", "-q"]);
        ok(&["config", "user.email", "test@example.com"]);
        ok(&["config", "user.name", "test"]);
        ok(&["config", "commit.gpgsign", "false"]);
        fs::write(root.join("README.md"), "hello").unwrap();
        ok(&["add", "."]);
        ok(&["commit", "-q", "-m", "init"]);

        let before_sha = git_stdout(&root, &["rev-parse", "HEAD"]).unwrap();
        let info = worktree_repo_info(root_str.clone()).unwrap();
        assert!(info.is_repo, "临时仓库应被识别: {:?}", info);
        let current = info.current_branch.clone().expect("应有当前分支");
        assert!(info.branches.contains(&current));

        let created = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "task-1".into(),
            init_command: None,
        })
        .expect("创建应成功");

        // AC3：baseSha 等于创建**之前**的 rev-parse
        assert_eq!(created.base_sha, before_sha);
        assert_eq!(created.base_branch, current);
        assert_eq!(created.work_branch, "task/task-1");
        assert!(created.init_error.is_none());
        assert!(Path::new(&created.worktree_path).is_dir());

        // AC4：HEAD 挂在 refs/heads/task/task-1
        let head = git_stdout(Path::new(&created.worktree_path), &["symbolic-ref", "HEAD"]).unwrap();
        assert_eq!(head, "refs/heads/task/task-1");
        let list = git_stdout(&root, &["worktree", "list"]).unwrap();
        assert!(list.contains("repo-task-task-1"), "worktree list: {}", list);

        // AC5：路径已存在 → 明确报错
        let err = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "task-1".into(),
            init_command: None,
        })
        .unwrap_err();
        assert!(err.starts_with("worktree-path-exists"), "实际 {}", err);

        // AC7 + AC8：无新提交 → 目录消失 + 分支被删
        remove_inner(
            &root_str,
            &created.worktree_path,
            &created.work_branch,
            Some(created.base_branch.clone()),
            Some(created.base_sha.clone()),
            false,
        )
        .unwrap();
        assert!(!Path::new(&created.worktree_path).exists());
        assert!(!git_stdout(&root, &["branch", "--list", "task/task-1"]).unwrap().contains("task/task-1"));

        // AC8：重新创建 + 造一个未落地提交 → 删工作树成功但分支保留
        let again = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "task-2".into(),
            init_command: None,
        })
        .unwrap();
        let wt = Path::new(&again.worktree_path);
        let out = run_git(wt, &["config", "user.email", "test@example.com"]).unwrap();
        assert!(out.ok);
        let out = run_git(wt, &["config", "user.name", "test"]).unwrap();
        assert!(out.ok);
        fs::write(wt.join("new.txt"), "x").unwrap();
        let out = run_git(wt, &["add", "."]).unwrap();
        assert!(out.ok);
        let out = run_git(wt, &["commit", "-q", "-m", "work"]).unwrap();
        assert!(out.ok, "提交失败: {}", out.tail());

        remove_inner(
            &root_str,
            &again.worktree_path,
            &again.work_branch,
            Some(again.base_branch.clone()),
            Some(again.base_sha.clone()),
            false,
        )
        .unwrap();
        assert!(!wt.exists());
        let kept = git_stdout(&root, &["branch", "--list", "task/task-2"]).unwrap();
        assert!(kept.contains("task/task-2"), "有未落地提交时分支必须保留");

        // AC1：非 git 目录不抛错
        let plain = tmp_dir("plain");
        let info = worktree_repo_info(plain.to_string_lossy().into_owned()).unwrap();
        assert!(!info.is_repo);
        assert!(info.detail.is_some());

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&plain);
    }

    /// init 命令：失败只记 initError，**不回滚**（目录与分支仍在原位）；
    /// 显式基线分支会被采用；不存在的基线分支在创建前就拒绝。
    #[test]
    fn init_command_failure_does_not_rollback_and_base_branch_is_honoured() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let base = tmp_dir("init");
        let root = base.join("repo");
        fs::create_dir_all(&root).unwrap();
        let ok = |args: &[&str]| {
            let out = run_git(&root, args).expect("git 调用失败");
            assert!(out.ok, "git {:?} 失败: {}", args, out.tail());
        };
        ok(&["init", "-q"]);
        ok(&["config", "user.email", "test@example.com"]);
        ok(&["config", "user.name", "test"]);
        ok(&["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "a").unwrap();
        ok(&["add", "."]);
        ok(&["commit", "-q", "-m", "init"]);
        // 第二条分支，作为显式基线
        ok(&["checkout", "-q", "-b", "feature"]);
        let feature_sha = git_stdout(&root, &["rev-parse", "feature"]).unwrap();
        ok(&["checkout", "-q", "-"]);

        // 不存在的基线分支 → 创建前拒绝（不产生目录）
        let err = create_inner(&CreateWorktreeRequest {
            root_path: root.to_string_lossy().into_owned(),
            base_branch: Some("ghost-branch".into()),
            task_id: "task-ghost".into(),
            init_command: None,
        })
        .unwrap_err();
        assert!(err.starts_with("worktree-base-branch"), "实际 {}", err);

        // 显式基线 + 注定失败的 init 命令（Windows / Unix 各用一个必然失败的命令）
        #[cfg(windows)]
        let failing_init = "exit /b 3";
        #[cfg(not(windows))]
        let failing_init = "exit 3";
        let created = create_inner(&CreateWorktreeRequest {
            root_path: root.to_string_lossy().into_owned(),
            base_branch: Some("feature".into()),
            task_id: "task-init".into(),
            init_command: Some(failing_init.into()),
        })
        .expect("init 失败不应让创建失败");
        assert_eq!(created.base_branch, "feature");
        assert_eq!(created.base_sha, feature_sha, "baseSha 应来自显式基线分支");
        assert!(created.init_error.is_some(), "init 失败应记录到 initError");
        // 不回滚：目录与分支都还在
        assert!(Path::new(&created.worktree_path).is_dir());
        assert!(git_stdout(&root, &["branch", "--list", "task/task-init"])
            .unwrap()
            .contains("task/task-init"));

        let _ = fs::remove_dir_all(&base);
    }

    /// `-b` 的残留分支陷阱：`git worktree add -b` **先建分支再建目录**，目录阶段失败时
    /// 分支已经落地。必须把本次刚建出来的分支回滚掉，否则下次重试永远撞
    /// 「a branch named '<x>' already exists」。
    #[test]
    fn failed_worktree_add_rolls_back_branch_it_created() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let base = tmp_dir("addfail");
        let root = base.join("repo");
        fs::create_dir_all(&root).unwrap();
        let ok = |args: &[&str]| {
            let out = run_git(&root, args).expect("git 调用失败");
            assert!(out.ok, "git {:?} 失败: {}", args, out.tail());
        };
        ok(&["init", "-q"]);
        ok(&["config", "user.email", "test@example.com"]);
        ok(&["config", "user.name", "test"]);
        ok(&["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "a").unwrap();
        ok(&["add", "."]);
        ok(&["commit", "-q", "-m", "init"]);

        // 目标路径落在主工作树里一个已存在**文件**之下：`repo/a.txt/nested` 的父是文件，
        // 第 5 步的 exists 检查只查目标自身（不存在）→ git 在「建分支之后、建目录之时」
        // 失败。这正是 `-b` 的残留分支形状（`create_inner` 无法把目标指向工作树内部，
        // 故这里直接调 git 复现该形状，再验证 rollback 清得干净）。
        let bad_str = root.join("a.txt").join("nested").to_string_lossy().into_owned();
        let add = run_git_write(
            &root,
            &["worktree", "add", "-b", "task/probe", &bad_str, "master"],
        )
        .unwrap();
        assert!(!add.ok, "预期 git 在目录阶段失败: {}", add.tail());
        assert!(
            work_branch_exists(&root, "task/probe"),
            "前提不成立：git 未在建目录前建出分支"
        );
        let base_sha = git_stdout(&root, &["rev-parse", "master"]).unwrap();
        rollback_work_branch(&root, "task/probe", &base_sha, &bad_str);
        assert!(!work_branch_exists(&root, "task/probe"), "回滚必须删掉残留分支");
        // 用户文件与登记表都不得被回滚误伤
        assert!(root.join("a.txt").exists());
        assert_eq!(git_stdout(&root, &["worktree", "list"]).unwrap().lines().count(), 1);

        let _ = fs::remove_dir_all(&base);
    }

    /// AC2：detached HEAD 时 currentBranch 为 None
    #[test]
    fn detached_head_reports_no_current_branch() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let base = tmp_dir("detached");
        let root = base.join("repo");
        fs::create_dir_all(&root).unwrap();
        let ok = |args: &[&str]| {
            let out = run_git(&root, args).expect("git 调用失败");
            assert!(out.ok, "git {:?} 失败: {}", args, out.tail());
        };
        ok(&["init", "-q"]);
        ok(&["config", "user.email", "test@example.com"]);
        ok(&["config", "user.name", "test"]);
        ok(&["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "a").unwrap();
        ok(&["add", "."]);
        ok(&["commit", "-q", "-m", "c1"]);
        // detached HEAD 指向第一个提交（没有分支指向它）
        let sha = git_stdout(&root, &["rev-parse", "HEAD"]).unwrap();
        ok(&["checkout", "-q", sha.as_str()]);

        let info = worktree_repo_info(root.to_string_lossy().into_owned()).unwrap();
        assert!(info.is_repo);
        assert_eq!(info.current_branch, None, "detached HEAD 应为 None");
        let _ = fs::remove_dir_all(&base);
    }

    /// 建一个带一次提交的临时仓库，返回 (父目录, 仓库根, 仓库根的字符串形式)。
    /// 仓库 fixture 与其它真 git 测试保持一致（user/email + 关掉 gpgsign）。
    fn init_real_repo(tag: &str) -> (PathBuf, PathBuf, String) {
        let parent = tmp_dir(tag);
        let root = parent.join("repo");
        fs::create_dir_all(&root).unwrap();
        let ok = |args: &[&str]| {
            let out = run_git(&root, args).expect("git 调用失败");
            assert!(out.ok, "git {:?} 失败: {}", args, out.tail());
        };
        ok(&["init", "-q"]);
        ok(&["config", "user.email", "test@example.com"]);
        ok(&["config", "user.name", "test"]);
        ok(&["config", "commit.gpgsign", "false"]);
        fs::write(root.join("README.md"), "hello").unwrap();
        ok(&["add", "."]);
        ok(&["commit", "-q", "-m", "init"]);
        let root_str = root.to_string_lossy().into_owned();
        (parent, root, root_str)
    }

    /// 任务终态清理：`keep_branch = true` → 目录消失、**分支仍在**（这是这次工作的产品）。
    #[test]
    fn keep_branch_leaves_the_branch() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let (parent, root, root_str) = init_real_repo("keepbranch");
        let created = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "keep-1".into(),
            init_command: None,
        })
        .expect("创建应成功");
        let wt = PathBuf::from(&created.worktree_path);
        assert!(wt.is_dir());

        // 模拟 Agent 在分支上留下一个提交（正常任务的产品）
        let wt_ref = Path::new(&created.worktree_path);
        assert!(run_git(wt_ref, &["config", "user.email", "test@example.com"]).unwrap().ok);
        assert!(run_git(wt_ref, &["config", "user.name", "test"]).unwrap().ok);
        fs::write(wt.join("work.txt"), "agent output").unwrap();
        assert!(run_git(wt_ref, &["add", "."]).unwrap().ok);
        let commit = run_git(wt_ref, &["commit", "-q", "-m", "work"]).unwrap();
        assert!(commit.ok, "提交失败: {}", commit.tail());

        remove_inner(
            &root_str,
            &created.worktree_path,
            &created.work_branch,
            Some(created.base_branch.clone()),
            Some(created.base_sha.clone()),
            true,
        )
        .unwrap();

        assert!(!wt.exists(), "目录必须消失");
        // 注册表恢复：只剩主工作树
        let list = git_stdout(&root, &["worktree", "list"]).unwrap();
        assert_eq!(list.lines().count(), 1, "worktree list: {}", list);
        // 分支必须保留，且提交还在（不是「守卫放行后照删」那种假保留）
        let kept = git_stdout(&root, &["branch", "--list", "task/keep-1"]).unwrap();
        assert!(kept.contains("task/keep-1"), "keep_branch 必须保留分支");
        let subject = git_stdout(&root, &["log", "-1", "--format=%s", "task/keep-1"]).unwrap();
        assert_eq!(subject, "work", "分支上的提交必须完好");

        let _ = fs::remove_dir_all(&parent);
    }

    /// 默认行为（AC4）：`keep_branch = false` → 目录与分支都消失（启动失败回滚的清理）。
    #[test]
    fn default_remove_still_deletes_the_branch() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let (parent, root, root_str) = init_real_repo("dropbranch");
        let created = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "drop-1".into(),
            init_command: None,
        })
        .expect("创建应成功");
        let wt = PathBuf::from(&created.worktree_path);
        assert!(work_branch_exists(&root, &created.work_branch));

        // 无新提交 → 守卫放行 → 分支被删
        remove_inner(
            &root_str,
            &created.worktree_path,
            &created.work_branch,
            Some(created.base_branch.clone()),
            Some(created.base_sha.clone()),
            false,
        )
        .unwrap();

        assert!(!wt.exists(), "目录必须消失");
        assert!(!work_branch_exists(&root, &created.work_branch), "默认必须删掉分支");

        let _ = fs::remove_dir_all(&parent);
    }

    /// 详情抽屉「删除分支」（R3 / AC5）：目录已删、只剩分支，前端只能传空路径。
    /// 空路径必须**跳过目录删除**而不是报 `path-not-absolute`，分支要真的被删掉。
    #[test]
    fn empty_worktree_path_deletes_only_the_branch() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let (parent, root, root_str) = init_real_repo("onlybranch");
        let created = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "only-1".into(),
            init_command: None,
        })
        .expect("创建应成功");
        // 先按终态语义删目录（保留分支）
        remove_inner(
            &root_str,
            &created.worktree_path,
            &created.work_branch,
            Some(created.base_branch.clone()),
            Some(created.base_sha.clone()),
            true,
        )
        .unwrap();
        assert!(!Path::new(&created.worktree_path).exists());
        assert!(work_branch_exists(&root, &created.work_branch));

        // 手动删分支：前端此时 worktreePath 已是 null → 传空串
        remove_inner_allow_absent_dir(
            &root_str,
            "",
            &created.work_branch,
            Some(created.base_branch.clone()),
            Some(created.base_sha.clone()),
            false,
        )
        .unwrap();
        assert!(!work_branch_exists(&root, &created.work_branch), "空路径也要删掉分支");

        let _ = fs::remove_dir_all(&parent);
    }

    /// 自动提交（task 10-26-worktree-autocommit / AC1 / AC2 / AC7）：
    /// 改动 = 一个修改过的跟踪文件 + 一个未跟踪文件 → 一次提交落到 **task 分支**，
    /// message 生效，提交后工作区干净，目录里之后 `git log` 能看到它。
    #[test]
    fn commit_all_commits_dirty_worktree() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let (parent, root, root_str) = init_real_repo("commitdirty");
        let created = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "dirty-1".into(),
            init_command: None,
        })
        .expect("创建应成功");
        let wt = Path::new(&created.worktree_path);

        // Agent 只改文件、不 commit（真实场景：忘了提交）
        fs::write(wt.join("README.md"), "agent edited, uncommitted").unwrap();
        fs::write(wt.join("new.txt"), "untracked output").unwrap();
        let message = "chore(task): 自动提交未保存改动 — 任务标题";

        let result = commit_all_inner(&CommitWorktreeRequest {
            worktree_path: created.worktree_path.clone(),
            message: message.into(),
        })
        .expect("有改动时提交应成功");

        assert!(result.committed, "有改动必须真的提交");
        let sha = result.sha.expect("提交后应有 sha");
        assert!(!sha.is_empty());
        // 提交落在任务分支上，message 原样保留
        assert_eq!(
            git_stdout(&root, &["log", "-1", "--format=%s", "task/dirty-1"]).unwrap(),
            message
        );
        assert_eq!(git_stdout(&root, &["rev-parse", "task/dirty-1"]).unwrap(), sha);
        // 工作区已干净：不会再产生第二次提交
        assert!(git_stdout(wt, &["status", "--porcelain"]).unwrap().is_empty());

        // 收尾：目录删掉、分支保留（终态语义）
        remove_inner(
            &root_str,
            &created.worktree_path,
            &created.work_branch,
            Some(created.base_branch.clone()),
            Some(created.base_sha.clone()),
            true,
        )
        .unwrap();
        assert!(!wt.exists());
        let kept = git_stdout(&root, &["branch", "--list", "task/dirty-1"]).unwrap();
        assert!(kept.contains("task/dirty-1"), "自动提交的分支必须保留");
        // 目录删掉也不影响：提交已在对象库里（AC1 的最终目的）
        assert_eq!(git_stdout(&root, &["log", "-1", "--format=%s", "task/dirty-1"]).unwrap(), message);

        let _ = fs::remove_dir_all(&parent);
    }

    /// 自动提交（AC3 / AC7）：干净工作区 → `committed: false`，不产生空提交。
    #[test]
    fn commit_all_skips_a_clean_worktree() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let (parent, _root, root_str) = init_real_repo("commitclean");
        let created = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "clean-1".into(),
            init_command: None,
        })
        .expect("创建应成功");
        let wt = Path::new(&created.worktree_path);
        let before = git_stdout(wt, &["rev-list", "--count", "HEAD"]).unwrap();

        let result = commit_all_inner(&CommitWorktreeRequest {
            worktree_path: created.worktree_path.clone(),
            message: "不该出现".into(),
        })
        .expect("干净工作区不是错误");

        assert!(!result.committed, "干净工作区不得产生提交");
        assert!(result.sha.is_none(), "没提交就没有 sha");
        assert_eq!(
            git_stdout(wt, &["rev-list", "--count", "HEAD"]).unwrap(),
            before,
            "提交条数必须不变"
        );
        assert_eq!(
            git_stdout(wt, &["log", "-1", "--format=%s"]).unwrap(),
            "init",
            "HEAD 仍应是原来的提交"
        );

        let _ = fs::remove_dir_all(&parent);
    }

    /// 自动提交的安全校验（AC6）：相对路径 / 盘符相对路径在**执行任何 git 命令之前**拒绝；
    /// 目录不存在报 `worktree-missing`。
    #[test]
    fn commit_all_rejects_relative_path() {
        assert_eq!(
            commit_all_inner(&CommitWorktreeRequest {
                worktree_path: "relative/wt".into(),
                message: "x".into(),
            })
            .unwrap_err(),
            "path-not-absolute"
        );
        assert_eq!(
            commit_all_inner(&CommitWorktreeRequest {
                worktree_path: "C:wt".into(),
                message: "x".into(),
            })
            .unwrap_err(),
            "path-not-absolute"
        );
        assert_eq!(
            commit_all_inner(&CommitWorktreeRequest {
                worktree_path: "  ".into(),
                message: "x".into(),
            })
            .unwrap_err(),
            "path-not-absolute"
        );
        // 绝对但不存在：直接判 worktree-missing，不调用 git
        let parent = tmp_dir("commitmissing");
        let missing = parent.join("nope").to_string_lossy().into_owned();
        assert_eq!(
            commit_all_inner(&CommitWorktreeRequest {
                worktree_path: missing,
                message: "x".into(),
            })
            .unwrap_err(),
            "worktree-missing"
        );
        let _ = fs::remove_dir_all(&parent);
    }

    /// 自动提交只落在任务分支上（prd 约束「绝不能碰项目主分支」）：结果里的 `branch`
    /// 必须就是 `task/<id>`，且**改动前后主分支的 HEAD 完全不动**。
    #[test]
    fn commit_all_reports_task_branch_and_leaves_main_untouched() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let (parent, root, root_str) = init_real_repo("commitbranch");
        let main_branch = git_stdout(&root, &["symbolic-ref", "--short", "HEAD"]).unwrap();
        let main_before = git_stdout(&root, &["rev-parse", "HEAD"]).unwrap();
        let created = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "branch-1".into(),
            init_command: None,
        })
        .expect("创建应成功");
        fs::write(Path::new(&created.worktree_path).join("new.txt"), "x").unwrap();

        let result = commit_all_inner(&CommitWorktreeRequest {
            worktree_path: created.worktree_path.clone(),
            message: "chore(task): 自动提交未保存改动 — 分支校验".into(),
        })
        .expect("提交应成功");

        assert_eq!(
            result.branch.as_deref(),
            Some("task/branch-1"),
            "提交必须落在任务分支上"
        );
        assert_eq!(
            git_stdout(&root, &["rev-parse", &main_branch]).unwrap(),
            main_before,
            "主分支 HEAD 绝不能因为自动提交而移动"
        );

        let _ = fs::remove_dir_all(&parent);
    }

    /// detached HEAD 的 worktree 拒接自动提交：那里的提交没有任何 ref 指向，
    /// 删目录后同样是永久丢失（与本任务要消灭的数据丢失一模一样）。
    #[test]
    fn commit_all_refuses_detached_head_without_touching_the_tree() {
        if run_git(Path::new("."), &["--version"]).is_err() {
            eprintln!("跳过：找不到 git");
            return;
        }
        let (parent, root, root_str) = init_real_repo("commitdetached");
        let created = create_inner(&CreateWorktreeRequest {
            root_path: root_str.clone(),
            base_branch: None,
            task_id: "detached-1".into(),
            init_command: None,
        })
        .expect("创建应成功");
        let wt = Path::new(&created.worktree_path);
        let out = run_git(wt, &["checkout", "--detach"]).unwrap();
        assert!(out.ok, "detach 应成功: {}", out.tail());
        fs::write(wt.join("new.txt"), "x").unwrap();

        assert_eq!(
            commit_all_inner(&CommitWorktreeRequest {
                worktree_path: created.worktree_path.clone(),
                message: "x".into(),
            })
            .unwrap_err(),
            "worktree-detached"
        );
        // 没有 `add -A`：未跟踪文件仍在工作区（没被偷偷暂存）
        assert!(
            git_stdout(wt, &["status", "--porcelain"]).unwrap().contains("new.txt"),
            "拒绝时必须不改工作区"
        );
        // 任务分支没有被移动
        assert_eq!(
            git_stdout(&root, &["log", "-1", "--format=%s", "task/detached-1"]).unwrap(),
            "init"
        );

        let _ = fs::remove_dir_all(&parent);
    }
}
