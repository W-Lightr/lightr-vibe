// 真实文件系统命令：目录懒加载列取 / 文件读写（冲突保护 + 原子写）/
// 应用自有文本文件读写（projects.json 等）。仅用 std::fs，不引入新 crate。
// 契约见 design.md §2：错误串约定 too-large / binary / not-found /
// outside-project / conflict；用户源码文件不做 .bak（design D3，避免污染 git 仓库）。

use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 单文件读取上限：2MB（超过返回 Err("too-large")）
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;

/// `fs_list_dir` 返回的单条目（camelCase 与前端 DirEntry 契约一致）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    /// 'dir' | 'file'（symlink 等按 metadata 跟随后的实际类型归类）
    #[serde(rename = "type")]
    pub entry_type: String,
    /// 文件字节数；目录为 None
    pub size: Option<u64>,
    pub mtime_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDirResult {
    pub entries: Vec<DirEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileResult {
    pub text: String,
    pub mtime_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileResult {
    pub mtime_ms: u64,
}

// ---------- 路径工具 ----------

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mtime_ms_of(path: &Path) -> Result<u64, String> {
    let meta = fs::metadata(path).map_err(|e| format!("读取文件元数据失败: {}", e))?;
    let modified = meta.modified().map_err(|e| format!("读取 mtime 失败: {}", e))?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64)
}

/// 词法规范化：折叠 `.` 与可回退的 `..` 组件（防越界逃逸）。
/// 不触文件系统，因此对尚不存在的路径也可用；`..` 到达根后直接忽略，
/// 保证结果不会越过原始盘符/根目录。
fn normalize_path(raw: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in Path::new(raw).components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // pop 失败（已在根）时忽略该 `..`，防止逃出根目录
                let _ = out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 项目边界校验：规范化后的 path 必须以规范化后的 root_path 为前缀，
/// 否则返回 Err("outside-project")。
fn ensure_inside(path: &str, root_path: &str) -> Result<PathBuf, String> {
    let root = normalize_path(root_path);
    let target = normalize_path(path);
    if target == root || target.starts_with(&root) {
        Ok(target)
    } else {
        Err("outside-project".into())
    }
}

/// 原子写：先写同目录临时文件再 rename（Windows 下 rename 覆盖既有文件），
/// 避免写一半崩溃留下损坏文件。失败时清理临时文件。
fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("目标文件名不合法")?;
    let tmp = path.with_file_name(format!("{}.lightr-tmp-{}", file_name, now_ms()));
    fs::write(&tmp, text).map_err(|e| {
        // 写失败同样清理半成品临时文件，避免残留
        let _ = fs::remove_file(&tmp);
        format!("写入临时文件失败: {}", e)
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("重命名写入失败: {}", e)
    })
}

// ---------- Tauri commands ----------

/// 列目录（懒加载）：dirs-first + 名称不区分大小写排序。
/// `.git` 目录节点在上层列表中正常显示，但对其列取返回空 entries
/// （内容可达万级节点且对用户无意义，design §2）。
#[tauri::command]
pub fn fs_list_dir(path: String, root_path: String) -> Result<ListDirResult, String> {
    let dir = ensure_inside(&path, &root_path)?;

    // .git 内容跳过（显示节点、不列内容）
    let is_git_dir = dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == ".git")
        .unwrap_or(false);
    if is_git_dir {
        return Ok(ListDirResult { entries: Vec::new() });
    }

    if !dir.exists() {
        return Err("not-found".into());
    }
    let read = fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {}", e))?;

    let mut dirs: Vec<DirEntry> = Vec::new();
    let mut files: Vec<DirEntry> = Vec::new();
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        // metadata 跟随 symlink；失败条目直接跳过（不阻塞整目录）
        let meta = match item.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        let entry = if meta.is_dir() {
            DirEntry {
                name,
                entry_type: "dir".into(),
                size: None,
                mtime_ms: mtime,
            }
        } else {
            DirEntry {
                name,
                entry_type: "file".into(),
                size: Some(meta.len()),
                mtime_ms: mtime,
            }
        };
        if meta.is_dir() {
            dirs.push(entry);
        } else {
            files.push(entry);
        }
    }
    // 名称排序（不区分大小写，近似 locale 顺序；不引入 locale crate）
    let by_name = |a: &DirEntry, b: &DirEntry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    dirs.sort_by(by_name);
    files.sort_by(by_name);
    dirs.append(&mut files);

    Ok(ListDirResult { entries: dirs })
}

/// 读文本文件：>2MB 拒绝（too-large）、NUL 字节/非 UTF-8 拒绝（binary）、
/// 不存在返回 not-found、越界返回 outside-project。
#[tauri::command]
pub fn fs_read_file(path: String, root_path: String) -> Result<ReadFileResult, String> {
    let target = ensure_inside(&path, &root_path)?;
    if !target.exists() {
        return Err("not-found".into());
    }
    let meta = fs::metadata(&target).map_err(|e| format!("读取文件元数据失败: {}", e))?;
    if meta.len() > MAX_FILE_SIZE {
        return Err("too-large".into());
    }
    let bytes = fs::read(&target).map_err(|e| format!("读取文件失败: {}", e))?;
    // 二进制探测：NUL 字节 + UTF-8 解码失败
    if bytes.contains(&0u8) {
        return Err("binary".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "binary".to_string())?;
    let mtime_ms = mtime_ms_of(&target)?;
    Ok(ReadFileResult { text, mtime_ms })
}

/// 写文本文件：mtime 冲突校验（conflict 语义同 tauri-bridge 规范）→ 原子写。
/// expected_mtime 为 None 表示读取时文件不存在；不一致统一 Err("conflict")，
/// 前端据此提供「重新加载 / 覆盖」二选一，绝不静默覆盖。
/// 用户源码文件不做 .bak（design D3）。
#[tauri::command]
pub fn fs_write_file(
    path: String,
    text: String,
    expected_mtime: Option<u64>,
    root_path: String,
) -> Result<WriteFileResult, String> {
    let target = ensure_inside(&path, &root_path)?;
    let exists = target.exists();
    match (expected_mtime, exists) {
        // 读时不存在、写时出现了 → 外部新建，视为冲突
        (None, true) => return Err("conflict".into()),
        // 读时存在、写时已被删除 → 状态漂移，视为冲突
        (Some(_), false) => return Err("conflict".into()),
        (Some(expected), true) => {
            let current = mtime_ms_of(&target)?;
            if current != expected {
                return Err("conflict".into());
            }
        }
        // (None, false)：首次创建，无需校验
        _ => {}
    }

    atomic_write(&target, &text)?;
    Ok(WriteFileResult {
        mtime_ms: mtime_ms_of(&target)?,
    })
}

/// 读应用自有文本文件（如 ~/.lightr-vibe/lightr-vibe/projects.json）：
/// 不存在返回 Ok(None)，非错误。无 mtime 语义（应用独占文件）。
#[tauri::command]
pub fn fs_read_text(path: String) -> Result<Option<String>, String> {
    let target = normalize_path(&path);
    if !target.exists() {
        return Ok(None);
    }
    fs::read_to_string(&target)
        .map(Some)
        .map_err(|e| format!("读取文件失败: {}", e))
}

/// 写应用自有文本文件：目录自动创建 + 原子写；无冲突校验（应用独占）。
#[tauri::command]
pub fn fs_write_text(path: String, text: String) -> Result<(), String> {
    let target = normalize_path(&path);
    atomic_write(&target, &text)
}

// ---------- 单元测试（cargo test -p lightr-vibe；覆盖 design §5 测试要点） ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lightr-fs-test-{}-{}", tag, now_ms()));
        fs::create_dir_all(&dir).expect("创建测试目录失败");
        dir
    }

    #[test]
    fn normalize_collapses_dotdot_and_cannot_escape_root() {
        let p = normalize_path("C:/a/b/../c/./d");
        assert_eq!(p, PathBuf::from("C:/a/c/d"));
        // `..` 越过根后被忽略，不会逃出盘符根
        let escaped = normalize_path("C:/a/../../../Windows");
        assert!(escaped.starts_with(Path::new("C:/")));
        assert!(!escaped.to_string_lossy().contains(".."));
    }

    #[test]
    fn boundary_outside_project_is_rejected() {
        // 越界：.. 逃逸出 rootPath
        assert_eq!(
            ensure_inside("C:/root/../secret.txt", "C:/root"),
            Err("outside-project".to_string())
        );
        // 前缀相似但非子路径（rootX vs root）也拒绝
        assert_eq!(
            ensure_inside("C:/rootEvil/x.txt", "C:/root"),
            Err("outside-project".to_string())
        );
        // 子路径与根本身通过
        assert!(ensure_inside("C:/root/a/./b.txt", "C:/root").is_ok());
        assert!(ensure_inside("C:/root", "C:/root").is_ok());
    }

    #[test]
    fn read_rejects_binary_and_missing() {
        let root = tmp_root("read");
        let bin = root.join("blob.bin");
        fs::write(&bin, [0u8, 1, 2, 3]).unwrap();
        let err = fs_read_file(
            bin.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        )
        .unwrap_err();
        assert_eq!(err, "binary");

        let missing = root.join("nope.txt");
        let err = fs_read_file(
            missing.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        )
        .unwrap_err();
        assert_eq!(err, "not-found");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_conflict_matrix_and_atomic_roundtrip() {
        let root = tmp_root("write");
        let file = root.join("a.txt");
        let root_str = root.to_string_lossy().into_owned();
        let path = file.to_string_lossy().into_owned();

        // (None, 不存在)：首次创建成功
        let r1 = fs_write_file(path.clone(), "v1".into(), None, root_str.clone()).unwrap();
        // (Some(旧), 存在但 mtime 已变)：conflict —— 用错误的 expected 模拟外部修改
        let err = fs_write_file(path.clone(), "v2".into(), Some(r1.mtime_ms + 12345), root_str.clone()).unwrap_err();
        assert_eq!(err, "conflict");
        // 磁盘内容未被覆盖
        assert_eq!(fs::read_to_string(&file).unwrap(), "v1");
        // (Some(正确 mtime), 存在)：成功，返回新 mtime
        let r2 = fs_write_file(path.clone(), "v2".into(), Some(r1.mtime_ms), root_str.clone()).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "v2");
        // (None, 已存在)：外部新建冲突
        let err = fs_write_file(path.clone(), "v3".into(), None, root_str.clone()).unwrap_err();
        assert_eq!(err, "conflict");
        // (Some(_), 已删除)：状态漂移冲突
        fs::remove_file(&file).unwrap();
        let err = fs_write_file(path.clone(), "v3".into(), Some(r2.mtime_ms), root_str).unwrap_err();
        assert_eq!(err, "conflict");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_dir_git_is_empty_and_dirs_first() {
        let root = tmp_root("list");
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::create_dir(root.join("zdir")).unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        fs::write(root.join("A.txt"), "a").unwrap();
        fs::write(root.join(".git/HEAD"), "ref").unwrap();

        let root_str = root.to_string_lossy().into_owned();
        let listed = fs_list_dir(root_str.clone(), root_str.clone()).unwrap();
        // dirs-first + 名称不区分大小写排序
        let names: Vec<String> = listed.entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec![".git", "zdir", "A.txt", "b.txt"]);

        // .git 节点显示但内容为空
        let git = fs_list_dir(root.join(".git").to_string_lossy().into_owned(), root_str.clone()).unwrap();
        assert!(git.entries.is_empty());

        // 不存在目录 → not-found
        let err = fs_list_dir(root.join("nope").to_string_lossy().into_owned(), root_str).unwrap_err();
        assert_eq!(err, "not-found");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_write_text_roundtrip_creates_dirs() {
        let root = tmp_root("text");
        let file = root.join("sub/dir/projects.json");
        let path = file.to_string_lossy().into_owned();
        // 不存在 → Ok(None)
        assert_eq!(fs_read_text(path.clone()).unwrap(), None);
        fs_write_text(path.clone(), "[]".into()).unwrap();
        assert_eq!(fs_read_text(path.clone()).unwrap(), Some("[]".to_string()));
        // 原子写不残留临时文件
        let leftovers: Vec<_> = fs::read_dir(file.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("lightr-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件: {:?}", leftovers);
        let _ = fs::remove_dir_all(&root);
    }
}
