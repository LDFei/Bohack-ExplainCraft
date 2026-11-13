//! 演示供应商：在四个模型还在调研时，用本地示例素材跑通完整流程。
//!
//! - llm1：按关键词套用咖啡 / 旅行 / 产品三个主题的文案与分镜，规则化处理修改意见；
//! - llm2：从示例图片中按镜头挑选画面；
//! - llm3：返回示例背景音；
//! - llm4：生成与镜头等长的静音 WAV 作为占位配音；
//! - 渲染器：返回与主题、比例匹配的预制 30 秒成片。
//!
//! 输出结构与真实供应商一致，替换时编排层无需改动。

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;

use super::*;
use crate::config::Config;
use crate::models::{Brief, Script, StageKey};

/// 演示供应商：同时实现四个模型和渲染器。
pub struct DemoProvider {
    /// 示例素材目录。
    media_dir: PathBuf,
    /// 每次调用的模拟耗时基准。
    delay: Duration,
}

/// 一个演示主题的全部内容。
struct Theme {
    /// 主题标识，同时是示例成片文件名的前缀。
    key: &'static str,
    /// 脚本标题。
    title: &'static str,
    /// 建议的项目标题。
    project_title: &'static str,
    /// 创作目的。
    label: &'static str,
    /// 四个镜头依次使用的示例图片（不含扩展名）。
    images: [&'static str; 4],
    /// 四句解说词，每句对应一个镜头。
    lines: [&'static str; 4],
    /// 四个镜头的画面描述。
    visuals: [&'static str; 4],
    /// 一句话创作方案。
    summary: &'static str,
}

/// 咖啡店、山野旅行、随行水杯三个演示主题，与前端演示数据一致。
const THEMES: [Theme; 3] = [
    Theme {
        key: "coffee",
        title: "一杯咖啡的慢时光",
        project_title: "咖啡店的温柔早晨",
        label: "店铺宣传",
        images: ["coffee-shop", "coffee-brew", "coffee-cup", "coffee-detail"],
        lines: [
            "城市醒来之前，先留一点时间给自己。",
            "听水流与咖啡相遇，让熟悉的香气慢慢散开。",
            "一杯刚刚好的温度，装下生活的小小美好。",
            "在这里，让忙碌慢下来。每一天，都值得好好开始。",
        ],
        visuals: [
            "清晨的咖啡店，阳光穿过窗户落在木质桌面",
            "手冲咖啡的细节，热水缓缓注入，蒸汽轻轻升起",
            "一杯精心制作的咖啡，温暖的光线与细腻的奶泡",
            "咖啡与日常的片刻，营造安静舒适的店铺氛围",
        ],
        summary: "用清晨的光线、手冲的细节和一杯咖啡的温度，讲一个让生活慢下来的故事。",
    },
    Theme {
        key: "travel",
        title: "去山野，找回自己",
        project_title: "把周末交给山野",
        label: "旅行记录",
        images: [
            "travel-road",
            "travel-mountain",
            "travel-lake",
            "travel-road",
        ],
        lines: [
            "把消息留在身后，把周末交给山野。",
            "走过的路，带我们看见更辽阔的世界。",
            "在山与湖之间，听一听自己的呼吸。",
            "不必去很远，只要向自由出发。",
        ],
        visuals: [
            "山间公路延伸向远方，阳光从树林间洒落",
            "巍峨山峰的远景，云层缓缓移动",
            "安静的高山湖泊，湖水倒映着远山",
            "回到开阔的自然风景，让画面缓缓淡出",
        ],
        summary: "从山间公路出发，经过远山与湖泊，用舒缓的节奏记录一次与自然的相遇。",
    },
    Theme {
        key: "product",
        title: "把日常，装进美好",
        project_title: "随行水杯产品短片",
        label: "产品介绍",
        images: [
            "product-bottle",
            "product-bottle",
            "product-bottle",
            "product-bottle",
        ],
        lines: [
            "好的设计，藏在每天都会用到的细节里。",
            "轻盈的手感，简单的线条，让日常多一点从容。",
            "无论工作还是出发，随时补充生活的能量。",
            "把美好带在身边，从每一口水开始。",
        ],
        visuals: [
            "水杯产品全貌，干净背景与柔和自然光",
            "产品瓶身和材质细节，突出简约设计",
            "水杯的日常使用场景，展现随行体验",
            "产品主体居中，留白呈现品牌文案",
        ],
        summary: "用干净的构图、产品细节和日常场景，让产品的设计与陪伴感自然呈现。",
    },
];

/// 按关键词推断主题；都不匹配时使用咖啡主题。
fn infer_theme(text: &str) -> &'static Theme {
    let any = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if any(&["旅行", "山野", "风景", "山峰", "露营", "湖泊"]) {
        &THEMES[1]
    } else if any(&["产品", "水杯", "保温", "商品", "耳机"]) {
        &THEMES[2]
    } else {
        &THEMES[0]
    }
}

/// 已有创作方案时按方案主题推断，否则按用户原话推断，保证同一项目前后一致。
fn theme_for(ctx: &ProductionContext) -> &'static Theme {
    match &ctx.brief {
        Some(brief) => infer_theme(&brief.subject),
        None => infer_theme(&ctx.user_text),
    }
}

/// 读取“30秒”“60 秒”之类的目标时长。
fn parse_seconds(text: &str) -> Option<u32> {
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c != '秒' {
            continue;
        }
        let mut j = i;
        while j > 0 && chars[j - 1] == ' ' {
            j -= 1;
        }
        let end = j;
        while j > 0 && chars[j - 1].is_ascii_digit() {
            j -= 1;
        }
        if j < end {
            let digits: String = chars[j..end].iter().collect();
            if let Ok(value) = digits.parse::<u32>() {
                return Some(value.clamp(15, 60));
            }
        }
    }
    None
}

/// 从用户原话中识别画面比例与时长并写入设置；返回设置是否有变化。
fn apply_text_settings(settings: &mut Settings, text: &str) -> bool {
    let before = settings.clone();
    if text.contains("横屏") {
        settings.aspect_ratio = "16:9".into();
    } else if text.contains("方形") {
        settings.aspect_ratio = "1:1".into();
    } else if text.contains("竖屏") {
        settings.aspect_ratio = "9:16".into();
    }
    if let Some(seconds) = parse_seconds(text) {
        settings.target_duration_seconds = seconds;
    }
    *settings != before
}

/// 风格 ID 对应的风格描述。
fn style_name(id: &str) -> &'static str {
    match id {
        "cinematic" => "电影质感，画面开阔",
        "minimal" => "简约高级，留白克制",
        "energetic" => "活力轻快，节奏明快",
        _ => "温暖自然，细腻真实",
    }
}

/// 生成指定时长的静音 WAV（8kHz 单声道 16bit）。
fn silent_wav(duration_ms: i64) -> Vec<u8> {
    let sample_rate: u32 = 8000;
    let samples = (sample_rate as i64 * duration_ms.max(200) / 1000) as u32;
    let data_len = samples * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(44 + data_len as usize, 0);
    out
}

impl DemoProvider {
    /// 按配置创建演示供应商。
    pub fn new(config: &Config) -> Self {
        Self {
            media_dir: config.demo_media_dir.clone(),
            delay: Duration::from_millis(config.demo_delay_ms),
        }
    }

    /// 模拟模型耗时；`factor` 为相对基准耗时的倍数。
    async fn think(&self, factor: f64) {
        tokio::time::sleep(self.delay.mul_f64(factor)).await;
    }

    /// 取示例素材的完整路径；文件缺失时返回可读的错误。
    fn file(&self, name: &str) -> ProviderResult<PathBuf> {
        let path = self.media_dir.join(name);
        if path.is_file() {
            Ok(path)
        } else {
            tracing::error!("demo media missing: {}", path.display());
            Err(ProviderError::unavailable(
                "示例素材缺失，请检查 DEMO_MEDIA_DIR 配置。",
            ))
        }
    }
}

#[async_trait]
impl ScriptModel for DemoProvider {
    async fn plan(&self, request: PlanRequest) -> ProviderResult<PlanOutcome> {
        self.think(1.0).await;
        let ctx = request.ctx;
        if ctx.demo_scenario.as_deref() == Some("question") && ctx.answers.is_empty() {
            return Ok(PlanOutcome::NeedInput(QuestionSpec {
                prompt: "你希望片尾展示什么店名？也可以填写“不展示”。".into(),
                input_type: "text".into(),
                options: vec![],
            }));
        }
        let theme = infer_theme(&ctx.user_text);
        let mut settings = ctx.settings.clone();
        apply_text_settings(&mut settings, &ctx.user_text);
        let mut subject = theme.project_title.to_string();
        if let Some(answer) = ctx.answers.last() {
            let value = answer.value.trim();
            if !value.is_empty() && !value.contains("不展示") {
                subject = format!("{subject}（店名：{value}）");
            }
        }
        Ok(PlanOutcome::Ready(Plan {
            brief: Brief {
                subject,
                audience: "热爱生活的年轻人".into(),
                purpose: theme.label.into(),
                style: style_name(&settings.style_id).into(),
            },
            project_title: theme.project_title.into(),
            summary: theme.summary.into(),
            settings,
        }))
    }

    async fn write_script(&self, request: ScriptRequest) -> ProviderResult<Script> {
        self.think(1.0).await;
        let theme = infer_theme(&request.brief.subject);
        let mut lines: Vec<String> = theme.lines.iter().map(|s| s.to_string()).collect();
        if let Some(start) = request.brief.subject.find("（店名：") {
            let name = request.brief.subject[start + "（店名：".len()..].trim_end_matches('）');
            lines.push(format!("{name}，等你来。"));
        }
        Ok(Script {
            title: theme.title.into(),
            text: lines.join("\n"),
        })
    }

    async fn storyboard(&self, request: StoryboardRequest) -> ProviderResult<Vec<ShotPlan>> {
        self.think(1.0).await;
        let theme = infer_theme(&request.brief.subject);
        let lines: Vec<&str> = request
            .script
            .text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(8)
            .collect();
        if lines.is_empty() {
            return Err(ProviderError::invalid("脚本内容为空，无法规划分镜。"));
        }
        let total = request.ctx.settings.target_duration_seconds as i64 * 1000;
        let each = total / lines.len() as i64;
        let sizes = ["远景", "中景", "特写", "近景"];
        let motions = ["缓慢推进", "平稳横移", "轻微拉远", "固定镜头"];
        Ok(lines
            .iter()
            .enumerate()
            .map(|(i, line)| ShotPlan {
                duration_ms: if i + 1 == lines.len() {
                    total - each * i as i64
                } else {
                    each
                },
                narration: line.to_string(),
                visual_prompt: theme.visuals[i % 4].into(),
                shot_size: sizes[i % 4].into(),
                camera_motion: motions[i % 4].into(),
            })
            .collect())
    }

    async fn revise(&self, request: ReviseRequest) -> ProviderResult<Revision> {
        self.think(1.0).await;
        let text = request.instruction.as_str();
        let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
        let mut draft = request.draft;
        let mut invalidated = vec![StageKey::Edit, StageKey::Finishing, StageKey::Render];
        let mut notes = vec![];

        if has(&["慢", "温柔"]) {
            draft.audio.speech_rate = 0.9;
            invalidated.push(StageKey::Audio);
            notes.push("语速放慢");
        }
        if has(&["轻快", "快一点"]) {
            draft.audio.speech_rate = 1.1;
            draft.settings.style_id = "energetic".into();
            invalidated.push(StageKey::Audio);
            notes.push("节奏更轻快");
        }
        if has(&["男声"]) {
            draft.audio.voice_id = "voice_calm".into();
            invalidated.push(StageKey::Audio);
            notes.push("换成男声配音");
        } else if has(&["女声"]) {
            draft.audio.voice_id = "voice_warm".into();
            invalidated.push(StageKey::Audio);
            notes.push("换成女声配音");
        }
        if has(&["不要音乐", "去掉音乐", "去掉背景音"]) {
            draft.audio.music_mode = "none".into();
            draft.audio.music_id = None;
            draft.audio.music_asset_id = None;
            invalidated.push(StageKey::Audio);
            notes.push("去掉背景音乐");
        }
        if has(&["电影"]) {
            draft.settings.style_id = "cinematic".into();
            draft.finishing.color_preset_id = "cool".into();
            notes.push("加强电影质感");
        }
        if has(&["暖色", "暖调"]) {
            draft.finishing.color_preset_id = "warm".into();
            notes.push("调成暖色调");
        }
        if has(&["不要字幕", "关闭字幕", "去掉字幕"]) {
            draft.finishing.subtitles_enabled = false;
            notes.push("关闭字幕");
        }
        if has(&["淡入", "淡出", "转场"]) {
            draft.finishing.effect_preset_id = "fade".into();
            notes.push("加入淡入淡出转场");
        }
        let mut settings = draft.settings.clone();
        if apply_text_settings(&mut settings, text) {
            if settings.target_duration_seconds != draft.settings.target_duration_seconds {
                // 时长变化：保留文案，重新分镜。
                draft.shots.clear();
                invalidated.extend(StageKey::Storyboard.and_after());
                notes.push("调整视频时长");
            } else {
                invalidated.extend(StageKey::Visuals.and_after());
                notes.push("调整画面比例");
            }
            draft.settings = settings;
        }
        let ordinals = [
            ("第一", 0),
            ("第1", 0),
            ("第二", 1),
            ("第2", 1),
            ("第三", 2),
            ("第3", 2),
            ("第四", 3),
            ("第4", 3),
        ];
        for (word, index) in ordinals {
            if text.contains(word) {
                if let Some(shot) = draft.shots.get_mut(index) {
                    shot.visual_prompt = format!("根据修改意见调整：{text}");
                    shot.asset_ids.clear();
                    invalidated.push(StageKey::Visuals);
                    notes.push("更换指定镜头的画面");
                }
                break;
            }
        }
        let summary = if notes.is_empty() {
            "已记录你的修改意见，重新整理剪辑节奏。".to_string()
        } else {
            format!("收到，这次会{}。", notes.join("、"))
        };
        Ok(Revision {
            draft,
            invalidated,
            summary,
        })
    }

    async fn edit_plan(&self, request: EditRequest) -> ProviderResult<EditPlan> {
        self.think(0.8).await;
        let draft = request.draft;
        let transition = if draft.finishing.effect_preset_id == "fade" {
            "fade"
        } else {
            "cut"
        };
        let mut start = 0;
        let clips = draft
            .shots
            .iter()
            .map(|shot| {
                let clip = Clip {
                    shot_id: shot.id.clone(),
                    start_ms: start,
                    duration_ms: shot.duration_ms,
                    visual_asset_id: shot.asset_ids.first().cloned(),
                    narration_asset_id: request.narration.get(&shot.id).map(|(id, _)| id.clone()),
                    subtitle: draft
                        .finishing
                        .subtitles_enabled
                        .then(|| shot.narration.clone()),
                    transition: transition.into(),
                    camera_motion: shot.camera_motion.clone(),
                };
                start += shot.duration_ms;
                clip
            })
            .collect();
        let mut plan = EditPlan {
            aspect_ratio: draft.settings.aspect_ratio.clone(),
            total_ms: start,
            clips,
            music_asset_id: draft.audio.resolved_music_asset_id.clone(),
            music_duration_ms: request.music_duration_ms,
            music_volume: draft.audio.music_volume,
            narration_volume: draft.audio.narration_volume,
            color_preset_id: draft.finishing.color_preset_id.clone(),
            effect_preset_id: draft.finishing.effect_preset_id.clone(),
            subtitles_enabled: draft.finishing.subtitles_enabled,
            ops: vec![],
        };
        // 演示版按时间线生成标准剪辑指令；真实模型可以直接输出指令数组（见 edit_ops）。
        plan.ops = super::edit_ops::ops_from_plan(&plan);
        Ok(plan)
    }

    async fn chat(&self, request: ChatRequest) -> ProviderResult<String> {
        self.think(1.0).await;
        let text = request.text;
        Ok(if text.contains("你好") || text.contains("怎么用") {
            "你好！告诉我视频的主题和想要的感觉，就可以开始。也可以上传自己的照片，或在右侧调整脚本、配音和字幕。切回「视频创作」即可生成。".into()
        } else {
            "建议先明确一个重点，用开头吸引注意，中段展示两三个有记忆点的细节，最后用一句话收尾。30 秒视频适合 4～6 个镜头。当前是仅讨论模式，不会修改或生成视频。".into()
        })
    }
}

#[async_trait]
impl VisualModel for DemoProvider {
    async fn generate_shot(&self, request: VisualRequest) -> ProviderResult<MediaOutput> {
        self.think(0.5).await;
        if request.ctx.demo_scenario.as_deref() == Some("failure") {
            return Err(ProviderError::unavailable(
                "画面准备暂时中断，已保留脚本和分镜。重试后可以继续。",
            ));
        }
        let theme = theme_for(&request.ctx);
        let name = format!("{}.jpg", theme.images[request.shot_index % 4]);
        Ok(MediaOutput {
            data: MediaData::Path(self.file(&name)?),
            file_name: name,
            mime_type: "image/jpeg".into(),
            kind: "image".into(),
            source: "library".into(),
            width: None,
            height: None,
            duration_ms: None,
        })
    }
}

#[async_trait]
impl MusicModel for DemoProvider {
    async fn compose(&self, request: MusicRequest) -> ProviderResult<MediaOutput> {
        self.think(0.8).await;
        Ok(MediaOutput {
            data: MediaData::Path(self.file("ambient.wav")?),
            file_name: format!("{}.wav", request.track_id.as_deref().unwrap_or("auto-bgm")),
            mime_type: "audio/wav".into(),
            kind: "audio".into(),
            source: if request.mode == "library" {
                "library".into()
            } else {
                "generated".into()
            },
            width: None,
            height: None,
            duration_ms: Some(request.duration_ms),
        })
    }
}

#[async_trait]
impl VoiceModel for DemoProvider {
    async fn synthesize(&self, request: VoiceRequest) -> ProviderResult<MediaOutput> {
        self.think(0.3).await;
        let duration = request.target_duration_ms.max(500);
        Ok(MediaOutput {
            data: MediaData::Bytes(silent_wav(duration)),
            file_name: format!("narration-{}.wav", request.voice_id),
            mime_type: "audio/wav".into(),
            kind: "audio".into(),
            source: "generated".into(),
            width: None,
            height: None,
            duration_ms: Some(duration),
        })
    }
}

#[async_trait]
impl Renderer for DemoProvider {
    async fn render(&self, request: RenderRequest) -> ProviderResult<MediaOutput> {
        self.think(1.2).await;
        let theme = theme_for(&request.ctx);
        let ratio = request.plan.aspect_ratio.replace(':', "x");
        let name = format!("{}-{ratio}.mp4", theme.key);
        let (width, height) = crate::util::dimensions(&request.plan.aspect_ratio);
        Ok(MediaOutput {
            data: MediaData::Path(
                self.file(&name)
                    .map_err(|_| ProviderError::render_failed("成片合成失败，请稍后重试。"))?,
            ),
            file_name: name,
            mime_type: "video/mp4".into(),
            kind: "video".into(),
            source: "rendered".into(),
            width: Some(width),
            height: Some(height),
            // 预制示例成片固定 30 秒。
            duration_ms: Some(30_000),
        })
    }

    async fn export(&self, request: ExportRequest) -> ProviderResult<MediaOutput> {
        self.think(1.0).await;
        if request.resolution != "720p" || request.format != "mp4" {
            return Err(ProviderError::invalid(
                "演示渲染器目前只支持 720p MP4 导出。",
            ));
        }
        let (width, height) = crate::util::dimensions(&request.aspect_ratio);
        Ok(MediaOutput {
            data: MediaData::Path(request.source),
            file_name: "export.mp4".into(),
            mime_type: "video/mp4".into(),
            kind: "video".into(),
            source: "rendered".into(),
            width: Some(width),
            height: Some(height),
            duration_ms: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_target_seconds() {
        assert_eq!(parse_seconds("帮我做一个30秒的视频"), Some(30));
        assert_eq!(parse_seconds("改成 60 秒"), Some(60));
        assert_eq!(parse_seconds("5秒"), Some(15));
        assert_eq!(parse_seconds("没有时长"), None);
    }

    #[test]
    fn infers_theme_by_keywords() {
        assert_eq!(infer_theme("山野旅行短片").key, "travel");
        assert_eq!(infer_theme("随行水杯").key, "product");
        assert_eq!(infer_theme("咖啡店").key, "coffee");
    }

    #[test]
    fn silent_wav_has_valid_header() {
        let wav = silent_wav(1000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 16000);
    }
}
