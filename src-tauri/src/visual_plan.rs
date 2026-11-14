//! 逐镜头画面路由与有效性检查。
//!
//! 能确定计算或用完整状态演变表达的知识，走受控 Vibe 图解；
//! 语义稀疏、缺少图解或无法计算的主题，交给模型生成 SVG/HTML 动画。
//! 生成结果必须通过「帧间有效变化」检查；失败或检查不通过时回退到受控渲染，
//! 每个镜头独立处理，不让单个镜头拖垮其他镜头。

use serde::Deserialize;

/// 单个镜头的画面生成路线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualRoute {
    /// 本地真实计算的搜索轨迹；一定包含逐步比较，确定性强。
    Search,
    /// 受控状态演变；状态足够多时画面持续变化。
    States,
    /// 模型自由生成 SVG/HTML 动画。
    Generate,
}

impl VisualRoute {
    /// 面向日志与界面的路线说明。
    pub fn label(self) -> &'static str {
        match self {
            VisualRoute::Search => "本地搜索轨迹",
            VisualRoute::States => "语义状态演变",
            VisualRoute::Generate => "模型生成动画",
        }
    }
}

/// `visual_prompt` 中路由需要的字段；其余字段不影响决策。
///
/// 字段层级与 `vibe.rs` 的 `Scene` 一致：图解指令在 `visual` 对象里。
#[derive(Default, Deserialize)]
#[serde(default)]
struct Routing {
    /// v3 图解指令；缺省表示没有可用语义。
    visual: Option<RoutingVisual>,
}

/// 图解指令里只关心类型、目标与状态数量。
#[derive(Default, Deserialize)]
#[serde(default)]
struct RoutingVisual {
    /// `search` / `states`；其他值视为没有可用语义。
    #[serde(rename = "type")]
    kind: String,
    /// 搜索轨迹必然可计算。
    target: Option<i64>,
    /// 状态数量。
    states: Vec<RoutingState>,
    /// 仅用户明确要文字状态演示时保留旧路线，不能由状态数量自动选中。
    presentation: String,
}

/// 仅解析状态数量与对象数量。
#[derive(Default, Deserialize)]
#[serde(default)]
struct RoutingState {
    /// 该状态的对象数；空状态对画面没有贡献。
    objects: Vec<serde_json::Value>,
}

/// 镜头按参考节奏应有有效变化的次数：每 1～3 秒一次，最少 3 次。
pub fn expected_events(duration_ms: i64) -> usize {
    ((duration_ms.max(0) / 1500) as usize).max(3)
}

/// 判断语义图解是否足够支撑整段时长。
///
/// 状态数与变化数低于目标次数、或状态对象始终不变时，语义模板会被静态停留，
/// 应交给模型生成。这里只读取受约束字段，不执行任何模型代码。
pub fn sufficient(prompt: &str, duration_ms: i64) -> bool {
    let Ok(routing) = serde_json::from_str::<Routing>(prompt.trim()) else {
        return false;
    };
    let Some(visual) = routing.visual else {
        return false;
    };
    let expected = expected_events(duration_ms);
    match visual.kind.as_str() {
        "search" => crate::vibe::valid_search(prompt),
        "states" if visual.presentation == "explicit_text" => {
            let usable: Vec<Vec<String>> = visual
                .states
                .iter()
                .map(|state| {
                    state
                        .objects
                        .iter()
                        .filter_map(|object| object.get("text")?.as_str().map(str::to_string))
                        .collect()
                })
                .filter(|objects: &Vec<String>| !objects.is_empty())
                .collect();
            if usable.len() < expected.min(4).max(3) {
                return false;
            }
            // 相邻状态完全相同等于没有推进；至少一半的相邻步骤要有变化。
            let transitions = usable.len().saturating_sub(1);
            let changed = usable.windows(2).filter(|pair| pair[0] != pair[1]).count();
            changed * 2 >= transitions
        }
        _ => false,
    }
}

/// 决定单个镜头的画面路线。
///
/// `vibe` 为真表示当前主题走 Vibe 视觉；其余主题（含 Shotcraft 显式模板）保持模型生成。
/// 只有能本地验证的语义才走受控渲染，其他情况都由模型自由绘制；
/// 模型不可用时由调用方回退到固定图解，保证仍能出片。
pub fn route(prompt: &str, duration_ms: i64, vibe: bool) -> VisualRoute {
    if !vibe {
        return VisualRoute::Generate;
    }
    let Ok(routing) = serde_json::from_str::<Routing>(prompt.trim()) else {
        return VisualRoute::Generate;
    };
    let Some(visual) = &routing.visual else {
        return VisualRoute::Generate;
    };
    match visual.kind.as_str() {
        "search" if visual.target.is_some() && crate::vibe::valid_search(prompt) => VisualRoute::Search,
        "states" if sufficient(prompt, duration_ms) => VisualRoute::States,
        _ => VisualRoute::Generate,
    }
}

/// 一帧的亮度、色彩与局部采样；用于检测「画面是否真的在变化」。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameProfile {
    /// 平均亮度 0～255。
    pub luma: u8,
    /// 平均颜色（RGB）。
    pub color: [u8; 3],
    /// 画面中央 16×16 网格的亮度。
    pub grid: Vec<u8>,
}

/// 单个采样格被视为「发生变化」的亮度差。
const CELL_DELTA: u32 = 8;

/// 判定两个采样帧之间是否存在有效变化。
///
/// 全画面亮度或主色明显变化，说明整幅内容重排，直接算推进。
/// 否则看中央网格：足够比例的采样格亮度差超过 [`CELL_DELTA`] 才算推进。
/// 只改变一两个采样格视为局部闪烁（如一个小光点闪动），不算教学推进；
/// 受控状态演变里对象整体移动、强调切换会带动大片网格一起变化。
pub fn frame_changed(a: &FrameProfile, b: &FrameProfile) -> bool {
    if (a.luma as i32 - b.luma as i32).unsigned_abs() >= 3 {
        return true;
    }
    let color_delta: u32 = a
        .color
        .iter()
        .zip(b.color.iter())
        .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs())
        .sum();
    if color_delta >= 12 {
        return true;
    }
    let cells = a.grid.len().max(b.grid.len()).max(1);
    let changed = a
        .grid
        .iter()
        .zip(b.grid.iter())
        .filter(|(x, y)| (**x as i32 - **y as i32).unsigned_abs() >= CELL_DELTA)
        .count();
    // 至少 8 个采样格（16×16 网格的 3%）一起变化；小范围闪烁远低于此比例。
    changed * 100 / cells >= 3
}

/// 把 RGB 帧转换为采样档案。
pub fn profile_frame(rgb: &[u8], width: usize, height: usize) -> FrameProfile {
    if rgb.len() < 3 || width == 0 || height == 0 {
        return FrameProfile {
            luma: 0,
            color: [0; 3],
            grid: vec![0; 16 * 16],
        };
    }
    let pixels = (rgb.len() / 3).min(width * height).max(1);
    let mut sums = [0u64; 3];
    for pixel in rgb.chunks_exact(3).take(pixels) {
        for (sum, value) in sums.iter_mut().zip(pixel) {
            *sum += *value as u64;
        }
    }
    let color = [
        (sums[0] / pixels as u64) as u8,
        (sums[1] / pixels as u64) as u8,
        (sums[2] / pixels as u64) as u8,
    ];
    let luma =
        ((color[0] as u32 * 299 + color[1] as u32 * 587 + color[2] as u32 * 114) / 1000) as u8;
    let mut grid = vec![0u8; 16 * 16];
    for (cell_y, row) in grid.chunks_mut(16).enumerate() {
        for (cell_x, cell) in row.iter_mut().enumerate() {
            // 中央区域对文字与对象变化最敏感，采样范围收在画面中部。
            let start_x = width * (cell_x + 16) / 48;
            let start_y = height * (cell_y + 16) / 48;
            let end_x = (width * (cell_x + 17) / 48).max(start_x + 1);
            let end_y = (height * (cell_y + 17) / 48).max(start_y + 1);
            let mut sum = 0u64;
            let mut count = 0u64;
            for y in start_y.min(height - 1)..=end_y.min(height - 1) {
                for x in start_x.min(width - 1)..=end_x.min(width - 1) {
                    let offset = (y * width + x) * 3;
                    if offset + 2 < rgb.len() {
                        let (r, g, b) = (
                            rgb[offset] as u32,
                            rgb[offset + 1] as u32,
                            rgb[offset + 2] as u32,
                        );
                        sum += (r * 299 + g * 587 + b * 114) as u64 / 1000;
                        count += 1;
                    }
                }
            }
            *cell = if count == 0 { 0 } else { (sum / count) as u8 };
        }
    }
    FrameProfile { luma, color, grid }
}

/// 抽帧检测时把画面缩到多大；太小会把文字与对象缩小成几像素，测不到真实变化。
const PROBE_SIZE: usize = 128;

/// 在多个时间点抽取 RGB 帧，判断是否存在有效教学变化。
///
/// 两段检查缺一不可：
/// 1. 关键帧对比——取若干等分时间点的画面，相邻关键帧必须有实质不同，
///    保证镜头不是「一张静止卡片」；
/// 2. 局部窗口对比——在关键帧之间再比较相差 200ms 的两帧，
///    捕捉连续运动，避免整段只在结尾跳一次的假推进。
/// 视频无法解码时返回 `None`；调用方据此记录「未验证」而不是伪造通过。
pub async fn media_has_motion(path: &std::path::Path, duration_ms: i64) -> Option<bool> {
    const SIZE: usize = PROBE_SIZE;
    const KEYFRAMES: i64 = 5;
    let duration = duration_ms.max(600);
    let filter = format!("scale={SIZE}:{SIZE}");
    let frame = |at: i64| {
        let path = path.to_path_buf();
        let filter = filter.clone();
        async move {
            let output = tokio::process::Command::new("ffmpeg")
                .args(["-v", "error", "-ss"])
                .arg(format!("{:.3}", at as f64 / 1000.))
                .arg("-i")
                .arg(&path)
                .args([
                    "-vf",
                    &filter,
                    "-frames:v",
                    "1",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "rgb24",
                    "pipe:1",
                ])
                .output()
                .await
                .ok()?;
            if !output.status.success() || output.stdout.len() < SIZE * SIZE * 3 {
                return None;
            }
            Some(profile_frame(&output.stdout, SIZE, SIZE))
        }
    };

    // 关键帧：等分时间段的中点，避开镜头开场的淡入瞬间。
    let mut previous: Option<FrameProfile> = None;
    for index in 0..KEYFRAMES {
        let offset = (duration * (2 * index + 1) / (2 * KEYFRAMES)).clamp(0, duration - 120);
        let profile = frame(offset).await?;
        if let Some(before) = previous.as_ref() {
            if frame_changed(before, &profile) {
                return Some(true);
            }
        }
        previous = Some(profile);
    }

    // 局部运动：相邻关键帧之间再看一对 200ms 画面。
    for index in 0..(KEYFRAMES - 1) {
        let offset = (duration * (2 * index + 1) / (2 * KEYFRAMES)).clamp(0, (duration - 200).max(0));
        let (first, second) = tokio::join!(frame(offset), frame(offset + 200));
        let (first, second) = (first?, second?);
        if frame_changed(&first, &second) {
            return Some(true);
        }
    }
    Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(count: usize, changing: bool) -> String {
        let list: Vec<String> = (0..count)
            .map(|index| {
                let text = if changing {
                    format!("状态{}", index)
                } else {
                    "同一对象".to_string()
                };
                format!("{{\"caption\":\"第{index}步\",\"objects\":[{{\"text\":\"{text}\",\"emphasis\":\"plain\"}}]}}")
            })
            .collect();
        format!(
            "{{\"kind\":\"mechanism\",\"title\":\"T\",\"labels\":[],\"note\":\"\",\"visual\":{{\"type\":\"states\",\"states\":[{}],\"conclusion\":\"结论\"}}}}",
            list.join(",")
        )
    }

    #[test]
    fn computed_search_always_uses_local_trace() {
        let prompt = r#"{"kind":"mechanism","visual":{"type":"search","domain":{"from":1,"to":100},"target":74}}"#;
        assert_eq!(route(prompt, 6000, true), VisualRoute::Search);
        let without_target =
            r#"{"kind":"mechanism","visual":{"type":"search","domain":{"from":1,"to":100}}}"#;
        // 缺少目标值无法本地计算，交给模型绘制而不是固定图解。
        assert_eq!(route(without_target, 6000, true), VisualRoute::Generate);
    }

    #[test]
    fn sparse_or_frozen_states_route_to_generation() {
        let long = 9000;
        assert_eq!(route(&states(2, true), long, true), VisualRoute::Generate);
        assert_eq!(route(&states(4, false), long, true), VisualRoute::Generate);
        // 足够多且有推进的状态保留受控渲染。
        assert_eq!(route(&states(5, true), long, true), VisualRoute::Generate);
    }

    #[test]
    fn missing_semantics_and_shotcraft_keep_their_paths() {
        // 没有图解规格的普通主题由模型绘制，不再退回固定文字布局。
        assert_eq!(
            route(r#"{"kind":"hook","title":"T"}"#, 5000, true),
            VisualRoute::Generate
        );
        assert_eq!(route("自由画面描述", 5000, true), VisualRoute::Generate);
        assert_eq!(route("自由画面描述", 5000, false), VisualRoute::Generate);
    }

    #[test]
    fn frame_change_ignores_local_flicker_and_detects_motion() {
        let base = FrameProfile {
            luma: 40,
            color: [10, 20, 30],
            grid: vec![60; 256],
        };
        let mut flicker = base.clone();
        flicker.grid[0] = 255;
        flicker.grid[1] = 0;
        assert!(!frame_changed(&base, &flicker));
        // 小范围光点（7 个采样格，约 2.7%）仍是局部闪烁，不算推进。
        let mut sparkle = base.clone();
        sparkle.grid[..7].fill(200);
        assert!(!frame_changed(&base, &sparkle));
        // 受控状态演变里对象整体移动会带动大片采样格（18.75%）一起变化。
        let mut shift = base.clone();
        shift.grid[..48].fill(120);
        assert!(frame_changed(&base, &shift));
        let mut brighter = base.clone();
        brighter.luma = 44;
        assert!(frame_changed(&base, &brighter));
    }

    #[test]
    fn expected_events_follow_reference_density() {
        assert_eq!(expected_events(1500), 3);
        assert_eq!(expected_events(6000), 4);
        assert_eq!(expected_events(12000), 8);
    }

    /// 通用主题：先受控渲染 TCP 状态演变，再用抽帧检查确认画面真的在推进。
    ///
    /// 这个测试只在本地执行 FFmpeg，不调用任何付费接口。
    #[tokio::test]
    async fn rendered_states_shot_has_measurable_motion() {
        use crate::providers::MediaData;
        let prompt = r#"{"kind":"mechanism","title":"TCP 三次握手","labels":[],"note":"",
            "visual":{"type":"states","presentation":"explicit_text","conclusion":"连接建立，双方状态同步","states":[
            {"caption":"客户端请求建连","objects":[{"text":"客户端","emphasis":"accent"},{"text":"服务端","emphasis":"plain"},{"text":"CLOSED","emphasis":"dim"}]},
            {"caption":"发送 SYN","objects":[{"text":"客户端","emphasis":"accent"},{"text":"SYN","emphasis":"accent"},{"text":"服务端","emphasis":"plain"}]},
            {"caption":"服务端回复 SYN+ACK","objects":[{"text":"客户端","emphasis":"plain"},{"text":"SYN+ACK","emphasis":"accent"},{"text":"服务端","emphasis":"accent"}]},
            {"caption":"客户端确认 ACK","objects":[{"text":"客户端","emphasis":"accent"},{"text":"ACK","emphasis":"accent"},{"text":"服务端","emphasis":"plain"}]},
            {"caption":"连接已建立","objects":[{"text":"客户端","emphasis":"plain"},{"text":"ESTABLISHED","emphasis":"accent"},{"text":"服务端","emphasis":"plain"}]}]}}"#;
        let shot: crate::models::Shot = serde_json::from_value(serde_json::json!({
            "id": "tcp",
            "duration_ms": 9000,
            "narration": "客户端发送 SYN，服务端回 SYN+ACK，客户端再发 ACK，连接建立。",
            "visual_prompt": prompt,
        }))
        .unwrap();
        assert_eq!(route(prompt, 9000, true), VisualRoute::States);
        let settings = crate::models::Settings {
            target_duration_seconds: 15,
            aspect_ratio: "9:16".into(),
            language: "zh-CN".into(),
            style_id: "minimal".into(),
            visual_format: "svg".into(),
            render_profile: "vibe".into(),
            output_resolution: "1080p".into(),
        };
        let media = crate::vibe::render(&shot, 0, 1, &settings, None, 0, true)
            .await
            .expect("状态演变镜头应渲染为真实 MP4");
        let MediaData::Bytes(bytes) = media.data else {
            panic!("必须返回可入库的视频字节");
        };
        assert!(bytes.len() > 50_000, "成片字节过少：{}", bytes.len());
        let path = std::env::temp_dir().join("explaincraft-visual-plan-tcp.mp4");
        tokio::fs::write(&path, &bytes).await.unwrap();
        if let Ok(dir) = std::env::var("EXPLAINCRAFT_ARTIFACT_DIR") {
            std::fs::create_dir_all(&dir).ok();
            std::fs::write(format!("{dir}/tcp-states-1080p.mp4"), &bytes).ok();
        }
        let motion = media_has_motion(&path, shot.duration_ms).await;
        let _ = tokio::fs::remove_file(&path).await;
        assert_eq!(motion, Some(true), "受控状态演变必须包含可见推进");
    }

    /// 通用主题：牛顿第二定律同样走受控状态演变，不写任何主题专用动画函数。
    ///
    /// 同一个通用流水线、同一个渲染器；只在本地执行 FFmpeg，不调用付费接口。
    #[tokio::test]
    async fn rendered_newton_states_shot_has_measurable_motion() {
        use crate::providers::MediaData;
        let prompt = r#"{"kind":"mechanism","title":"牛顿第二定律 F=ma","labels":[],"note":"恒力 12N",
            "visual":{"type":"states","presentation":"explicit_text","conclusion":"合外力不变时，质量越大加速度越小","states":[
            {"caption":"初始静止，合外力为零","objects":[{"text":"物块 m","emphasis":"plain"},{"text":"F = 0 N","emphasis":"plain"},{"text":"a = 0","emphasis":"dim"}]},
            {"caption":"施加恒力 12N","objects":[{"text":"物块 m=3kg","emphasis":"accent"},{"text":"F = 12 N","emphasis":"accent"},{"text":"a = 4 m/s²","emphasis":"plain"}]},
            {"caption":"质量增大到 6kg","objects":[{"text":"物块 m=6kg","emphasis":"plain"},{"text":"F = 12 N","emphasis":"plain"},{"text":"a = 2 m/s²","emphasis":"accent"}]},
            {"caption":"质量再增大到 12kg","objects":[{"text":"物块 m=12kg","emphasis":"plain"},{"text":"F = 12 N","emphasis":"plain"},{"text":"a = 1 m/s²","emphasis":"accent"}]},
            {"caption":"F 不变，m 越大 a 越小","objects":[{"text":"物块 m","emphasis":"plain"},{"text":"F = ma","emphasis":"accent"},{"text":"a = F / m","emphasis":"accent"}]}]}}"#;
        let shot: crate::models::Shot = serde_json::from_value(serde_json::json!({
            "id": "newton",
            "duration_ms": 9000,
            "narration": "恒力为 12 牛，物块 3 千克时加速度 4 米每二次方秒；质量变成 6 千克，加速度降到 2。",
            "visual_prompt": prompt,
        }))
        .unwrap();
        assert_eq!(route(prompt, 9000, true), VisualRoute::States);
        let settings = crate::models::Settings {
            target_duration_seconds: 15,
            aspect_ratio: "9:16".into(),
            language: "zh-CN".into(),
            style_id: "minimal".into(),
            visual_format: "svg".into(),
            render_profile: "vibe".into(),
            output_resolution: "1080p".into(),
        };
        let media = crate::vibe::render(&shot, 0, 1, &settings, None, 0, true)
            .await
            .expect("牛顿第二定律镜头应渲染为真实 MP4");
        let MediaData::Bytes(bytes) = media.data else {
            panic!("必须返回可入库的视频字节");
        };
        assert!(bytes.len() > 50_000, "成片字节过少：{}", bytes.len());
        let path = std::env::temp_dir().join("explaincraft-visual-plan-newton.mp4");
        tokio::fs::write(&path, &bytes).await.unwrap();
        if let Ok(dir) = std::env::var("EXPLAINCRAFT_ARTIFACT_DIR") {
            std::fs::create_dir_all(&dir).ok();
            std::fs::write(format!("{dir}/newton-states-1080p.mp4"), &bytes).ok();
        }
        let motion = media_has_motion(&path, shot.duration_ms).await;
        let _ = tokio::fs::remove_file(&path).await;
        assert_eq!(motion, Some(true), "牛顿第二定律镜头必须包含可见推进");
    }
}
