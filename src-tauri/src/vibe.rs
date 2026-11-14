//! Vibe 深色几何知识动画：模型只填写语义，字体、布局、配色与运动由固定代码保证。
use crate::{
    models::Shot,
    providers::{MediaOutput, ProviderResult},
    rhythm::Rhythm,
};
use serde::Deserialize;

/// 知识动画的受控语义布局；不运行模型提供的任意代码。
#[derive(Default, Deserialize)]
#[serde(default)]
struct Scene {
    /// hook / comparison / mechanism / cycle / conclusion。
    kind: String,
    /// 最多两行的主标题。
    title: String,
    /// 几何节点标签，不包含凭空编造的数值。
    labels: Vec<String>,
    /// 图形的解释或示意声明。
    note: String,
}

/// 把结构化视觉规范转换为审阅用的人类描述。
pub fn describe(raw: &str) -> String {
    serde_json::from_str::<Scene>(raw)
        .map(|s| format!("{} · {}", s.title, s.labels.join(" → ")))
        .unwrap_or_else(|_| raw.into())
}

/// 所有普通主题默认使用 Vibe；Shotcraft 模板显式保留自己的风格。
pub fn enabled(settings: &crate::models::Settings, topic: &str) -> bool {
    settings.render_profile == "vibe" && !topic.contains("[template:shotcraft-")
}

/// 所有内容在进入 SVG 前转义，避免文字成为标签或脚本。
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
/// 文本按汉字近似宽度换行；保留全部字幕，不将溢出内容截掉。
fn lines(text: &str, columns: usize) -> Vec<String> {
    let mut out = vec![];
    let mut line = String::new();
    let mut width = 0.;
    for ch in text.chars() {
        if ch == '\n' || width + if ch.is_ascii() { 0.55 } else { 1. } > columns as f64 {
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            width = 0.;
        }
        if ch != '\n' {
            line.push(ch);
            width += if ch.is_ascii() { 0.55 } else { 1. };
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}
/// 将标题限制为适合画面的短句；完整解说在独立的分页字幕中保留。
fn heading(text: &str) -> String {
    text.split(['，', '。', '？', '！', '：', '\n'])
        .find(|s| !s.trim().is_empty())
        .unwrap_or("看懂背后的原理")
        .chars()
        .take(22)
        .collect()
}
/// 构造可由 resvg 和浏览器共同执行的入场动画。
fn enter(at: i64) -> String {
    format!("<animate attributeName=\"opacity\" values=\"0;1\" dur=\"260ms\" begin=\"{at}ms\" fill=\"freeze\"/>")
}
/// 以同一字体系统绘制文本，所有位置以 720x1280 设计坐标计算。
fn text(x: f64, y: f64, size: f64, color: &str, value: &str, anchor: &str) -> String {
    format!("<text x=\"{x}\" y=\"{y}\" font-size=\"{size}\" font-weight=\"700\" fill=\"{color}\" text-anchor=\"{anchor}\">{}</text>",escape(value))
}

/// 将实际拍点/起音转换为确定性的脉冲关键帧，不用固定周期冒充音乐分析。
fn pulse(attribute: &str, base: f64, peak: f64, beats: &[i64], duration: i64) -> String {
    if beats.is_empty() {
        return String::new();
    }
    let mut points = vec![(0, base), (duration, base)];
    for beat in beats {
        for (t, v) in [(*beat, base), (*beat + 50, peak), (*beat + 180, base)] {
            if t >= 0 && t < duration {
                points.push((t, v));
            }
        }
    }
    points.sort_by_key(|p| p.0);
    points.dedup_by_key(|p| p.0);
    let times = points
        .iter()
        .map(|p| format!("{:.6}", p.0 as f64 / duration as f64))
        .collect::<Vec<_>>()
        .join(";");
    let values = points
        .iter()
        .map(|p| p.1.to_string())
        .collect::<Vec<_>>()
        .join(";");
    format!("<animate attributeName=\"{attribute}\" values=\"{values}\" keyTimes=\"{times}\" dur=\"{duration}ms\" fill=\"freeze\"/>")
}
/// 明确插入的本地素材也保留深色外框、大字标题和字幕，避免整片风格突然切换。
pub fn reference_svg(narration: &str, width: u32, height: u32) -> String {
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 720 1280\"><g font-family=\"PingFang SC,Microsoft YaHei,Noto Sans CJK SC,sans-serif\">{}
    <rect x=\"52\" y=\"408\" width=\"616\" height=\"568\" rx=\"6\" fill=\"none\" stroke=\"#78F0D0\" stroke-width=\"2\"/>",text(52.,79.,17.,"#8594B2","VIBE · 参考素材","start"));
    for (i, line) in lines(&heading(narration), 11).iter().take(2).enumerate() {
        svg.push_str(&text(
            52.,
            206. + i as f64 * 76.,
            58.,
            "#F4F7FF",
            line,
            "start",
        ));
    }
    svg.push_str("</g></svg>");
    svg
}

/// 从语义规范生成确定性的 SVG；同一输入产生相同画面，不依赖模型绘图能力。
pub fn scene_svg(
    shot: &Shot,
    index: usize,
    count: usize,
    rhythm: Option<&Rhythm>,
    offset: i64,
    style: &str,
    subtitles: bool,
) -> String {
    let mut scene: Scene = serde_json::from_str(&shot.visual_prompt).unwrap_or_default();
    if scene.title.is_empty() {
        scene.title = heading(&shot.narration);
    }
    if scene.kind.is_empty() {
        scene.kind = if index == 0 {
            "hook"
        } else if index + 1 == count {
            "conclusion"
        } else {
            ["mechanism", "comparison", "cycle"][index % 3]
        }
        .into();
    }
    if scene.labels.is_empty() {
        scene.labels = shot
            .narration
            .split(['，', '。', '；', '：'])
            .filter(|x| !x.trim().is_empty())
            .take(3)
            .map(heading)
            .collect();
    }
    // 互斥概率不能画成因果链；用并列结果表达，保留模型给出的真实数值。
    if scene
        .labels
        .iter()
        .take(2)
        .filter(|s| s.contains('%') || s.contains('％'))
        .count()
        == 2
    {
        scene.kind = "comparison".into();
    }
    scene.labels.truncate(4);
    let accent = match style {
        "warm" => "#FFD07A",
        "cinematic" => "#B9A6FF",
        "energetic" => "#FF8B94",
        _ => "#78F0D0",
    };
    let beats: Vec<i64> = rhythm
        .map(|r| {
            r.positions(offset + shot.duration_ms)
                .into_iter()
                .filter(|b| *b >= offset && *b < offset + shot.duration_ms)
                .map(|b| b - offset)
                .collect()
        })
        .unwrap_or_default();
    let event = |n: usize| {
        beats
            .get(n)
            .copied()
            .unwrap_or(450 + n as i64 * 600)
            .min(shot.duration_ms.saturating_sub(600))
            .max(0)
    };
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"720\" height=\"1280\" viewBox=\"0 0 720 1280\"><rect width=\"720\" height=\"1280\" fill=\"#0B1120\"/><g font-family=\"PingFang SC,Microsoft YaHei,Noto Sans CJK SC,sans-serif\">");
    for x in (0..720).step_by(60) {
        svg.push_str(&format!(
            "<path d=\"M{x} 0V1280\" stroke=\"#172236\" stroke-width=\"1\"/>"
        ));
    }
    for y in (0..1280).step_by(60) {
        svg.push_str(&format!(
            "<path d=\"M0 {y}H720\" stroke=\"#172236\" stroke-width=\"1\"/>"
        ));
    }
    svg.push_str(&text(52., 79., 17., "#8594B2", "VIBE · 知识图解", "start"));
    svg.push_str(&text(
        668.,
        79.,
        17.,
        "#8594B2",
        &format!("{:02} / {:02}", index + 1, count),
        "end",
    ));
    svg.push_str(&format!(
        "<rect x=\"52\" y=\"110\" width=\"64\" height=\"5\" rx=\"2\" fill=\"{accent}\"/>"
    ));
    let title = lines(&scene.title, 11);
    let title_size = if title.len() > 2 { 48. } else { 58. };
    svg.push_str(&format!("<g opacity=\"0\">{}", enter(0)));
    for (i, line) in title.iter().take(3).enumerate() {
        svg.push_str(&text(
            52.,
            206. + i as f64 * 76.,
            title_size,
            "#F4F7FF",
            line,
            "start",
        ));
    }
    svg.push_str("</g>");
    if scene.kind == "hook" || scene.kind == "conclusion" {
        svg.push_str(&format!("<g opacity=\"0\">{}<circle cx=\"360\" cy=\"620\" r=\"166\" fill=\"none\" stroke=\"#253650\" stroke-width=\"2\"/><circle cx=\"360\" cy=\"620\" r=\"126\" fill=\"#101E30\" stroke=\"{accent}\" stroke-width=\"3\">{}</circle>",enter(event(0)),pulse("r",120.,132.,&beats,shot.duration_ms)));

        let label = scene
            .labels
            .first()
            .map(|s| heading(s))
            .unwrap_or_else(|| scene.title.clone());
        for (i, l) in lines(&label, 7).iter().take(3).enumerate() {
            svg.push_str(&text(360., 600. + i as f64 * 38., 29., accent, l, "middle"));
        }
        svg.push_str("</g>");
    } else if scene.kind == "comparison" {
        for (i, label) in scene.labels.iter().take(2).enumerate() {
            let x = 52. + i as f64 * 322.;
            svg.push_str(&format!("<g opacity=\"0\">{}<rect x=\"{x}\" y=\"466\" width=\"294\" height=\"300\" rx=\"20\" fill=\"#101C2D\" stroke=\"{}\" stroke-width=\"2\"/>",enter(event(i)),if i==0{"#34435A"}else{accent}));
            svg.push_str(&text(
                x + 26.,
                514.,
                18.,
                "#8594B2",
                if i == 0 { "情形 A" } else { "情形 B" },
                "start",
            ));
            for (j, l) in lines(label, 7).iter().take(4).enumerate() {
                svg.push_str(&text(
                    x + 147.,
                    584. + j as f64 * 40.,
                    29.,
                    "#F4F7FF",
                    l,
                    "middle",
                ));
            }
            svg.push_str("</g>");
        }
    } else {
        for (i, label) in scene.labels.iter().enumerate() {
            let y = 438. + i as f64 * 112.;
            if i > 0 {
                svg.push_str(&format!("<path d=\"M360 {}V{}m-6 -8l6 8 6 -8\" fill=\"none\" stroke=\"{accent}\" stroke-width=\"2\"/>",y-32.,y-8.));
            }
            svg.push_str(&format!("<g opacity=\"0\">{}<rect x=\"100\" y=\"{y}\" width=\"520\" height=\"82\" rx=\"14\" fill=\"#101E30\" stroke=\"#34435A\" stroke-width=\"2\"/>",enter(event(i))));
            svg.push_str(&text(
                126.,
                y + 48.,
                20.,
                accent,
                &format!("{:02}", i + 1),
                "start",
            ));
            svg.push_str(&text(
                380.,
                y + 50.,
                27.,
                "#F4F7FF",
                &label.chars().take(17).collect::<String>(),
                "middle",
            ));
            svg.push_str("</g>");
        }
    }
    let subtitle = if subtitles {
        lines(&shot.narration, 21)
    } else {
        vec![]
    };
    let pages = (subtitle.len() + 1) / 2;
    let page_ms = shot.duration_ms / pages.max(1) as i64;
    for (page, pair) in subtitle.chunks(2).enumerate() {
        let start = page as i64 * page_ms;
        let end = if page + 1 == pages {
            shot.duration_ms
        } else {
            start + page_ms
        };
        svg.push_str(&format!("<g opacity=\"0\"><animate attributeName=\"opacity\" values=\"0;1;1;0\" keyTimes=\"0;0.02;0.98;1\" begin=\"{start}ms\" dur=\"{}ms\" fill=\"freeze\"/>",end-start));
        for (j, l) in pair.iter().enumerate() {
            svg.push_str(&text(
                360.,
                1050. + j as f64 * 43.,
                29.,
                "#E5EBF8",
                l,
                "middle",
            ));
        }
        svg.push_str("</g>");
    }
    let note = if scene.note.is_empty() {
        "几何示意 · 以解说为准"
    } else {
        &scene.note
    };
    svg.push_str(&text(
        52.,
        1192.,
        16.,
        "#8594B2",
        &note.chars().take(35).collect::<String>(),
        "start",
    ));
    svg.push_str(&format!("<rect x=\"52\" y=\"1223\" width=\"616\" height=\"3\" fill=\"#253650\"/><rect x=\"52\" y=\"1223\" width=\"0\" height=\"3\" fill=\"{accent}\"><animate attributeName=\"width\" from=\"0\" to=\"616\" dur=\"{}ms\" fill=\"freeze\"/></rect></g></svg>",shot.duration_ms));
    svg
}

/// 使用固定视觉系统输出真实镜头；SVG 与 HTML 选择保持一致的内容和风格。
pub async fn render(
    shot: &Shot,
    index: usize,
    count: usize,
    settings: &crate::models::Settings,
    rhythm: Option<&Rhythm>,
    offset: i64,
    subtitles: bool,
) -> ProviderResult<MediaOutput> {
    let svg = scene_svg(
        shot,
        index,
        count,
        rhythm,
        offset,
        &settings.style_id,
        subtitles,
    );
    let (w, h) =
        crate::util::output_dimensions(&settings.aspect_ratio, &settings.output_resolution);
    // 竖屏设计坐标按 contain 缩放到其他画幅，保持字形比例，不拉伸。
    let svg = svg.replacen(
        "width=\"720\" height=\"1280\"",
        &format!("width=\"{w}\" height=\"{h}\""),
        1,
    );
    if settings.visual_format == "html" {
        crate::html_animation::render(
            &format!("<style>body{{background:#0B1120}}svg{{width:100%;height:100%}}</style>{svg}"),
            shot.duration_ms,
            w as u32,
            h as u32,
            index,
        )
        .await
    } else {
        crate::svg_animation::render(&svg, shot.duration_ms, w as u32, h as u32, index).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// 标准模板固定深色与大字；恶意文字不成为代码，完整字幕仍在文档里。
    #[test]
    fn style_is_deterministic_and_escapes_content() {
        let s:Shot=serde_json::from_value(serde_json::json!({"id":"s","duration_ms":6000,"narration":"为什么概率会欺骗直觉？<script>不执行</script>","visual_prompt":"{}"})).unwrap();
        let a = scene_svg(&s, 0, 3, None, 0, "minimal", true);
        assert_eq!(a, scene_svg(&s, 0, 3, None, 0, "minimal", true));
        assert!(a.contains("#0B1120"));
        assert!(a.contains("font-size=\"58\""));
        assert!(!a.contains("<script>"));
        assert!(a.contains("&lt;script&gt;"));
    }
}
