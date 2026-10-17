//! Dedicated read-only MCP relay. Data is supplied by the owning UI Session via stable
//! app-server elicitation, so no socket, context file, control key, or broad Backend is needed.
use serde_json::{Value, json};
use std::io::{BufReader, Write};
use wordcraft_assistant::native::read_message;
use wordcraft_mcp::codex_profile::{READ_TOOLS, tool_definitions};
use wordcraft_mcp::tools::ToolResult;
fn send(output: &mut impl Write, value: Value) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(&mut *output, &value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = std::io::stdout().lock();
    let mut initialized = false;
    let mut next = 1u64;
    let mut queued = std::collections::VecDeque::new();
    while let Some(message) = match queued.pop_front() {
        Some(message) => Some(message),
        None => read_message(&mut input)?,
    } {
        let Some(id) = message.get("id") else {
            if message.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
                initialized = true;
            }
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => {
                json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"wordcraft-document","version":env!("CARGO_PKG_VERSION")}})
            }
            "ping" => json!({}),
            "tools/list" if initialized => json!({"tools":tool_definitions()}),
            "resources/list" if initialized => json!({"resources":[]}),
            "resources/templates/list" if initialized => json!({"resourceTemplates":[]}),
            "tools/call" if initialized => {
                let params = message.get("params").unwrap_or(&Value::Null);
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                if !READ_TOOLS.contains(&name) {
                    ToolResult::error("Tool is not on the WordCraft Codex allowlist.").to_value()
                } else {
                    let elicitation = format!("wordcraft-{next}");
                    next = next.checked_add(1).ok_or("request limit")?;
                    send(
                        &mut output,
                        json!({"jsonrpc":"2.0","id":elicitation,"method":"elicitation/create","params":{
                            "mode":"form","message":"WordCraft shared document context",
                            "_meta":{"wordcraftCall":{"name":name,"arguments":params.get("arguments").cloned().unwrap_or(Value::Null)}},
                            "requestedSchema":{"type":"object","properties":{"result":{"type":"string"}},"required":["result"]}
                        }}),
                    )?;
                    let mut result = ToolResult::error("WordCraft did not authorize document access.").to_value();
                    while let Some(reply) = read_message(&mut input)? {
                        if reply.get("id") == Some(&json!(elicitation)) {
                            if reply.pointer("/result/action").and_then(Value::as_str) == Some("accept")
                                && let Some(text) = reply.pointer("/result/content/result").and_then(Value::as_str)
                                && let Ok(value) = serde_json::from_str::<Value>(text)
                            {
                                result = value;
                            }
                            break;
                        }
                        if reply.get("method").and_then(Value::as_str) == Some("notifications/cancelled")
                            && reply.pointer("/params/requestId") == Some(id)
                        {
                            break;
                        }
                        if reply.get("method").and_then(Value::as_str) == Some("ping") {
                            send(&mut output, json!({"jsonrpc":"2.0","id":reply.get("id"),"result":{}}))?;
                        } else if reply.get("method").is_some() && reply.get("id").is_some() {
                            if queued.len() >= 32 {
                                return Err("too many pending calls".into());
                            }
                            queued.push_back(reply);
                        }
                    }
                    result
                }
            }
            _ => {
                send(&mut output, json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method is not allowlisted."}}))?;
                continue;
            }
        };
        send(&mut output, json!({"jsonrpc":"2.0","id":id,"result":result}))?;
    }
    Ok(())
}
