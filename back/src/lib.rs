//! 灵镜 · AI 视频创作空间 —— Rust 后端库。
//!
//! 同一套后端有两种运行方式：
//! - 独立服务：`src/main.rs`（`cargo run`），供 Vite 开发服务器代理联调；
//! - 桌面应用：`front/src-tauri` 在应用进程内调用 [`serve`]，监听本机随机端口。
//!
//! 分层：
//! - [`api`]：HTTP 接口与 SSE（`front/docs/API.md`）
//! - [`db`]：SQLite 持久化，状态与事件在同一事务提交
//! - [`worker`] + [`pipeline`]：可恢复的后台任务与制作流程编排
//! - [`providers`]：四个模型（llm1～llm4）与渲染器的适配接口
//! - [`media`]：文件存储、签名地址、媒体解析
//!
//! 运行 `cargo clippy` 时要求所有条目都有文档注释。

#![warn(clippy::missing_docs_in_private_items)]

mod api;
mod catalog;
mod config;
mod db;
mod error;
mod media;
mod models;
mod ops;
mod pipeline;
mod providers;
mod state;
mod util;
mod worker;

use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use tower_http::cors::CorsLayer;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::EnvFilter;

pub use crate::config::{AuthMode, Config, ModelAccess};
use crate::state::{AppState, WorkerHandle};

/// 默认日志级别：业务与流程日志为 info；依赖库只输出警告，避免刷屏。
///
/// 可用 `RUST_LOG` 覆盖，例如 `RUST_LOG=debug` 额外打印所有 GET 请求与 SSE 连接。
/// wgpu / naga 是 quercus 渲染用的图形库，它们的性能提示（如 GLES 像素传输同步）会大量刷屏，只保留错误。
const DEFAULT_LOG_FILTER: &str =
    "info,hyper=warn,tower_http=warn,axum=warn,wgpu_hal=error,wgpu_core=error,naga=error";

/// 初始化终端日志：本地时间、日志类别（http / api / job / worker）与彩色级别。
///
/// 可以重复调用：已经初始化过时直接返回（桌面端与独立服务共用）。
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

/// 读取本地配置文件（`.env`，模型密钥、供应商选择等）。已存在的环境变量优先。
///
/// `path` 为 `None` 时从当前目录向上查找 `.env`。需在 [`init_logging`] 之后调用，结果写入日志。
/// 返回是否成功加载了文件。
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
        // 没有 .env 是正常情况（全部使用默认值或系统环境变量）。
        Err(error) if error.not_found() => false,
        // 解析失败时出错行之后的配置都不会生效，必须明确提示。
        Err(error) => {
            tracing::error!(
                target: "startup",
                "本地配置 .env 解析失败（含空格或反斜杠的值请用单引号包裹）：{error}"
            );
            false
        }
    }
}

/// 启动时打印运行配置，便于确认连的是哪个数据库、用的是哪些供应商。
fn log_startup(config: &Config) {
    let media_dir = std::fs::canonicalize(&config.demo_media_dir)
        .unwrap_or_else(|_| config.demo_media_dir.clone());
    tracing::info!(target: "startup", "========== 灵镜后端 ==========");
    tracing::info!(target: "startup", "数据库：{}", config.database_path.display());
    tracing::info!(target: "startup", "文件存储：{}", config.storage_dir.display());
    // Windows 规范化路径带有 `\\?\` 前缀，打印时去掉以便阅读。
    let media_text = media_dir.display().to_string();
    let media_text = media_text.trim_start_matches(r"\\?\");
    tracing::info!(target: "startup", "演示素材：{media_text}");
    tracing::info!(
        target: "startup",
        "模型供应商：llm1(文案/分镜/剪辑)={} llm2(画面)={} llm3(背景音)={} llm4(配音)={} 渲染={}",
        config.provider_llm1,
        config.provider_llm2,
        config.provider_llm3,
        config.provider_llm4,
        config.provider_renderer,
    );
    tracing::info!(
        target: "startup",
        "模型：llm1={:?} llm2={:?} llm3={:?}，ComfyUI={}",
        config.llm1,
        config.llm2,
        config.llm3,
        config.comfyui_url,
    );
    tracing::info!(
        target: "startup",
        "鉴权模式：{:?}，并发任务上限：{}，日志级别可用 RUST_LOG 调整（debug 显示全部请求）",
        config.auth_mode,
        config.max_concurrent_jobs,
    );
}

/// 在已绑定的监听器上运行后端，直到 `shutdown` 完成。
///
/// 流程：注册供应商 → 打开数据库 → 启动 worker（恢复上次中断的任务）→ 处理 HTTP 请求。
/// 必须在 tokio 运行时中调用（桌面端使用 `tauri::async_runtime`）。
///
/// # 错误
/// 供应商名称写错、数据库无法打开、存储目录无法创建时返回错误。
pub async fn serve(
    config: Config,
    listener: tokio::net::TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    log_startup(&config);
    // 供应商名称写错时直接启动失败，避免运行到一半才报错。
    let providers = providers::Providers::from_config(&config)?;
    if !config.demo_media_dir.is_dir() {
        tracing::warn!(
            target: "startup",
            "演示素材目录不存在：{}（可设置 DEMO_MEDIA_DIR），演示生成会失败",
            config.demo_media_dir.display()
        );
    }
    let db = Arc::new(db::Db::open(&config.database_path)?);
    let media = Arc::new(media::Media::new(&config)?);
    let address = listener.local_addr()?;
    let state = AppState {
        config: Arc::new(config.clone()),
        db,
        media,
        providers,
        catalog: Arc::new(catalog::Catalog::default()),
        worker: WorkerHandle::default(),
    };
    worker::spawn(state.clone());

    // 允许跨域：桌面端页面来源（tauri.localhost）与后端端口不同；Vite 开发期也可直连。
    let app = api::router(state)
        .layer(axum::middleware::from_fn(api::access_log))
        .layer(CorsLayer::permissive());
    tracing::info!(
        target: "startup",
        "后端已启动，等待请求：http://{address}{}",
        config.public_prefix
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    tracing::info!(target: "startup", "后端已停止；未完成的任务会在下次启动时继续。");
    Ok(())
}
