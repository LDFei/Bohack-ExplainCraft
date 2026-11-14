//! 本地 breezeTTS2 模型的按需下载。
//!
//! 完整模型约 5.2 GB，放进安装包会让分发和更新都不可接受，因此改为：用户
//! 第一次选择「本地 breezeTTS2」配音时才征询同意，再下载到 ComfyUI 的
//! `models/breezetts2/` 目录。下载支持断点续传，进度通过事件推送给前端。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::config::Config;

/// HuggingFace 上的模型仓库。
const REPO_ID: &str = "drbaph/Breeze-TTS-2-comfyui";
/// ComfyUI 的模型子目录名，必须与 breezeTTS2 自定义节点一致。
const FOLDER: &str = "breezetts2";
/// 进度事件名。
const EVENT: &str = "breeze-model-progress";
/// 默认下载端点；可改用镜像（例如 `https://hf-mirror.com`）。
const DEFAULT_ENDPOINT: &str = "https://huggingface.co";
/// 进度事件的最小间隔，避免下载时每秒推送数百次。
const EMIT_INTERVAL: Duration = Duration::from_millis(250);

/// 一个需要下载的模型文件。
struct ModelFile {
    /// 在仓库内的相对路径，同时也是目标目录内的相对路径。
    path: &'static str,
    /// 期望的字节数，用于断点续传与完整性判断。
    size: u64,
}

/// 模型文件清单；与 `Breeze-TTS-2-int8-hybrid.safetensors` 配套的最小集合。
const FILES: &[ModelFile] = &[
    ModelFile {
        path: "Breeze-TTS-2-int8-hybrid.safetensors",
        size: 4_863_779_632,
    },
    ModelFile {
        path: "audio_tokenizer/model.safetensors",
        size: 682_293_092,
    },
    ModelFile {
        path: "tokenizer.json",
        size: 33_386_945,
    },
    ModelFile {
        path: "tokenizer_config.json",
        size: 1_157_960,
    },
    ModelFile {
        path: "config.json",
        size: 10_161,
    },
    ModelFile {
        path: "audio_tokenizer/config.json",
        size: 2_336,
    },
    ModelFile {
        path: "special_tokens_map.json",
        size: 886,
    },
    ModelFile {
        path: "generation_config.json",
        size: 251,
    },
    ModelFile {
        path: "audio_tokenizer/preprocessor_config.json",
        size: 234,
    },
    ModelFile {
        path: "audio_tokenizer/configuration.json",
        size: 76,
    },
];

/// 是否有下载正在进行，避免重复点击并发写同一目录。
static BUSY: AtomicBool = AtomicBool::new(false);
/// 用户是否请求中止当前下载。
static CANCEL: AtomicBool = AtomicBool::new(false);

/// 全部文件的字节数合计。
fn total_bytes() -> u64 {
    FILES.iter().map(|file| file.size).sum()
}

/// 本地模型状态，也是下载完成后的返回值。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    /// 全部文件是否已就绪。
    pub installed: bool,
    /// 模型目录（`<ComfyUI>/models/breezetts2/drbaph_Breeze-TTS-2-comfyui`）。
    pub dir: String,
    /// ComfyUI 根目录；没有找到时为空字符串。
    pub comfyui_dir: String,
    /// ComfyUI 根目录的来源：`config`、`detected` 或 `unset`。
    pub source: String,
    /// 需要下载的总字节数。
    pub total_bytes: u64,
    /// 是否有下载正在进行。
    pub downloading: bool,
}

/// 推送给前端的下载进度。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// 阶段：`downloading`、`done` 或 `cancelled`。
    phase: String,
    /// 当前文件在模型目录内的相对路径。
    file: String,
    /// 当前文件序号，从 1 开始。
    file_index: usize,
    /// 文件总数。
    file_count: usize,
    /// 已写入的字节数（含本次之前已完成的部分）。
    received: u64,
    /// 全部文件的总字节数。
    total: u64,
    /// 完成百分比，0～100。
    percent: f64,
}

/// 一个目录是否像 ComfyUI 安装目录：有 `models`，并且有入口或自定义节点目录。
fn looks_like_comfyui(dir: &Path) -> bool {
    dir.join("models").is_dir() && (dir.join("main.py").is_file() || dir.join("custom_nodes").is_dir())
}

/// 常见安装位置，只用于少让用户手填路径；都需要通过 [`looks_like_comfyui`] 校验。
fn candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    for dir in ["D:/software/ComfyUI", "D:/ComfyUI", "C:/ComfyUI"] {
        list.push(PathBuf::from(dir));
    }
    for key in ["USERPROFILE", "LOCALAPPDATA", "PROGRAMFILES"] {
        if let Some(base) = std::env::var_os(key) {
            list.push(PathBuf::from(base).join("ComfyUI"));
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        list.push(PathBuf::from(home).join("Documents").join("ComfyUI"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        list.push(PathBuf::from(home).join("ComfyUI"));
    }
    list
}

/// 确定 ComfyUI 根目录：配置优先，其次探测常见位置。
pub fn resolve_comfyui_dir(config: &Config) -> Option<(PathBuf, &'static str)> {
    let configured = config.comfyui_dir.trim();
    if !configured.is_empty() {
        let path = PathBuf::from(configured);
        if path.is_dir() {
            return Some((path, "config"));
        }
    }
    candidates()
        .into_iter()
        .find(|path| path.is_dir() && looks_like_comfyui(path))
        .map(|path| (path, "detected"))
}

/// 仓库名在磁盘上的目录名（`/` 换成 `_`，与 breezeTTS2 节点的规则一致）。
fn repo_folder() -> String {
    REPO_ID.replace(['/', '\\', ':'], "_")
}

/// 模型目录：`<ComfyUI>/models/breezetts2/<仓库名>`。
pub fn model_dir(comfyui_dir: &Path) -> PathBuf {
    comfyui_dir.join("models").join(FOLDER).join(repo_folder())
}

/// 全部文件是否都已按预期大小落盘。
fn installed(dir: &Path) -> bool {
    FILES.iter().all(|file| {
        std::fs::metadata(dir.join(file.path))
            .map(|meta| meta.is_file() && meta.len() == file.size)
            .unwrap_or(false)
    })
}

/// 读取当前状态；没有找到 ComfyUI 目录时 `source` 为 `unset`。
pub fn status(config: &Config) -> ModelStatus {
    match resolve_comfyui_dir(config) {
        Some((root, source)) => {
            let dir = model_dir(&root);
            ModelStatus {
                installed: installed(&dir),
                dir: dir.display().to_string(),
                comfyui_dir: root.display().to_string(),
                source: source.into(),
                total_bytes: total_bytes(),
                downloading: BUSY.load(Ordering::SeqCst),
            }
        }
        None => ModelStatus {
            installed: false,
            dir: String::new(),
            comfyui_dir: String::new(),
            source: "unset".into(),
            total_bytes: total_bytes(),
            downloading: BUSY.load(Ordering::SeqCst),
        },
    }
}

/// 中止正在进行的下载；没有下载时无副作用。
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// 下载进度累加器，负责限频推送事件。
struct Reporter<'a, R: Runtime> {
    /// 用于推送事件的句柄。
    app: &'a AppHandle<R>,
    /// 已完成的字节数。
    received: u64,
    /// 全部文件的总字节数。
    total: u64,
    /// 上一次推送的时间，用于限频。
    last: Instant,
}

impl<R: Runtime> Reporter<'_, R> {
    /// 推送一帧进度；`force` 为真时忽略限频。
    fn push(&mut self, phase: &str, file: &str, index: usize, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last) < EMIT_INTERVAL {
            return;
        }
        self.last = now;
        let percent = if self.total == 0 {
            0.0
        } else {
            (self.received as f64 * 100.0 / self.total as f64 * 10.0).round() / 10.0
        };
        let _ = self.app.emit(
            EVENT,
            Progress {
                phase: phase.into(),
                file: file.into(),
                file_index: index,
                file_count: FILES.len(),
                received: self.received,
                total: self.total,
                percent,
            },
        );
    }
}

/// 拼接 `.part` 临时文件路径。
fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

/// 下载端点；允许用镜像替换。
fn endpoint() -> String {
    std::env::var("BREEZE_MODEL_ENDPOINT")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string())
}

/// 下载单个文件，支持续传；返回本次新写入的字节数。
async fn fetch_file<R: Runtime>(
    client: &reqwest::Client,
    reporter: &mut Reporter<'_, R>,
    dest: &Path,
    file: &ModelFile,
    index: usize,
) -> anyhow::Result<()> {
    let part = part_path(dest);
    let mut offset = std::fs::metadata(&part).map(|meta| meta.len()).unwrap_or(0);
    if offset > file.size {
        // 临时文件比预期还大，说明来源变了，重新下载。
        let _ = std::fs::remove_file(&part);
        offset = 0;
    }
    reporter.received += offset;
    if offset == file.size {
        // 上次已下满但还没来得及改名，直接收尾，避免再发一次请求。
        tokio::fs::rename(&part, dest).await?;
        reporter.push("downloading", file.path, index, true);
        return Ok(());
    }
    let mut request = client.get(format!(
        "{}/{}/resolve/main/{}",
        endpoint(),
        REPO_ID,
        file.path
    ));
    if offset > 0 {
        request = request.header("range", format!("bytes={offset}-"));
    }
    let response = request.send().await?;
    let status = response.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && offset > 0 {
        // 续传位置已不可用，从头再来。
        let _ = std::fs::remove_file(&part);
        reporter.received -= offset;
        return Box::pin(fetch_file(client, reporter, dest, file, index)).await;
    }
    if !status.is_success() {
        anyhow::bail!("下载 {path} 失败：HTTP {status}", path = file.path);
    }
    let mut target = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&part)
        .await?;
    if offset > 0 && status == reqwest::StatusCode::OK {
        // 服务端忽略了 Range，回的是完整文件：丢掉旧分片，否则会拼出错误的文件。
        target.set_len(0).await?;
        reporter.received -= offset;
    } else {
        target.seek(std::io::SeekFrom::End(0)).await?;
    }
    let mut body = response;
    while let Some(chunk) = body.chunk().await? {
        if CANCEL.load(Ordering::SeqCst) {
            target.flush().await?;
            reporter.push("cancelled", file.path, index, true);
            anyhow::bail!("下载已取消");
        }
        target.write_all(&chunk).await?;
        reporter.received += chunk.len() as u64;
        reporter.push("downloading", file.path, index, false);
    }
    target.flush().await?;
    drop(target);
    let written = std::fs::metadata(&part).map(|meta| meta.len()).unwrap_or(0);
    if written != file.size {
        anyhow::bail!(
            "{path} 大小不符：期望 {size} 字节，实际 {written} 字节",
            path = file.path,
            size = file.size
        );
    }
    tokio::fs::rename(&part, dest).await?;
    reporter.push("downloading", file.path, index, true);
    Ok(())
}

/// 下载全部模型文件到 `<comfyui_dir>/models/breezetts2/`。
/// 桌面端只运行 Wry 运行时；写成具体类型，才能被 `generate_handler!` 接受。
pub async fn download(app: AppHandle<tauri::Wry>, comfyui_dir: PathBuf) -> anyhow::Result<ModelStatus> {
    if BUSY.swap(true, Ordering::SeqCst) {
        anyhow::bail!("已有模型下载正在进行");
    }
    // 无论成功、失败还是取消，都要放开锁，否则之后再也无法重试。
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::SeqCst);
        }
    }
    let _release = Release;
    CANCEL.store(false, Ordering::SeqCst);

    let dir = model_dir(&comfyui_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()?;
    let mut reporter: Reporter<'_, tauri::Wry> = Reporter {
        app: &app,
        received: 0,
        total: total_bytes(),
        last: Instant::now() - EMIT_INTERVAL,
    };
    for (index, file) in FILES.iter().enumerate() {
        let dest = dir.join(file.path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if std::fs::metadata(&dest)
            .map(|meta| meta.len() == file.size)
            .unwrap_or(false)
        {
            reporter.received += file.size;
            reporter.push("downloading", file.path, index + 1, false);
            continue;
        }
        fetch_file(&client, &mut reporter, &dest, file, index + 1).await?;
    }
    reporter.push("done", "", FILES.len(), true);
    Ok(ModelStatus {
        installed: installed(&dir),
        dir: dir.display().to_string(),
        comfyui_dir: comfyui_dir.display().to_string(),
        source: "config".into(),
        total_bytes: total_bytes(),
        downloading: false,
    })
}
