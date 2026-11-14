//! 灵镜桌面应用与制作后端：Tauri commands、SQLite、任务编排和模型供应商。
//!
//! 工作台通过 `#[tauri::command]` 访问进程内的 [`AppState`]，没有 TCP 监听器、
//! 独立 Web 服务或 SSE 连接。视频、图片和曲库使用 Tauri 自定义媒体协议。

#![warn(clippy::missing_docs_in_private_items)]

mod api;
mod catalog;
mod commands;
mod config;
mod db;
mod error;
mod media;
mod models;
#[cfg(feature = "quercus")]
mod oak;
mod ops;
mod pipeline;
mod providers;
mod state;
mod svg_animation;
mod util;
mod worker;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::EnvFilter;

pub use crate::config::{AuthMode, Config, ModelAccess};
use crate::state::{AppState, WorkerHandle};

/// 默认日志级别：业务与流程日志为 info；依赖库只输出警告，避免刷屏。
const DEFAULT_LOG_FILTER: &str =
    "info,hyper=warn,tower_http=warn,axum=warn,wgpu_hal=error,wgpu_core=error,naga=error";

/// 初始化终端日志：本地时间、日志类别（api / job / worker）与彩色级别。
pub fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .with_timer(ChronoLocal::new("%m-%d %H:%M:%S%.3f".into()))
        .with_target(true)
        .try_init();
}

/// 读取本地配置文件；已存在的环境变量优先。
pub fn load_env_file(path: Option<&Path>) -> bool {
    let result = match path {
        Some(path) => dotenvy::from_path(path).map(|_| path.to_path_buf()),
        None => dotenvy::dotenv(),
    };
    match result {
        Ok(path) => {
            tracing::info!(target: "startup", "已加载本地配置：{}", path.display());
            true
        }
        Err(error) if error.not_found() => false,
        Err(error) => {
            tracing::error!(
                target: "startup",
                "本地配置 .env 解析失败（含空格或反斜杠的值请用单引号包裹）：{error}"
            );
            false
        }
    }
}

/// 调试构建时的 `src-tauri/.env`，便于开发者沿用本地配置。
fn dev_env_file() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env");
    path.is_file().then_some(path)
}

/// 示例与曲库素材目录：优先使用应用资源，开发时回退到共享媒体目录。
fn demo_media_dir(app: &tauri::App) -> PathBuf {
    if let Ok(dir) = app.path().resource_dir() {
        let bundled = dir.join("media");
        if bundled.join("ambient.wav").is_file() {
            return bundled;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../shared/public/media")
}

/// 自定义媒体协议的源；Windows WebView2 使用 Tauri 的 HTTP 映射形式。
fn media_origin() -> &'static str {
    if cfg!(windows) {
        "http://explaincraft.localhost"
    } else {
        "explaincraft://localhost"
    }
}

/// 生成进程内后端配置，数据放在系统应用数据目录。
fn backend_config(app: &tauri::App) -> anyhow::Result<Config> {
    let mut config = Config::from_env();
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    if std::env::var_os("DATABASE_PATH").is_none() && std::env::var_os("DATA_DIR").is_none() {
        config.database_path = data_dir.join("explaincraft.sqlite3");
    }
    if std::env::var_os("STORAGE_DIR").is_none() && std::env::var_os("DATA_DIR").is_none() {
        config.storage_dir = data_dir.join("storage");
    }
    if std::env::var_os("DEMO_MEDIA_DIR").is_none() {
        config.demo_media_dir = demo_media_dir(app);
    }
    config.settings_file = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env")
    } else {
        app.path().app_config_dir()?.join(".env")
    };
    config.public_origin = media_origin().into();
    Ok(config)
}

/// 创建共享业务状态并启动可恢复的后台 worker。
fn build_state(config: Config) -> anyhow::Result<AppState> {
    let providers = providers::Providers::from_config(&config)?;
    let model_config = Arc::new(ArcSwap::from_pointee(config.clone()));
    if !config.demo_media_dir.is_dir() {
        tracing::warn!(
            target: "startup",
            "演示素材目录不存在：{}（可设置 DEMO_MEDIA_DIR），演示生成会失败",
            config.demo_media_dir.display()
        );
    }
    let state = AppState {
        db: Arc::new(db::Db::open(&config.database_path)?),
        media: Arc::new(media::Media::new(&config)?),
        providers: Arc::new(ArcSwap::from_pointee(providers)),
        model_config,
        config: Arc::new(config),
        catalog: Arc::new(catalog::Catalog::default()),
        worker: WorkerHandle::default(),
    };
    worker::spawn(state.clone());
    Ok(state)
}

/// 主窗口标题。
const WINDOW_TITLE: &str = "灵镜 · AI 视频创作空间";

/// 注册前端可调用的全部 commands；生产应用与 IPC 集成测试共用同一份清单。
fn command_handler<R: tauri::Runtime>(
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::capabilities,
        commands::list_projects,
        commands::create_project,
        commands::get_project,
        commands::rename_project,
        commands::delete_project,
        commands::send_message,
        commands::patch_draft,
        commands::list_assets,
        commands::upload_asset,
        commands::get_asset,
        commands::get_job,
        commands::cancel_job,
        commands::retry_job,
        commands::answer_job,
        commands::list_versions,
        commands::get_version,
        commands::export_video,
        commands::download_asset,
        commands::project_events,
        commands::get_model_settings,
        commands::update_model_settings,
        commands::list_models,
        commands::list_voices,
    ]
}

/// 初始化共享状态并创建工作台窗口。
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(dir) = app.path().app_config_dir() {
        let path = dir.join(".env");
        if path.is_file() {
            load_env_file(Some(&path));
        }
    }
    if let Some(path) = dev_env_file() {
        load_env_file(Some(&path));
    }
    // worker 在创建时调用 tokio::spawn，必须先进入 Tauri 的异步运行时。
    let config = backend_config(app)?;
    let state = tauri::async_runtime::block_on(async move { build_state(config) })?;
    app.manage(state);

    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("studio.html".into()))
        .title(WINDOW_TITLE)
        .inner_size(1440.0, 900.0)
        .min_inner_size(1024.0, 680.0)
        .center()
        .build()?;
    tracing::info!(target: "startup", "Tauri 工作台已创建，业务调用使用 commands");
    Ok(())
}

/// 桌面端入口（`main.rs` 调用）。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    let app = tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("explaincraft", |ctx, request, responder| {
            let state = ctx.app_handle().state::<AppState>().inner().clone();
            tauri::async_runtime::spawn(async move {
                responder.respond(commands::media_response(state, request).await);
            });
        })
        .invoke_handler(command_handler())
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("启动灵镜桌面应用失败");
    app.run(|_, _| {});
}

#[cfg(test)]
mod tests {
    use super::media_origin;

    #[test]
    fn media_origin_matches_the_webview_platform() {
        if cfg!(windows) {
            assert_eq!(media_origin(), "http://explaincraft.localhost");
        } else {
            assert_eq!(media_origin(), "explaincraft://localhost");
        }
    }
}
