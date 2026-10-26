// git worktree 隔离层（task 10-26-worktree-backend / design §2）：
// 仓库信息探测、工作树创建、工作树删除。仅用 std::process::Command 调 git，
// 不引入新 crate、不加 tauri capability。**全仓唯一调用 git 的地方**（PAC2）。
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
// `worktree-remove: <stderr>` / `git: <stderr>`（其余 git 失败）。

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

fn remove_inner(
    root_path: &str,
    worktree_path: &str,
    work_branch: &str,
    base_branch: Option<String>,
    base_sha: Option<String>,
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

    // 4) 分支守卫：base 优先用创建时记录的 sha（分支可能已被移动），退化到分支名
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

    // 5) 删除分支；不存在（已被手工删掉）视为成功
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

/// 删除工作树 +（无未落地提交时）删除任务分支。
#[tauri::command]
pub fn worktree_remove(
    root_path: String,
    worktree_path: String,
    work_branch: String,
    base_branch: Option<String>,
    base_sha: Option<String>,
) -> Result<(), String> {
    remove_inner(&root_path, &worktree_path, &work_branch, base_branch, base_sha)
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
        let err = remove_inner("C:/work/repo", "relative/wt", "task/task-1", None, None)
            .unwrap_err();
        assert_eq!(err, "path-not-absolute");
        // 盘符相对同样拒绝
        let err = remove_inner("C:/work/repo", "C:wt", "task/task-1", None, None).unwrap_err();
        assert_eq!(err, "path-not-absolute");
        // root 本身非法时也在执行 git 前返回
        let err = remove_inner("repo", "C:/work/wt", "task/task-1", None, None).unwrap_err();
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
}
