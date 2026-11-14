//! 开始制作前的依赖检查，输出可操作的状态，不回显密钥或个人路径。
use crate::config::Config;
use serde_json::{json, Value};

/// 检查必需工具与可选本地服务；所有进程和网络检查均有明确期限。
pub async fn inspect(config: &Config) -> Value {
    let mut items = vec![];
    for (name, exe) in [
        (
            "FFmpeg",
            std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
        ),
        ("ffprobe", config.ffprobe.clone()),
        (
            "Node.js",
            std::env::var("NODE_PATH_BIN").unwrap_or_else(|_| "node".into()),
        ),
    ] {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(4),
            tokio::process::Command::new(exe)
                .arg(if name == "Node.js" {
                    "--version"
                } else {
                    "-version"
                })
                .kill_on_drop(true)
                .output(),
        )
        .await;
        let ok = matches!(result,Ok(Ok(ref o))if o.status.success() && (name!="Node.js" || String::from_utf8_lossy(&o.stdout).trim().trim_start_matches('v').split('.').next().and_then(|s|s.parse::<u32>().ok()).is_some_and(|v|v>=24)));
        items.push(json!({"name":name,"ready":ok,"required":name!="Node.js","message":if ok{"可用"}else if name=="Node.js"{"HTML模式需要Node.js 24或更新版本"}else{"未找到，请安装或检查工具路径"}}));
    }
    let chrome = std::env::var("CHROME_PATH")
        .ok()
        .filter(|p| std::path::Path::new(p).is_file())
        .is_some()
        || [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
        ]
        .iter()
        .any(|p| std::path::Path::new(p).is_file())
        || std::env::var("PROGRAMFILES").ok().is_some_and(|p| {
            std::path::Path::new(&p)
                .join("Google/Chrome/Application/chrome.exe")
                .is_file()
        });
    items.push(json!({"name":"Chrome","required":false,"ready":chrome,"message":if chrome{"可用于HTML镜头"}else{"HTML模式需要系统Chrome；SVG模式不需要"}}));
    let model = crate::breeze_model::status(config);
    let references = [&config.breeze_voice_male, &config.breeze_voice_female]
        .iter()
        .any(|p| {
            let p = std::path::Path::new(p);
            p.is_file() && (p.with_extension("txt").is_file() || p.with_extension("json").is_file())
        });
    let service = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok();
    let (service, nodes) = if let Some(c) = service {
        let response = c
            .get(format!(
                "{}/object_info",
                config.comfyui_url.trim_end_matches('/')
            ))
            .send()
            .await;
        if let Ok(response) = response {
            let available = response.status().is_success();
            let info = response.json::<Value>().await.unwrap_or(Value::Null);
            (
                available,
                [
                    "BreezeTTS2LoadModel",
                    "BreezeTTS2VoiceClone",
                    "LoadAudio",
                    "SaveAudio",
                ]
                .iter()
                .all(|name| info.get(*name).is_some()),
            )
        } else {
            (false, false)
        }
    } else {
        (false, false)
    };
    items.push(json!({"name":"breezeTTS2","required":false,"ready":model.installed&&references&&service&&nodes,"model_installed":model.installed,"reference_ready":references,"service_ready":service,"nodes_ready":nodes,"message":if model.installed&&references&&service&&nodes{"模型、节点与参考音已就绪"}else{"本地配音需要模型、ComfyUI节点与参考音/转写同时就绪；缺失时可返回改选API或纯BGM"}}));
    json!({"items":items,"script_configured":!config.llm1.api_key.is_empty(),"music_configured":config.provider_llm3!="procedural"&&!config.llm3.api_key.is_empty(),"voice_configured":!config.qwen_tts.api_key.is_empty()||!config.siliconflow_tts.api_key.is_empty(),"music_provider":config.provider_llm3,"voice_provider":config.provider_llm4})
}
