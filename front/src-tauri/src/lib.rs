//! 灵镜桌面应用：Tauri 窗口 + 进程内嵌入的 Rust 后端。
//!
//! 启动流程：
//! 1. 读取本地配置（应用配置目录的 `.env`；调试构建再读仓库 `back/.env`）；
//! 2. 在 `127.0.0.1` 上绑定一个空闲端口，在 Tauri 的异步运行时里启动后端
//!    （与独立服务 `back/src/main.rs` 是同一套代码）；
//! 3. 创建主窗口，并通过初始化脚本注入 `window.__EXPLAINCRAFT_API__`，前端据此走 HTTP 接口；
//! 4. 应用退出时通知后端优雅停止；未完成的任务状态在数据库中，下次启动会继续。
//!
//! 数据库与生成的文件保存在系统的应用数据目录（Windows：`%APPDATA%\com.explaincraft.lingjing`）。

use std::net::TcpListener as StdTcpListener;
use std::path::PathBuf;
use std::sync::Mutex;

use explaincraft_backend::{init_logging, load_env_file, serve, Config};
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

/// 主窗口标题。
const WINDOW_TITLE: &str = "灵镜 · AI 视频创作空间";

/// 后端停止信号；应用退出时取出并发送。
struct BackendShutdown(Mutex<Option<oneshot::Sender<()>>>);

/// 调试构建时仓库里的 `back/.env`（编译期路径），便于开发者沿用后端的本地配置。
fn dev_env_file() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../back/.env");
    path.is_file().then_some(path)
}

/// 演示素材目录：优先使用打包进应用的资源，开发时回退到前端工程的 `public/media`。
fn demo_media_dir(app: &tauri::App) -> PathBuf {
    if let Ok(dir) = app.path().resource_dir() {
        let bundled = dir.join("media");
        if bundled.join("ambient.wav").is_file() {
            return bundled;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../public/media")
}

/// 生成后端配置：环境变量与 `.env` 决定供应商和模型，路径与对外地址由桌面端决定。
fn backend_config(app: &tauri::App, port: u16) -> anyhow::Result<Config> {
    let mut config = Config::from_env();
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    // 未显式配置时，数据放在应用数据目录，而不是当前工作目录。
    if std::env::var_os("DATABASE_PATH").is_none() && std::env::var_os("DATA_DIR").is_none() {
        config.database_path = data_dir.join("explaincraft.sqlite3");
    }
    if std::env::var_os("STORAGE_DIR").is_none() && std::env::var_os("DATA_DIR").is_none() {
        config.storage_dir = data_dir.join("storage");
    }
    if std::env::var_os("DEMO_MEDIA_DIR").is_none() {
        config.demo_media_dir = demo_media_dir(app);
    }
    config.bind = format!("127.0.0.1:{port}");
    // 页面来源（tauri.localhost）与后端不同，媒体地址必须是绝对地址。
    config.public_origin = format!("http://127.0.0.1:{port}");
    Ok(config)
}

/// 初始化脚本：在页面任何脚本之前告诉前端后端接口地址。
fn init_script(api_base: &str) -> String {
    let literal = serde_json::to_string(api_base).expect("字符串总能序列化");
    format!("window.__EXPLAINCRAFT_API__ = {literal};")
}

/// 应用启动：读取配置 → 启动内嵌后端 → 创建主窗口。
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // 已存在的环境变量优先；先读应用配置目录，再读开发用的 back/.env。
    if let Ok(dir) = app.path().app_config_dir() {
        let path = dir.join(".env");
        if path.is_file() {
            load_env_file(Some(&path));
        }
    }
    if let Some(path) = dev_env_file() {
        load_env_file(Some(&path));
    }

    // 绑定随机空闲端口，避免与其他程序（或另一个开发中的后端）冲突。
    let listener = StdTcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let config = backend_config(app, port)?;
    let api_base = config.public_base();

    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    app.manage(BackendShutdown(Mutex::new(Some(stop_tx))));
    tauri::async_runtime::spawn(async move {
        let result = async {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            serve(config, listener, async {
                let _ = stop_rx.await;
            })
            .await
        }
        .await;
        if let Err(error) = result {
            tracing::error!(target: "startup", "内嵌后端启动失败：{error:#}");
        }
    });

    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title(WINDOW_TITLE)
        .inner_size(1440.0, 900.0)
        .min_inner_size(1024.0, 680.0)
        .center()
        .initialization_script(init_script(&api_base))
        .build()?;
    tracing::info!(target: "startup", "桌面窗口已创建，接口地址：{api_base}");
    Ok(())
}

/// 桌面端入口（`main.rs` 调用）。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    let app = tauri::Builder::default()
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("启动灵镜桌面应用失败");
    app.run(|app, event| {
        if let RunEvent::Exit = event {
            // 通知后端停止接收新请求；任务状态已持久化，下次启动继续。
            if let Some(state) = app.try_state::<BackendShutdown>() {
                if let Some(stop) = state.0.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = stop.send(());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::init_script;

    #[test]
    fn init_script_escapes_the_api_base() {
        assert_eq!(
            init_script("http://127.0.0.1:5000/api/v1"),
            r#"window.__EXPLAINCRAFT_API__ = "http://127.0.0.1:5000/api/v1";"#
        );
        assert!(init_script("a\"b").contains(r#""a\"b""#));
    }
}
