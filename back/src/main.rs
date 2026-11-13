//! 独立运行的后端服务（`cargo run`）：供前端 Vite 开发服务器代理联调、接口测试使用。
//!
//! 桌面应用不使用这个入口，而是在应用进程内调用 [`explaincraft_backend::serve`]。

use explaincraft_backend::{init_logging, load_env_file, serve, Config};

/// 读取配置 → 绑定 `BIND_ADDR` → 运行到 Ctrl+C。
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();
    // 从当前目录向上查找 back/.env（模型密钥、供应商选择等本地配置）。
    load_env_file(None);
    let config = Config::from_env();
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    serve(config, listener, async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!(target: "startup", "收到停止信号，正在退出。");
    })
    .await
}
