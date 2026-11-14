// 发布版不弹出控制台窗口；调试版保留控制台以查看后端日志。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 自检只执行离线依赖验证；普通启动沿用 Tauri 窗口和单实例行为。
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--self-check") {
        let result = lingjing_desktop_lib::package_self_check();
        let report = match &result {
            Ok(report) => report.to_string(),
            Err(error) => {
                serde_json::json!({"status":"failed","error":error.to_string()}).to_string()
            }
        };
        if let Some(path) = std::env::args_os().nth(2) {
            if let Err(error) = std::fs::write(path, &report) {
                eprintln!("无法写入安装包自检报告：{error}");
                std::process::exit(1);
            }
        }
        println!("{report}");
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    lingjing_desktop_lib::run()
}
