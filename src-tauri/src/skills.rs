//! 应用内技能执行协议：规则随版本打包，确认由持久化状态机执行，不依赖模型自觉遵守。
use crate::{
    db::{JobRow, ProjectRow, Tx},
    error::ApiResult,
    models::*,
    ops,
    util::{iso, make_id, now},
};
use serde_json::{json, Value};

/// 发布的技能清单；不从用户输入加载可执行文件。
pub fn manifest() -> Value {
    serde_json::from_str(include_str!("../skills/manifest.json")).expect("打包的技能清单必须合法")
}
/// 读取当前发布版本。
pub fn version() -> String {
    use sha2::{Digest, Sha256};
    let manifest = manifest();
    let mut hash = Sha256::new();
    hash.update(include_str!("../skills/manifest.json"));
    for step in manifest["steps"].as_array().expect("技能步骤") {
        hash.update(policy(step["id"].as_str().unwrap()));
    }
    hash.update(include_str!("../skills/shotcraft/recipes.json"));
    hash.update(include_str!("../skills/vibe.md"));
    format!(
        "{}+{}",
        manifest["version"].as_str().unwrap_or("1.0.0"),
        &hex::encode(hash.finalize())[..12]
    )
}
/// 固定逻辑问题键用于恢复已确认的选择；展示 ID 另加任务与本次提问标识。
pub fn question_id(step: &str) -> String {
    format!("skill:{step}:1")
}
/// 从任务的审计回答中读取明确选择，不用模型推断用户已经同意。
pub fn choice<'a>(input: &'a JobInput, step: &str) -> Option<&'a str> {
    let key = question_id(step);
    input
        .answers
        .iter()
        .rev()
        .find(|answer| {
            answer.question_id == key || answer.question_id.starts_with(&(key.clone() + ":"))
        })
        .map(|answer| answer.value.as_str())
}
/// 安装缺失时的改选同样是明确用户回答，不改变全局供应商配置。
pub fn voice_mode(input: &JobInput) -> &str {
    if let Some(value) = choice(input, "voice_failed") {
        if ["api", "breeze", "bgm"].contains(&value) {
            return value;
        }
    }
    match choice(input, "breeze_install") {
        Some("choose_api") => "api",
        Some("bgm") => "bgm",
        _ => choice(input, "voice").unwrap_or("api"),
    }
}
/// 构造技能问题；没有登记的步骤拒绝运行。
pub fn question(step: &str, prompt: String, ttl: i64) -> Question {
    let manifest = manifest();
    let list = manifest["steps"].as_array().expect("技能步骤清单");
    let index = list
        .iter()
        .position(|s| s["id"] == step)
        .expect("必须登记技能步骤");
    let options = list[index]["options"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| QuestionOption {
                    value: item["value"].as_str().unwrap().into(),
                    label: item["label"].as_str().unwrap().into(),
                })
                .collect()
        })
        .unwrap_or_default();
    Question {
        id: question_id(step),
        prompt,
        input_type: if matches!(
            step,
            "plan" | "script" | "storyboard" | "production" | "post_edit" | "delivery"
        ) {
            "review"
        } else {
            "single_select"
        }
        .into(),
        options,
        expires_at: iso(now() + chrono::Duration::seconds(ttl)),
        skill_id: Some(step.into()),
        step: Some(index + 1),
        total_steps: Some(list.len()),
    }
}
/// 在同一事务内暂停与写入问题卡片，避免界面刷新后丢失确认。
pub fn pause(
    t: &mut Tx,
    job: &mut JobRow,
    project: &mut ProjectRow,
    mut q: Question,
) -> ApiResult<()> {
    // 安装检测或上传列表可能重复提问；每次发布都换 ID，旧选项不能误答新列表。
    q.id = format!("{}:{}:{}", q.id, job.job.id, make_id("question"));
    job.job.status = JobStatus::WaitingInput;
    job.job.finished_at = None;
    project.project.status = "waiting_input".into();
    project.project.active_job_id = Some(job.job.id.clone());
    if let Some(id) = job.assistant_message_id.clone() {
        ops::update_blocks(t, &id, |blocks| {
            blocks.retain(|b| b["type"] != "question");
            blocks.push(json!({"type":"question","job_id":job.job.id,"question_id":q.id}));
        })?;
    }
    t.emit(
        &project.project.id,
        "input.required",
        json!({"job_id":job.job.id,"question_id":q.id}),
    )?;
    job.job.question = Some(q);
    ops::emit_job_updated(t, &job.job, "等待你的选择后继续")
}
/// 每次模型调用都能读取打包的技能文字；执行边界仍由服务端状态机保证。
pub fn policy(step: &str) -> &'static str {
    match step {
        "style" => include_str!("../skills/style.md"),
        "duration" => include_str!("../skills/duration.md"),
        "visual" => include_str!("../skills/visual.md"),
        "voice" => include_str!("../skills/voice.md"),
        "music" => include_str!("../skills/music.md"),
        "plan" => include_str!("../skills/plan.md"),
        "script" => include_str!("../skills/script.md"),
        "storyboard" => include_str!("../skills/storyboard.md"),
        "production" => include_str!("../skills/production.md"),
        "render" => include_str!("../skills/render.md"),
        "post_edit" => include_str!("../skills/post-edit.md"),
        "delivery" => include_str!("../skills/delivery.md"),
        _ => "只能完成当前授权步骤，不得擅自推进、下载模型或调用剪辑工具。",
    }
}
/// 将真实上游镜头配方送入生成上下文，而非仅替换模板名称。
pub fn template_recipe(topic: &str) -> String {
    if !topic.contains("[template:shotcraft-") {
        return include_str!("../skills/vibe.md").into();
    }
    let data: Value = serde_json::from_str(include_str!("../skills/shotcraft/recipes.json"))
        .expect("镜头配方合法");
    let preset = data["presets"].as_array().and_then(|v| {
        v.iter()
            .find(|p| topic.contains(p["id"].as_str().unwrap_or("---")))
    });
    format!(
        "当前模板配方：{}；只使用该模板列出的镜头语言，用户确认参数优先。",
        preset.unwrap_or(&data)
    )
}

/// 清除指定步骤以后的确认；恢复产物和重新生成产物由调用方分别决定。
pub fn rewind(input: &mut JobInput, step: &str) {
    let m = manifest();
    let steps = m["steps"].as_array().unwrap();
    let at = steps.iter().position(|s| s["id"] == step).unwrap_or(0);
    input.answers.retain(|a| {
        !steps[at..].iter().any(|s| {
            let key = question_id(s["id"].as_str().unwrap());
            a.question_id == key || a.question_id.starts_with(&(key + ":"))
        }) && !(at <= 3
            && (a.question_id.contains("breeze_install") || a.question_id.contains("voice_failed")))
            && !(at <= 4
                && (a.question_id.contains("music_asset")
                    || a.question_id.contains("music_failed")))
    });
    input.awaiting_post_edit = false;
    input.post_edit_applied = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 全部技能必须实际有规则文字、唯一步骤和可选择项。
    #[test]
    fn every_step_has_policy_and_explicit_options() {
        let m = manifest();
        let mut ids = std::collections::HashSet::new();
        for step in m["steps"].as_array().unwrap() {
            let id = step["id"].as_str().unwrap();
            assert!(ids.insert(id));
            assert!(policy(id).len() > 30);
            if id != "delivery" {
                assert!(!question(id, "确认".into(), 60).options.is_empty());
            }
        }
    }
}
