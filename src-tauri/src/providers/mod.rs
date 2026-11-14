//! 供应商适配层：视频制作流程里的四个模型和剪辑渲染器。
//!
//! | 流程环节               | trait            | 编号   |
//! | ---------------------- | ---------------- | ------ |
//! | 选题、文案、分镜、剪辑 | [`ScriptModel`]  | llm1   |
//! | SVG 场景（画面生成）   | [`VisualModel`]  | llm1   |
//! | 背景音乐               | [`MusicModel`]   | llm3   |
//! | 配音（TTS）            | [`VoiceModel`]   | llm4   |
//! | 剪辑执行 / 成片输出    | [`Renderer`]     | 剪辑库 |
//!
//! 流程编排（`pipeline.rs`）只依赖这些 trait。接入真实模型时：
//! 1. 在 `providers/` 下新建实现文件，实现对应 trait；
//! 2. 在 [`Providers::from_config`] 中按名称注册；
//! 3. 设置环境变量 `PROVIDER_LLM1=<名称>` 等切换。
//!
//! 约定：
//! - 供应商不访问数据库、不感知项目 ID；生成的文件交给编排层落盘。
//! - 错误使用 [`ProviderError`]，`message` 必须是可以直接给用户看的中文说明，
//!   不要放供应商原始报错或密钥。
//! - 调用可能被用户取消（编排层会丢弃未完成的 future），实现中不要依赖调用一定执行完。

// 请求结构里的部分字段供不同供应商按需使用。
#![allow(dead_code)]

pub mod breeze;
pub mod edit_ops;
pub mod encoding;
pub mod openai;
pub mod procedural;

pub mod quercus;
pub mod siliconflow;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::models::{Answer, Brief, Draft, Script, Settings, Shot, StageKey};

// ---------- 通用类型 ----------

/// 供应商调用失败的原因。编排层会把它转为任务的 `error`。
#[derive(Debug, Clone)]
pub struct ProviderError {
    /// 文档第 7 节的错误码，常用 `PROVIDER_UNAVAILABLE` / `RENDER_FAILED` / `QUOTA_EXCEEDED`。
    pub code: &'static str,
    /// 面向用户的中文说明。
    pub message: String,
    /// 是否允许用户点击重试。
    pub retryable: bool,
}

impl ProviderError {
    /// 外部模型或素材服务暂时不可用（可重试）。
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "PROVIDER_UNAVAILABLE",
            message: message.into(),
            retryable: true,
        }
    }

    /// 剪辑或编码失败（可重试）。
    pub fn render_failed(message: impl Into<String>) -> Self {
        Self {
            code: "RENDER_FAILED",
            message: message.into(),
            retryable: true,
        }
    }

    /// 输入内容不合法，重试也无法成功（不可重试，需要用户修改需求）。
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "VALIDATION_ERROR",
            message: message.into(),
            retryable: false,
        }
    }
}

/// 供应商调用的结果类型。
pub type ProviderResult<T> = Result<T, ProviderError>;

/// 供应商产出的媒体文件。编排层负责把它保存为项目素材（Asset）。
#[derive(Debug, Clone)]
pub struct MediaOutput {
    /// 文件内容来源。
    pub data: MediaData,
    /// 文件名，扩展名决定存储文件的后缀。
    pub file_name: String,
    /// MIME 类型，播放时作为 `Content-Type`。
    pub mime_type: String,
    /// `image` / `video` / `audio`。
    pub kind: String,
    /// `generated`（模型生成）/ `library`（素材库）/ `rendered`（渲染输出）。
    pub source: String,
    /// 宽度；未知时为 `None`，编排层会尝试用 ffprobe 解析。
    pub width: Option<i64>,
    /// 高度；未知时为 `None`。
    pub height: Option<i64>,
    /// 时长（毫秒）；图片为 `None`。
    pub duration_ms: Option<i64>,
}

/// 媒体文件内容。
#[derive(Debug, Clone)]
pub enum MediaData {
    /// 已存在的本地文件（会被复制进项目存储）。
    Path(PathBuf),
    /// 内存中的文件内容（会被写入项目存储）。
    Bytes(Vec<u8>),
}

/// 一次制作的公共上下文，各个模型都能拿到。
#[derive(Debug, Clone)]
pub struct ProductionContext {
    /// 已确定的创作方案；选题前为 `None`。
    pub brief: Option<Brief>,
    /// 当前设置（时长、比例、语言、风格）。
    pub settings: Settings,
    /// 用户本次输入的原话。
    pub user_text: String,
    /// 用户对追问的回答。
    pub answers: Vec<Answer>,
}

/// 本次消息附带的用户素材摘要，供选题阶段参考。
#[derive(Debug, Clone, Serialize)]
pub struct AttachmentInfo {
    /// 素材 ID。
    pub asset_id: String,
    /// `image` / `video` / `audio`。
    pub kind: String,
    /// 原始文件名。
    pub file_name: String,
    /// 时长（毫秒），图片为 `None`。
    pub duration_ms: Option<i64>,
}

// ---------- llm1：选题 / 文案 / 分镜 / 剪辑 ----------

/// 选题请求。
pub struct PlanRequest {
    /// 制作上下文。
    pub ctx: ProductionContext,
    /// 用户附带的素材。
    pub attachments: Vec<AttachmentInfo>,
}

/// 选题结果：创作方案。
pub struct Plan {
    /// 主题、受众、目的、风格。
    pub brief: Brief,
    /// 建议的项目标题（仅在用户尚未命名时使用）。
    pub project_title: String,
    /// 一句话创作方案，展示在对话中的方案卡片。
    pub summary: String,
    /// 根据用户原话调整后的设置（如“横屏”“60 秒”）。
    pub settings: Settings,
}

/// 追问内容。一次最多一个问题。
pub struct QuestionSpec {
    /// 问题文字。
    pub prompt: String,
    /// `text` / `single_select`。
    pub input_type: String,
    /// 单选选项 `(value, label)`；文本题为空。
    pub options: Vec<(String, String)>,
}

/// 选题的两种结果。
pub enum PlanOutcome {
    /// 信息足够，得到创作方案。
    Ready(Plan),
    /// 关键信息缺失、无法继续时才追问；一般偏好应直接使用默认值，不逐项追问小白用户。
    NeedInput(QuestionSpec),
}

/// 写文案请求。
pub struct ScriptRequest {
    /// 制作上下文。
    pub ctx: ProductionContext,
    /// 已确定的创作方案。
    pub brief: Brief,
}

/// 画分镜请求。
pub struct StoryboardRequest {
    /// 制作上下文（含目标时长、比例）。
    pub ctx: ProductionContext,
    /// 创作方案。
    pub brief: Brief,
    /// 完整脚本。
    pub script: Script,
}

/// llm1 产出的单个镜头（镜头 ID 由编排层分配）。
#[derive(Debug, Clone)]
pub struct ShotPlan {
    /// 镜头时长（毫秒）。
    pub duration_ms: i64,
    /// 该镜头的旁白。
    pub narration: String,
    /// 画面描述，交给 llm1 生成 SVG 场景。
    pub visual_prompt: String,
    /// 景别。
    pub shot_size: String,
    /// 运镜。
    pub camera_motion: String,
}

/// 对话修改请求。
pub struct ReviseRequest {
    /// 当前草稿。
    pub draft: Draft,
    /// 用户的修改意见原话。
    pub instruction: String,
}

/// 对话修改结果。
pub struct Revision {
    /// 修改后的草稿（保留镜头 ID；需要重新出画面的镜头清空 `asset_ids`）。
    pub draft: Draft,
    /// 需要重新计算的阶段，决定哪些产物可以复用。
    pub invalidated: Vec<StageKey>,
    /// 给用户的一句话说明。
    pub summary: String,
}

/// 生成剪辑方案的请求。
pub struct EditRequest {
    /// 当前草稿（画面、音频均已就绪）。
    pub draft: Draft,
    /// 每个镜头的配音：shot_id → (素材 ID, 时长毫秒)。
    pub narration: HashMap<String, (String, i64)>,
    /// 背景音乐自身的时长（毫秒）；比成片短时需要循环铺满。
    pub music_duration_ms: Option<i64>,
}

/// 剪辑方案：由 llm1 决定镜头顺序、时长、转场与字幕，渲染器按方案执行。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct EditPlan {
    /// 输出画面比例。
    pub aspect_ratio: String,
    /// 总时长（毫秒）。
    pub total_ms: i64,
    /// 时间线上的片段，按播放顺序排列。
    pub clips: Vec<Clip>,
    /// 背景音乐素材。
    pub music_asset_id: Option<String>,
    /// 背景音乐自身时长（毫秒），用于循环铺满。
    #[serde(default)]
    pub music_duration_ms: Option<i64>,
    /// 背景音乐音量，0～1。
    pub music_volume: f64,
    /// 配音音量，0～1。
    pub narration_volume: f64,
    /// 调色预设。
    pub color_preset_id: String,
    /// 特效预设。
    pub effect_preset_id: String,
    /// 是否烧录字幕。
    pub subtitles_enabled: bool,
    /// 剪辑指令：由 llm1 输出、剪辑库逐条执行（见 [`edit_ops`]）。
    #[serde(default)]
    pub ops: Vec<edit_ops::EditOp>,
}

/// 时间线上的一个片段，对应一个镜头。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Clip {
    /// 对应的镜头。
    pub shot_id: String,
    /// 在成片中的开始时间（毫秒）。
    pub start_ms: i64,
    /// 片段时长（毫秒）。
    pub duration_ms: i64,
    /// 画面素材。
    pub visual_asset_id: Option<String>,
    /// 配音素材。
    pub narration_asset_id: Option<String>,
    /// 字幕文字；关闭字幕时为 `None`。
    pub subtitle: Option<String>,
    /// 进入本片段的转场：`cut` / `fade` 等。
    pub transition: String,
    /// 运镜，渲染器据此对静态图片做推拉摇移。
    pub camera_motion: String,
}

/// 仅讨论模式的请求。
pub struct ChatRequest {
    /// 当前项目的创作方案，可用于结合上下文回答。
    pub brief: Option<Brief>,
    /// 用户的问题。
    pub text: String,
}

/// llm1：负责选题、文案、分镜、对话修改、剪辑方案与仅讨论回答的文本模型。
#[async_trait]
pub trait ScriptModel: Send + Sync {
    /// 选题：识别主题、受众、目的与风格，补齐默认值；关键信息缺失时可追问。
    async fn plan(&self, request: PlanRequest) -> ProviderResult<PlanOutcome>;
    /// 写文案：解说词 / 台词，要求逻辑清晰、语言精炼。
    async fn write_script(&self, request: ScriptRequest) -> ProviderResult<Script>;
    /// 画分镜：把文案转化为镜头，规划景别、运镜、画面内容与时长。
    async fn storyboard(&self, request: StoryboardRequest) -> ProviderResult<Vec<ShotPlan>>;
    /// 对话修改：理解修改意见，改写草稿并给出影响范围。
    async fn revise(&self, request: ReviseRequest) -> ProviderResult<Revision>;
    /// 剪辑方案：转场、字幕、音画对齐。
    async fn edit_plan(&self, request: EditRequest) -> ProviderResult<EditPlan>;
    /// 仅讨论模式的回答，不修改草稿。
    async fn chat(&self, request: ChatRequest) -> ProviderResult<String>;
}

// ---------- llm1 复用：SVG 场景生成 ----------

/// 单个镜头的画面生成请求。
pub struct VisualRequest {
    /// 制作上下文（含风格、比例）。
    pub ctx: ProductionContext,
    /// 要生成画面的镜头。
    pub shot: Shot,
    /// 镜头序号（从 0 开始）。
    pub shot_index: usize,
    /// 镜头总数，便于保持整体风格一致。
    pub shot_count: usize,
}

/// llm1 复用：为镜头生成或检索画面。
#[async_trait]
pub trait VisualModel: Send + Sync {
    /// 为一个镜头生成（或从素材库检索）画面，图片或视频片段均可。
    async fn generate_shot(&self, request: VisualRequest) -> ProviderResult<MediaOutput>;
}

// ---------- llm3：背景音乐 ----------

/// 背景音乐请求。
pub struct MusicRequest {
    /// 制作上下文（含风格）。
    pub ctx: ProductionContext,
    /// `auto`（由模型匹配 / 生成）或 `library`（指定曲库 ID）。
    pub mode: String,
    /// `library` 模式下的曲库 ID。
    pub track_id: Option<String>,
    /// 需要覆盖的总时长（毫秒）。
    pub duration_ms: i64,
}

/// llm3：背景音乐的匹配或生成。
#[async_trait]
pub trait MusicModel: Send + Sync {
    /// 生成或挑选一段背景音乐。
    async fn compose(&self, request: MusicRequest) -> ProviderResult<MediaOutput>;
}

// ---------- llm4：配音 TTS ----------

/// 配音请求，按镜头调用。
pub struct VoiceRequest {
    /// 要朗读的旁白。
    pub text: String,
    /// 音色 ID。
    pub voice_id: String,
    /// 语速，0.5～2.0。
    pub speech_rate: f64,
    /// 语言。
    pub language: String,
    /// 镜头时长，供应商可据此调整语速或返回实际时长。
    pub target_duration_ms: i64,
}

/// llm4：语音合成（TTS）。
#[async_trait]
pub trait VoiceModel: Send + Sync {
    /// 把一段旁白合成为音频。
    async fn synthesize(&self, request: VoiceRequest) -> ProviderResult<MediaOutput>;
}

// ---------- 剪辑库 / 渲染 ----------

/// 渲染请求。
pub struct RenderRequest {
    /// 制作上下文。
    pub ctx: ProductionContext,
    /// 剪辑方案（已应用调色、特效、字幕设置）。
    pub plan: EditPlan,
    /// 方案中引用的素材：asset_id → 本地文件路径。
    pub inputs: HashMap<String, PathBuf>,
}

/// 导出请求。
pub struct ExportRequest {
    /// 源成片文件。
    pub source: PathBuf,
    /// 源成片时长（毫秒）。
    pub duration_ms: i64,
    /// 画面比例。
    pub aspect_ratio: String,
    /// 目标分辨率。
    pub resolution: String,
    /// 目标帧率。
    pub fps: u32,
    /// 目标格式。
    pub format: String,
}

/// 工程包中的一份原始素材。
pub struct BundleAsset {
    /// 项目素材 ID，用于与时间线引用对应。
    pub id: String,
    /// 展示和归档时使用的原始文件名。
    pub file_name: String,
    /// 本地文件路径。
    pub path: PathBuf,
}

/// 导出可人工继续剪辑的工程包请求。
pub struct ProjectBundleRequest {
    /// 当前版本的成片。
    pub source: PathBuf,
    /// 当前版本不可变的剪辑时间线。
    pub plan: EditPlan,
    /// 时间线素材及用户上传的全部素材。
    pub assets: Vec<BundleAsset>,
    /// 项目标题，用于工程与说明文件。
    pub title: String,
    /// 当前成片版本号。
    pub version_number: i64,
}

/// 剪辑库：按方案合成成片，以及转换导出规格。
#[async_trait]
pub trait Renderer: Send + Sync {
    /// 按剪辑方案合成 720p 预览成片。
    async fn render(&self, request: RenderRequest) -> ProviderResult<MediaOutput>;
    /// 把已有成片转为其他导出规格。
    async fn export(&self, request: ExportRequest) -> ProviderResult<MediaOutput>;
    /// 导出全部素材、Quercus 工程和通用剪辑交换文件。
    async fn export_bundle(&self, request: ProjectBundleRequest) -> ProviderResult<MediaOutput>;
}

// ---------- 注册 ----------

/// 当前启用的全部供应商。各环节可以分别替换。
#[derive(Clone)]
pub struct Providers {
    /// llm1：文案、分镜、剪辑。
    pub script: Arc<dyn ScriptModel>,
    /// SVG 场景（与文案共用 llm1）。
    pub visual: Arc<dyn VisualModel>,
    /// llm3：背景音乐。
    pub music: Arc<dyn MusicModel>,
    /// llm4：配音。
    pub voice: Arc<dyn VoiceModel>,
    /// 剪辑库。
    pub renderer: Arc<dyn Renderer>,
}

impl Providers {
    /// 按配置中的名称创建各环节的供应商。
    ///
    /// # 错误
    /// 名称未注册时返回错误，进程启动失败并提示可用的名称。
    pub fn from_config(config: &Config) -> anyhow::Result<Self> {
        // 硅基流动三个模型共用一个 HTTP 客户端。
        let cloud = Arc::new(siliconflow::SiliconFlow::new(config)?);
        let script: Arc<dyn ScriptModel> = match config.provider_llm1.as_str() {
            "openai" | "aliyun" | "deepseek" | "minimax" | "siliconflow" | "glm" | "custom" => {
                Arc::new(siliconflow::GlmScript::new(cloud.clone(), config))
            }
            other => anyhow::bail!("PROVIDER_LLM1 不支持供应商 `{other}`"),
        };
        let visual: Arc<dyn VisualModel> = match config.provider_llm1.as_str() {
            "openai" | "aliyun" | "deepseek" | "minimax" | "siliconflow" | "glm" | "custom" => {
                // 画面单独配置时，按它自己的接口地址创建客户端；否则与文案共用 llm1。
                if config.visual_overrides
                    && config.provider_visual == "custom"
                    && config.visual_base.trim().is_empty()
                {
                    anyhow::bail!("自定义画面供应商需要设置 VISUAL_BASE_URL");
                }
                let (access, base) = config.visual_access();
                let visual_cloud = if config.visual_overrides {
                    Arc::new(cloud.with_base(base)?)
                } else {
                    cloud.clone()
                };
                Arc::new(siliconflow::GlmScript::with_access(
                    visual_cloud,
                    access.clone(),
                ))
            }
            _ => unreachable!(),
        };

        Ok(Self {
            script,
            // SVG 与文案共用 llm1，不再配置第二个视觉模型。
            visual,
            music: match config.provider_llm3.as_str() {
                "procedural" => Arc::new(procedural::ProceduralMusic),
                "siliconflow" => Arc::new(siliconflow::MiniMaxMusic::new(cloud.clone(), config)),
                "minimax" => Arc::new(openai::MiniMaxApiMusic::new(config)),
                "stable_audio" => Arc::new(openai::StableAudioMusic::new(config)),
                other => anyhow::bail!("PROVIDER_LLM3 不支持供应商 `{other}`"),
            },
            voice: match config.provider_llm4.as_str() {
                "breeze" => Arc::new(breeze::BreezeTts::new(config)?),
                "qwen" | "aliyun" => Arc::new(openai::OpenAiSpeech::qwen(config)),
                "siliconflow" => Arc::new(openai::OpenAiSpeech::siliconflow(config)),
                "indextts" => Arc::new(openai::OpenAiSpeech::indextts(config)),
                other => anyhow::bail!("PROVIDER_LLM4 不支持供应商 `{other}`"),
            },
            renderer: match config.provider_renderer.as_str() {
                
                "quercus" => Arc::new(quercus::QuercusRenderer::new(config)),
                other => anyhow::bail!("PROVIDER_RENDERER 不支持供应商 `{other}`"),
            },
        })
    }
}
