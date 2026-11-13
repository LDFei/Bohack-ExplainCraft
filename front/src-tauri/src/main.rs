// 发布版不弹出控制台窗口；调试版保留控制台以查看后端日志。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 桌面端入口：按 Tauri 约定只调用库中的 `run()`。
fn main() {
    lingjing_desktop_lib::run()
}
