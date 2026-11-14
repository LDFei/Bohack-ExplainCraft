//! OpenAI 兼容视觉模型、常见音乐模型，以及 Qwen-TTS / IndexTTS 配音。

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};

use super::*;
use crate::config::{provider_base, ModelAccess};

/// 把网络细节写入日志，给用户的错误保持简洁且不包含密钥。
fn failed(what: &str, detail: impl std::fmt::Display) -> ProviderError {
    tracing::warn!(target: "job", "{what}：{detail}");
    ProviderError::unavailable(format!("{what}暂时不可用，请稍后重试。"))
}

/// 检查密钥是否存在。
fn require_key(access: &ModelAccess, what: &str) -> ProviderResult<()> {
    if access.api_key.trim().is_empty() {
        Err(ProviderError::unavailable(format!(
            "{what}未配置 API 密钥，请在 src-tauri/.env 中设置。"
        )))
    } else {
        Ok(())
    }
}

/// 根据制作上下文生成音乐提示词。
fn music_prompt(ctx: &ProductionContext) -> String {
    let brief = ctx.brief.as_ref();
    format!(
        "为主题「{}」创作{}风格的纯音乐，不要有歌词或人声，适合短视频背景，旋律完整、音量柔和。",
        brief
            .map(|brief| brief.subject.as_str())
            .unwrap_or("生活记录"),
        brief.map(|brief| brief.style.as_str()).unwrap_or("温暖")
    )
}

/// MiniMax 原生 music_generation 编曲。
pub struct MiniMaxApiMusic {
    /// HTTP 客户端。
    http: reqwest::Client,
    /// base URL。
    base: String,
    /// 密钥与模型。
    access: ModelAccess,
}

impl MiniMaxApiMusic {
    /// 创建 MiniMax 编曲供应商。
    pub fn new(config: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: if config.llm3_base.is_empty() {
                provider_base("minimax").into()
            } else {
                config.llm3_base.clone()
            },
            access: config.llm3.clone(),
        }
    }
}

#[async_trait]
impl MusicModel for MiniMaxApiMusic {
    async fn compose(&self, request: MusicRequest) -> ProviderResult<MediaOutput> {
        require_key(&self.access, "MiniMax 编曲")?;
        let response = self
            .http
            .post(format!("{}/music_generation", self.base))
            .bearer_auth(&self.access.api_key)
            .json(&json!({
                "model": self.access.model,
                "prompt": music_prompt(&request.ctx),
                "lyrics": "",
                "audio_format": "mp3",
            }))
            .send()
            .await
            .map_err(|error| failed("MiniMax 编曲", error))?;
        let status = response.status();
        let data: Value = response
            .json()
            .await
            .map_err(|error| failed("MiniMax 编曲", error))?;
        if !status.is_success() {
            return Err(failed("MiniMax 编曲", data));
        }
        let encoded = data["data"]["audio"]
            .as_str()
            .or_else(|| data["audio"].as_str())
            .ok_or_else(|| failed("MiniMax 编曲", "响应缺少音频"))?;
        let bytes = hex::decode(encoded)
            .or_else(|_| base64::engine::general_purpose::STANDARD.decode(encoded));
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(_) => self
                .http
                .get(encoded)
                .send()
                .await
                .map_err(|error| failed("MiniMax 编曲下载", error))?
                .bytes()
                .await
                .map_err(|error| failed("MiniMax 编曲下载", error))?
                .to_vec(),
        };
        Ok(MediaOutput {
            data: MediaData::Bytes(bytes),
            file_name: "minimax-music.mp3".into(),
            mime_type: "audio/mpeg".into(),
            kind: "audio".into(),
            source: "generated".into(),
            width: None,
            height: None,
            duration_ms: Some(request.duration_ms),
        })
    }
}

/// Stability AI Stable Audio 编曲。
pub struct StableAudioMusic {
    /// HTTP 客户端。
    http: reqwest::Client,
    /// base URL。
    base: String,
    /// 密钥与模型。
    access: ModelAccess,
}

impl StableAudioMusic {
    /// 创建 Stable Audio 供应商。
    pub fn new(config: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: if config.llm3_base.is_empty() {
                provider_base("stable_audio").into()
            } else {
                config.llm3_base.clone()
            },
            access: config.llm3.clone(),
        }
    }
}

#[async_trait]
impl MusicModel for StableAudioMusic {
    async fn compose(&self, request: MusicRequest) -> ProviderResult<MediaOutput> {
        require_key(&self.access, "Stable Audio")?;
        let seconds = (request.duration_ms as f64 / 1000.0).ceil() as u32;
        let form = reqwest::multipart::Form::new()
            .text("prompt", music_prompt(&request.ctx))
            .text("duration", seconds.min(190).to_string())
            .text("output_format", "mp3");
        let response = self
            .http
            .post(format!(
                "{}/v2beta/audio/stable-audio-2/text-to-audio",
                self.base
            ))
            .bearer_auth(&self.access.api_key)
            .header("accept", "audio/*")
            .multipart(form)
            .send()
            .await
            .map_err(|error| failed("Stable Audio", error))?;
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(failed("Stable Audio", text));
        }
        Ok(MediaOutput {
            data: MediaData::Bytes(
                response
                    .bytes()
                    .await
                    .map_err(|error| failed("Stable Audio", error))?
                    .to_vec(),
            ),
            file_name: "stable-audio.mp3".into(),
            mime_type: "audio/mpeg".into(),
            kind: "audio".into(),
            source: "generated".into(),
            width: None,
            height: None,
            duration_ms: Some(request.duration_ms),
        })
    }
}

/// OpenAI `audio/speech` 兼容配音，覆盖阿里云 Qwen-TTS 与本地 IndexTTS。
pub struct OpenAiSpeech {
    /// HTTP 客户端。
    http: reqwest::Client,
    /// base URL。
    base: String,
    /// 密钥（本地 IndexTTS 可为空）。
    api_key: String,
    /// 模型名。
    model: String,
    /// 默认音色。
    default_voice: String,
    /// 接口不可用时的内置候选。
    voice_options: Vec<String>,
    /// 是否云端服务，云端必须配置密钥。
    cloud: bool,
    /// 是否硅基流动 TTS，voice 需要 `model:voice` 格式。
    siliconflow: bool,
}

impl OpenAiSpeech {
    /// 创建阿里云 Qwen-TTS。
    pub fn qwen(config: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: config.qwen_tts_base.clone(),
            api_key: config.qwen_tts.api_key.clone(),
            model: config.qwen_tts.model.clone(),
            default_voice: config.qwen_tts_voice.clone(),
            voice_options: config.tts_voices.clone(),
            cloud: true,
            siliconflow: false,
        }
    }

    /// 创建硅基流动 TTS。
    pub fn siliconflow(config: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: config.siliconflow_tts_base.clone(),
            api_key: config.siliconflow_tts.api_key.clone(),
            model: config.siliconflow_tts.model.clone(),
            default_voice: config.siliconflow_tts_voice.clone(),
            voice_options: config.tts_voices.clone(),
            cloud: true,
            siliconflow: true,
        }
    }

    /// 创建本地 IndexTTS。
    pub fn indextts(config: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: config.indextts_base.clone(),
            api_key: String::new(),
            model: config.indextts_model.clone(),
            default_voice: config.indextts_voice.clone(),
            voice_options: config.tts_voices.clone(),
            cloud: false,
            siliconflow: false,
        }
    }

    /// 未明确选择声线时，向供应商获取第一个可用 voice ID。
    async fn first_available_voice(&self) -> ProviderResult<String> {
        let fallback = self.voice_options.first().cloned();
        let response = self
            .http
            .get(format!("{}/audio/voices", self.base))
            .bearer_auth(&self.api_key)
            .send()
            .await;
        let Ok(response) = response else {
            return fallback
                .ok_or_else(|| ProviderError::invalid("无法自动获取声线，请手动填写 voice ID。"));
        };
        // 硅基流动等供应商没有 /audio/voices 接口，404 时应回退到配置声线而不是中断配音。
        if !response.status().is_success() {
            return fallback
                .ok_or_else(|| ProviderError::invalid("无法自动获取声线，请手动填写 voice ID。"));
        }
        let data: Value = match response.json().await {
            Ok(data) => data,
            Err(_) => {
                return fallback.ok_or_else(|| {
                    ProviderError::invalid("声线服务返回格式不正确，请手动填写 voice ID。")
                });
            }
        };
        for item in data["data"]
            .as_array()
            .or_else(|| data["voices"].as_array())
            .into_iter()
            .flatten()
        {
            if let Some(id) = item.as_str().or_else(|| {
                item["id"]
                    .as_str()
                    .or_else(|| item["voice_id"].as_str())
                    .or_else(|| item["name"].as_str())
            }) {
                return Ok(id.into());
            }
        }
        fallback.ok_or_else(|| ProviderError::invalid("TTS 未返回声线，请手动填写 voice ID。"))
    }
}

#[async_trait]
impl VoiceModel for OpenAiSpeech {
    async fn synthesize(&self, request: VoiceRequest) -> ProviderResult<MediaOutput> {
        if self.cloud && self.api_key.trim().is_empty() {
            return Err(ProviderError::unavailable(
                "Qwen-TTS 未配置 API 密钥，请设置 QWEN_TTS_API_KEY 或 DASHSCOPE_API_KEY。",
            ));
        }
        let voice = if request.voice_id.trim().is_empty()
            || matches!(
                request.voice_id.as_str(),
                "voice_warm" | "voice_calm" | "voice_bright"
            ) {
            if self.default_voice.is_empty() && self.cloud {
                self.first_available_voice().await?
            } else {
                self.default_voice.clone()
            }
        } else {
            request.voice_id.trim().into()
        };
        let voice = if self.siliconflow {
            // 旧草稿里可能已保存完整 `model:voice`，避免再拼一次前缀。
            let prefix = format!("{}:", self.model);
            if voice.starts_with(&prefix) {
                voice
            } else {
                format!("{prefix}{voice}")
            }
        } else {
            voice
        };
        if voice.is_empty() {
            return Err(ProviderError::invalid("请在作品设置中选择或填写声线。"));
        }
        let builder = self.http.post(format!("{}/audio/speech", self.base));
        let builder = if self.api_key.is_empty() {
            builder
        } else {
            builder.bearer_auth(&self.api_key)
        };
        let payload = json!({
            "base_url": self.base,
            "model": self.model,
            "input": request.text,
            "voice": voice,
            "response_format": if self.siliconflow { "mp3" } else { "wav" },
            "speed": request.speech_rate.clamp(0.5, 2.0),
        });
        tracing::info!(target: "job", "TTS 请求（已脱敏）：{payload}");
        let response = builder
            .json(&payload)
            .send()
            .await
            .map_err(|error| failed("语音合成", error))?;
        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(failed("语音合成", text));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| failed("语音合成", error))?
            .to_vec();
        if bytes.first() == Some(&b'{') {
            return Err(failed("语音合成", "服务返回了错误而不是音频"));
        }
        Ok(MediaOutput {
            data: MediaData::Bytes(bytes),
            file_name: if self.siliconflow {
                "narration.mp3".into()
            } else {
                "narration.wav".into()
            },
            mime_type: if self.siliconflow {
                "audio/mpeg".into()
            } else {
                "audio/wav".into()
            },
            kind: "audio".into(),
            source: "generated".into(),
            width: None,
            height: None,
            duration_ms: None,
        })
    }
}
