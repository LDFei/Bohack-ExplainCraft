//! llm4：本地 breezeTTS2 配音，经 ComfyUI 的 HTTP 接口调用（Voice Clone 语音克隆）。
//!
//! 工作流：`BreezeTTS2LoadModel` → `LoadAudio`（参考音）→ `BreezeTTS2VoiceClone` → `SaveAudio`。
//!
//! - 参考音按音色选择：`voice_calm`（沉稳男声）用男声参考，其余用女声参考；
//!   参考音首次使用时上传到 ComfyUI 的 input 目录，转写文本取同名 `.json`（`lines[].text`）或 `.txt`；
//! - 种子固定，同一句话多次生成音色一致；
//! - ComfyUI 未启动时返回可读错误（“本地配音服务未启动”），任务可在启动后重试；
//! - breezeTTS2 没有语速参数，`speech_rate` 目前不生效（剪辑时按实际配音时长排布镜头）。

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::*;
use crate::config::Config;

/// 一个参考音色。
#[derive(Clone)]
struct VoiceRef {
    /// 本地参考音路径。
    path: String,
    /// 参考音的逐字转写。
    transcript: String,
}

/// breezeTTS2 配音供应商。
pub struct BreezeTts {
    /// HTTP 客户端（直连本机）。
    http: reqwest::Client,
    /// ComfyUI 地址。
    base: String,
    /// 检查点名称。
    model: String,
    /// 男声参考。
    male: Option<VoiceRef>,
    /// 女声参考。
    female: Option<VoiceRef>,
    /// 已上传到 ComfyUI 的参考音：本地路径 → ComfyUI 中的文件名。
    uploaded: Mutex<HashMap<String, String>>,
}

/// 读取参考音的转写文本：优先同名 `.json` 的 `lines[].text`，其次同名 `.txt`。
fn load_transcript(audio: &Path) -> Option<String> {
    let json_path = audio.with_extension("json");
    if let Ok(text) = std::fs::read_to_string(&json_path) {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            let lines: Vec<&str> = value["lines"]
                .as_array()
                .map(|l| l.iter().filter_map(|x| x["text"].as_str()).collect())
                .unwrap_or_default();
            if !lines.is_empty() {
                return Some(lines.join(""));
            }
        }
    }
    std::fs::read_to_string(audio.with_extension("txt"))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// 根据配置构造参考音色；文件或转写缺失时返回 `None` 并告警。
fn voice_ref(path: &str, label: &str) -> Option<VoiceRef> {
    if path.is_empty() {
        return None;
    }
    let audio = Path::new(path);
    if !audio.is_file() {
        tracing::warn!(target: "startup", "breezeTTS2 {label}参考音不存在：{path}");
        return None;
    }
    let Some(transcript) = load_transcript(audio) else {
        tracing::warn!(target: "startup", "breezeTTS2 {label}参考音缺少转写文本（同名 .json 或 .txt）：{path}");
        return None;
    };
    Some(VoiceRef {
        path: path.into(),
        transcript,
    })
}

impl BreezeTts {
    /// 创建配音供应商。至少需要配置一个参考音色。
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let male = voice_ref(&config.breeze_voice_male, "男声");
        let female = voice_ref(&config.breeze_voice_female, "女声");
        if male.is_none() && female.is_none() {
            anyhow::bail!(
                "PROVIDER_LLM4=breeze 需要配置 BREEZE_VOICE_MALE 或 BREEZE_VOICE_FEMALE 参考音"
            );
        }
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(120))
                .build()?,
            base: config.comfyui_url.clone(),
            model: config.breeze_model.clone(),
            male,
            female,
            uploaded: Mutex::new(HashMap::new()),
        })
    }

    /// 按音色 ID 选择参考音：沉稳男声用男声，其余用女声；缺哪个就用另一个。
    fn pick_voice(&self, voice_id: &str) -> &VoiceRef {
        let prefer_male = voice_id == "voice_calm";
        let (first, second) = if prefer_male {
            (&self.male, &self.female)
        } else {
            (&self.female, &self.male)
        };
        first
            .as_ref()
            .or(second.as_ref())
            .expect("至少配置了一个参考音")
    }

    /// ComfyUI 不可达时的统一错误。
    fn offline(detail: impl std::fmt::Display) -> ProviderError {
        tracing::warn!(target: "job", "breezeTTS2（ComfyUI）不可用：{detail}");
        ProviderError::unavailable("本地配音服务（ComfyUI）未启动或无响应，请启动后重试。")
    }

    /// 把参考音上传到 ComfyUI 的 input 目录（每个文件只上传一次），返回 ComfyUI 中的文件名。
    async fn ensure_uploaded(&self, voice: &VoiceRef) -> ProviderResult<String> {
        let mut uploaded = self.uploaded.lock().await;
        if let Some(name) = uploaded.get(&voice.path) {
            return Ok(name.clone());
        }
        let bytes = resample_reference(&voice.path).await?;
        // 文件名用 ASCII，避免中文路径在 ComfyUI 下拉框与 URL 中出问题。
        let digest = sha2::Sha256::digest(voice.path.as_bytes());
        let name = format!("explaincraft_voice_{}.wav", &hex::encode(digest)[..8]);
        let form = reqwest::multipart::Form::new()
            .part(
                "image",
                reqwest::multipart::Part::bytes(bytes).file_name(name.clone()),
            )
            .text("type", "input")
            .text("overwrite", "true");
        let response = self
            .http
            .post(format!("{}/upload/image", self.base))
            .multipart(form)
            .send()
            .await
            .map_err(Self::offline)?;
        let value: Value = response.json().await.map_err(Self::offline)?;
        let stored = value["name"].as_str().unwrap_or(&name).to_string();
        uploaded.insert(voice.path.clone(), stored.clone());
        Ok(stored)
    }
}

use sha2::Digest;

/// breezeTTS2 编解码器的输入采样率（`audio_tokenizer/config.json` 的 `input_sample_rate`）。
const CODEC_SAMPLE_RATE: &str = "24000";

/// 把参考音转为 24 kHz 单声道 WAV。
///
/// 参考音采样率与编解码器不一致时，breezeTTS2 节点会调用 `torchaudio` 重采样；
/// ComfyUI 环境里没有 torchaudio 时会直接报错。预先转好采样率可以绕开这一步，
/// 不需要改动用户的 ComfyUI 环境。
async fn resample_reference(path: &str) -> ProviderResult<Vec<u8>> {
    let output = tokio::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-i",
            path,
            "-ac",
            "1",
            "-ar",
            CODEC_SAMPLE_RATE,
            "-f",
            "wav",
            "-",
        ])
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| {
            tracing::warn!(target: "job", "调用 ffmpeg 转换参考音失败：{e}");
            ProviderError::unavailable("配音参考音转换失败：请确认 ffmpeg 已安装并在 PATH 中。")
        })?;
    if !output.status.success() || output.stdout.len() < 1000 {
        tracing::warn!(target: "job", "ffmpeg 转换参考音失败：{}", String::from_utf8_lossy(&output.stderr));
        return Err(ProviderError::unavailable(
            "配音参考音无法读取，请检查参考音文件。",
        ));
    }
    Ok(output.stdout)
}

#[async_trait]
impl VoiceModel for BreezeTts {
    async fn synthesize(&self, request: VoiceRequest) -> ProviderResult<MediaOutput> {
        let voice = self.pick_voice(&request.voice_id).clone();
        let reference = self.ensure_uploaded(&voice).await?;
        let graph = json!({
            "1": { "class_type": "BreezeTTS2LoadModel", "inputs": {
                "model": self.model, "dtype": "auto", "device": "auto",
                "attention": "auto", "decode_mode": "eager", "download_if_missing": false } },
            "2": { "class_type": "LoadAudio", "inputs": { "audio": reference } },
            "3": { "class_type": "BreezeTTS2VoiceClone", "inputs": {
                "breeze_model": ["1", 0], "text": request.text, "reference_audio": ["2", 0],
                "reference_text": voice.transcript, "cfg_scale": 1.0, "max_new_tokens": 1500,
                "temperature": 0.9, "top_k": 50, "top_p": 1.0, "repetition_penalty": 1.1,
                "depth_temperature": 0.9, "depth_top_k": 50, "depth_top_p": 1.0, "seed": 42 } },
            "4": { "class_type": "SaveAudio", "inputs": {
                "audio": ["3", 0], "filename_prefix": "explaincraft/narration" } }
        });
        let response = self
            .http
            .post(format!("{}/prompt", self.base))
            .json(&json!({ "prompt": graph, "client_id": "explaincraft-backend" }))
            .send()
            .await
            .map_err(Self::offline)?;
        let status = response.status();
        let submitted: Value = response.json().await.map_err(Self::offline)?;
        if !status.is_success() {
            tracing::warn!(target: "job", "breezeTTS2 工作流被 ComfyUI 拒绝：{}", crate::util::preview(&submitted.to_string(), 400));
            return Err(ProviderError::unavailable(
                "本地配音工作流提交失败，请检查 ComfyUI 中的 breezeTTS2 节点。",
            ));
        }
        let prompt_id = submitted["prompt_id"]
            .as_str()
            .ok_or_else(|| Self::offline("没有返回 prompt_id"))?
            .to_string();
        // 首次调用需要加载模型（数十秒）；之后每句数秒。最多等待 10 分钟。
        for _ in 0..600 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let history: Value = match self
                .http
                .get(format!("{}/history/{prompt_id}", self.base))
                .send()
                .await
            {
                Ok(response) => response.json().await.unwrap_or(Value::Null),
                Err(_) => continue,
            };
            let entry = &history[&prompt_id];
            if entry.is_null() {
                continue;
            }
            if entry["status"]["status_str"].as_str() == Some("error") {
                tracing::warn!(target: "job", "breezeTTS2 生成出错：{}", crate::util::preview(&entry["status"].to_string(), 400));
                return Err(ProviderError::unavailable("本地配音生成失败，请稍后重试。"));
            }
            let Some(file) = entry["outputs"]["4"]["audio"].get(0) else {
                continue;
            };
            let url = format!("{}/view", self.base);
            let bytes = self
                .http
                .get(url)
                .query(&[
                    ("filename", file["filename"].as_str().unwrap_or_default()),
                    ("subfolder", file["subfolder"].as_str().unwrap_or_default()),
                    ("type", file["type"].as_str().unwrap_or("output")),
                ])
                .send()
                .await
                .map_err(Self::offline)?
                .bytes()
                .await
                .map_err(Self::offline)?;
            return Ok(MediaOutput {
                data: MediaData::Bytes(bytes.to_vec()),
                file_name: "narration.flac".into(),
                mime_type: "audio/flac".into(),
                kind: "audio".into(),
                source: "generated".into(),
                width: None,
                height: None,
                // 实际时长由编排层用 ffprobe 解析。
                duration_ms: None,
            });
        }
        Err(Self::offline("生成超时"))
    }
}
