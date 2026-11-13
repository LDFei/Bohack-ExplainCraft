//! 通用小工具：ID 生成、时间格式与画面尺寸换算。

use chrono::{DateTime, SecondsFormat, Utc};

/// 生成带前缀的不透明 ID，例如 `prj_3f2a…`。
///
/// 前缀只用于排查问题时辨认类型，前端不得据此推导含义（文档 2 节）。
pub fn make_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// 日志中使用的短 ID：保留前缀和前 8 位，例如 `job_3f2a1b9c`，足以在同一时段内区分对象。
pub fn short(id: &str) -> &str {
    match id.find('_') {
        Some(at) => &id[..(at + 9).min(id.len())],
        None => &id[..8.min(id.len())],
    }
}

/// 日志中的文本预览：最多保留 `max` 个字符并去掉换行，避免长文本刷屏。
pub fn preview(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

/// 毫秒耗时的可读形式：`850ms` / `3.2s`。
pub fn elapsed(started: std::time::Instant) -> String {
    let ms = started.elapsed().as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

/// 生成响应中的 `request_id`，便于前后端对照日志。
pub fn request_id() -> String {
    make_id("req")
}

/// 当前 UTC 时间。集中在这里，方便以后在测试中替换时钟。
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// 格式化为 RFC 3339（UTC、毫秒精度、`Z` 结尾），与接口约定一致。
pub fn iso(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// 当前时间的 RFC 3339 字符串。
pub fn now_iso() -> String {
    iso(now())
}

/// 解析 RFC 3339 时间；格式不正确时返回 `None`。
pub fn parse_iso(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// 画面比例对应的 720p 尺寸 `(宽, 高)`，短边固定 720（文档 5.1）。
///
/// 未知比例按竖屏 9:16 处理。
pub fn dimensions(aspect_ratio: &str) -> (i64, i64) {
    match aspect_ratio {
        "16:9" => (1280, 720),
        "1:1" => (720, 720),
        _ => (720, 1280),
    }
}
