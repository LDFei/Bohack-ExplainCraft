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
    /// v3 受约束图解指令；缺省时继续使用上面的五类固定布局。
    visual: Option<Visual>,
}

/// 受约束的图解指令：模型只描述对象、状态变化、推理步骤与结论。
///
/// 坐标、时序、字体和运动全部由本文件计算；这里不保存也不执行任何模型生成的代码，
/// 因此非法规格只会回落到固定布局，不会让画面脚本访问本机资源。
#[derive(Default, Deserialize, Clone)]
#[serde(default)]
struct Visual {
    /// `search`：真实算法轨迹；`states`：对象状态演变。
    #[serde(rename = "type")]
    kind: String,
    /// 搜索空间，如 `{"from":1,"to":100}`。区间与目标必须由模型给出真实值。
    domain: Option<Domain>,
    /// 搜索目标值。
    target: Option<i64>,
    /// 状态演变：每个状态是一步知识推理。
    states: Vec<VisualState>,
    /// 动画最终要呈现的结论。
    conclusion: String,
}

/// 搜索空间取值区间。
#[derive(Default, Deserialize, Clone)]
#[serde(default)]
struct Domain {
    from: i64,
    to: i64,
}

/// 一个可审阅的知识状态：这一步有哪些对象、对应哪条推理。
#[derive(Default, Deserialize, Clone)]
#[serde(default)]
struct VisualState {
    /// 该状态对应的知识推理步骤，显示为画面说明。
    caption: String,
    /// 该状态中的对象。
    objects: Vec<VisualObject>,
}

/// 状态中的一个对象；只有文字与强调级别，没有坐标。
#[derive(Default, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct VisualObject {
    /// 对象文字。
    text: String,
    /// `accent` / `plain` / `dim`。
    emphasis: String,
}

/// 把结构化视觉规范转换为审阅用的人类描述。
pub fn describe(raw: &str) -> String {
    if let Some(plan) = crate::scene_plan::from_prompt(raw) {
        return format!(
            "{} · {} · {}",
            plan.objective,
            plan.visual_concept,
            plan.events
                .iter()
                .map(|e| e.reasoning.as_str())
                .collect::<Vec<_>>()
                .join(" → ")
        );
    }
    serde_json::from_str::<Scene>(raw)
        .map(|s| {
            // v3 指令：审阅时展示真实推理链，而不是对象的原始 JSON。
            if let Some(trace) = s.visual.as_ref().and_then(|v| v.search_trace()) {
                let steps = trace
                    .steps
                    .iter()
                    .map(|step| step.caption.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ");
                return format!("{} · {}", s.title, steps);
            }
            // 状态演变同样展开每一步推理，审阅时能看到对象如何推进。
            if let Some(visual) = s.visual.as_ref().filter(|v| v.kind == "states") {
                let steps: Vec<&str> = visual
                    .states
                    .iter()
                    .map(|state| state.caption.trim())
                    .filter(|caption| !caption.is_empty())
                    .collect();
                if !steps.is_empty() {
                    return format!("{} · {}", s.title, steps.join(" → "));
                }
                let objects: Vec<&str> = visual
                    .states
                    .iter()
                    .flat_map(|state| state.objects.iter())
                    .map(|object| object.text.trim())
                    .filter(|text| !text.is_empty())
                    .collect();
                if !objects.is_empty() {
                    return format!("{} · {}", s.title, objects.join(" → "));
                }
            }
            format!("{} · {}", s.title, s.labels.join(" → "))
        })
        .unwrap_or_else(|_| raw.into())
}

impl Visual {
    /// 用本地算法真实计算搜索轨迹；参数非法时返回 `None`，让画面回到固定模板。
    fn search_trace(&self) -> Option<Trace> {
        if self.kind != "search" {
            return None;
        }
        let domain = self.domain.as_ref()?;
        let target = self.target?;
        let size = domain.to.checked_sub(domain.from)?.checked_add(1)?;
        if size < 2 || size > MAX_SEARCH_CELLS || target < domain.from || target > domain.to {
            return None;
        }
        let mut lo = 0;
        let mut hi = size - 1;
        let mut steps = Vec::new();
        // 每一步都由真实中点和真实比较结果生成，不采用模型写出的过程。
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let Some(value) = self.domain.as_ref().map(|d| d.from + mid) else {
                return None;
            };
            let comparison = value.cmp(&target);
            steps.push(TraceStep {
                lo,
                hi,
                mid,
                value,
                caption: format!(
                    "取中点 {}，{} {} {}",
                    value,
                    value,
                    if comparison == std::cmp::Ordering::Equal {
                        "="
                    } else if comparison == std::cmp::Ordering::Less {
                        "<"
                    } else {
                        ">"
                    },
                    target
                ),
                verdict: comparison,
            });
            if steps.len() > 32 {
                return None;
            }
            match comparison {
                std::cmp::Ordering::Equal => break,
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid - 1,
            }
        }
        Some(Trace {
            from: domain.from,
            size,
            target,
            steps,
        })
    }
}

/// 单个搜索空间的格子上限，避免模型给出超大范围拖慢逐帧渲染。
const MAX_SEARCH_CELLS: i64 = 200;

/// 真实执行出的搜索轨迹。
struct Trace {
    from: i64,
    size: i64,
    target: i64,
    steps: Vec<TraceStep>,
}

/// 一步真实比较。
struct TraceStep {
    lo: i64,
    hi: i64,
    mid: i64,
    value: i64,
    caption: String,
    verdict: std::cmp::Ordering,
}

/// 所有普通主题默认使用 Vibe；Shotcraft 模板显式保留自己的风格。
pub fn enabled(settings: &crate::models::Settings, topic: &str) -> bool {
    settings.render_profile == "vibe" && !topic.contains("[template:shotcraft-")
}

/// 路由与渲染共享真实参数校验；有目标字段不代表搜索范围可计算。
pub fn valid_search(raw: &str) -> bool {
    serde_json::from_str::<Scene>(raw)
        .ok()
        .and_then(|s| s.visual)
        .and_then(|v| v.search_trace())
        .is_some()
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
    let w = width as f64;
    let h = height as f64;
    let size = w.min(h) * 0.06;
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><g font-family=\"Hiragino Sans GB,Microsoft YaHei,sans-serif\">{}<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#78F0D0\" stroke-width=\"2\"/></g></svg>", text(w*0.08,h*0.18,size,"#F4F7FF",&heading(narration),"start"),w*0.08,h*0.32,w*0.84,h*0.44)
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
    // v3：模型提供了受约束图解规格时，按数据生成画面；规格非法才回落到固定布局。
    if let Some(visual) = scene.visual.as_ref() {
        if let Some(svg) = data_svg(&scene, visual, shot, accent, subtitles, &beats) {
            return svg;
        }
        // 图解规格不完整或状态不足时回落固定布局；记录原因，避免静默降级。
        tracing::warn!(
            target: "job",
            "受控图解规格无法渲染（type={}），回退固定布局",
            visual.kind
        );
    }
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

// ---------- v3：数据驱动的动态图解 ----------

/// 把时间点换算成 0～1 的关键帧位置，并保证严格递增。
fn key_times(times: &[i64], total: i64) -> String {
    let mut previous = -1.0;
    times
        .iter()
        .map(|time| {
            let value = (*time as f64 / total.max(1) as f64).clamp(0.0, 0.999);
            let value = value.max(previous + 0.001).min(0.999);
            previous = value;
            format!("{value:.6}")
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// 逐状态取值的关键帧：每个值保持到下一次切换前 `transition` 毫秒，再短促过渡到新值。
///
/// 返回 `(values, keyTimes)`。过渡短促，避免整段时间都在缓慢漂移，
/// 让画面里的强调、位置变化与旁白的推理步骤对齐。
fn state_keyframes(
    values: &[String],
    starts: &[i64],
    transition: i64,
    total: i64,
) -> (Vec<String>, String) {
    let mut frames = Vec::with_capacity(values.len() * 2);
    let mut times = Vec::with_capacity(values.len() * 2);
    frames.push(values[0].clone());
    times.push(0i64);
    for index in 1..values.len() {
        let switch = starts[index];
        // 过渡不早于上一状态的起点，避免极短状态下关键帧倒序。
        let begin = (switch - transition).max(starts[index - 1] + 1);
        frames.push(values[index - 1].clone());
        times.push(begin);
        frames.push(values[index].clone());
        times.push(switch);
    }
    (frames, key_times(&times, total))
}

/// 强调级别的图形配色：`(描边, 填充, 文字)`。
///
/// 强调对象用更亮的填充与主题色描边，在深色画布上有明确的面积级对比，
/// 不依赖闪烁或装饰光效；降级对象压暗，形成真实的注意力转移。
fn emphasis_colors<'a>(emphasis: &str, accent: &'a str) -> (&'a str, &'a str, &'a str) {
    match emphasis {
        "accent" => (accent, "#173F5F", "#F4F7FF"),
        "dim" => ("#243350", "#0D1622", "#6F7E9C"),
        _ => ("#34435A", "#101B2C", "#E6EDF9"),
    }
}

/// 深色画布与标题；新布局不使用页眉、页码和装饰网格。
fn open_canvas(title: &str, accent: &str) -> String {
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"720\" height=\"1280\" viewBox=\"0 0 720 1280\">\
         <rect width=\"720\" height=\"1280\" fill=\"#0B1120\"/>\
         <g font-family=\"PingFang SC,Microsoft YaHei,Noto Sans CJK SC,sans-serif\">",
    );
    let heading = lines(title, 12);
    let size = if heading.len() > 1 { 44. } else { 52. };
    svg.push_str("<g opacity=\"0\">");
    svg.push_str(&format!(
        "<animate attributeName=\"opacity\" values=\"0;1\" keyTimes=\"0;1\" begin=\"0ms\" dur=\"420ms\" fill=\"freeze\"/>"
    ));
    for (i, line) in heading.iter().take(2).enumerate() {
        svg.push_str(&text(
            44.,
            96. + i as f64 * 54.,
            size,
            "#F4F7FF",
            line,
            "start",
        ));
    }
    svg.push_str("</g>");
    let rule_y = 96. + (heading.len().min(2) as f64 - 1.) * 54. + 34.;
    svg.push_str(&format!(
        "<rect x=\"44\" y=\"{rule_y}\" width=\"72\" height=\"5\" rx=\"2\" fill=\"{accent}\"/>"
    ));
    svg
}

/// 新布局共用字幕：底部两行分页，不叠加页脚与进度条。
fn subtitle_block(shot: &Shot, subtitles: bool, top: f64, size: f64) -> String {
    if !subtitles {
        return String::new();
    }
    let pages = lines(&shot.narration, 22);
    let count = (pages.len() + 1) / 2;
    let page_ms = shot.duration_ms / count.max(1) as i64;
    let mut out = String::new();
    for (page, pair) in pages.chunks(2).enumerate() {
        let start = page as i64 * page_ms;
        let dur = if page + 1 == count {
            shot.duration_ms - start
        } else {
            page_ms
        };
        out.push_str("<g opacity=\"0\">");
        out.push_str(&format!(
            "<animate attributeName=\"opacity\" values=\"0;1;1;0\" keyTimes=\"0;0.02;0.98;1\" begin=\"{start}ms\" dur=\"{dur}ms\" fill=\"freeze\"/>"
        ));
        for (i, line) in pair.iter().enumerate() {
            out.push_str(&text(
                360.,
                top + i as f64 * (size + 12.),
                size,
                "#C9D3E6",
                line,
                "middle",
            ));
        }
        out.push_str("</g>");
    }
    out
}

/// 受约束的数据驱动画面；规格非法时返回 `None`，回落到五类固定布局。
fn data_svg(
    scene: &Scene,
    visual: &Visual,
    shot: &Shot,
    accent: &str,
    subtitles: bool,
    beats: &[i64],
) -> Option<String> {
    match visual.kind.as_str() {
        "search" => search_svg(scene, visual, shot, accent, subtitles, beats),
        "states" => states_svg(scene, visual, shot, accent, subtitles),
        _ => None,
    }
}

/// 有序搜索空间：真实计算每一步的中点、排除区间与命中位置。
fn search_svg(
    scene: &Scene,
    visual: &Visual,
    shot: &Shot,
    accent: &str,
    subtitles: bool,
    beats: &[i64],
) -> Option<String> {
    let trace = visual.search_trace()?;
    let total = shot.duration_ms.max(1200);
    let step_count = trace.steps.len() as i64;
    let intro = (total / 8).clamp(220, 620);
    let outro = (total / 5).clamp(320, 1400);
    let step_len = ((total - intro - outro).max(step_count * 280) / step_count).max(280);
    let step_start = |index: usize| intro + index as i64 * step_len;
    let starts: Vec<i64> = (0..trace.steps.len()).map(step_start).collect();

    // 10×10 网格呈现完整搜索空间；更大范围自动加列，但不超过 200 个格子。
    let size = trace.size as usize;
    let columns = if size <= 100 { 10 } else { 20 };
    let rows = (size + columns - 1) / columns;
    let grid_x = 40.;
    let cell_w = 640. / columns as f64;
    let cell_h = (460. / rows as f64).min(46.);
    let grid_y = 206.;
    let cell_x = |index: usize| grid_x + (index % columns) as f64 * cell_w;
    let cell_y = |index: usize| grid_y + (index / columns) as f64 * cell_h;
    let font = if columns >= 20 {
        16.
    } else if columns >= 16 {
        19.
    } else {
        25.
    };

    let mut svg = open_canvas(&scene.title, accent);
    svg.push_str(&format!(
        "<text x=\"676\" y=\"96\" font-size=\"17\" font-weight=\"600\" fill=\"#8594B2\" text-anchor=\"end\">{}–{} 找 {}</text>",
        trace.from, trace.from + trace.size - 1, trace.target
    ));

    // 整个搜索空间一次性入场；后续只有被排除的格子单独变化。
    svg.push_str("<g opacity=\"0\">");
    svg.push_str(&format!(
        "<animate attributeName=\"opacity\" values=\"0;1\" keyTimes=\"0;1\" begin=\"0ms\" dur=\"520ms\" fill=\"freeze\"/>"
    ));
    for index in 0..size {
        let value = trace.from + index as i64;
        let left = cell_x(index);
        let top = cell_y(index);
        let baseline = top + cell_h * 0.68;
        svg.push_str(&format!(
            "<g opacity=\"1\"><text x=\"{:.2}\" y=\"{:.2}\" font-size=\"{font}\" font-weight=\"600\" fill=\"#DDE6F5\" text-anchor=\"middle\">{value}</text>",
            left + cell_w / 2.,
            baseline
        ));
        // 第一次落在区间外的那一刻：淡出并轻微下沉，表示这一半已被排除。
        if let Some(step) = trace
            .steps
            .iter()
            .position(|step| (index as i64) < step.lo || (index as i64) > step.hi)
        {
            let at = step_start(step);
            svg.push_str(&format!(
                "<animate attributeName=\"opacity\" values=\"1;0.12\" keyTimes=\"0;1\" begin=\"{at}ms\" dur=\"300ms\" fill=\"freeze\"/>\
                 <animateTransform attributeName=\"transform\" type=\"translate\" values=\"0 0;0 12\" keyTimes=\"0;1\" begin=\"{at}ms\" dur=\"300ms\" fill=\"freeze\"/>"
            ));
        }
        svg.push_str("</g>");
    }
    svg.push_str("</g>");

    // 中点游标在真实中点之间移动，保持连续运动而不是逐个闪现。
    let first = trace.steps.first()?;
    let ring_left = cell_x(first.mid as usize) + 3.;
    let ring_top = cell_y(first.mid as usize) + 3.;
    let ring_w = (cell_w - 6.).max(10.);
    let ring_h = (cell_h - 8.).max(10.);
    let moves: Vec<String> = trace
        .steps
        .iter()
        .map(|step| {
            format!(
                "{} {}",
                cell_x(step.mid as usize) - cell_x(first.mid as usize),
                cell_y(step.mid as usize) - cell_y(first.mid as usize)
            )
        })
        .collect();
    svg.push_str(&format!(
        "<rect x=\"{ring_left:.2}\" y=\"{ring_top:.2}\" width=\"{ring_w:.2}\" height=\"{ring_h:.2}\" rx=\"8\" fill=\"#12283A\" stroke=\"{accent}\" stroke-width=\"3\" opacity=\"0\">\
         <animateTransform attributeName=\"transform\" type=\"translate\" values=\"{}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
         <animate attributeName=\"opacity\" values=\"0;1\" keyTimes=\"0;1\" begin=\"{intro}ms\" dur=\"180ms\" fill=\"freeze\"/>",
        moves.join(";"),
        key_times(&starts, total)
    ));
    // 命中后强调：环体加粗并跟随真实拍点脉冲。
    if trace
        .steps
        .last()
        .is_some_and(|step| step.verdict == std::cmp::Ordering::Equal)
    {
        let found = step_start(trace.steps.len() - 1);
        let pulse_values = if beats.is_empty() {
            "3;7;3".to_string()
        } else {
            let mut points = vec![("0".to_string(), 3.)];
            for beat in beats.iter().filter(|b| **b >= found) {
                points.push(((beat - found).to_string(), 7.));
                points.push(((beat - found + 140).to_string(), 3.));
            }
            points.push(((total - found).to_string(), 3.));
            points.sort_by_key(|p| p.0.parse::<i64>().unwrap_or(0));
            let values = points
                .iter()
                .map(|p| p.1.to_string())
                .collect::<Vec<_>>()
                .join(";");
            values
        };
        svg.push_str(&format!(
            "<animate attributeName=\"stroke-width\" values=\"{pulse_values}\" begin=\"{found}ms\" dur=\"{total}ms\" fill=\"freeze\"/>"
        ));
    }
    svg.push_str("</rect>");

    // 每一步的比较说明与剩余个数，全部来自真实轨迹。
    for (index, step) in trace.steps.iter().enumerate() {
        let verdict = match step.verdict {
            std::cmp::Ordering::Equal => format!("中点 {} 就是目标", step.value),
            std::cmp::Ordering::Less => format!("{} > {}，排除左半段", trace.target, step.value),
            std::cmp::Ordering::Greater => format!("{} < {}，排除右半段", trace.target, step.value),
        };
        let start = step_start(index);
        let remaining = step.hi - step.lo + 1;
        svg.push_str(&format!(
            "<g opacity=\"0\"><animate attributeName=\"opacity\" values=\"0;1;1;0\" keyTimes=\"0;0.06;0.9;1\" begin=\"{start}ms\" dur=\"{step_len}ms\" fill=\"freeze\"/>"
        ));
        svg.push_str(&text(44., 744., 23., accent, &verdict, "start"));
        svg.push_str(&text(
            676.,
            744.,
            18.,
            "#8594B2",
            &format!("剩 {remaining} 个"),
            "end",
        ));
        svg.push_str("</g>");
    }

    // 缩小区间条：真实区间在 1–100 的线性刻度上收缩。
    let bar_x = 40.;
    let bar_w = 640.;
    let bar_y = 668.;
    let segment = |step: &TraceStep| {
        (
            bar_x + bar_w * step.lo as f64 / size as f64,
            bar_w * (step.hi - step.lo + 1) as f64 / size as f64,
        )
    };
    let (fx, fw) = segment(first);
    let xs = trace
        .steps
        .iter()
        .map(|step| format!("{:.2}", segment(step).0))
        .collect::<Vec<_>>()
        .join(";");
    let ws = trace
        .steps
        .iter()
        .map(|step| format!("{:.2}", segment(step).1))
        .collect::<Vec<_>>()
        .join(";");
    svg.push_str(&format!(
        "<rect x=\"{bar_x}\" y=\"{bar_y}\" width=\"{bar_w}\" height=\"10\" rx=\"5\" fill=\"#1B2941\"/>\
         <rect x=\"{fx:.2}\" y=\"{bar_y}\" width=\"{fw:.2}\" height=\"10\" rx=\"5\" fill=\"{accent}\">\
         <animate attributeName=\"x\" values=\"{xs}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
         <animate attributeName=\"width\" values=\"{ws}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
         </rect>",
        key_times(&starts, total),
        key_times(&starts, total)
    ));
    svg.push_str(&text(
        40.,
        740.,
        16.,
        "#8594B2",
        &trace.from.to_string(),
        "start",
    ));
    svg.push_str(&text(
        680.,
        740.,
        16.,
        "#8594B2",
        &(trace.from + trace.size - 1).to_string(),
        "end",
    ));

    // O(log n) 总结：用两种搜索的最坏比较次数做长度对比。
    let summary_at = (total - outro).max(intro);
    let chart_x = 252.;
    let chart_w = 424.;
    let linear_h = 20.;
    let binary_w = (chart_w * trace.steps.len() as f64 / size as f64).max(18.);
    svg.push_str(&text(
        44.,
        880.,
        18.,
        "#8594B2",
        &format!("线性扫描 · 最多 {size} 次"),
        "start",
    ));
    svg.push_str(&text(
        44.,
        928.,
        18.,
        accent,
        &format!("二分查找 · {} 次", trace.steps.len()),
        "start",
    ));
    for (y, width, color) in [(868., chart_w, "#2A3A55"), (916., binary_w, accent)] {
        svg.push_str(&format!(
            "<rect x=\"{chart_x}\" y=\"{y}\" width=\"0\" height=\"{linear_h}\" rx=\"6\" fill=\"{color}\">\
             <animate attributeName=\"width\" values=\"0;{width:.2}\" keyTimes=\"0;1\" begin=\"{summary_at}ms\" dur=\"700ms\" fill=\"freeze\"/>\
             </rect>"
        ));
    }

    let conclusion = if scene
        .visual
        .as_ref()
        .is_some_and(|v| !v.conclusion.trim().is_empty())
    {
        scene
            .visual
            .as_ref()
            .map(|v| v.conclusion.trim().chars().take(26).collect::<String>())
            .unwrap_or_default()
    } else if trace
        .steps
        .last()
        .is_some_and(|s| s.verdict == std::cmp::Ordering::Equal)
    {
        format!(
            "{} 次比较命中 {} · 复杂度 O(log n)",
            trace.steps.len(),
            trace.target
        )
    } else {
        format!("{} 次比较未命中 · 复杂度 O(log n)", trace.steps.len())
    };
    svg.push_str(&text(360., 1000., 27., accent, &conclusion, "middle"));
    svg.push_str(&subtitle_block(shot, subtitles, 1108., 26.));
    svg.push_str("</g></svg>");
    Some(svg)
}

/// 状态演变：同一对象在推理步骤之间移动、出现或离开；每步都有对应说明。
fn states_svg(
    scene: &Scene,
    visual: &Visual,
    shot: &Shot,
    accent: &str,
    subtitles: bool,
) -> Option<String> {
    if visual.states.is_empty() || visual.states.len() > 6 {
        return None;
    }
    let mut distinct: Vec<String> = Vec::new();
    for state in &visual.states {
        if state.objects.is_empty() || state.objects.len() > 5 {
            return None;
        }
        for object in &state.objects {
            let value = object.text.trim();
            if value.is_empty() || value.chars().count() > 24 {
                return None;
            }
            if !distinct.iter().any(|item| item == value) {
                distinct.push(value.into());
            }
        }
    }
    if distinct.len() > 8 {
        return None;
    }
    let total = shot.duration_ms.max(1200);
    let intro = (total / 10).clamp(180, 420);
    let state_len = ((total - intro) / visual.states.len() as i64).max(320);
    let starts: Vec<i64> = (0..visual.states.len())
        .map(|index| intro + index as i64 * state_len)
        .collect();
    let slot_y = |slot: usize| 424. + slot as f64 * 96.;
    let mut svg = open_canvas(&scene.title, accent);

    // 当前推理步骤说明，按状态窗口切换。
    for (index, state) in visual.states.iter().enumerate() {
        if state.caption.trim().is_empty() {
            continue;
        }
        let start = starts[index];
        let dur = if index + 1 == visual.states.len() {
            total - start
        } else {
            state_len
        };
        svg.push_str(&format!(
            "<g opacity=\"0\"><animate attributeName=\"opacity\" values=\"0;1;1;0\" keyTimes=\"0;0.06;0.9;1\" begin=\"{start}ms\" dur=\"{dur}ms\" fill=\"freeze\"/>"
        ));
        for (line, part) in lines(state.caption.trim(), 18).iter().take(2).enumerate() {
            svg.push_str(&text(
                360.,
                306. + line as f64 * 38.,
                25.,
                accent,
                part,
                "middle",
            ));
        }
        svg.push_str("</g>");
    }

    // 同一对象在相邻状态之间平移；新对象淡入，离场对象淡出。
    for name in &distinct {
        let locations: Vec<Option<usize>> = visual
            .states
            .iter()
            .map(|state| {
                state
                    .objects
                    .iter()
                    .position(|object| object.text.trim() == name)
            })
            .collect();
        let first_state = locations.iter().position(|slot| slot.is_some())?;
        let first_slot = locations[first_state]?;
        let width = (name.chars().count() as f64 * 26. + 64.).clamp(260., 600.);
        // 每个状态里这个对象的位置、强调级别与是否在场；强调切换是这一步的可见推进。
        let mut slot = first_slot;
        let mut deltas: Vec<String> = Vec::with_capacity(locations.len());
        for location in &locations {
            if let Some(current) = location {
                slot = *current;
            }
            deltas.push(format!("0 {:.2}", slot_y(slot) - slot_y(first_slot)));
        }
        // 对象离场后沿用最后一次的强调级别，让它淡出时不突变成另一种样式。
        let mut last = visual.states[first_state]
            .objects
            .get(first_slot)
            .map(|object| object.emphasis.clone())
            .unwrap_or_else(|| "plain".into());
        let resolved: Vec<String> = locations
            .iter()
            .enumerate()
            .map(|(state, location)| {
                if let Some(current) = location {
                    if let Some(emphasis) = visual.states[state]
                        .objects
                        .get(*current)
                        .map(|object| object.emphasis.clone())
                        .filter(|emphasis| !emphasis.is_empty())
                    {
                        last = emphasis;
                    }
                }
                last.clone()
            })
            .collect();
        // 强调切换用短促过渡；只有强调变化、位置不变的对象也能被看见。
        let transition = (state_len / 5).clamp(140, 360);
        let color_of = |emphasis: &str| emphasis_colors(emphasis, accent);
        let (strokes, stroke_times) = state_keyframes(
            &resolved
                .iter()
                .map(|e| color_of(e).0.into())
                .collect::<Vec<_>>(),
            &starts,
            transition,
            total,
        );
        let (fills, fill_times) = state_keyframes(
            &resolved
                .iter()
                .map(|e| color_of(e).1.into())
                .collect::<Vec<_>>(),
            &starts,
            transition,
            total,
        );
        let (widths, width_times) = state_keyframes(
            &resolved
                .iter()
                .map(|e| if e == "accent" { "4" } else { "2" }.to_string())
                .collect::<Vec<_>>(),
            &starts,
            transition,
            total,
        );
        let (text_colors, text_times) = state_keyframes(
            &resolved
                .iter()
                .map(|e| color_of(e).2.into())
                .collect::<Vec<_>>(),
            &starts,
            transition,
            total,
        );
        let opacities: Vec<&str> = locations
            .iter()
            .map(|location| if location.is_some() { "1" } else { "0" })
            .collect();
        svg.push_str(&format!(
            "<g opacity=\"{}\">\
             <animate attributeName=\"opacity\" values=\"{}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
             <animateTransform attributeName=\"transform\" type=\"translate\" values=\"{}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
             <rect x=\"{:.2}\" y=\"{:.2}\" width=\"{width:.2}\" height=\"76\" rx=\"16\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\">\
             <animate attributeName=\"fill\" values=\"{}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
             <animate attributeName=\"stroke\" values=\"{}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
             <animate attributeName=\"stroke-width\" values=\"{}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
             </rect>",
            opacities[0],
            opacities.join(";"),
            key_times(&starts, total),
            deltas.join(";"),
            key_times(&starts, total),
            360. - width / 2.,
            slot_y(first_slot),
            fills[0],
            strokes[0],
            widths[0],
            fills.join(";"),
            fill_times,
            strokes.join(";"),
            stroke_times,
            widths.join(";"),
            width_times,
        ));
        svg.push_str(&format!(
            "<text x=\"360\" y=\"{:.2}\" font-size=\"27\" font-weight=\"700\" fill=\"{}\" text-anchor=\"middle\">{}\
             <animate attributeName=\"fill\" values=\"{}\" keyTimes=\"{}\" begin=\"0ms\" dur=\"{total}ms\" fill=\"freeze\"/>\
             </text>",
            slot_y(first_slot) + 49.,
            text_colors[0],
            escape(name),
            text_colors.join(";"),
            text_times,
        ));
        svg.push_str("</g>");
    }

    let conclusion = visual.conclusion.trim();
    if !conclusion.is_empty() {
        let start = starts[visual.states.len() - 1];
        svg.push_str(&format!(
            "<g opacity=\"0\"><animate attributeName=\"opacity\" values=\"0;1\" keyTimes=\"0;1\" begin=\"{start}ms\" dur=\"420ms\" fill=\"freeze\"/>"
        ));
        for (line, part) in lines(conclusion, 18).iter().take(2).enumerate() {
            svg.push_str(&text(
                360.,
                976. + line as f64 * 38.,
                29.,
                accent,
                part,
                "middle",
            ));
        }
        svg.push_str("</g>");
    }
    svg.push_str(&subtitle_block(shot, subtitles, 1108., 26.));
    svg.push_str("</g></svg>");
    Some(svg)
}

/// 横屏和方形旧镜头也重新排版；不用缩放竖屏 viewBox 冒充原生构图。
pub fn scene_svg_at(
    shot: &Shot,
    index: usize,
    count: usize,
    rhythm: Option<&Rhythm>,
    offset: i64,
    style: &str,
    subtitles: bool,
    w: u32,
    h: u32,
) -> String {
    if w == 720 && h == 1280 {
        return scene_svg(shot, index, count, rhythm, offset, style, subtitles);
    }
    let scene: Scene = serde_json::from_str(&shot.visual_prompt).unwrap_or_default();
    let title = if scene.title.is_empty() {
        heading(&shot.narration)
    } else {
        scene.title.clone()
    };
    let (wf, hf) = (w as f64, h as f64);
    let accent = match style {
        "warm" | "scientific" => "#D8B56E",
        "cinematic" => "#B9A6FF",
        "energetic" => "#FF8B94",
        _ => "#78F0D0",
    };
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><rect width=\"{w}\" height=\"{h}\" fill=\"#0B1120\"/><g font-family=\"Hiragino Sans GB,Microsoft YaHei,sans-serif\">{}",text(wf*0.06,hf*0.15,40.,"#F4F7FF",&title,"start"));
    if let Some(trace) = scene.visual.as_ref().and_then(|v| v.search_trace()) {
        let columns = if w > h { 20usize } else { 10 };
        let rows = (trace.size as usize + columns - 1) / columns;
        let cw = wf * 0.88 / columns as f64;
        let ch = hf * 0.48 / rows as f64;
        for cell in 0..trace.size as usize {
            svg.push_str("<g opacity=\"1\">");
            svg.push_str(&text(
                wf * 0.06 + (cell % columns) as f64 * cw + cw / 2.,
                hf * 0.28 + (cell / columns) as f64 * ch,
                ch.min(cw) * 0.53,
                "#E6EDF9",
                &(trace.from + cell as i64).to_string(),
                "middle",
            ));
            if let Some(step) = trace
                .steps
                .iter()
                .position(|s| (cell as i64) < s.lo || (cell as i64) > s.hi)
            {
                svg.push_str(&format!("<animate attributeName=\"opacity\" values=\"1;0.12\" begin=\"{}ms\" dur=\"250ms\" fill=\"freeze\"/>",step as i64*shot.duration_ms/(trace.steps.len()as i64+1)));
            }
            svg.push_str("</g>");
        }
        for (i, step) in trace.steps.iter().enumerate() {
            let start = i as i64 * shot.duration_ms / (trace.steps.len() as i64 + 1);
            let dur = shot.duration_ms / (trace.steps.len() as i64 + 1);
            svg.push_str(&format!("<g opacity=\"0\"><animate attributeName=\"opacity\" values=\"1;1;0\" keyTimes=\"0;0.95;1\" begin=\"{start}ms\" dur=\"{dur}ms\" fill=\"freeze\"/>{}</g>",text(wf*0.5,hf*0.79,26.,accent,&step.caption,"middle")));
        }
    } else {
        // 仅用于不可用模型的明确降级；通用主题的正常路径由 scene_plan 和模型画面承接。
        let labels = if scene.labels.is_empty() {
            vec![heading(&shot.narration)]
        } else {
            scene.labels
        };
        let n = labels.len().min(4);
        for (i, label) in labels.iter().take(n).enumerate() {
            let x = wf * (0.12 + (i as f64 + 0.5) * 0.76 / n as f64);
            svg.push_str(&format!("<g opacity=\"0\">{}<circle cx=\"{x}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{accent}\" stroke-width=\"3\"/>{}</g>",enter(i as i64*400),hf*0.44,(wf*0.24/n as f64).min(hf*0.15),text(x,hf*0.68,24.,"#F4F7FF",label,"middle")));
        }
    }
    if subtitles {
        for (i, line) in lines(&shot.narration, (wf * 0.84 / 25.) as usize)
            .iter()
            .take(2)
            .enumerate()
        {
            svg.push_str(&text(
                wf * 0.5,
                hf * 0.89 + i as f64 * 32.,
                25.,
                "#C9D3E6",
                line,
                "middle",
            ));
        }
    }
    svg.push_str("</g></svg>");
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
    let (cw, ch) = crate::util::dimensions(&settings.aspect_ratio);
    let svg = if let Some(plan) = crate::scene_plan::from_prompt(&shot.visual_prompt) {
        plan.svg(shot.duration_ms, cw as u32, ch as u32)?
    } else {
        scene_svg_at(
            shot,
            index,
            count,
            rhythm,
            offset,
            &settings.style_id,
            subtitles,
            cw as u32,
            ch as u32,
        )
    };
    let (w, h) =
        crate::util::output_dimensions(&settings.aspect_ratio, &settings.output_resolution);
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

    /// 搜索轨迹必须由本地算法真实计算，不能采用模型写出的步骤。
    #[test]
    fn binary_search_trace_is_computed_locally() {
        let visual: Visual = serde_json::from_value(serde_json::json!({
            "type": "search",
            "domain": { "from": 1, "to": 100 },
            "target": 74
        }))
        .unwrap();
        let trace = visual.search_trace().expect("合法区间应得到真实轨迹");
        let mids: Vec<i64> = trace.steps.iter().map(|step| step.value).collect();
        assert_eq!(mids, vec![50, 75, 62, 68, 71, 73, 74]);
        assert_eq!(
            trace.steps.last().unwrap().verdict,
            std::cmp::Ordering::Equal
        );
        // 区间每一步都真实收缩，最后只剩命中的那个数字。
        assert_eq!(
            (
                trace.steps.last().unwrap().lo,
                trace.steps.last().unwrap().hi
            ),
            (73, 73)
        );
        // 模型编造的步骤字段不会被读取：即使额外给出 steps 也不影响结果。
        let fabricated: Visual = serde_json::from_value(serde_json::json!({
            "type": "search",
            "domain": { "from": 1, "to": 100 },
            "target": 74,
            "steps": [{"mid": 1, "value": 74}]
        }))
        .unwrap();
        assert_eq!(
            fabricated
                .search_trace()
                .unwrap()
                .steps
                .iter()
                .map(|step| step.value)
                .collect::<Vec<_>>(),
            mids
        );
    }

    /// 搜索画面必须来自真实数据：中点、排除说明与命中结论都要出现，且不使用旧页眉。
    #[test]
    fn search_scene_draws_real_midpoints_and_states() {
        let shot: Shot = serde_json::from_value(serde_json::json!({
            "id": "s",
            "duration_ms": 9000,
            "narration": "在 1 到 100 中找 74。",
            "visual_prompt": r#"{"kind":"mechanism","title":"在 100 个数中找 74","labels":[],"note":"","visual":{"type":"search","domain":{"from":1,"to":100},"target":74}}"#,
        }))
        .unwrap();
        let svg = scene_svg(&shot, 0, 1, None, 0, "minimal", false);
        assert_eq!(svg, scene_svg(&shot, 0, 1, None, 0, "minimal", false));
        assert!(svg.contains(">50<"), "应画出第一个真实中点");
        assert!(svg.contains(">74<"), "应包含目标数字");
        assert!(svg.contains("排除左半段"));
        assert!(svg.contains("7 次比较命中 74"));
        assert!(svg.contains("线性扫描 · 最多 100 次"));
        assert!(!svg.contains("VIBE · 知识图解"), "新布局不使用重复页眉");
        assert!(!svg.contains("stroke=\"#172236\""), "新布局不使用装饰网格");
        // 区间与中点都用关键帧连续运动，而不是逐帧替换。
        assert!(svg.contains("<animateTransform"));
        assert!(svg.contains("attributeName=\"x\""));
    }

    /// 状态演变：同一对象连续移动，状态说明与结论都要出现在画面中。
    #[test]
    fn states_scene_moves_shared_objects() {
        let shot: Shot = serde_json::from_value(serde_json::json!({
            "id": "s",
            "duration_ms": 8000,
            "narration": "利率上升会压低估值。",
                        "visual_prompt": r#"{"kind":"mechanism","title":"利率与估值","visual":{"type":"states","states":[{"caption":"资金成本上升","objects":[{"text":"利率","emphasis":"accent"},{"text":"估值","emphasis":"plain"}]},{"caption":"折现率提高","objects":[{"text":"折现率","emphasis":"accent"},{"text":"利率","emphasis":"plain"},{"text":"估值","emphasis":"dim"}]}],"conclusion":"利率上升，估值承压"}}"#,
        }))
        .unwrap();
        let svg = scene_svg(&shot, 0, 1, None, 0, "minimal", false);
        assert!(svg.contains("资金成本上升"));
        assert!(svg.contains("折现率提高"));
        assert!(svg.contains("利率上升，估值承压"));
        // 共享对象"利率"与"估值"有位移关键帧，说明是连续状态变化。
        assert_eq!(svg.matches("<animateTransform").count(), 3);
    }

    /// 非法规格不能生成任意内容，必须回落到固定布局。
    #[test]
    fn invalid_visual_falls_back_to_fixed_layout() {
        let shot: Shot = serde_json::from_value(serde_json::json!({
            "id": "s",
            "duration_ms": 6000,
            "narration": "一段解说。",
            "visual_prompt": "{\"title\":\"兜底\",\"visual\":{\"type\":\"search\",\"domain\":{\"from\":1,\"to\":100000},\"target\":74}}"
        }))
        .unwrap();
        let svg = scene_svg(&shot, 0, 3, None, 0, "minimal", true);
        assert!(svg.contains("VIBE · 知识图解"), "超出上限的搜索空间应回落");
    }

    /// 真实渲染 9 秒 MP4；设置 EXPLAINCRAFT_ARTIFACT_DIR 时把成片和 SVG 写到该目录。
    #[tokio::test]
    #[ignore = "渲染真实 9 秒 MP4，需要 FFmpeg"]
    async fn binary_search_shot_renders_a_real_mp4() {
        use crate::providers::MediaData;
        let shot: Shot = serde_json::from_value(serde_json::json!({
            "id": "search",
            "duration_ms": 9000,
            "narration": "在 1 到 100 的有序整数中，二分查找只需七次比较就能找到 74。",
            "visual_prompt": r#"{"kind":"mechanism","title":"在 100 个数中找 74","visual":{"type":"search","domain":{"from":1,"to":100},"target":74}}"#,
        }))
        .unwrap();
        let settings = crate::models::Settings {
            target_duration_seconds: 15,
            aspect_ratio: "9:16".into(),
            language: "zh-CN".into(),
            style_id: "minimal".into(),
            visual_format: "svg".into(),
            render_profile: "vibe".into(),
            output_resolution: "1080p".into(),
        };
        let svg = scene_svg(&shot, 0, 1, None, 0, "minimal", true);
        let media = render(&shot, 0, 1, &settings, None, 0, true)
            .await
            .expect("搜索镜头应渲染为真实 MP4");
        let MediaData::Bytes(bytes) = media.data else {
            panic!("必须返回可入库的视频字节");
        };
        assert!(bytes.len() > 50_000, "成片字节过少：{}", bytes.len());
        if let Ok(dir) = std::env::var("EXPLAINCRAFT_ARTIFACT_DIR") {
            std::fs::create_dir_all(&dir).ok();
            std::fs::write(format!("{dir}/binary-search-1080p.mp4"), &bytes).ok();
            std::fs::write(format!("{dir}/binary-search-scene.svg"), svg).ok();
        }
    }
}
