//! Synthetic app-server used only by integration tests; never enabled in shipping builds.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
use serde_json::{Value, json};
use std::io::{BufRead, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let mode = std::fs::read_to_string(exe.parent().ok_or("parent")?.join("mode")).unwrap_or_default();
    if std::env::args().any(|a| a == "--version") {
        println!("codex-cli {}", if mode == "unsupported" { "0.0.0" } else { "0.162.1" });
        return Ok(());
    }
    std::fs::write(exe.parent().ok_or("parent")?.join("pid"), std::process::id().to_string())?;
    if mode == "exit" {
        return Ok(());
    }
    if mode == "hang" {
        std::thread::sleep(std::time::Duration::from_secs(60));
        return Ok(());
    }
    let mut output = std::io::stdout().lock();
    if mode == "invalid" {
        writeln!(output, "not-json")?;
        return Ok(());
    }
    if mode == "truncated" {
        write!(output, "{{\"id\":0")?;
        return Ok(());
    }
    if mode == "oversize" {
        output.write_all(&vec![b'x'; wordcraft_assistant::MAX_MESSAGE + 1])?;
        return Ok(());
    }
    let send = |output: &mut std::io::StdoutLock<'_>, value: Value| -> Result<(), Box<dyn std::error::Error>> {
        let bytes = serde_json::to_vec(&value)?;
        for chunk in bytes.chunks(3) {
            output.write_all(chunk)?;
            output.flush()?;
        }
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    };
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        if method == "initialized" {
            if mode == "document" || mode == "foreign-document" {
                send(
                    &mut output,
                    json!({"id":"document-1","method":"mcpServer/elicitation/request","params":{
                        "serverName":if mode == "document" {"wordcraft"} else {"foreign"},"threadId":"fixture-thread","mode":"form",
                        "_meta":{"wordcraftCall":{"name":"read_context","arguments":{"documentId":1,"revision":1}}}
                    }}),
                )?;
            }
            if mode == "blocked-write" {
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
            continue;
        }
        if id == json!("document-1") {
            let valid = if mode == "document" {
                request.pointer("/result/content/result").and_then(Value::as_str) == Some("{\"ok\":true}")
            } else {
                request.pointer("/result/action").and_then(Value::as_str) == Some("decline")
            };
            send(&mut output, json!({"method":"item/agentMessage/delta","params":{"delta":if valid {"reply accepted"} else {"bad reply"}}}))?;
            continue;
        }
        if method == "initialize" {
            send(&mut output, json!({"id":id,"result":{"userAgent":"fixture"}}))?;
            continue;
        }
        if mode == "duplicate" {
            send(&mut output, json!({"id":id,"result":{}}))?;
            send(&mut output, json!({"id":id,"result":{}}))?;
            continue;
        }
        if mode == "timeout" {
            continue;
        }
        match method {
            "account/read" => send(&mut output, json!({"id":id,"result":{"account":null,"requiresOpenaiAuth":true}}))?,
            "thread/start" | "thread/resume" => send(&mut output, json!({"id":id,"result":{"thread":{"id":"fixture-thread"}}}))?,
            "turn/start" => {
                send(&mut output, json!({"id":id,"result":{"turn":{"id":"fixture-turn"}}}))?;
                for delta in ["hello ", "world"] {
                    send(
                        &mut output,
                        json!({"method":"item/agentMessage/delta","params":{"threadId":"fixture-thread","turnId":"fixture-turn","itemId":"fixture-item","delta":delta}}),
                    )?;
                }
            }
            "turn/interrupt" => {
                send(&mut output, json!({"id":id,"result":{}}))?;
                send(
                    &mut output,
                    json!({"method":"turn/completed","params":{"threadId":"fixture-thread","turn":{"id":"fixture-turn","status":"interrupted"}}}),
                )?;
            }
            _ => send(&mut output, json!({"id":id,"error":{"code":-32601,"message":"SENSITIVE_SYNTHETIC_ERROR"}}))?,
        }
    }
    Ok(())
}
