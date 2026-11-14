//! 成片本地文件定位与系统文件管理器唤起。
//!
//! 路径只来自「当前项目 → 当前版本 → 视频素材 → 数据库 storage_path」，
//! 不允许前端拼接或按标题猜测。唤起系统文件管理器时不使用 shell，
//! 所有平台都以独立参数传入路径。

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::{ApiError, ApiResult};

/// 返回给前端的当前版本文件位置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoFileLocation {
    /// 真实绝对路径。
    pub path: String,
    /// 所在目录。
    pub directory: String,
    /// 展示用文件名。
    pub file_name: String,
    /// 文件当前是否存在；成功返回时恒为 true。
    pub exists: bool,
}

/// 系统文件管理器调用：程序名与逐项参数。
///
/// 参数分隔保存，绝不拼接成一条 shell 字符串。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevealInvocation {
    /// 可执行程序。
    pub program: &'static str,
    /// 独立参数。
    pub args: Vec<String>,
}

/// 校验版本归属并定位成片文件。
///
/// 依次验证：项目属于当前用户；版本属于该项目；版本引用的素材属于该项目且为视频；
/// 文件为 MP4；数据库路径位于 `storage_dir/<project_id>` 内且真实存在。
pub fn locate(
    conn: &Connection,
    storage_dir: &Path,
    project_id: &str,
    version_id: &str,
    user_id: &str,
) -> ApiResult<VideoFileLocation> {
    db::project_for(conn, project_id, user_id)?;
    let version = db::load_version(conn, version_id)?
        .filter(|row| row.project_id == project_id)
        .ok_or_else(|| ApiError::not_found("这个视频版本不存在。"))?;
    let asset = db::load_asset(conn, &version.video_asset_id)?
        .filter(|row| {
            row.asset.project_id == project_id
                && row.asset.kind == "video"
                && row.asset.source == "rendered"
        })
        .ok_or_else(|| ApiError::not_found("这个版本的成片素材不存在。"))?;
    if !is_mp4(&asset.asset.file_name, &asset.asset.mime_type) {
        return Err(ApiError::validation("当前版本不是可定位的 MP4 成片。"));
    }
    if asset.storage_path.trim().is_empty() {
        return Err(ApiError::not_found("这个版本没有本地成片文件。"));
    }

    let stored = PathBuf::from(&asset.storage_path);
    let root = storage_dir.join(project_id);
    // 存储目录可能尚未创建；先确认文件本身，再分别规范化比较，避免符号链接绕过。
    if !stored.is_file() {
        return Err(ApiError::not_found(
            "成片文件已不在本地，可能被移动或删除；请重新生成或导出。",
        ));
    }
    let canonical = stored
        .canonicalize()
        .map_err(|error| ApiError::internal(format!("规范化成片路径失败：{error}")))?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| ApiError::internal(format!("规范化项目存储目录失败：{error}")))?;
    if !canonical.starts_with(&canonical_root) {
        tracing::error!(
            target: "media",
            "拒绝访问项目存储目录之外的成片：{}",
            canonical.display()
        );
        return Err(ApiError::not_found("成片文件不可用。"));
    }

    let path = utf8_path(&canonical)?;
    let directory = canonical
        .parent()
        .map(utf8_path)
        .transpose()?
        .ok_or_else(|| ApiError::not_found("成片文件不可用。"))?;
    let file_name = if asset.asset.file_name.trim().is_empty() {
        canonical
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("成片.mp4")
            .to_string()
    } else {
        asset.asset.file_name.clone()
    };
    Ok(VideoFileLocation {
        path,
        directory,
        file_name,
        exists: true,
    })
}

/// 构建指定平台的文件管理器调用。
///
/// macOS 在 Finder 中选中文件；Windows 在资源管理器中选中文件；
/// Linux 优先由调用方尝试 FileManager1，此函数给出打开父目录的兜底。
pub fn reveal_invocation_for(os: &str, file: &Path, directory: &Path) -> RevealInvocation {
    match os {
        "macos" => RevealInvocation {
            program: "open",
            args: vec!["-R".into(), file.display().to_string()],
        },
        "windows" => RevealInvocation {
            program: "explorer",
            // `/select,<path>` 必须作为单个参数，路径中的空格不能拆分。
            args: vec![format!("/select,{}", file.display())],
        },
        _ => RevealInvocation {
            program: "xdg-open",
            args: vec![directory.display().to_string()],
        },
    }
}

/// 在系统文件管理器中定位或打开当前成片。
///
/// 所有路径都来自 [`locate`] 校验后的结果；这里不接收前端任意路径。
pub async fn reveal(location: &VideoFileLocation) -> Result<(), String> {
    let file = Path::new(&location.path);
    let directory = Path::new(&location.directory);
    match std::env::consts::OS {
        "macos" => {
            let invocation = reveal_invocation_for("macos", file, directory);
            match run_checked(invocation).await {
                Ok(()) => Ok(()),
                // Finder 定位失败时退化为打开所在目录。
                Err(_) => {
                    run_checked(RevealInvocation {
                        program: "open",
                        args: vec![directory.display().to_string()],
                    })
                    .await
                }
            }
        }
        "windows" => {
            // explorer 即使成功也常返回非零退出码，这里只要求进程能启动。
            let invocation = reveal_invocation_for("windows", file, directory);
            tokio::process::Command::new(invocation.program)
                .args(&invocation.args)
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("无法打开文件管理器：{error}"))
        }
        _ => {
            // Linux 桌面优先通过 FileManager1 选中文件；不可用时打开父目录。
            if try_linux_file_manager(file).await.unwrap_or(false) {
                return Ok(());
            }
            let invocation = reveal_invocation_for("linux", file, directory);
            run_checked(invocation).await
        }
    }
}

/// 执行调用并检查退出状态。
async fn run_checked(invocation: RevealInvocation) -> Result<(), String> {
    let status = tokio::process::Command::new(invocation.program)
        .args(&invocation.args)
        .status()
        .await
        .map_err(|error| format!("无法打开文件管理器：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("系统文件管理器没有接受定位请求。".into())
    }
}

/// 尝试使用 freedesktop 的 FileManager1 接口选中文件；失败时返回 false。
#[cfg(target_os = "linux")]
async fn try_linux_file_manager(file: &Path) -> Result<bool, String> {
    let uri = file_uri(file);
    let status = tokio::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("array:string:{uri}"),
            "string:",
        ])
        .status()
        .await;
    Ok(status.map(|s| s.success()).unwrap_or(false))
}

/// 非 Linux 平台不提供 FileManager1。
#[cfg(not(target_os = "linux"))]
async fn try_linux_file_manager(_file: &Path) -> Result<bool, String> {
    Ok(false)
}

/// 把绝对路径转换为 `file://` URI，供 freedesktop 接口使用。
#[cfg(target_os = "linux")]
fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// 判断素材是否为 MP4；扩展名或 MIME 任一命中即可。
fn is_mp4(file_name: &str, mime_type: &str) -> bool {
    file_name.to_ascii_lowercase().ends_with(".mp4") || mime_type.eq_ignore_ascii_case("video/mp4")
}

/// 路径必须是合法 UTF-8，避免前端复制到不可用的替代字符路径。
fn utf8_path(path: &Path) -> ApiResult<String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| ApiError::validation("成片路径包含无法显示的文件名。"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::models::Asset;
    use rusqlite::params;

    /// 创建带真实文件的项目存储与数据库记录。
    fn fixture(storage: &Path) -> (Db, PathBuf) {
        let db = Db::open(&storage.join("test.sqlite3")).unwrap();
        let project_dir = storage.join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("video.mp4");
        std::fs::write(&file, b"fake-mp4").unwrap();
        db.write(|conn| {
            conn.execute(
                "INSERT INTO projects
                 (id, user_id, title, status, draft_revision, draft_dirty, draft, created_at, updated_at)
                 VALUES ('project', 'local_user', 'test', 'ready', 1, 0, ?1, ?2, ?2)",
                params![
                    serde_json::to_string(&crate::catalog::empty_draft()).unwrap(),
                    "2026-01-01T00:00:00Z"
                ],
            )?;
            db::insert_asset(
                conn,
                &db::AssetRow {
                    asset: Asset {
                        id: "video".into(),
                        project_id: "project".into(),
                        kind: "video".into(),
                        source: "rendered".into(),
                        status: "ready".into(),
                        file_name: "成片.mp4".into(),
                        mime_type: "video/mp4".into(),
                        size_bytes: 8,
                        duration_ms: Some(1000),
                        width: Some(720),
                        height: Some(1280),
                        thumbnail_url: None,
                        error: None,
                    },
                    storage_path: file.display().to_string(),
                },
            )?;
            conn.execute(
                "INSERT INTO versions
                 (id, project_id, version_number, source_draft_revision, source_job_id, created_at,
                  video_asset_id, duration_ms, width, height, snapshot, exports)
                 VALUES ('version', 'project', 1, 1, 'job', ?1, 'video', 1000, 720, 1280, ?2, '[]')",
                params![
                    "2026-01-01T00:00:00Z",
                    serde_json::to_string(&crate::catalog::empty_draft()).unwrap()
                ],
            )?;
            Ok(())
        })
        .unwrap();
        (db, file)
    }

    #[test]
    fn locate_returns_real_path_for_selected_version() {
        let root = std::env::temp_dir().join(crate::util::make_id("file-location"));
        let (db, file) = fixture(&root);
        let location = db
            .read(|conn| locate(conn, &root, "project", "version", "local_user"))
            .unwrap();
        assert_eq!(Path::new(&location.path), file.canonicalize().unwrap());
        assert_eq!(location.file_name, "成片.mp4");
        assert!(location.exists);
        assert_eq!(
            Path::new(&location.directory),
            file.canonicalize().unwrap().parent().unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn locate_rejects_asset_outside_project_storage() {
        let root = std::env::temp_dir().join(crate::util::make_id("file-location-escape"));
        let (db, _file) = fixture(&root);
        let outside = root.join("outside.mp4");
        std::fs::write(&outside, b"fake-mp4").unwrap();
        db.write(|conn| {
            conn.execute(
                "UPDATE assets SET storage_path = ?1 WHERE id = 'video'",
                params![outside.display().to_string()],
            )?;
            Ok(())
        })
        .unwrap();
        let error = db
            .read(|conn| locate(conn, &root, "project", "version", "local_user"))
            .unwrap_err();
        assert_eq!(error.code, "NOT_FOUND");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn locate_rejects_missing_file_and_wrong_owner() {
        let root = std::env::temp_dir().join(crate::util::make_id("file-location-missing"));
        let (db, file) = fixture(&root);
        std::fs::remove_file(file).unwrap();
        let missing = db
            .read(|conn| locate(conn, &root, "project", "version", "local_user"))
            .unwrap_err();
        assert_eq!(missing.code, "NOT_FOUND");

        let intruder = db
            .read(|conn| locate(conn, &root, "project", "version", "someone_else"))
            .unwrap_err();
        assert_eq!(intruder.code, "NOT_FOUND");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reveal_commands_keep_path_as_single_argument() {
        let file = Path::new("/data/storage/project/asset.mp4");
        let dir = Path::new("/data/storage/project");
        let mac = reveal_invocation_for("macos", file, dir);
        assert_eq!(mac.program, "open");
        assert_eq!(mac.args, vec!["-R", "/data/storage/project/asset.mp4"]);

        let win = reveal_invocation_for("windows", Path::new(r"C:\data\a b\asset.mp4"), dir);
        assert_eq!(win.program, "explorer");
        assert_eq!(win.args, vec![r"/select,C:\data\a b\asset.mp4"]);

        let linux = reveal_invocation_for("linux", file, dir);
        assert_eq!(linux.program, "xdg-open");
        assert_eq!(linux.args, vec!["/data/storage/project"]);
    }

    #[test]
    fn mp4_detection_accepts_extension_or_mime() {
        assert!(is_mp4("成片.MP4", "application/octet-stream"));
        assert!(is_mp4("成片.bin", "video/mp4"));
        assert!(!is_mp4("成片.mov", "video/quicktime"));
    }

    /// 在真实应用数据上只读核查，验证开发环境下的默认数据目录与历史成片。
    ///
    /// 需要显式提供 `EXPLAINCRAFT_LIVE_DATA_DIR`（应用数据目录）、
    /// `EXPLAINCRAFT_LIVE_PROJECT` 与 `EXPLAINCRAFT_LIVE_VERSION`；未设置时跳过，
    /// 正常测试套件不依赖任何用户数据。数据库以只读方式打开，不写入。
    #[test]
    fn locate_against_live_app_data_when_requested() {
        let Ok(data) = std::env::var("EXPLAINCRAFT_LIVE_DATA_DIR") else {
            return;
        };
        let project = std::env::var("EXPLAINCRAFT_LIVE_PROJECT").expect("需要项目 ID");
        let version = std::env::var("EXPLAINCRAFT_LIVE_VERSION").expect("需要版本 ID");
        let root = PathBuf::from(&data);
        let conn = Connection::open_with_flags(
            root.join("explaincraft.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect("只读打开应用数据库");
        let location = locate(
            &conn,
            &root.join("storage"),
            &project,
            &version,
            "local_user",
        )
        .unwrap();
        assert!(location.exists);
        assert!(Path::new(&location.path).is_absolute());
        assert!(Path::new(&location.path).starts_with(root.join("storage").join(&project)));
        let header = std::fs::read(&location.path).unwrap();
        // MP4 的 ftyp box：前 8 字节后紧跟文件类型标识。
        assert_eq!(&header[4..8], b"ftyp");
    }

    #[test]
    fn utf8_path_rejects_non_utf8() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff]));
            assert!(utf8_path(&path).is_err());
        }
    }
}
