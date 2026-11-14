//! 硅基流动（SiliconFlow）上的云端模型：
//!
//! | 环节                     | 模型（默认）              | 接口                                   |
//! | ------------------------ | ------------------------- | -------------------------------------- |
//! | llm1 文案 / 分镜 / 剪辑  | `zai-org/GLM-5.3`         | `/chat/completions`（关闭思考，输出 JSON） |
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
        let base = if !config.llm1_base.is_empty() {
            config.llm1_base.clone()
        } else if config.provider_llm1 == "siliconflow" {
            config.siliconflow_base.clone()
        } else {
            crate::config::provider_base(&config.provider_llm1).into()
        };
        if config.provider_llm1 == "custom" && base.trim().is_empty() {
            anyhow::bail!("自定义供应商需要设置 LLM1_BASE_URL");
        }
        Ok(Self { http, base })
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
                "{what}未配置密钥，请在 src-tauri/.env 中设置。"
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
        content_text(&data["choices"][0]["message"]["content"])
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

    /// 使用 OpenAI tool_calls 驱动 50 个 Quercus 工具，返回最终剪辑指令。

    async fn chat_agent(
        &self,
        access: &ModelAccess,
        system: &str,
        user: &str,
    ) -> ProviderResult<Option<Vec<EditOp>>> {
        let mut oak = crate::oak::state::ServerState::new();
        let mut messages = json!([
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ]);
        for _ in 0..8 {
            let data = self
                .post(
                    access,
                    "/chat/completions",
                    &json!({
                        "model": access.model,
                        "enable_thinking": false,
                        "temperature": 0.3,
                        "max_tokens": 4000,
                        "messages": messages,
                        "tools": crate::oak::openai_tool_schemas(),
                        "tool_choice": "auto"
                    }),
                    "剪辑工具模型",
                )
                .await?;
            let message = data["choices"][0]["message"].clone();
            messages.as_array_mut().unwrap().push(message.clone());
            let calls = message["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if calls.is_empty() {
                let content = message["content"].as_str().unwrap_or("");
                let Some(value) = extract_json(content) else {
                    return Ok(None);
                };
                return Ok(serde_json::from_value::<Vec<EditOp>>(value["ops"].clone()).ok());
            }
            for call in calls {
                let name = call["function"]["name"].as_str().unwrap_or("");
                let arguments = call["function"]["arguments"]
                    .as_str()
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or_else(|| json!({}));
                let reply = crate::oak::tools::call(&mut oak, name, arguments);
                let (structured, is_error) = match reply {
                    Some(reply) => (reply.structured, reply.is_error),
                    None => (Some(json!({"error": "unknown tool"})), true),
                };
                messages.as_array_mut().unwrap().push(json!({
                    "role": "tool",
                    "tool_call_id": call["id"],
                    "content": json!({
                        "result": structured,
                        "isError": is_error,
                    }).to_string(),
                }));
            }
        }
        Ok(None)
    }
}

/// 兼容 OpenAI 字符串、文本对象和内容块数组三种 message.content 形式。
fn content_text(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    if let Some(text) = content["text"].as_str() {
        return Some(text.to_string());
    }
    content.as_array().map(|parts| {
        parts
            .iter()
            .filter_map(|part| {
                part["text"]
                    .as_str()
                    .or_else(|| part["content"].as_str())
                    .or_else(|| part["output_text"].as_str())
            })
            .collect::<Vec<_>>()
            .join("")
    })
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

/// 从 llm1 文本中截取 SVG，复用同一文案模型完成场景绘制。
fn extract_svg(text: &str) -> Option<String> {
    let start = text.find("<svg")?;
    let end = text[start..].rfind("</svg>").map(|end| start + end + 6)?;
    Some(text[start..end].to_string())
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

/// 从文案模型输出的 JSON 里取出逐句解说词。
fn script_lines(value: &Value) -> Vec<String> {
    value["lines"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|line| line.as_str())
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 一句话的朗读时长估计（毫秒）：中文约每秒 4 字，另加收尾缓冲。
fn narration_estimate_ms(line: &str) -> i64 {
    ((line.chars().count() as f64 / CHARS_PER_SECOND) * 1000.0) as i64 + 800
}

/// 镜头数：每个镜头约 7 秒，限制在 3～8 个。
fn shot_count(seconds: u32) -> usize {
    ((seconds as f64 / 7.0).round() as usize).clamp(3, 40)
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
             \"target_duration_seconds\": 用户明确提到时长时填 15～300 的整数，否则沿用当前设置}}",
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
            settings.target_duration_seconds = (seconds as u32).clamp(15, 300);
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
        let mut lines = script_lines(&value);
        if lines.is_empty() {
            return Err(ProviderError::unavailable("文案生成结果为空，请重试。"));
        }
        // 文案明显超出参考时长时重写一版更短的：成片长度跟着配音走，
        // 文案越长成片越长；这里只是尽量贴近用户要的时长。
        let budget = (seconds as f64 * CHARS_PER_SECOND) as usize;
        if lines.iter().map(|line| line.chars().count()).sum::<usize>() > budget * 11 / 10 {
            let actual: usize = lines.iter().map(|line| line.chars().count()).sum();
            let retry = format!(
                "{user}\n\n上一版解说词共 {actual} 字，超过 {seconds} 秒能读完的长度。请重写：\
                 仍然 {count} 句，全部加起来不超过 {budget} 字（每句约 {per_line} 字）。"
            );
            if let Ok(value) = self
                .client
                .chat_json(
                    &self.access,
                    "你是短视频文案，擅长把解说词压缩到指定字数，语言仍然自然、可朗读。",
                    &retry,
                    1200,
                )
                .await
            {
                let shorter = script_lines(&value);
                let shorter_len: usize = shorter.iter().map(|line| line.chars().count()).sum();
                if !shorter.is_empty() && shorter_len < actual {
                    lines = shorter;
                }
            }
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
        let expected = shot_count(settings.target_duration_seconds);
        let numbered: Vec<String> = lines
            .iter()
            .enumerate()
            .map(|(i, l)| format!("{}. {l}", i + 1))
            .collect();
        // 镜头不要求和解说词一一对应：一句可以拆成两个镜头，短句也可以合并。
        // 镜头时长由它自己的旁白长度决定，所以这里不要模型填时长。
        let user = format!(
            "主题：{}\n风格：{}\n画面比例：{}\n参考总时长：{} 秒（只作节奏参考，成片按每段配音的实际长度排，不必精确等于它）\n\
             解说词：\n{}\n\
             请把解说词组织成约 {} 个镜头（可以上下浮动 1～2 个）：一句可以拆成两个镜头（各自配前半句、后半句），\
             两三句短句也可以合并到一个镜头。每个镜头的 narration 必须是从解说词里连续截取的原文，不要改写、不要漏字。\n\
             输出 JSON：{{\"shots\": [{{\"narration\": 该镜头要念的原文, \
             \"visual_prompt\": 用于画面的详细中文描述（主体、环境、光线、色调、构图，不要出现文字和人物对白）, \
             \"shot_size\": 景别（远景/全景/中景/近景/特写）, \"camera_motion\": 运镜（推/拉/摇/移/固定）}}]}}，\
             不要输出时长字段，时长由配音决定。",
            request.brief.subject,
            request.brief.style,
            settings.aspect_ratio,
            settings.target_duration_seconds,
            numbered.join("\n"),
            expected,
        );
        let value = self
            .client
            .chat_json(
                &self.access,
                "你是短视频分镜师，把文案拆成画面清晰、节奏合适的镜头。",
                &user,
                2500,
            )
            .await?;
        let items = value["shots"].as_array().cloned().unwrap_or_default();
        let mut shots: Vec<ShotPlan> = items
            .iter()
            .filter_map(|item| {
                let narration = item["narration"].as_str().map(str::trim)?.to_string();
                if narration.is_empty() {
                    return None;
                }
                Some(ShotPlan {
                    // 时长先按旁白字数估一个，配音生成后会以配音的实际长度为准。
                    duration_ms: narration_estimate_ms(&narration),
                    visual_prompt: text_or(
                        item,
                        "visual_prompt",
                        &format!("{}，{}", request.brief.subject, narration),
                    ),
                    shot_size: text_or(item, "shot_size", "中景"),
                    camera_motion: text_or(item, "camera_motion", "缓慢推进"),
                    narration,
                })
            })
            .collect();
        // 模型没给分镜时退回“一句一个镜头”。
        if shots.is_empty() {
            shots = lines
                .iter()
                .map(|line| ShotPlan {
                    duration_ms: narration_estimate_ms(line),
                    narration: line.to_string(),
                    visual_prompt: format!("{}，{}", request.brief.subject, line),
                    shot_size: "中景".into(),
                    camera_motion: "缓慢推进".into(),
                })
                .collect();
        }
        let total: i64 = shots.iter().map(|shot| shot.duration_ms).sum();
        let target = settings.target_duration_seconds as i64 * 1000;
        if (total - target).abs() * 2 > target {
            tracing::info!(
                target: "job",
                "  · 分镜按旁白估算共 {:.1}s，与参考时长 {}s 有明显差异，成片长度以配音为准",
                total as f64 / 1000.0,
                settings.target_duration_seconds
            );
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
             \"aspect_ratio\": \"9:16\"/\"16:9\"/\"1:1\"/null, \"target_duration_seconds\": 15～300 或 null, \
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
                // 时长变化：文案要按新时长重写，否则配音只能被压成倍速，再重新分镜。
                draft.settings.target_duration_seconds = seconds;
                draft.script = None;
                draft.shots.clear();
                invalidated.extend(StageKey::Script.and_after());
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

        let agent_ops = self
            .client
            .chat_agent(
                &self.access,
                "你是视频剪辑师。可以使用 Quercus 工具创建工程、导入素材、调整时间线并预览；最后只输出符合用户给定格式的 ops JSON。",
                &user,
            )
            .await
            .ok()
            .flatten();
        let ops = match agent_ops {
            Some(ops) if validate_ops(&ops, &plan).is_ok() => ops,
            _ => match self
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
            },
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
    let mut clips: Vec<Clip> = vec![];
    for shot in &draft.shots {
        let narration = request.narration.get(&shot.id);
        // 长旁白切成多个画面切点：每个切点一段画面，配音整段从第一个切点开始播。
        let cuts = shot.asset_ids.len().max(1);
        let durations = cut_durations(shot.duration_ms.max(1), cuts);
        let subtitle = draft
            .finishing
            .subtitles_enabled
            .then(|| shot.narration.clone());
        for (index, duration) in durations.iter().enumerate() {
            clips.push(Clip {
                shot_id: shot.id.clone(),
                start_ms: start,
                duration_ms: *duration,
                visual_asset_id: shot
                    .asset_ids
                    .get(index)
                    .cloned()
                    .or_else(|| shot.asset_ids.first().cloned()),
                // 整段配音只放在第一个画面切点上：音频可以跨多个画面片段播放，
                // 这样长旁白不会在每个切点被截断重放。
                narration_asset_id: (index == 0)
                    .then(|| narration.map(|(id, _)| id.clone()))
                    .flatten(),
                narration_duration_ms: if index == 0 {
                    narration.map(|(_, ms)| *ms)
                } else {
                    None
                },
                subtitle: if index == 0 { subtitle.clone() } else { None },
                transition: transition.into(),
                camera_motion: shot.camera_motion.clone(),
            });
            start += duration;
        }
    }
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
            let narration_end = clip
                .narration_duration_ms
                .map(|ms| clip.start_ms + ms)
                .unwrap_or(end);
            lines.push(format!(
                "- 镜头{} 配音 asset_id={id}，应从 {} 毫秒开始，到 {} 毫秒结束（配音可以跨多个画面片段）",
                i + 1,
                clip.start_ms,
                narration_end
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

// ---------- llm1 复用：SVG 动画场景 ----------

#[async_trait]
impl VisualModel for GlmScript {
    async fn generate_shot(&self, request: VisualRequest) -> ProviderResult<MediaOutput> {
        let (width, height) = crate::util::dimensions(&request.ctx.settings.aspect_ratio);
        let prompt = format!(
            "为以下镜头绘制 {}×{} 的 SVG 动画：{}\n风格：{}\n镜头时长：{} 毫秒。\n\
             只输出完整 SVG，不要 Markdown。使用 rect/circle/ellipse/path/text 和 SMIL animate/animateTransform；\
             禁止 script、foreignObject、image、animateMotion、外链和 CSS 动画。",
            width,
            height,
            request.shot.visual_prompt,
            request
                .ctx
                .brief
                .as_ref()
                .map(|brief| brief.style.as_str())
                .unwrap_or("温暖"),
            request.shot.duration_ms,
        );
        let text = self
            .client
            .chat(
                &self.access,
                "你同时负责短视频文案与 SVG 场景绘制，输出简洁、可循环的矢量动画，控制在6000字符以内且不超过60个元素。",
                &prompt,
                12000,
            )
            .await?;
        let svg = extract_svg(&text).ok_or_else(|| {
            tracing::warn!(target: "job", "SVG 场景模型返回内容不完整：{}", preview(&text));
            ProviderError::invalid("模型没有返回完整 SVG，请重试或调整创作要求。")
        })?;
        crate::svg_animation::render(
            &svg,
            request.shot.duration_ms.max(1000),
            width as u32,
            height as u32,
            request.shot_index,
        )
        .await
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
            access: ModelAccess {
                api_key: config.llm3.api_key.clone(),
                model: std::env::var("LLM3_MODEL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "MiniMaxAI/MiniMax-H3".into()),
            },
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
    fn picks_shot_count() {
        assert_eq!(shot_count(15), 3);
        assert_eq!(shot_count(30), 4);
        assert_eq!(shot_count(60), 9);
        assert_eq!(shot_count(300), 40);
    }



    /// 长旁白配多个画面切点：每个切点一段画面，配音整段只在第一个切点开始。
    #[test]
    fn long_narration_is_split_into_several_visual_cuts() {
        let shot: crate::models::Shot = serde_json::from_value(json!({
            "id": "s1",
            "duration_ms": 9600,
            "narration": "一段很长的旁白",
            "visual_prompt": "测试画面",
            "shot_size": "中景",
            "camera_motion": "推",
            "asset_ids": ["ast_cut1", "ast_cut2", "ast_cut3"],
        }))
        .expect("镜头");
        let mut draft = crate::catalog::empty_draft();
        draft.shots = vec![shot];
        let plan = timeline(&EditRequest {
            draft,
            narration: std::collections::HashMap::from([(
                "s1".to_string(),
                ("ast_voice".to_string(), 9300),
            )]),
            music_duration_ms: None,
        });
        assert_eq!(plan.clips.len(), 3);
        assert_eq!(plan.total_ms, 9600);
        assert_eq!(
            plan.clips
                .iter()
                .map(|clip| clip.visual_asset_id.clone().unwrap())
                .collect::<Vec<_>>(),
            vec!["ast_cut1", "ast_cut2", "ast_cut3"]
        );
        // 配音只挂第一个切点，但长度是整段旁白（音频跨画面播放）。
        assert_eq!(
            plan.clips[0].narration_asset_id.as_deref(),
            Some("ast_voice")
        );
        assert_eq!(plan.clips[0].narration_duration_ms, Some(9300));
        assert!(plan.clips[1].narration_asset_id.is_none());
        assert!(plan.clips[2].narration_asset_id.is_none());
        // 字幕只在第一个切点上，避免重复显示。
        assert!(plan.clips[0].subtitle.is_some());
        assert!(plan.clips[1].subtitle.is_none());
    }

    /// 一段素材配一段配音：片段时长以分镜为准，配音只在窗口内播放。
    #[test]
    fn timeline_keeps_shot_lengths_and_fits_narration() {
        let shot = |id: &str, duration_ms: i64, narration: &str| {
            serde_json::from_value::<crate::models::Shot>(json!({
                "id": id,
                "duration_ms": duration_ms,
                "narration": narration,
                "visual_prompt": "测试画面",
                "shot_size": "中景",
                "camera_motion": "固定",
                "asset_ids": ["ast_visual"],
            }))
            .expect("镜头")
        };
        let mut draft = crate::catalog::empty_draft();
        draft.shots = vec![shot("s1", 3000, "第一句"), shot("s2", 2000, "第二句")];
        let request = EditRequest {
            draft,
            narration: std::collections::HashMap::from([
                ("s1".to_string(), ("ast_voice1".to_string(), 5000)),
                ("s2".to_string(), ("ast_voice2".to_string(), 1200)),
            ]),
            music_duration_ms: None,
        };
        let plan = timeline(&request);
        // 片段时长永远是镜头自己的长度：配音不会把画面拉长。
        assert_eq!(plan.total_ms, 5000);
        assert_eq!(plan.clips[0].duration_ms, 3000);
        assert_eq!(plan.clips[0].narration_duration_ms, Some(5000));
        assert_eq!(plan.clips[1].duration_ms, 2000);
        assert_eq!(plan.clips[1].narration_duration_ms, Some(1200));
        // 段与段首尾相接；配音跨片段播放，不改变画面边界。
        assert_eq!(plan.clips[1].start_ms, plan.clips[0].start_ms + 3000);
    }

    #[test]
    fn parses_openai_content_blocks() {
        let content = json!([{"type": "text", "text": "<svg/>"}]);
        assert_eq!(content_text(&content).unwrap(), "<svg/>");
    }
}
