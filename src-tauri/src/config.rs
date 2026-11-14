//! 运行配置。
//!
//! 所有配置都从环境变量读取，并带有真实供应商的默认值。
//! 变量的完整列表与说明见 `src-tauri/README.md` 的“配置”一节。

use std::env;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 后端运行配置，启动时由 [`Config::from_env`] 一次性读取，之后只读共享。
#[derive(Clone, Debug)]
pub struct Config {
    /// SQLite 数据库文件路径。
    pub database_path: PathBuf,
    /// 上传、生成与渲染文件的存放目录，按项目分子目录。
    pub storage_dir: PathBuf,
    /// 本地示例与曲库素材目录。
    pub demo_media_dir: PathBuf,
    /// 对外暴露的接口前缀（不含末尾斜杠），用于路由挂载和拼接媒体地址。
    pub public_prefix: String,
    /// 自定义媒体协议的源（如 `explaincraft://localhost`）。
    pub public_origin: String,
    /// 媒体签名密钥；为空时每次启动随机生成（重启后旧链接失效，前端重新查询即可）。
    pub media_secret: Option<String>,
    /// 播放 / 下载地址有效期（秒）。
    pub media_url_ttl_seconds: i64,
    /// 同时执行的任务数上限。
    pub max_concurrent_jobs: usize,
    /// 追问的等待时长（秒），超时后任务以 `INPUT_TIMEOUT` 失败。
    pub question_ttl_seconds: i64,
    /// 项目事件保留时长（秒），更早的事件会被清理，旧游标返回 410。
    pub event_retention_seconds: i64,
    /// 鉴权模式，见 [`AuthMode`]。
    pub auth_mode: AuthMode,
    /// llm1（选题、文案、分镜、剪辑方案）的供应商名称。
    pub provider_llm1: String,
    /// llm3（背景音乐）的供应商名称。
    pub provider_llm3: String,
    /// llm4（配音 TTS）的供应商名称。
    pub provider_llm4: String,
    /// 剪辑渲染器的供应商名称。
    pub provider_renderer: String,
    /// ffprobe 可执行文件，用于解析上传素材的真实宽高和时长。
    pub ffprobe: String,
    /// 硅基流动 OpenAI 兼容接口地址。
    pub siliconflow_base: String,
    /// llm1（文案 / 分镜 / 剪辑）的密钥与模型。
    pub llm1: ModelAccess,
    /// llm1 的 OpenAI 兼容 base URL。
    pub llm1_base: String,
    /// llm1 中「SVG 画面」环节是否单独配置；为 `false` 时直接复用 llm1。
    pub visual_overrides: bool,
    /// SVG 画面的密钥与模型（默认 `deepseek-ai/DeepSeek-V4.1-Flash`）。
    pub visual: ModelAccess,
    /// SVG 画面单独配置时使用的供应商名称。
    pub provider_visual: String,
    /// SVG 画面的 base URL。
    pub visual_base: String,
    /// llm3（背景音乐）的密钥与模型。
    pub llm3: ModelAccess,
    /// llm3 的 base URL。
    pub llm3_base: String,
    /// 阿里云 Qwen-TTS 的密钥与模型。
    pub qwen_tts: ModelAccess,
    /// Qwen-TTS base URL。
    pub qwen_tts_base: String,
    /// Qwen-TTS 默认声线。
    pub qwen_tts_voice: String,
    /// 硅基流动 TTS 的密钥与模型。
    pub siliconflow_tts: ModelAccess,
    /// 硅基流动 TTS base URL。
    pub siliconflow_tts_base: String,
    /// 硅基流动默认声线。
    pub siliconflow_tts_voice: String,
    /// 用户可选择的声线 ID 列表。
    pub tts_voices: Vec<String>,
    /// 本地 IndexTTS 的 OpenAI 兼容 base URL。
    pub indextts_base: String,
    /// 本地 IndexTTS 模型名。
    pub indextts_model: String,
    /// 本地 IndexTTS 默认音色或参考音标识。
    pub indextts_voice: String,
    /// 设置写入的 `.env` 文件。
    pub settings_file: PathBuf,
    /// 本地 ComfyUI 地址（breezeTTS2 配音）。
    pub comfyui_url: String,
    /// 本地 ComfyUI 安装目录；模型下载到这里，空表示自动探测。
    pub comfyui_dir: String,
    /// breezeTTS2 加载的检查点（ComfyUI 节点下拉框中的名称）。
    pub breeze_model: String,
    /// 男声参考音（音色克隆用），同名 `.json` / `.txt` 为转写文本。
    pub breeze_voice_male: String,
    /// 女声参考音。
    pub breeze_voice_female: String,
}

/// 一个云端模型的访问信息。密钥只从环境变量 / src-tauri/.env 读取，绝不写进代码或日志。
#[derive(Clone)]
pub struct ModelAccess {
    /// API 密钥。
    pub api_key: String,
    /// 模型 ID，如 `zai-org/GLM-5.3`。
    pub model: String,
}

/// 调试输出时隐藏密钥。
impl std::fmt::Debug for ModelAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let masked = if self.api_key.is_empty() {
            "未设置"
        } else {
            "已设置"
        };
        write!(f, "{}（密钥{masked}）", self.model)
    }
}

/// 鉴权模式。登录模块接入前用于区分本机联调与正式部署。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// 本机联调：没有令牌时视为 `local_user`，有令牌时按令牌区分用户。
    Dev,
    /// 正式部署：请求必须携带 `Authorization: Bearer <token>`，否则返回 401。
    Strict,
}

/// 默认视觉/文案/剪辑模型：硅基流动托管的 DeepSeek-V4.1-Flash。
pub const DEFAULT_VISION_MODEL: &str = "deepseek-ai/DeepSeek-V4.1-Flash";

/// 读取字符串变量；未设置或只有空白时使用默认值。
fn var(name: &str, default: &str) -> String {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// 读取数值变量；未设置或无法解析时使用默认值，避免因配置笔误导致启动失败。
fn number<T: std::str::FromStr>(name: &str, default: T) -> T {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

impl Config {
    /// SVG 画面实际使用的模型访问信息：未单独配置时回收 llm1。
    pub fn visual_access(&self) -> (&ModelAccess, &str) {
        if self.visual_overrides {
            let base: &str = if self.visual_base.is_empty() {
                self.provider_base_for(&self.provider_visual)
            } else {
                &self.visual_base
            };
            (&self.visual, base)
        } else {
            (&self.llm1, &self.llm1_base)
        }
    }

    /// 供应商对应的 base URL；显式配置优先。
    fn provider_base_for(&self, provider: &str) -> &str {
        match provider {
            "siliconflow" => &self.siliconflow_base,
            _ => provider_base(provider),
        }
    }
    /// 当前供应商的默认声线。
    pub fn default_voice(&self) -> String {
        match self.provider_llm4.as_str() {
            "siliconflow" => self.siliconflow_tts_voice.clone(),
            "indextts" => self.indextts_voice.clone(),
            "breeze" => "voice_warm".into(),
            _ => self.qwen_tts_voice.clone(),
        }
    }

    /// 返回给前端的媒体地址前缀：`PUBLIC_ORIGIN` + 接口前缀。
    pub fn public_base(&self) -> String {
        format!("{}{}", self.public_origin, self.public_prefix)
    }

    /// 从环境变量构建配置。
    ///
    /// `DATABASE_PATH`、`STORAGE_DIR` 未设置时放在 `DATA_DIR`（默认 `data`）下。
    pub fn from_env() -> Self {
        let data_dir = PathBuf::from(var("DATA_DIR", "data"));
        Self {
            database_path: env::var("DATABASE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| data_dir.join("explaincraft.sqlite3")),
            storage_dir: env::var("STORAGE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| data_dir.join("storage")),
            demo_media_dir: PathBuf::from(var("DEMO_MEDIA_DIR", "../shared/public/media")),
            public_origin: var("PUBLIC_ORIGIN", "").trim_end_matches('/').to_string(),
            public_prefix: var("PUBLIC_API_PREFIX", "/api/v1")
                .trim_end_matches('/')
                .to_string(),
            media_secret: env::var("MEDIA_SIGNING_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            media_url_ttl_seconds: number("MEDIA_URL_TTL_SECONDS", 1800),
            max_concurrent_jobs: number("MAX_CONCURRENT_JOBS", 4),
            question_ttl_seconds: number("QUESTION_TTL_SECONDS", 86_400),
            event_retention_seconds: number("EVENT_RETENTION_SECONDS", 86_400),
            auth_mode: if var("AUTH_MODE", "dev") == "strict" {
                AuthMode::Strict
            } else {
                AuthMode::Dev
            },
            provider_llm1: var("PROVIDER_LLM1", "siliconflow"),
            provider_llm3: var("PROVIDER_LLM3", "procedural"),
            provider_llm4: var("PROVIDER_LLM4", "qwen"),
            provider_renderer: var("PROVIDER_RENDERER", "quercus"),
            ffprobe: var("FFPROBE_PATH", "ffprobe"),
            siliconflow_base: var("SILICONFLOW_BASE_URL", "https://api.siliconflow.cn/v1")
                .trim_end_matches('/')
                .to_string(),
            llm1: ModelAccess {
                api_key: var("LLM1_API_KEY", ""),
                model: var("LLM1_MODEL", DEFAULT_VISION_MODEL),
            },
            llm1_base: var("LLM1_BASE_URL", "").trim_end_matches('/').to_string(),
            // 视觉模型可用独立的 OpenAI 兼容供应商；未设置 VISUAL_MODEL 时复用 llm1，
            // 默认预设整条流程都走 DeepSeek-V4.1-Flash。
            visual_overrides: env::var("VISUAL_MODEL")
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),            visual: ModelAccess {
                api_key: var("VISUAL_API_KEY", &var("LLM1_API_KEY", "")),
                model: var("VISUAL_MODEL", &var("LLM1_MODEL", DEFAULT_VISION_MODEL)),
            },
            provider_visual: var("PROVIDER_VISUAL", &var("PROVIDER_LLM1", "siliconflow")),
            visual_base: var("VISUAL_BASE_URL", "").trim_end_matches('/').to_string(),
            llm3: ModelAccess {
                api_key: var("LLM3_API_KEY", ""),
                model: var("LLM3_MODEL", "MiniMaxAI/MiniMax-H3"),
            },
            llm3_base: var("LLM3_BASE_URL", "").trim_end_matches('/').to_string(),
            qwen_tts: ModelAccess {
                api_key: var("QWEN_TTS_API_KEY", &var("DASHSCOPE_API_KEY", "")),
                model: var("QWEN_TTS_MODEL", "qwen-tts"),
            },
            qwen_tts_base: var(
                "QWEN_TTS_BASE_URL",
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
            )
            .trim_end_matches('/')
            .to_string(),
            qwen_tts_voice: var("QWEN_TTS_VOICE", "Cherry"),
            siliconflow_tts: ModelAccess {
                api_key: var("SILICONFLOW_TTS_API_KEY", &var("LLM1_API_KEY", "")),
                model: var("SILICONFLOW_TTS_MODEL", "fnlp/MOSS-TTSD-v0.5"),
            },
            siliconflow_tts_base: var("SILICONFLOW_TTS_BASE_URL", "https://api.siliconflow.cn/v1")
                .trim_end_matches('/')
                .to_string(),
            siliconflow_tts_voice: var("SILICONFLOW_TTS_VOICE", ""),
            tts_voices: var("TTS_VOICES", "alex,anna,bella,claire,diana,eric,jason,joanna,joseph,julia,liam,lily,mark,molly,noah,oliver,ryan,sophia,tony,uk,Cherry,Ethan,Serena")
                .split(',')
                .map(str::trim)
                .filter(|voice| !voice.is_empty())
                .map(str::to_string)
                .collect(),
            indextts_base: var("INDEXTTS_BASE_URL", "http://127.0.0.1:8000/v1")
                .trim_end_matches('/')
                .to_string(),
            indextts_model: var("INDEXTTS_MODEL", "indextts"),
            indextts_voice: var("INDEXTTS_VOICE", "default"),
            comfyui_url: var("COMFYUI_URL", "http://127.0.0.1:8188")
                .trim_end_matches('/')
                .to_string(),
            comfyui_dir: var("COMFYUI_DIR", ""),
            breeze_model: var("BREEZE_MODEL", "int8 hybrid (recommended)"),
            breeze_voice_male: var("BREEZE_VOICE_MALE", ""),
            breeze_voice_female: var("BREEZE_VOICE_FEMALE", ""),
            settings_file: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env"),
        }
    }
}

/// 一组可编辑的供应商配置；密钥只在更新时传入，读取时不返回。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetting {
    /// 供应商 ID。
    pub provider: String,
    /// OpenAI 兼容或服务 base URL。
    #[serde(alias = "base_url")]
    pub base_url: String,
    /// 模型名。
    pub model: String,
    /// 新密钥；`None` 表示保留现有密钥。
    #[serde(alias = "api_key")]
    pub api_key: Option<String>,
    /// 是否已有密钥，供 UI 显示而不泄露内容。
    pub has_api_key: bool,
    /// 默认声线；每次创作仍可在作品设置中重新选择。
    #[serde(default)]
    pub voice_id: String,
    /// 是否单独指定该流程的模型；`None` 时沿用统一模型，忽略其余覆盖字段。
    #[serde(default)]
    pub overridden: Option<bool>,
}

impl ProviderSetting {
    /// 设置默认声线。
    pub(crate) fn with_voice(mut self, voice_id: String) -> Self {
        self.voice_id = voice_id;
        self
    }

    /// 声明该流程是否单独指定模型。
    pub(crate) fn overridden(mut self, overridden: bool) -> Self {
        self.overridden = Some(overridden);
        self
    }

    /// 是否启用独立配置：未显式声明时继承统一模型。
    pub fn is_overridden(&self) -> bool {
        self.overridden.unwrap_or(false)
    }
}

/// 创作、画面、音乐与配音四组配置。
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettings {
    /// 文案、对话、分镜和 SVG 共用的 llm1。
    pub script: ProviderSetting,
    /// 仅 SVG 画面使用的模型；未覆盖时与 `script` 相同。
    #[serde(default)]
    pub visual: ProviderSetting,
    /// 背景音乐。
    pub music: ProviderSetting,
    /// 配音。
    pub voice: ProviderSetting,
}

/// OpenAI 兼容及原生模型供应商的默认 base URL。
pub fn provider_base(provider: &str) -> &'static str {
    match provider {
        "openai" => "https://api.openai.com/v1",
        "aliyun" | "qwen" => "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "deepseek" => "https://api.deepseek.com/v1",
        "minimax" => "https://api.minimaxi.com/v1",
        "glm" => "https://open.bigmodel.cn/api/paas/v4",
        "stable_audio" => "https://api.stability.ai",
        "custom" => "",
        _ => "https://api.siliconflow.cn/v1",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn includes_all_documented_openai_compatible_presets() {
        for provider in [
            "openai",
            "aliyun",
            "deepseek",
            "minimax",
            "siliconflow",
            "glm",
        ] {
            assert!(super::provider_base(provider).starts_with("https://"));
        }
    }
}
