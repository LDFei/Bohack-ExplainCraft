//! 数据对象。
//!
//! 对外的结构与 `docs/API.md` 第 3 节、前端 `src/domain/types.ts` 一一对应，字段名使用 `snake_case`。
//! 文件末尾的 [`JobInput`]、[`Artifacts`] 等是后端内部结构，只存入数据库，不直接返回给前端。

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------- 阶段 / 状态枚举 ----------

/// 制作阶段，顺序即流程顺序（派生的 `Ord` 依赖声明顺序）。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StageKey {
    /// 选题：理解主题、受众、目的与风格（llm1）。
    Planning,
    /// 文案：解说词 / 台词（llm1）。
    Script,
    /// 分镜：景别、运镜、画面内容与时长（llm1）。
    Storyboard,
    /// 拍摄筹备：为每个镜头准备画面（llm2）。
    Visuals,
    /// 音频：背景音乐（llm3）与配音（llm4）。
    Audio,
    /// 剪辑：转场、字幕、音画对齐（llm1 生成剪辑方案）。
    Edit,
    /// 调色与特效：应用用户选择的预设。
    Finishing,
    /// 输出：渲染成片（剪辑库）。
    Render,
}

impl StageKey {
    /// 按流程顺序排列的全部阶段。
    pub const ALL: [StageKey; 8] = [
        StageKey::Planning,
        StageKey::Script,
        StageKey::Storyboard,
        StageKey::Visuals,
        StageKey::Audio,
        StageKey::Edit,
        StageKey::Finishing,
        StageKey::Render,
    ];

    /// 阶段在界面上展示的中文名称。
    pub fn label(self) -> &'static str {
        match self {
            StageKey::Planning => "理解你的想法",
            StageKey::Script => "创作文案",
            StageKey::Storyboard => "规划分镜",
            StageKey::Visuals => "准备画面",
            StageKey::Audio => "配音与音乐",
            StageKey::Edit => "剪辑合成",
            StageKey::Finishing => "调色与字幕",
            StageKey::Render => "输出成片",
        }
    }

    /// 本阶段及其之后的全部阶段。上游产物变化时，下游阶段一并失效。
    pub fn and_after(self) -> Vec<StageKey> {
        StageKey::ALL
            .iter()
            .copied()
            .filter(|s| *s >= self)
            .collect()
    }
}

/// 合并两组失效阶段：去重并按流程顺序排列。
pub fn merge_stages(current: &[StageKey], extra: &[StageKey]) -> Vec<StageKey> {
    let mut all: Vec<StageKey> = current.iter().chain(extra).copied().collect();
    all.sort();
    all.dedup();
    all
}

/// 任务状态（文档 3.3 的状态机）。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// 已受理，等待 worker 认领。
    Queued,
    /// 正在执行。
    Running,
    /// 等待用户回答追问，不占用执行线程。
    WaitingInput,
    /// 已请求停止，等待执行者收尾。
    Cancelling,
    /// 终态：成功。
    Succeeded,
    /// 终态：失败。
    Failed,
    /// 终态：已停止。
    Cancelled,
}

impl JobStatus {
    /// 数据库中保存的字符串形式。
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::WaitingInput => "waiting_input",
            JobStatus::Cancelling => "cancelling",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }

    /// 从数据库字符串还原；未知值按 `cancelled` 处理，保证不会卡在活动状态。
    pub fn parse(text: &str) -> Self {
        match text {
            "queued" => JobStatus::Queued,
            "running" => JobStatus::Running,
            "waiting_input" => JobStatus::WaitingInput,
            "cancelling" => JobStatus::Cancelling,
            "succeeded" => JobStatus::Succeeded,
            "failed" => JobStatus::Failed,
            _ => JobStatus::Cancelled,
        }
    }
}

/// 任务类型。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// 首次生成：按当前草稿、消息和素材制作视频。
    Generate,
    /// 对话修改：按修改意见更新草稿，复用有效产物后生成新版本。
    Revise,
    /// 仅讨论：只回答问题，不修改草稿、不生成视频。
    Chat,
    /// 导出：把指定成片转为其他输出规格。
    Export,
}

impl JobKind {
    /// 数据库中保存的字符串形式。
    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Generate => "generate",
            JobKind::Revise => "revise",
            JobKind::Chat => "chat",
            JobKind::Export => "export",
        }
    }

    /// 从数据库字符串还原。
    pub fn parse(text: &str) -> Self {
        match text {
            "generate" => JobKind::Generate,
            "revise" => JobKind::Revise,
            "chat" => JobKind::Chat,
            _ => JobKind::Export,
        }
    }
}

// ---------- Draft ----------

/// 草稿中的基础设置。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settings {
    /// 目标时长（秒）；实际时长以成片 `duration_ms` 为准。
    pub target_duration_seconds: u32,
    /// 画面比例：`9:16` / `16:9` / `1:1`。
    pub aspect_ratio: String,
    /// 语言，如 `zh-CN`。
    pub language: String,
    /// 风格 ID，取值来自 capabilities。
    pub style_id: String,
}

/// 草稿中的音频设置。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AudioSettings {
    /// 配音音色 ID。
    pub voice_id: String,
    /// 语速，0.5～2.0。
    pub speech_rate: f64,
    /// 音乐来源：`auto` 自动匹配、`none` 不配乐、`library` 曲库、`upload` 用户上传。
    pub music_mode: String,
    /// `library` 模式下的曲库 ID，其他模式为 `null`。
    pub music_id: Option<String>,
    /// `upload` 模式下的音频素材 ID，其他模式为 `null`。
    pub music_asset_id: Option<String>,
    /// 只读：音频准备完成时由后端填入的实际音乐素材。客户端提交的值会被忽略。
    #[serde(default)]
    pub resolved_music_asset_id: Option<String>,
    /// 配音音量，0～1。
    pub narration_volume: f64,
    /// 背景音乐音量，0～1。
    pub music_volume: f64,
}

/// 草稿中的调色、特效与字幕设置。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Finishing {
    /// 调色预设 ID。
    pub color_preset_id: String,
    /// 特效预设 ID。
    pub effect_preset_id: String,
    /// 是否烧录字幕。
    pub subtitles_enabled: bool,
}

/// 分镜中的一个镜头，按数组顺序播放。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Shot {
    /// 镜头 ID。保存分镜时新增镜头可省略，由后端分配。
    #[serde(default)]
    pub id: String,
    /// 镜头时长（毫秒），必须大于 0。
    pub duration_ms: i64,
    /// 旁白，可以为空。
    #[serde(default)]
    pub narration: String,
    /// 画面描述，作为画面生成的提示词。
    #[serde(default)]
    pub visual_prompt: String,
    /// 景别，如“远景”“特写”。
    #[serde(default)]
    pub shot_size: String,
    /// 运镜，如“缓慢推进”。
    #[serde(default)]
    pub camera_motion: String,
    /// 镜头使用的素材；无素材时为空数组。
    #[serde(default)]
    pub asset_ids: Vec<String>,
    /// 只在响应中填充的临时缩略图地址，不落库。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
}

/// 创作方案摘要，由选题阶段整理。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Brief {
    /// 主题。
    pub subject: String,
    /// 受众。
    pub audience: String,
    /// 目的，如“店铺宣传”。
    pub purpose: String,
    /// 内容风格描述。
    pub style: String,
}

/// 完整脚本。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Script {
    /// 标题，1～100 字。
    pub title: String,
    /// 完整解说词，1～20000 字。
    pub text: String,
}

/// 项目草稿：一次生成所需的全部输入与中间结果。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Draft {
    /// 创作方案，选题完成前为 `null`。
    pub brief: Option<Brief>,
    /// 基础设置。
    pub settings: Settings,
    /// 脚本，文案阶段完成前为 `null`。
    pub script: Option<Script>,
    /// 分镜。
    pub shots: Vec<Shot>,
    /// 音频设置。
    pub audio: AudioSettings,
    /// 调色、特效与字幕。
    pub finishing: Finishing,
    /// 需要重新计算的阶段，按流程顺序排列。
    pub invalidated_stages: Vec<StageKey>,
}

/// `PATCH /draft` 的修改内容：每个顶层字段整体替换，未提交的字段保持原值。
///
/// `deny_unknown_fields` 保证 `brief` 等只读字段无法被提交（返回 422）。
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct DraftChanges {
    /// 新的基础设置。
    pub settings: Option<Settings>,
    /// 新的完整脚本（不能与 `shots` 同时提交）。
    pub script: Option<Script>,
    /// 新的完整分镜数组。
    pub shots: Option<Vec<Shot>>,
    /// 新的音频设置（需带齐全部可写字段）。
    pub audio: Option<AudioSettings>,
    /// 新的调色、特效与字幕设置。
    pub finishing: Option<Finishing>,
}

// ---------- Project / Message / Job / Asset / Version ----------

/// 项目：一段对话加一部视频的工作空间。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    /// 项目 ID。
    pub id: String,
    /// 标题，1～100 字。
    pub title: String,
    /// 页面摘要状态：`draft / processing / waiting_input / ready / failed`。
    pub status: String,
    /// 当前草稿版本，从 1 开始。
    pub draft_revision: i64,
    /// 当前活动任务；每个项目最多一个。
    pub active_job_id: Option<String>,
    /// 最近成功生成的成片版本。
    pub latest_video_version_id: Option<String>,
    /// 当前草稿是否还没有生成对应的成片。
    pub draft_dirty: bool,
    /// 创建时间。
    pub created_at: String,
    /// 最近变更时间。
    pub updated_at: String,
}

/// 对话消息。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    /// 消息 ID。
    pub id: String,
    /// 所属项目。
    pub project_id: String,
    /// `user` 或 `assistant`。
    pub role: String,
    /// 当前可见的完整文本。
    pub text: String,
    /// `pending / streaming / completed / failed / cancelled`。
    pub status: String,
    /// 用户消息附带的素材。
    pub asset_ids: Vec<String>,
    /// 关联的任务。
    pub job_id: Option<String>,
    /// 结构化卡片（plan / progress / question / video / error），前端据此渲染。
    pub blocks: Vec<Value>,
    /// 已持久化的最后一个增量序号，供前端从快照恢复时去重。
    pub last_delta_seq: i64,
    /// 创建时间。
    pub created_at: String,
}

/// 任务中的单个阶段状态。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Stage {
    /// 阶段。
    pub key: StageKey,
    /// `pending / running / succeeded / failed / skipped / cancelled`。
    pub status: String,
    /// 阶段进度估算，0～100。
    pub progress_percent: Option<u8>,
    /// 给用户看的阶段说明。
    pub message: String,
}

/// 单选题的一个选项。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QuestionOption {
    /// 提交给后端的值。
    pub value: String,
    /// 展示给用户的文字。
    pub label: String,
}

/// 任务追问：无法继续制作时向用户补充询问的问题。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Question {
    /// 问题 ID，回答时原样提交。
    pub id: String,
    /// 问题内容。
    pub prompt: String,
    /// `text` 或 `single_select`。
    pub input_type: String,
    /// 单选选项；文本题为空数组。
    pub options: Vec<QuestionOption>,
    /// 过期时间，过期后任务以 `INPUT_TIMEOUT` 失败。
    pub expires_at: String,
}

/// 任务失败信息。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JobError {
    /// 错误码，如 `PROVIDER_UNAVAILABLE`。
    pub code: String,
    /// 面向用户的说明。
    pub message: String,
    /// 是否允许重试。
    pub retryable: bool,
    /// 失败的阶段。
    pub failed_stage: Option<StageKey>,
}

/// 制作任务（文档 3.3）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Job {
    /// 任务 ID。
    pub id: String,
    /// 所属项目。
    pub project_id: String,
    /// 重试来源任务；非重试为 `null`。
    pub parent_job_id: Option<String>,
    /// 任务类型。
    pub kind: JobKind,
    /// 任务状态。
    pub status: JobStatus,
    /// 当前阶段；排队和聊天任务可为 `null`。
    pub current_stage: Option<StageKey>,
    /// 整体进度估算，0～100。
    pub progress_percent: Option<u8>,
    /// 全部阶段状态。chat 为空，export 只有 render。
    pub stages: Vec<Stage>,
    /// 当前待回答的问题。
    pub question: Option<Question>,
    /// 成功结果，结构随任务类型不同（文档 3.3）。
    pub result: Option<Value>,
    /// 失败信息。
    pub error: Option<JobError>,
    /// 创建时间。
    pub created_at: String,
    /// 首次开始执行的时间。
    pub started_at: Option<String>,
    /// 进入终态的时间。
    pub finished_at: Option<String>,
}

/// 素材：上传文件、素材库选取、模型生成或渲染出的文件。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Asset {
    /// 素材 ID。
    pub id: String,
    /// 所属项目。
    pub project_id: String,
    /// `image / video / audio`。
    pub kind: String,
    /// `upload / library / generated / rendered`。
    pub source: String,
    /// `processing / ready / failed`。
    pub status: String,
    /// 原始或生成的文件名。
    pub file_name: String,
    /// 按文件头识别的真实类型。
    pub mime_type: String,
    /// 文件大小（字节）。
    pub size_bytes: i64,
    /// 时长（毫秒），图片为 `null`。
    pub duration_ms: Option<i64>,
    /// 宽度（像素），音频为 `null`。
    pub width: Option<i64>,
    /// 高度（像素），音频为 `null`。
    pub height: Option<i64>,
    /// 临时缩略图地址，只在响应中填充。
    pub thumbnail_url: Option<String>,
    /// 解析失败原因 `{code, message}`，正常时为 `null`。
    pub error: Option<Value>,
}

/// 成片版本的一个导出文件。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VideoExport {
    /// 导出文件的素材 ID。
    pub asset_id: String,
    /// 分辨率，如 `720p`。
    pub resolution: String,
    /// 帧率。
    pub fps: u32,
    /// 容器格式，如 `mp4`。
    pub format: String,
    /// 生成时间。
    pub created_at: String,
}

/// 成片版本：每次生成成功创建一个，不可修改。
#[derive(Serialize, Clone, Debug)]
pub struct VideoVersion {
    /// 版本 ID。
    pub id: String,
    /// 所属项目。
    pub project_id: String,
    /// 项目内递增的版本号，从 1 开始。
    pub version_number: i64,
    /// 生成该版本时的草稿版本。
    pub source_draft_revision: i64,
    /// 生成该版本的任务。
    pub source_job_id: String,
    /// 创建时间。
    pub created_at: String,
    /// 预览成片的素材 ID。
    pub video_asset_id: String,
    /// 临时缩略图地址。
    pub thumbnail_url: Option<String>,
    /// 实际时长（毫秒）。
    pub duration_ms: i64,
    /// 宽度（像素）。
    pub width: i64,
    /// 高度（像素）。
    pub height: i64,
    /// 临时播放地址（列表接口省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_url: Option<String>,
    /// 播放地址过期时间（列表接口省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_expires_at: Option<String>,
    /// 生成时的完整草稿快照（列表接口省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Draft>,
    /// 导出文件列表（列表接口省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<Vec<VideoExport>>,
}

// ---------- 任务内部输入（不直接暴露） ----------

/// 用户对追问的一次回答。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Answer {
    /// 回答的问题 ID。
    pub question_id: String,
    /// 回答内容。
    pub value: String,
}

/// 导出任务的规格。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExportSpec {
    /// 要导出的成片版本。
    pub version_id: String,
    /// 容器格式。
    pub format: String,
    /// 分辨率。
    pub resolution: String,
    /// 帧率。
    pub fps: u32,
}

/// 任务受理时保存的完整输入，重试时原样复用，保证结果可复现。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct JobInput {
    /// 用户本次消息原文。
    pub text: String,
    /// 本次消息附带的素材。
    #[serde(default)]
    pub asset_ids: Vec<String>,
    /// revise 时的基准成片版本。
    #[serde(default)]
    pub base_video_version_id: Option<String>,
    /// 仅演示供应商使用：`failure` / `question`，用于体验失败重试与追问。
    #[serde(default)]
    pub demo_scenario: Option<String>,
    /// 用户对追问的回答，按时间顺序。
    #[serde(default)]
    pub answers: Vec<Answer>,
    /// revise 的修改意见是否已经写入草稿（重试时不再重复应用）。
    #[serde(default)]
    pub revision_applied: bool,
    /// 用户附件是否已经放入分镜 / 背景音乐。
    #[serde(default)]
    pub attachments_applied: bool,
    /// 导出任务的规格。
    #[serde(default)]
    pub export: Option<ExportSpec>,
}

/// 项目内部的制作产物（检查点），供后续阶段和后续版本复用，不对前端暴露。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Artifacts {
    /// 每个镜头的配音，键为 shot_id。
    #[serde(default)]
    pub narration: std::collections::BTreeMap<String, NarrationArtifact>,
    /// 最近一次的剪辑方案。
    #[serde(default)]
    pub edit_plan: Option<crate::providers::EditPlan>,
    /// 最近一次的创作方案摘要。
    #[serde(default)]
    pub plan_summary: Option<String>,
}

/// 单个镜头的配音产物。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NarrationArtifact {
    /// 配音音频的素材 ID。
    pub asset_id: String,
    /// 生成指纹（旁白 + 音色 + 语速 + 语言 + 时长），相同则复用。
    pub key: String,
    /// 配音时长（毫秒）。
    pub duration_ms: i64,
}
