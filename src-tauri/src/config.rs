//! 运行配置。
//!
//! 所有配置都从环境变量读取，并带有适合本机演示的默认值，因此不设置任何变量也能直接启动。
//! 变量的完整列表与说明见 `src-tauri/README.md` 的“配置”一节。

use std::env;
use std::path::PathBuf;

/// 后端运行配置，启动时由 [`Config::from_env`] 一次性读取，之后只读共享。
#[derive(Clone, Debug)]
pub struct Config {
    /// SQLite 数据库文件路径。
    pub database_path: PathBuf,
    /// 上传、生成与渲染文件的存放目录，按项目分子目录。
    pub storage_dir: PathBuf,
    /// 演示供应商使用的示例素材目录（图片、示例成片、背景音）。
    pub demo_media_dir: PathBuf,
    /// 对外暴露的接口前缀（不含末尾斜杠），用于路由挂载和拼接媒体地址。
    pub public_prefix: String,
    /// 自定义媒体协议的源（如 `explaincraft://localhost`）。
    pub public_origin: String,
    /// 媒体签名密钥；为空时每次启动随机生成（重启后旧链接失效，前端重新查询即可）。
    pub media_secret: Option<String>,
    /// 播放 / 下载地址有效期（秒）。
    pub media_url_ttl_seconds: i64,
    /// 演示供应商每一步的模拟耗时（毫秒），便于在界面上看到制作进度。
    pub demo_delay_ms: u64,
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
    /// llm2（拍摄筹备 / 画面生成）的供应商名称。
    pub provider_llm2: String,
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
    /// llm2（画面生成）的密钥与模型。
    pub llm2: ModelAccess,
    /// llm3（背景音乐）的密钥与模型。
    pub llm3: ModelAccess,
    /// 本地 ComfyUI 地址（breezeTTS2 配音）。
    pub comfyui_url: String,
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
            demo_media_dir: PathBuf::from(var("DEMO_MEDIA_DIR", "../front/public/media")),
            public_origin: var("PUBLIC_ORIGIN", "").trim_end_matches('/').to_string(),
            public_prefix: var("PUBLIC_API_PREFIX", "/api/v1")
                .trim_end_matches('/')
                .to_string(),
            media_secret: env::var("MEDIA_SIGNING_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            media_url_ttl_seconds: number("MEDIA_URL_TTL_SECONDS", 1800),
            demo_delay_ms: number("DEMO_DELAY_MS", 900),
            max_concurrent_jobs: number("MAX_CONCURRENT_JOBS", 4),
            question_ttl_seconds: number("QUESTION_TTL_SECONDS", 86_400),
            event_retention_seconds: number("EVENT_RETENTION_SECONDS", 86_400),
            auth_mode: if var("AUTH_MODE", "dev") == "strict" {
                AuthMode::Strict
            } else {
                AuthMode::Dev
            },
            provider_llm1: var("PROVIDER_LLM1", "demo"),
            provider_llm2: var("PROVIDER_LLM2", "demo"),
            provider_llm3: var("PROVIDER_LLM3", "demo"),
            provider_llm4: var("PROVIDER_LLM4", "demo"),
            provider_renderer: var("PROVIDER_RENDERER", "demo"),
            ffprobe: var("FFPROBE_PATH", "ffprobe"),
            siliconflow_base: var("SILICONFLOW_BASE_URL", "https://api.siliconflow.cn/v1")
                .trim_end_matches('/')
                .to_string(),
            llm1: ModelAccess {
                api_key: var("LLM1_API_KEY", ""),
                model: var("LLM1_MODEL", "zai-org/GLM-5.3"),
            },
            llm2: ModelAccess {
                api_key: var("LLM2_API_KEY", ""),
                model: var("LLM2_MODEL", "Qwen/Qwen-Image"),
            },
            llm3: ModelAccess {
                api_key: var("LLM3_API_KEY", ""),
                model: var("LLM3_MODEL", "MiniMaxAI/MiniMax-H3"),
            },
            comfyui_url: var("COMFYUI_URL", "http://127.0.0.1:8188")
                .trim_end_matches('/')
                .to_string(),
            breeze_model: var("BREEZE_MODEL", "int8 hybrid (recommended)"),
            breeze_voice_male: var("BREEZE_VOICE_MALE", ""),
            breeze_voice_female: var("BREEZE_VOICE_FEMALE", ""),
        }
    }
}
