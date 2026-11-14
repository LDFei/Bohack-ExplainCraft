//! 受约束的 SVG/SMIL 动画渲染：本地插值、resvg 光栅化、FFmpeg 编码。
//!
//! 不接受脚本、外部资源、foreignObject 或 animateMotion，避免模型输出访问本机文件。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::providers::{MediaData, MediaOutput, ProviderError, ProviderResult};

/// 一段 SMIL 数值动画。
#[derive(Clone)]
struct Animation {
    /// animateTransform 的类型；普通 animate 为 None。
    transform_type: Option<String>,
    /// 要修改的属性（transform 表示 animateTransform）。
    attribute: String,
    /// 关键帧值。
    values: Vec<String>,
    /// 0～1 的关键帧位置。
    key_times: Vec<f64>,
    /// 开始时间（毫秒）。
    begin_ms: i64,
    /// 单次持续时间（毫秒）。
    duration_ms: i64,
    /// 是否无限重复。
    repeat: bool,
}

/// 解析时长，支持 `ms`、`s` 与纯秒数。
fn duration(text: &str, default_ms: i64) -> i64 {
    let text = text.trim();
    if let Some(value) = text.strip_suffix("ms") {
        value.parse::<f64>().unwrap_or(default_ms as f64) as i64
    } else {
        text.trim_end_matches('s')
            .parse::<f64>()
            .map(|value| (value * 1000.0) as i64)
            .unwrap_or(default_ms)
    }
}

/// 把浮点值压缩为 SVG 数值文本。
fn number(value: f64) -> String {
    let mut formatted = format!("{value:.3}");
    while formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    if formatted.is_empty() || formatted == "-0" {
        "0".into()
    } else {
        formatted
    }
}

/// 在两个数值向量之间线性插值；非数值属性在关键帧之间保持。
fn interpolate(from: &str, to: &str, progress: f64) -> String {
    let parse = |text: &str| -> Option<Vec<f64>> {
        text.split([',', ' ', '\t'])
            .filter(|part| !part.is_empty())
            .map(|part| part.parse::<f64>().ok())
            .collect()
    };
    let color = |text: &str| -> Option<Vec<f64>> {
        let hex = text.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        Some(vec![
            u8::from_str_radix(&hex[0..2], 16).ok()? as f64,
            u8::from_str_radix(&hex[2..4], 16).ok()? as f64,
            u8::from_str_radix(&hex[4..6], 16).ok()? as f64,
        ])
    };
    if let (Some(a), Some(b)) = (color(from), color(to)) {
        let channels: Vec<String> = a
            .iter()
            .zip(&b)
            .map(|(a, b)| format!("{:02x}", (a + (b - a) * progress).round() as u8))
            .collect();
        return format!("#{}", channels.join(""));
    }
    if let (Some(a), Some(b)) = (parse(from), parse(to)) {
        if a.len() == b.len() {
            let separator = if from.contains(',') { "," } else { " " };
            return a
                .iter()
                .zip(&b)
                .map(|(a, b)| number(a + (b - a) * progress))
                .collect::<Vec<_>>()
                .join(separator);
        }
    }
    if progress >= 1.0 {
        to.into()
    } else {
        from.into()
    }
}

/// 计算动画在指定时刻的属性值。
fn value_at(animation: &Animation, time_ms: i64) -> String {
    let first = animation.values.first().map(String::as_str).unwrap_or("");
    let last = animation.values.last().map(String::as_str).unwrap_or(first);
    if time_ms <= animation.begin_ms {
        return first.into();
    }
    let elapsed = time_ms - animation.begin_ms;
    let local = if animation.repeat && animation.duration_ms > 0 {
        elapsed % animation.duration_ms
    } else {
        elapsed.min(animation.duration_ms)
    };
    let position = (local as f64 / animation.duration_ms.max(1) as f64).clamp(0.0, 1.0);
    let mut index = animation
        .key_times
        .iter()
        .position(|key| *key >= position)
        .unwrap_or(animation.values.len().saturating_sub(1));
    if index == 0 {
        index = 1;
    }
    if index >= animation.values.len() {
        return last.into();
    }
    let start = animation.key_times[index - 1];
    let end = animation.key_times[index];
    let progress = ((position - start) / (end - start).max(f64::EPSILON)).clamp(0.0, 1.0);
    let value = interpolate(
        &animation.values[index - 1],
        &animation.values[index],
        progress,
    );
    if let Some(kind) = &animation.transform_type {
        if value
            .trim_start()
            .starts_with(|ch: char| ch.is_alphabetic())
        {
            value
        } else {
            format!("{kind}({value})")
        }
    } else {
        value
    }
}

/// 把 start tag 中的旧属性替换为计算后的属性集合。
fn update_tag(tag: &str, attributes: &BTreeMap<String, String>) -> String {
    let mut output = tag.to_string();
    for (name, value) in attributes {
        let escaped = value.replace('&', "&amp;").replace('"', "&quot;");
        let needle = format!("{name}=\"");
        if let Some(start) = output.find(&needle) {
            let value_start = start + needle.len();
            if let Some(end) = output[value_start..].find('"') {
                output.replace_range(value_start..value_start + end, &escaped);
                continue;
            }
        }
        let insert_at = output
            .rfind("/>")
            .or_else(|| output.rfind('>'))
            .unwrap_or(output.len());
        output.insert_str(insert_at, &format!(" {name}=\"{escaped}\""));
    }
    output
}

/// 在指定时刻计算整份 SVG；拒绝可执行或外部加载内容。
fn frame_svg(source: &str, time_ms: i64) -> ProviderResult<String> {
    let document = roxmltree::Document::parse(source)
        .map_err(|_| ProviderError::invalid("SVG 格式不正确，请重新生成。"))?;
    if source.contains("<script")
        || source.contains("<foreignObject")
        || source.contains("<animateMotion")
        || source.contains("<image")
        || source.contains("javascript:")
        || source.contains("href=\"http")
        || source.contains("href='http")
        || source.contains("url(http")
    {
        return Err(ProviderError::invalid(
            "SVG 不能包含脚本、外链图片或外部资源。",
        ));
    }
    let animations: Vec<roxmltree::Node> = document
        .descendants()
        .filter(|node| {
            matches!(
                node.tag_name().name(),
                "animate" | "animateTransform" | "set"
            )
        })
        .collect();
    let mut edits: BTreeMap<usize, (usize, usize, BTreeMap<String, String>)> = BTreeMap::new();
    for node in &animations {
        let Some(parent) = node.parent() else {
            continue;
        };
        let attribute = if node.tag_name().name() == "animateTransform" {
            "transform"
        } else {
            node.attribute("attributeName").unwrap_or("opacity")
        };
        let values: Option<Vec<String>> = node
            .attribute("values")
            .map(|values| values.split(';').map(|value| value.trim().into()).collect())
            .or_else(|| {
                Some(vec![
                    node.attribute("from").unwrap_or("").into(),
                    node.attribute("to").unwrap_or("").into(),
                ])
            })
            .or_else(|| node.attribute("to").map(|to| vec![to.into(), to.into()]))
            .filter(|values| !values.is_empty());
        let Some(values) = values else {
            continue;
        };
        let key_times: Vec<f64> = node
            .attribute("keyTimes")
            .map(|times| {
                times
                    .split(';')
                    .filter_map(|time| time.trim().parse::<f64>().ok())
                    .collect()
            })
            .unwrap_or_else(|| {
                (0..values.len())
                    .map(|index| index as f64 / values.len().saturating_sub(1).max(1) as f64)
                    .collect()
            });
        if key_times.len() != values.len() {
            continue;
        }
        let animation = Animation {
            transform_type: (attribute == "transform").then(|| {
                if node.tag_name().name() == "animateTransform" {
                    node.attribute("type").unwrap_or("translate").to_string()
                } else {
                    "translate".into()
                }
            }),
            attribute: attribute.into(),
            values,
            key_times,
            begin_ms: duration(node.attribute("begin").unwrap_or("0"), 0),
            duration_ms: duration(node.attribute("dur").unwrap_or("1s"), 1000).max(1),
            repeat: node
                .attribute("repeatCount")
                .is_some_and(|repeat| repeat == "indefinite" || repeat.parse::<f64>().is_ok()),
        };
        let range = parent.range();
        let tag_end = source[range.start..range.end]
            .find('>')
            .map(|offset| range.start + offset + 1)
            .unwrap_or(range.end);
        edits
            .entry(range.start)
            .or_insert_with(|| (range.start, tag_end, BTreeMap::new()))
            .2
            .insert(animation.attribute.clone(), value_at(&animation, time_ms));
    }
    // 组装替换片段：父元素起始标签换成算好的属性，动画节点本身整段删除
    // （动画已经在这里插值成静态属性，交给 resvg 的只剩最终画面）。
    let mut ranges: Vec<(usize, usize, String)> = edits
        .values()
        .map(|(start, end, attributes)| {
            (*start, *end, update_tag(&source[*start..*end], attributes))
        })
        .collect();
    for node in &animations {
        let range = node.range();
        ranges.push((range.start, range.end, String::new()));
    }
    ranges.sort_by_key(|(start, _, _)| *start);
    let mut output = String::with_capacity(source.len() + ranges.len() * 32);
    let mut cursor = 0;
    for (start, end, replacement) in ranges {
        if start < cursor {
            // 理论上不会重叠；保底跳过，避免切出坏结构。
            continue;
        }
        output.push_str(&source[cursor..start]);
        output.push_str(&replacement);
        cursor = end;
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}

/// 加载各平台中文字体，供 SVG 镜头与透明字幕共用。
fn svg_options() -> usvg::Options<'static> {
    let mut options = usvg::Options::default();
    for (path, family) in [
        (
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "Hiragino Sans GB",
        ),
        ("/System/Library/Fonts/STHeiti Medium.ttc", "STHeiti"),
        ("C:\\Windows\\Fonts\\msyh.ttc", "Microsoft YaHei"),
        ("C:\\Windows\\Fonts\\simhei.ttf", "SimHei"),
        (
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "Noto Sans CJK SC",
        ),
        (
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "Noto Sans CJK SC",
        ),
        (
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "WenQuanYi Micro Hei",
        ),
    ] {
        let path = std::path::Path::new(path);
        if path.is_file() {
            options.fontdb_mut().load_font_file(path).ok();
            options.fontdb_mut().set_sans_serif_family(family);
            options.fontdb_mut().set_serif_family(family);
            options.font_family = family.into();
            break;
        }
    }
    options
}

/// 运动检测网格：把每帧缩成 16×16 的亮度网格来比较，成本可忽略。
const MOTION_GRID: usize = 16;

/// 亮度网格的最小波动（0～255）。纯静态 SVG 的每一帧完全相同，波动为 0。
const MOTION_THRESHOLD: f32 = 2.0;

/// 把一帧压成 16×16 的平均亮度网格，用于判断画面是否在动。
fn frame_signature(pixmap: &tiny_skia::Pixmap) -> Vec<f32> {
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let cell_width = width.div_ceil(MOTION_GRID).max(1);
    let cell_height = height.div_ceil(MOTION_GRID).max(1);
    let mut sums = vec![0f64; MOTION_GRID * MOTION_GRID];
    let mut counts = vec![0usize; MOTION_GRID * MOTION_GRID];
    let pixels = pixmap.pixels();
    // 每两个像素取样一次，足够判断有没有运动会，也避免逐像素扫描拖慢渲染。
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let pixel = pixels[y * width + x];
            let luminance = 0.299 * pixel.red() as f64
                + 0.587 * pixel.green() as f64
                + 0.114 * pixel.blue() as f64;
            let cell = (y / cell_height) * MOTION_GRID + (x / cell_width);
            sums[cell] += luminance;
            counts[cell] += 1;
        }
    }
    sums.iter()
        .zip(&counts)
        .map(|(sum, count)| {
            if *count == 0 {
                0.0
            } else {
                (sum / *count as f64) as f32
            }
        })
        .collect()
}

/// 同步渲染并编码一个动画；调用方需放到阻塞线程。
///
/// 返回值里的布尔量表示各帧之间是否存在可见运动（判断依据是缩略亮度网格的波动），
/// 供画面生成阶段识别“模型只画了一张静态图”的情况。
fn render_blocking(
    source: &str,
    duration_ms: i64,
    width: u32,
    height: u32,
    work: &Path,
) -> ProviderResult<(PathBuf, bool)> {
    let fps = 24;
    let frames = ((duration_ms as f64 / 1000.0 * fps as f64).ceil() as i64).clamp(1, 720);
    std::fs::create_dir_all(work)
        .map_err(|_| ProviderError::render_failed("无法创建 SVG 渲染目录"))?;
    let options = svg_options();
    let cells = MOTION_GRID * MOTION_GRID;
    let mut lowest = vec![f32::MAX; cells];
    let mut highest = vec![f32::MIN; cells];
    for frame in 0..frames {
        let time_ms = (frame as i64 * duration_ms / frames.max(1)).min(duration_ms);
        let svg = frame_svg(source, time_ms)?;
        let tree = usvg::Tree::from_str(&svg, &options)
            .map_err(|_| ProviderError::render_failed("SVG 场景解析失败"))?;
        let mut pixmap = tiny_skia::Pixmap::new(width, height)
            .ok_or_else(|| ProviderError::render_failed("SVG 画布尺寸无效"))?;
        let size = tree.size();
        let scale = (width as f32 / size.width()).min(height as f32 / size.height());
        let transform = tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        for (cell, value) in frame_signature(&pixmap).into_iter().enumerate() {
            lowest[cell] = lowest[cell].min(value);
            highest[cell] = highest[cell].max(value);
        }
        pixmap
            .encode_png()
            .map_err(|_| ProviderError::render_failed("SVG 帧编码失败"))
            .and_then(|bytes| {
                std::fs::write(work.join(format!("frame-{frame:06}.png")), bytes)
                    .map_err(|_| ProviderError::render_failed("无法保存 SVG 帧"))
            })?;
    }
    let motion = lowest
        .iter()
        .zip(&highest)
        .map(|(low, high)| high - low)
        .fold(0.0_f32, f32::max);
    let output = work.with_extension("mp4");
    crate::providers::encoding::encode_h264("SVG 动画", |encoder| {
        let mut command =
            Command::new(std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()));
        command
            .args([
                "-y",
                "-loglevel",
                "warning",
                "-framerate",
                &fps.to_string(),
                "-i",
            ])
            .arg(work.join("frame-%06d.png"));
        encoder.apply(&mut command);
        command
            .args([
                "-pix_fmt",
                "yuv420p",
                "-color_range",
                "tv",
                "-r",
                "30",
                "-movflags",
                "+faststart",
            ])
            .arg(&output)
            .status()
    })?;
    if !output.is_file() {
        return Err(ProviderError::render_failed("FFmpeg 编码 SVG 动画失败"));
    }
    Ok((output, motion >= MOTION_THRESHOLD))
}

/// 把一段字幕绘制成全画幅透明 PNG，供 Quercus 按时间区间放到视频轨。
///
/// 不依赖 FFmpeg 的 libass/drawtext 插件；字体加载与 SVG 镜头共用，中文环境一致。
pub fn write_subtitle_overlay(
    text: &str,
    width: u32,
    height: u32,
    output: &Path,
) -> ProviderResult<()> {
    let font_size = (height as f32 * 0.034).clamp(24.0, 44.0);
    let line_height = font_size * 1.35;
    let max_chars = (width as f32 * 0.84 / font_size).floor().max(8.0) as usize;
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for ch in paragraph.chars() {
            if line.chars().count() >= max_chars {
                lines.push(std::mem::take(&mut line));
            }
            line.push(ch);
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        return Err(ProviderError::invalid("字幕内容为空。"));
    }
    let escape = |line: &str| {
        line.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let first_baseline = height as f32 * 0.89 - (lines.len() - 1) as f32 * line_height;
    let tspans = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            format!(
                r#"<tspan x="{}" y="{}">{}</tspan>"#,
                width as f32 / 2.0,
                first_baseline + index as f32 * line_height,
                escape(line)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let box_height = lines.len() as f32 * line_height + font_size * 0.65;
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
          <rect x="{}" y="{}" width="{}" height="{box_height}" rx="{}" fill="#000000" opacity="0.46"/>
          <text text-anchor="middle" font-family="{}" font-size="{font_size}" font-weight="500"
            fill="#ffffff" stroke="#000000" stroke-width="{}" paint-order="stroke">{tspans}</text>
        </svg>"##,
        width as f32 * 0.04,
        first_baseline - font_size * 1.05,
        width as f32 * 0.92,
        font_size * 0.35,
        svg_options().font_family,
        (font_size * 0.055).max(1.2),
    );
    let options = svg_options();
    let tree = usvg::Tree::from_str(&svg, &options)
        .map_err(|error| ProviderError::render_failed(format!("字幕 SVG 解析失败：{error}")))?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| ProviderError::render_failed("字幕画布尺寸无效"))?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let png = pixmap
        .encode_png()
        .map_err(|error| ProviderError::render_failed(format!("字幕 PNG 编码失败：{error}")))?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| ProviderError::render_failed(format!("创建字幕目录失败：{error}")))?;
    }
    std::fs::write(output, png)
        .map_err(|error| ProviderError::render_failed(format!("保存字幕 PNG 失败：{error}")))
}

/// 把 SVG/SMIL 渲染为可交给 Quercus 的镜头 MP4。
pub async fn render(
    source: &str,
    duration_ms: i64,
    width: u32,
    height: u32,
    shot_index: usize,
) -> ProviderResult<MediaOutput> {
    render_animation(source, duration_ms, width, height, shot_index)
        .await
        .map(|(media, _)| media)
}

/// 渲染镜头动画，同时报告画面里是否存在可见运动。
///
/// 模型经常只画一张静态 SVG；调用方可用第二个返回值判断是否需要重画，
/// 而不是把一个不动的画面放进成片。
pub async fn render_animation(
    source: &str,
    duration_ms: i64,
    width: u32,
    height: u32,
    shot_index: usize,
) -> ProviderResult<(MediaOutput, bool)> {
    let work = std::env::temp_dir().join(format!(
        "explaincraft-svg-{}-{}",
        shot_index,
        uuid::Uuid::new_v4()
    ));
    let source = source.to_string();
    let blocking_work = work.clone();
    let (output, has_motion) = tokio::task::spawn_blocking(move || {
        render_blocking(&source, duration_ms, width, height, &blocking_work)
    })
    .await
    .map_err(|_| ProviderError::render_failed("SVG 渲染线程失败"))??;
    let _ = tokio::fs::remove_dir_all(work).await;
    let media = MediaOutput {
        data: MediaData::Path(output),
        file_name: format!("svg-shot-{}.mp4", shot_index + 1),
        mime_type: "video/mp4".into(),
        kind: "video".into(),
        source: "generated".into(),
        width: Some(width as i64),
        height: Some(height as i64),
        duration_ms: Some(duration_ms),
    };
    Ok((media, has_motion))
}

/// 给静态画面补一个缓慢推镜：把整份内容包进 `<g>` 并加 `animateTransform`。
///
/// 模型两次都只画出静态图时用它兜底，保证每个镜头至少在动，不会出现定格画面。
pub fn with_camera_move(source: &str, duration_ms: i64) -> String {
    let duration = format!("{:.3}s", duration_ms.max(1000) as f64 / 1000.0);
    let Some(open) = source.find('>').map(|index| index + 1) else {
        return source.to_string();
    };
    let Some(close) = source.rfind("</svg>") else {
        return source.to_string();
    };
    if close < open {
        return source.to_string();
    }
    let wrapper = format!(
        r#"<g><animateTransform attributeName="transform" type="scale" values="1.02;1.08" dur="{duration}"/>"#
    );
    let mut output = String::with_capacity(source.len() + 160);
    output.push_str(&source[..open]);
    output.push_str(&wrapper);
    output.push_str(&source[open..close]);
    output.push_str("</g>");
    output.push_str(&source[close..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_smil_transform_and_opacity() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect x="0" y="0" width="20" height="20" opacity="0"><animate attributeName="opacity" values="0;1" dur="1s"/><animateTransform attributeName="transform" type="translate" values="0 0;10 20" dur="1s"/></rect></svg>"#;
        let frame = frame_svg(svg, 500).unwrap();
        assert!(frame.contains("opacity=\"0.5\""));
        assert!(frame.contains("transform=\"translate(5 10)\""), "{frame}");
    }

    #[test]
    fn bare_animate_transform_also_gets_a_function_name() {
        let svg = r#"<svg><rect><animate attributeName="transform" values="0 0;10 20" dur="1s"/></rect></svg>"#;
        let frame = frame_svg(svg, 500).unwrap();
        assert!(frame.contains("transform=\"translate(5 10)\""));
    }

    #[test]
    fn rejects_external_svg_content() {
        let error = frame_svg("<svg><script>alert(1)</script></svg>", 0);
        assert!(error.is_err());
    }

    #[tokio::test]
    async fn renders_svg_animation_to_a_real_mp4() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#ddeeff"/><circle cx="12" cy="12" r="8" fill="#336699"><animate attributeName="cx" values="12;52;12" dur="1s"/></circle></svg>"##;
        let output = render(svg, 1000, 64, 64, 0)
            .await
            .expect("SVG 动画应渲染为 MP4");
        let MediaData::Path(path) = output.data else {
            panic!("渲染应返回文件路径")
        };
        assert!(path.is_file());
        assert_eq!(output.kind, "video");
        let _ = std::fs::remove_file(path);
    }

    /// 静态 SVG 会被识别成“没有可见运动”，补上推镜后就能检测到运动。
    #[tokio::test]
    async fn detects_static_artwork_and_adds_camera_move() {
        let still = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 160"><rect width="160" height="160" fill="#eeeeee"/><rect x="20" y="60" width="40" height="40" fill="#337799"/></svg>"##;
        let (media, has_motion) = render_animation(still, 1200, 160, 160, 0)
            .await
            .expect("静态画面仍应渲染成功");
        assert!(!has_motion, "静态 SVG 不应被当成动画");
        let MediaData::Path(static_path) = media.data else {
            panic!("成片路径")
        };
        let _ = std::fs::remove_file(static_path);

        let animated = with_camera_move(still, 1200);
        assert!(animated.contains("<g><animateTransform"));
        assert!(animated.contains("</g></svg>"));
        let (media, has_motion) = render_animation(&animated, 1200, 160, 160, 0)
            .await
            .expect("补推镜后应渲染成功");
        assert!(has_motion, "补了推镜的画面应该检测到运动");
        let MediaData::Path(animated_path) = media.data else {
            panic!("成片路径")
        };
        let _ = std::fs::remove_file(animated_path);
    }

    /// 逐帧画面里的 SMIL 节点会被删掉：动画已经插值成静态属性，
    /// 留下的动画节点只会让渲染器重复解析、刷警告甚至二次变形。
    #[test]
    fn frames_are_baked_and_animation_nodes_removed() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect width="100" height="100" fill="#eeeeee"/><circle cx="10" cy="10" r="6" fill="#336699"><animate attributeName="cx" values="10;90" dur="1s"/><animateTransform attributeName="transform" type="scale" values="1;1.08" dur="1s"/></circle></svg>"##;
        let frame = frame_svg(svg, 500).expect("逐帧 SVG");
        assert!(!frame.contains("<animate"), "{frame}");
        assert!(!frame.contains("</animate"), "{frame}");
        assert!(!frame.contains("animateTransform"), "{frame}");
        // 动画算出来的属性仍在：cx 走了一半，scale 也走了一半。
        assert!(frame.contains("cx=\"50\""), "{frame}");
        assert!(frame.contains("transform=\"scale(1.04)\""), "{frame}");
        // 剩下的仍然是合法 XML。
        roxmltree::Document::parse(&frame).expect("逐帧 SVG 结构完整");
    }

    /// 运镜包装不能丢掉原有内容，也必须保持标签闭合。
    #[test]
    fn camera_move_wraps_content_in_a_group() {
        let source = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>"##;
        let wrapped = with_camera_move(source, 2500);
        assert!(wrapped
            .starts_with(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><g>"##));
        assert!(wrapped.contains("<circle cx=\"5\" cy=\"5\" r=\"4\"/>"));
        assert!(wrapped.ends_with("</g></svg>"));
        assert!(wrapped.contains("dur=\"2.500s\""));
        // 没有 </svg> 的输入原样返回，不产生坏结构。
        assert_eq!(with_camera_move("<svg>", 1000), "<svg>");
    }
}
