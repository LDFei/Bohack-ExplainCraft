//! 制作前检索可追溯的参考资料；不可用时明确说明，绝不伪造来源或已核实状态。
use serde_json::{json, Value};

/// 查询维基百科的公开检索 API，取短摘要与永久可访问的页面地址作为审阅材料。
pub async fn collect(subject: &str) -> Vec<Value> {
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .user_agent("ExplainCraft/0.1 (local knowledge-video authoring)")
        .timeout(std::time::Duration::from_secs(8))
        .build()
    else {
        return vec![];
    };
    let request = client.get("https://zh.wikipedia.org/w/api.php").query(&[
        ("action", "query"),
        ("format", "json"),
        ("generator", "search"),
        ("gsrsearch", subject),
        ("gsrlimit", "3"),
        ("prop", "extracts|info"),
        ("explaintext", "1"),
        ("exintro", "1"),
        ("inprop", "url"),
        ("redirects", "1"),
    ]);
    let Ok(response) = request.send().await else {
        return vec![];
    };
    let Ok(data) = response.json::<Value>().await else {
        return vec![];
    };
    data["query"]["pages"].as_object().into_iter().flat_map(|p|p.values()).filter_map(|p|{
        let url=p["fullurl"].as_str()?;let title=p["title"].as_str()?;let excerpt=p["extract"].as_str()?;
        if !url.starts_with("https://zh.wikipedia.org/")||excerpt.trim().is_empty(){return None;}
        Some(json!({"title":title,"url":url,"excerpt":excerpt.chars().take(1800).collect::<String>(),"accessed_at":crate::util::now_iso(),"status":"retrieved_requires_review"}))
    }).collect()
}
