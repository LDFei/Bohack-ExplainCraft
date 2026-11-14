//! Oak/Quercus 的 50 个引擎工具，来源见 `UPSTREAM.md` 与 `LICENSE`。
//!
//! 工具在进程内由 LLM agent 调用，不启动 MCP 网络服务。

// 上游工具集同时保留 MCP 资源适配；本应用只使用其中一部分接口。
#![allow(dead_code)]

pub mod convert;
pub mod image;
pub mod sidecar;
pub mod state;
pub mod tools;

use serde_json::{json, Value};

/// 转换为 OpenAI `tools` 参数，保留参考项目的 JSON Schema 与只读标记。
pub fn openai_tool_schemas() -> Vec<Value> {
    tools::all()
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": (tool.input_schema)(),
                    "strict": false
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_all_fifty_quercus_tools_to_the_llm() {
        let schemas = super::openai_tool_schemas();
        assert_eq!(schemas.len(), 50);
        assert!(schemas
            .iter()
            .any(|tool| tool["function"]["name"] == "clip_place"));
        assert!(schemas
            .iter()
            .any(|tool| tool["function"]["name"] == "export_start"));
    }
}
