//! 硅基流动（SiliconFlow）上的云端模型：
//!
//! | 环节                     | 模型（默认）              | 接口                                   |
//! | ------------------------ | ------------------------- | -------------------------------------- |
//! | llm1 文案 / 分镜 / 剪辑  | `zai-org/GLM-5.3`         | `/chat/completions`（关闭思考，输出 JSON） |
//! | llm2 画面                | `Qwen/Qwen-Image`         | `/images/generations`                  |
//! | llm3 背景音乐            | `MiniMaxAI/MiniMax-H3`    | `/video/submit` + `/video/status`（取生成视频的音轨） |
//!
//! 约定：
//! - 文本模型一律要求只输出 JSON，解析时容忍代码块包裹和前后多余文字；
//! - 模型输出只作为“建议”，进入草稿前做范围校验与兜底（时长归一、比例白名单等）；
//! - 网络或模型错误转换为可重试的 [`ProviderError`]，原始信息只写日志，不展示给用户，也不记录密钥。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::edit_ops::{ops_from_plan, EditOp, TrackType};
use super::*;
use crate::config::{Config, ModelAccess};
use crate::models::{Brief, Draft, Script, StageKey};

// ---------- HTTP 客户端 ----------

/// 硅基流动 HTTP 客户端，三个模型共用连接池。
pub struct SiliconFlow {
    /// HTTP 客户端（直连，不走系统代理）。
    http: reqwest::Client,
    /// 接口地址，如 `https://api.siliconflow.cn/v1`。
    base: String,
}

impl SiliconFlow {
    /// 创建客户端。单次请求超时 5 分钟（文生图、长文本生成可能较慢）。
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self {
            http,
            base: config.siliconflow_base.clone(),
        })
    }

    /// POST JSON 并解析 JSON 响应；HTTP 错误转为可重试的供应商错误。
    async fn post(
        &self,
        access: &ModelAccess,
        path: &str,
        body: &Value,
        what: &str,
    ) -> ProviderResult<Value> {
        if access.api_key.is_empty() {
            return Err(ProviderError::unavailable(format!(
                "{what}未配置密钥，请在 back/.env 中设置。"
            )));
        }
        let response = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&access.api_key)
            // 文生图默认带“AI生成”水印，成片中不需要。
            .header("X-Enable-Watermark", "0")
            .json(body)
            .send()
            .await
            .map_err(|e| unavailable(what, format!("请求失败：{e}")))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| unavailable(what, format!("读取响应失败：{e}")))?;
        if !status.is_success() {
            let reason = match status.as_u16() {
                401 | 403 => "密钥无效或无权访问该模型",
                429 => "请求过于频繁或额度不足",
                _ => "服务暂时不可用",
            };
            tracing::warn!(target: "job", "{what} HTTP {status}: {}", preview(&text));
            return Err(ProviderError::unavailable(format!(
                "{what}{reason}，请稍后重试。"
            )));
        }
        serde_json::from_str(&text).map_err(|e| unavailable(what, format!("响应不是 JSON：{e}")))
    }

    /// 下载生成结果（图片、视频）。
    async fn download(&self, url: &str, what: &str) -> ProviderResult<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| unavailable(what, format!("下载失败：{e}")))?;
        if !response.status().is_success() {
            return Err(unavailable(
                what,
                format!("下载失败：HTTP {}", response.status()),
            ));
        }
        Ok(response
            .bytes()
            .await
            .map_err(|e| unavailable(what, format!("下载失败：{e}")))?
            .to_vec())
    }

    /// 调用对话模型，返回文本内容。关闭思考模式（GLM-5.3 关闭后约 2 秒返回，输出更稳定）。
    async fn chat(
        &self,
        access: &ModelAccess,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> ProviderResult<String> {
        let body = json!({
            "model": access.model,
            "enable_thinking": false,
            "temperature": 0.7,
            "max_tokens": max_tokens,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        });
        let data = self
            .post(access, "/chat/completions", &body, "文案模型")
            .await?;
        data["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| unavailable("文案模型", "返回内容为空".to_string()))
    }

    /// 调用对话模型并解析 JSON 输出。
    async fn chat_json(
        &self,
        access: &ModelAccess,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> ProviderResult<Value> {
        let system =
            format!("{system}\n只输出一个 JSON 对象，不要输出解释、标题或 Markdown 代码块。");
        let text = self.chat(access, &system, user, max_tokens).await?;
        extract_json(&text).ok_or_else(|| {
            tracing::warn!(target: "job", "文案模型输出不是 JSON：{}", preview(&text));
            ProviderError::unavailable("文案模型返回的格式不正确，请重试。")
        })
    }
}

/// 构造可重试的供应商错误，详细原因只写日志。
fn unavailable(what: &str, detail: String) -> ProviderError {
    tracing::warn!(target: "job", "{what}：{detail}");
    ProviderError::unavailable(format!("{what}暂时不可用，请稍后重试。"))
}

/// 日志中的长文本预览。
fn preview(text: &str) -> String {
    crate::util::preview(text, 200)
}

/// 从模型输出中提取 JSON：容忍 ```json 代码块与前后多余文字。
fn extract_json(text: &str) -> Option<Value> {
    let trimmed = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Some(value);
    }
    // 退而求其次：取第一个 { 到最后一个 } 之间的内容。
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str(&trimmed[start..=end]).ok()
}

/// 读取字符串字段，去掉首尾空白；缺失时返回默认值。
fn text_or(value: &Value, key: &str, default: &str) -> String {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// 风格 ID 对应的中文描述，写进提示词。
fn style_text(style_id: &str) -> &'static str {
    match style_id {
        "cinematic" => "电影质感，画面开阔，光影对比强",
        "minimal" => "简约高级，构图干净，留白克制",
        "energetic" => "活力轻快，色彩明亮，节奏明快",
        _ => "温暖治愈，柔和自然光，细腻真实",
    }
}

/// 中文旁白的语速估算：每秒约 4 个字。
const CHARS_PER_SECOND: f64 = 4.0;

// ---------- llm1：GLM 文案 / 分镜 / 剪辑 ----------

/// llm1：基于 GLM 的文案、分镜、修改意见理解与剪辑指令生成。
pub struct GlmScript {
    /// 共用客户端。
    client: Arc<SiliconFlow>,
    /// 密钥与模型。
    access: ModelAccess,
}

impl GlmScript {
    /// 创建 llm1 供应商。
    pub fn new(client: Arc<SiliconFlow>, config: &Config) -> Self {
        Self {
            client,
            access: config.llm1.clone(),
        }
    }
}

/// 镜头数：每个镜头约 7 秒，限制在 3～8 个。
fn shot_count(seconds: u32) -> usize {
    ((seconds as f64 / 7.0).round() as usize).clamp(3, 8)
}

#[async_trait]
impl ScriptModel for GlmScript {
    async fn plan(&self, request: PlanRequest) -> ProviderResult<PlanOutcome> {
        let ctx = request.ctx;
        let answers: Vec<&str> = ctx.answers.iter().map(|a| a.value.as_str()).collect();
        let attachments: Vec<String> = request
            .attachments
            .iter()
            .map(|a| format!("{}（{}）", a.file_name, a.kind))
            .collect();
        let user = format!(
            "用户需求：{}\n补充信息：{}\n用户上传的素材：{}\n当前设置：比例 {}，目标时长 {} 秒，风格「{}」。\n\
             请整理创作方案，输出 JSON：\n\
             {{\"subject\": 视频主题, \"audience\": 目标受众, \"purpose\": 目的（如店铺宣传、知识科普、情感共鸣）, \
             \"style\": 内容风格描述, \"project_title\": 不超过12字的项目名, \"summary\": 不超过50字的一句话创作方案, \
             \"aspect_ratio\": 用户明确要求横屏填\"16:9\"、方形填\"1:1\"、竖屏填\"9:16\"，否则沿用当前设置, \
             \"target_duration_seconds\": 用户明确提到时长时填 15～60 的整数，否则沿用当前设置}}",
            ctx.user_text,
            if answers.is_empty() { "无".into() } else { answers.join("；") },
            if attachments.is_empty() { "无".into() } else { attachments.join("、") },
            ctx.settings.aspect_ratio,
            ctx.settings.target_duration_seconds,
            style_text(&ctx.settings.style_id),
        );
        let value = self
            .client
            .chat_json(
                &self.access,
                "你是短视频创作策划，帮完全不懂视频制作的用户把一句话需求整理成创作方案。",
                &user,
                800,
            )
            .await?;
        let mut settings = ctx.settings.clone();
        if let Some(ratio) = value["aspect_ratio"]
            .as_str()
            .filter(|r| ["9:16", "16:9", "1:1"].contains(r))
        {
            settings.aspect_ratio = ratio.into();
        }
        if let Some(seconds) = value["target_duration_seconds"].as_u64() {
            settings.target_duration_seconds = (seconds as u32).clamp(15, 60);
        }
        let subject = text_or(&value, "subject", &ctx.user_text);
        Ok(PlanOutcome::Ready(Plan {
            brief: Brief {
                subject: subject.clone(),
                audience: text_or(&value, "audience", "大众"),
                purpose: text_or(&value, "purpose", "分享"),
                style: text_or(&value, "style", style_text(&settings.style_id)),
            },
            project_title: crate::util::preview(&text_or(&value, "project_title", &subject), 20),
            summary: text_or(&value, "summary", &subject),
            settings,
        }))
    }

    async fn write_script(&self, request: ScriptRequest) -> ProviderResult<Script> {
        let seconds = request.ctx.settings.target_duration_seconds;
        let count = shot_count(seconds);
        let per_line = ((seconds as f64 / count as f64) * CHARS_PER_SECOND).round() as usize;
        let brief = &request.brief;
        let user = format!(
            "主题：{}\n受众：{}\n目的：{}\n风格：{}\n用户原话：{}\n\
             请写一段 {seconds} 秒短视频的解说词，共 {count} 句，每句约 {per_line} 个字以内（中文语速约每秒 4 字），\
             第一句吸引注意，最后一句收尾点题。不要出现镜头说明、括号或表情符号。\n\
             输出 JSON：{{\"title\": 不超过15字的标题, \"lines\": [{count} 句解说词]}}",
            brief.subject, brief.audience, brief.purpose, brief.style, request.ctx.user_text,
        );
        let value = self
            .client
            .chat_json(
                &self.access,
                "你是短视频文案，擅长写逻辑清晰、语言精炼、适合朗读的解说词。",
                &user,
                1200,
            )
            .await?;
        let lines: Vec<String> = value["lines"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|line| line.as_str())
                    .map(|line| line.trim().to_string())
                    .filter(|line| !line.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if lines.is_empty() {
            return Err(ProviderError::unavailable("文案生成结果为空，请重试。"));
        }
        Ok(Script {
            title: crate::util::preview(&text_or(&value, "title", &brief.subject), 30),
            text: lines.join("\n"),
        })
    }

    async fn storyboard(&self, request: StoryboardRequest) -> ProviderResult<Vec<ShotPlan>> {
        let lines: Vec<&str> = request
            .script
            .text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            return Err(ProviderError::invalid("脚本内容为空，无法规划分镜。"));
        }
        let settings = &request.ctx.settings;
        let total_ms = settings.target_duration_seconds as i64 * 1000;
        let numbered: Vec<String> = lines
            .iter()
            .enumerate()
            .map(|(i, l)| format!("{}. {l}", i + 1))
            .collect();
        let user = format!(
            "主题：{}\n风格：{}\n画面比例：{}\n总时长：{} 秒\n解说词（每句对应一个镜头）：\n{}\n\
             请为每句解说词设计一个镜头，输出 JSON：{{\"shots\": [{{\"narration\": 原句, \
             \"visual_prompt\": 用于文生图的详细中文画面描述（主体、环境、光线、色调、构图，不要出现文字和人物对白）, \
             \"shot_size\": 景别（远景/全景/中景/近景/特写）, \"camera_motion\": 运镜（推/拉/摇/移/固定）, \
             \"duration_ms\": 该镜头毫秒数}}]}}，镜头数与解说词句数相同，时长总和约等于总时长。",
            request.brief.subject,
            request.brief.style,
            settings.aspect_ratio,
            settings.target_duration_seconds,
            numbered.join("\n"),
        );
        let value = self
            .client
            .chat_json(
                &self.access,
                "你是短视频分镜师，把文案转化为具体、可拍摄的画面。",
                &user,
                2500,
            )
            .await?;
        let items = value["shots"].as_array().cloned().unwrap_or_default();
        // 以解说词为准：镜头数与句数一致，模型缺的补默认画面。
        let mut shots: Vec<ShotPlan> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let item = items.get(i).cloned().unwrap_or(Value::Null);
                let estimate =
                    ((line.chars().count() as f64 / CHARS_PER_SECOND) * 1000.0) as i64 + 800;
                ShotPlan {
                    duration_ms: item["duration_ms"]
                        .as_i64()
                        .filter(|d| *d > 0)
                        .unwrap_or(estimate),
                    narration: line.to_string(),
                    visual_prompt: text_or(
                        &item,
                        "visual_prompt",
                        &format!("{}，{}", request.brief.subject, line),
                    ),
                    shot_size: text_or(&item, "shot_size", "中景"),
                    camera_motion: text_or(&item, "camera_motion", "缓慢推进"),
                }
            })
            .collect();
        // 时长归一到目标总时长，每个镜头不少于 2 秒。
        let sum: i64 = shots.iter().map(|s| s.duration_ms).sum::<i64>().max(1);
        for shot in &mut shots {
            shot.duration_ms =
                ((shot.duration_ms as f64 * total_ms as f64 / sum as f64) as i64).max(2000);
        }
        Ok(shots)
    }

    async fn revise(&self, request: ReviseRequest) -> ProviderResult<Revision> {
        let mut draft = request.draft;
        let shots: Vec<String> = draft
            .shots
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "{}. 旁白「{}」画面「{}」",
                    i + 1,
                    s.narration,
                    s.visual_prompt
                )
            })
            .collect();
        let user = format!(
            "当前设置：比例 {}，时长 {} 秒，风格 {}，语速 {}，音色 {}，背景音乐 {}，字幕 {}，转场 {}，调色 {}\n\
             当前分镜：\n{}\n\n用户的修改意见：{}\n\n\
             只改用户提到的部分，输出 JSON（没有改动的字段填 null）：\n\
             {{\"summary\": 一句话说明这次改了什么, \"speech_rate\": 0.5～2.0 或 null, \
             \"voice\": \"male\"/\"female\"/null, \"style_id\": \"warm\"/\"cinematic\"/\"minimal\"/\"energetic\"/null, \
             \"aspect_ratio\": \"9:16\"/\"16:9\"/\"1:1\"/null, \"target_duration_seconds\": 15～60 或 null, \
             \"music\": \"on\"/\"off\"/null, \"subtitles\": true/false/null, \"fade\": true/false/null, \
             \"color_preset_id\": \"natural\"/\"warm\"/\"cool\"/null, \
             \"shots\": [{{\"index\": 镜头序号（从1开始）, \"visual_prompt\": 新画面描述或 null, \"narration\": 新旁白或 null}}]}}",
            draft.settings.aspect_ratio,
            draft.settings.target_duration_seconds,
            draft.settings.style_id,
            draft.audio.speech_rate,
            draft.audio.voice_id,
            draft.audio.music_mode,
            draft.finishing.subtitles_enabled,
            draft.finishing.effect_preset_id,
            draft.finishing.color_preset_id,
            shots.join("\n"),
            request.instruction,
        );
        let value = self
            .client
            .chat_json(
                &self.access,
                "你是短视频剪辑助手，把用户的口语化修改意见转换为结构化的修改。",
                &user,
                1500,
            )
            .await?;
        let mut invalidated = vec![StageKey::Edit, StageKey::Finishing, StageKey::Render];
        if let Some(rate) = value["speech_rate"].as_f64() {
            draft.audio.speech_rate = rate.clamp(0.5, 2.0);
            invalidated.push(StageKey::Audio);
        }
        match value["voice"].as_str() {
            Some("male") => {
                draft.audio.voice_id = "voice_calm".into();
                invalidated.push(StageKey::Audio);
            }
            Some("female") => {
                draft.audio.voice_id = "voice_warm".into();
                invalidated.push(StageKey::Audio);
            }
            _ => {}
        }
        match value["music"].as_str() {
            Some("off") => {
                draft.audio.music_mode = "none".into();
                draft.audio.music_id = None;
                draft.audio.music_asset_id = None;
                draft.audio.resolved_music_asset_id = None;
                invalidated.push(StageKey::Audio);
            }
            Some("on") if draft.audio.music_mode == "none" => {
                draft.audio.music_mode = "auto".into();
                invalidated.push(StageKey::Audio);
            }
            _ => {}
        }
        if let Some(on) = value["subtitles"].as_bool() {
            draft.finishing.subtitles_enabled = on;
        }
        if let Some(fade) = value["fade"].as_bool() {
            draft.finishing.effect_preset_id = if fade { "fade".into() } else { "none".into() };
        }
        if let Some(color) = value["color_preset_id"]
            .as_str()
            .filter(|c| ["natural", "warm", "cool"].contains(c))
        {
            draft.finishing.color_preset_id = color.into();
        }
        let mut regenerate_all_visuals = false;
        if let Some(style) = value["style_id"]
            .as_str()
            .filter(|s| ["warm", "cinematic", "minimal", "energetic"].contains(s))
        {
            if style != draft.settings.style_id {
                draft.settings.style_id = style.into();
                regenerate_all_visuals = true;
            }
        }
        if let Some(ratio) = value["aspect_ratio"]
            .as_str()
            .filter(|r| ["9:16", "16:9", "1:1"].contains(r))
        {
            if ratio != draft.settings.aspect_ratio {
                draft.settings.aspect_ratio = ratio.into();
                regenerate_all_visuals = true;
            }
        }
        if let Some(seconds) = value["target_duration_seconds"].as_u64() {
            let seconds = (seconds as u32).clamp(15, 60);
            if seconds != draft.settings.target_duration_seconds {
                // 时长变化：保留文案，重新分镜。
                draft.settings.target_duration_seconds = seconds;
                draft.shots.clear();
                invalidated.extend(StageKey::Storyboard.and_after());
            }
        }
        let mut narration_changed = false;
        for change in value["shots"].as_array().cloned().unwrap_or_default() {
            let Some(index) = change["index"]
                .as_u64()
                .and_then(|i| (i as usize).checked_sub(1))
            else {
                continue;
            };
            let Some(shot) = draft.shots.get_mut(index) else {
                continue;
            };
            if let Some(prompt) = change["visual_prompt"]
                .as_str()
                .filter(|p| !p.trim().is_empty())
            {
                shot.visual_prompt = prompt.trim().into();
                shot.asset_ids.clear();
                invalidated.push(StageKey::Visuals);
            }
            if let Some(narration) = change["narration"]
                .as_str()
                .filter(|n| !n.trim().is_empty())
            {
                shot.narration = narration.trim().into();
                narration_changed = true;
                invalidated.push(StageKey::Audio);
            }
        }
        if narration_changed {
            // 分镜旁白是脚本的来源：同步完整解说词。
            if let Some(script) = draft.script.as_mut() {
                script.text = draft
                    .shots
                    .iter()
                    .map(|s| s.narration.clone())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        if regenerate_all_visuals {
            for shot in &mut draft.shots {
                shot.asset_ids.clear();
            }
            invalidated.extend(StageKey::Visuals.and_after());
        }
        Ok(Revision {
            draft,
            invalidated,
            summary: text_or(&value, "summary", "已按你的意见调整。"),
        })
    }

    async fn edit_plan(&self, request: EditRequest) -> ProviderResult<EditPlan> {
        // 1. 时间线：镜头时长至少容纳配音（配音比镜头长时延长镜头，避免旁白被截断）。
        let mut plan = timeline(&request);
        // 2. 让 LLM 输出剪辑指令；不合法时退回标准指令。
        let fallback = ops_from_plan(&plan);
        let assets = describe_assets(&plan);
        let (width, height) = crate::util::dimensions(&plan.aspect_ratio);
        let user = format!(
            "请为短视频生成剪辑指令。画布 {width}x{height}，30fps，成片总时长 {} 毫秒。\n可用素材：\n{assets}\n\
             转场要求：{}。\n\
             指令格式（JSON 对象 {{\"ops\": [...]}}，按顺序执行）：\n\
             1. {{\"op\":\"create_sequence\",\"width\":{width},\"height\":{height},\"fps\":30}} —— 必须是第一条；\n\
             2. {{\"op\":\"import_media\",\"ref\":别名,\"asset_id\":素材ID}} —— 先导入再使用；\n\
             3. {{\"op\":\"place_clip\",\"media\":别名,\"track\":\"video\"或\"audio\",\"track_index\":整数,\"start_ms\":开始,\"end_ms\":结束,\"media_in_ms\":0,\"clip\":片段别名或null}} —— \
             画面放 video 轨 0；配音放 audio 轨 0，与对应画面同时开始；背景音乐放 audio 轨 1，比成片短时首尾相接重复铺满；\n\
             4. {{\"op\":\"add_crossfade\",\"previous\":前片段别名,\"next\":后片段别名,\"half_ms\":300}} —— 只用于 video 轨相邻片段。\n\
             时间不得超过总时长，所有素材都要用到。",
            plan.total_ms,
            if plan.effect_preset_id == "fade" { "相邻镜头之间加交叉淡化" } else { "直接硬切，不加转场" },
        );
        let ops = match self
            .client
            .chat_json(
                &self.access,
                "你是视频剪辑师，通过输出剪辑指令来操作剪辑软件。",
                &user,
                4000,
            )
            .await
        {
            Ok(value) => match serde_json::from_value::<Vec<EditOp>>(value["ops"].clone()) {
                Ok(ops) => match validate_ops(&ops, &plan) {
                    Ok(()) => {
                        tracing::info!(target: "job", "  · llm1 输出 {} 条剪辑指令，校验通过", ops.len());
                        ops
                    }
                    Err(reason) => {
                        tracing::warn!(target: "job", "  · llm1 剪辑指令校验未通过（{reason}），改用标准指令");
                        fallback
                    }
                },
                Err(error) => {
                    tracing::warn!(target: "job", "  · llm1 剪辑指令格式错误（{error}），改用标准指令");
                    fallback
                }
            },
            // 剪辑指令有确定的兜底方案：模型调用失败不让整个任务失败。
            Err(_) => fallback,
        };
        plan.ops = ops;
        Ok(plan)
    }

    async fn chat(&self, request: ChatRequest) -> ProviderResult<String> {
        let context = request
            .brief
            .map(|b| {
                format!(
                    "当前项目：{}（{}，面向{}）。",
                    b.subject, b.purpose, b.audience
                )
            })
            .unwrap_or_default();
        self.client
            .chat(
                &self.access,
                "你是“灵镜”视频创作助手，用简洁、友好的中文回答小白用户关于短视频创作的问题。当前是仅讨论模式，不会修改或生成视频。",
                &format!("{context}{}", request.text),
                800,
            )
            .await
    }
}

/// 根据草稿与配音时长生成时间线（剪辑方案中除指令外的部分）。
fn timeline(request: &EditRequest) -> EditPlan {
    let draft: &Draft = &request.draft;
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
            let narration = request.narration.get(&shot.id);
            // 配音比镜头长时延长镜头，并在句尾留 0.4 秒呼吸。
            let duration = narration
                .map(|(_, ms)| (*ms + 400).max(shot.duration_ms))
                .unwrap_or(shot.duration_ms);
            let clip = Clip {
                shot_id: shot.id.clone(),
                start_ms: start,
                duration_ms: duration,
                visual_asset_id: shot.asset_ids.first().cloned(),
                narration_asset_id: narration.map(|(id, _)| id.clone()),
                subtitle: draft
                    .finishing
                    .subtitles_enabled
                    .then(|| shot.narration.clone()),
                transition: transition.into(),
                camera_motion: shot.camera_motion.clone(),
            };
            start += duration;
            clip
        })
        .collect();
    EditPlan {
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
    }
}

/// 把时间线上的素材整理成提示词中的列表。
fn describe_assets(plan: &EditPlan) -> String {
    let mut lines = vec![];
    for (i, clip) in plan.clips.iter().enumerate() {
        let end = clip.start_ms + clip.duration_ms;
        if let Some(id) = &clip.visual_asset_id {
            lines.push(format!(
                "- 镜头{} 画面 asset_id={id}，应放在 {}～{} 毫秒",
                i + 1,
                clip.start_ms,
                end
            ));
        }
        if let Some(id) = &clip.narration_asset_id {
            lines.push(format!(
                "- 镜头{} 配音 asset_id={id}，应从 {} 毫秒开始，到 {} 毫秒结束",
                i + 1,
                clip.start_ms,
                end
            ));
        }
    }
    if let Some(id) = &plan.music_asset_id {
        let length = plan
            .music_duration_ms
            .map(|ms| format!("{ms} 毫秒"))
            .unwrap_or_else(|| "未知".into());
        lines.push(format!("- 背景音乐 asset_id={id}，自身时长 {length}"));
    }
    lines.join("\n")
}

/// 校验 LLM 输出的剪辑指令：顺序、引用、素材与时间范围都必须合法。
fn validate_ops(ops: &[EditOp], plan: &EditPlan) -> Result<(), String> {
    let mut known: std::collections::HashSet<&str> = plan
        .clips
        .iter()
        .flat_map(|c| {
            [
                c.visual_asset_id.as_deref(),
                c.narration_asset_id.as_deref(),
            ]
        })
        .flatten()
        .collect();
    known.extend(plan.music_asset_id.as_deref());
    if !matches!(ops.first(), Some(EditOp::CreateSequence { .. })) {
        return Err("第一条不是 create_sequence".into());
    }
    let mut imported = std::collections::HashSet::new();
    let mut used_assets = std::collections::HashSet::new();
    let mut aliases_to_assets = std::collections::HashMap::new();
    let mut clips = std::collections::HashSet::new();
    let mut video_clips = 0;
    for op in ops {
        match op {
            EditOp::CreateSequence { .. } => {}
            EditOp::ImportMedia { alias, asset_id } => {
                if !known.contains(asset_id.as_str()) {
                    return Err(format!("未知素材 {asset_id}"));
                }
                imported.insert(alias.clone());
                aliases_to_assets.insert(alias.clone(), asset_id.clone());
            }
            EditOp::PlaceClip {
                media,
                track,
                start_ms,
                end_ms,
                clip,
                ..
            } => {
                if !imported.contains(media) {
                    return Err(format!("未导入的别名 {media}"));
                }
                if *start_ms < 0 || end_ms <= start_ms || *end_ms > plan.total_ms + 50 {
                    return Err(format!("时间区间不合法 {start_ms}～{end_ms}"));
                }
                if *track == TrackType::Video {
                    video_clips += 1;
                }
                used_assets.insert(aliases_to_assets[media].clone());
                if let Some(name) = clip {
                    clips.insert(name.clone());
                }
            }
            EditOp::AddCrossfade { previous, next, .. } => {
                if !clips.contains(previous) || !clips.contains(next) {
                    return Err("转场引用了不存在的片段".into());
                }
            }
        }
    }
    let expected_video = plan
        .clips
        .iter()
        .filter(|c| c.visual_asset_id.is_some())
        .count();
    if video_clips < expected_video {
        return Err(format!(
            "画面片段只有 {video_clips} 个，应为 {expected_video} 个"
        ));
    }
    if used_assets.len() < known.len() {
        return Err("有素材没有用到".into());
    }
    Ok(())
}

// ---------- llm2：Qwen-Image 画面 ----------

/// llm2：用文生图模型为镜头生成画面。
pub struct QwenImage {
    /// 共用客户端。
    client: Arc<SiliconFlow>,
    /// 密钥与模型。
    access: ModelAccess,
}

impl QwenImage {
    /// 创建 llm2 供应商。
    pub fn new(client: Arc<SiliconFlow>, config: &Config) -> Self {
        Self {
            client,
            access: config.llm2.clone(),
        }
    }
}

/// Qwen-Image 推荐的分辨率（宽 x 高），按画面比例选择。
fn qwen_image_size(aspect_ratio: &str) -> &'static str {
    match aspect_ratio {
        "16:9" => "1664x928",
        "1:1" => "1328x1328",
        _ => "928x1664",
    }
}

#[async_trait]
impl VisualModel for QwenImage {
    async fn generate_shot(&self, request: VisualRequest) -> ProviderResult<MediaOutput> {
        let ctx = &request.ctx;
        let prompt = format!(
            "{}。{}，{}，景别：{}，高清摄影，画面中不要出现任何文字、字幕或水印。",
            request.shot.visual_prompt,
            ctx.brief.as_ref().map(|b| b.style.as_str()).unwrap_or(""),
            style_text(&ctx.settings.style_id),
            request.shot.shot_size,
        );
        let body = json!({
            "model": self.access.model,
            "prompt": prompt,
            "image_size": qwen_image_size(&ctx.settings.aspect_ratio),
        });
        let data = self
            .client
            .post(&self.access, "/images/generations", &body, "画面生成模型")
            .await?;
        let url = data["images"][0]["url"]
            .as_str()
            .ok_or_else(|| unavailable("画面生成模型", "响应中没有图片地址".into()))?;
        let bytes = self.client.download(url, "画面生成模型").await?;
        let (mime, ext) = match crate::media::sniff(&bytes) {
            Some(("image/jpeg", _, _)) => ("image/jpeg", "jpg"),
            Some(("image/webp", _, _)) => ("image/webp", "webp"),
            _ => ("image/png", "png"),
        };
        Ok(MediaOutput {
            data: MediaData::Bytes(bytes),
            file_name: format!("shot-{}.{ext}", request.shot_index + 1),
            mime_type: mime.into(),
            kind: "image".into(),
            source: "generated".into(),
            width: None,
            height: None,
            duration_ms: None,
        })
    }
}

// ---------- llm3：MiniMax-H3 背景音乐 ----------

/// llm3：MiniMax-H3 是带音轨的视频生成模型，这里只取它的音轨作为背景音乐。
pub struct MiniMaxMusic {
    /// 共用客户端。
    client: Arc<SiliconFlow>,
    /// 密钥与模型。
    access: ModelAccess,
}

impl MiniMaxMusic {
    /// 创建 llm3 供应商。
    pub fn new(client: Arc<SiliconFlow>, config: &Config) -> Self {
        Self {
            client,
            access: config.llm3.clone(),
        }
    }
}

#[async_trait]
impl MusicModel for MiniMaxMusic {
    async fn compose(&self, request: MusicRequest) -> ProviderResult<MediaOutput> {
        let ctx = &request.ctx;
        let subject = ctx
            .brief
            .as_ref()
            .map(|b| b.subject.as_str())
            .unwrap_or(&ctx.user_text);
        let mood = request
            .track_id
            .as_deref()
            .map(|t| format!("，曲风参考「{t}」"))
            .unwrap_or_default();
        let prompt = format!(
            "为短视频《{subject}》制作背景音乐：{}{mood}。纯器乐，旋律舒缓连贯，没有人声、对白和音效；画面保持简洁的氛围镜头。",
            style_text(&ctx.settings.style_id),
        );
        let submitted = self
            .client
            .post(
                &self.access,
                "/video/submit",
                &json!({ "model": self.access.model, "prompt": prompt }),
                "背景音乐模型",
            )
            .await?;
        let request_id = submitted["requestId"]
            .as_str()
            .ok_or_else(|| unavailable("背景音乐模型", "没有返回 requestId".into()))?
            .to_string();
        // 异步任务：每 5 秒查询一次，最多约 10 分钟。取消由编排层中止整个 future。
        for _ in 0..120 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let status = match self
                .client
                .post(
                    &self.access,
                    "/video/status",
                    &json!({ "requestId": request_id }),
                    "背景音乐模型",
                )
                .await
            {
                Ok(status) => status,
                // 查询接口偶发空响应：继续轮询。
                Err(_) => continue,
            };
            match status["status"].as_str() {
                Some("InQueue") | Some("InProgress") => continue,
                Some("Succeed") => {
                    let url = status["results"]["videos"][0]["url"]
                        .as_str()
                        .ok_or_else(|| unavailable("背景音乐模型", "结果中没有文件地址".into()))?;
                    let bytes = self.client.download(url, "背景音乐模型").await?;
                    return Ok(MediaOutput {
                        data: MediaData::Bytes(bytes),
                        // 文件本身是带音轨的 MP4；剪辑时只放在音频轨上使用它的声音。
                        file_name: "bgm.mp4".into(),
                        mime_type: "video/mp4".into(),
                        kind: "audio".into(),
                        source: "generated".into(),
                        width: None,
                        height: None,
                        duration_ms: None,
                    });
                }
                other => {
                    return Err(unavailable(
                        "背景音乐模型",
                        format!(
                            "任务失败：{other:?} {}",
                            status["reason"].as_str().unwrap_or("")
                        ),
                    ))
                }
            }
        }
        Err(unavailable("背景音乐模型", "生成超时".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_noisy_output() {
        assert_eq!(extract_json(r#"{"a":1}"#).unwrap()["a"], 1);
        assert_eq!(extract_json("```json\n{\"a\":2}\n```").unwrap()["a"], 2);
        assert_eq!(
            extract_json("好的，结果如下：{\"a\":3} 希望有帮助").unwrap()["a"],
            3
        );
        assert!(extract_json("没有 JSON").is_none());
    }

    #[test]
    fn picks_shot_count_and_image_size() {
        assert_eq!(shot_count(15), 3);
        assert_eq!(shot_count(30), 4);
        assert_eq!(shot_count(60), 8);
        assert_eq!(qwen_image_size("9:16"), "928x1664");
        assert_eq!(qwen_image_size("16:9"), "1664x928");
    }
}
