//! Opt-in installed-runtime qualification, never uses the normal Codex profile or hosted inference.
use serde_json::json;
use std::time::{Duration, Instant};
use wordcraft_assistant::{
    Event, Request,
    native::{DocumentBridge, Launch, Runtime},
};
use wordcraft_engine::Session;
use wordcraft_mcp::codex_profile::{CodexProfile, INSERTION_TOOL, PARAGRAPHS_TOOL, PROPOSAL_TOOL, Scope, SharedContext};
fn next(runtime: &mut Runtime) -> Event {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(event) = runtime.poll() {
            return event;
        }
        assert!(Instant::now() < deadline, "installed runtime timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "requires installed Codex 0.162.1"]
fn installed_runtime_document_relay_uses_only_private_stdio() {
    let temp = tempfile::tempdir().unwrap();
    let binary = std::env::var_os("WORDCRAFT_CODEX_BINARY").map(std::path::PathBuf::from).unwrap_or_else(|| "/opt/homebrew/bin/codex".into());
    let mut runtime = Runtime::start(Launch {
        binary,
        home: temp.path().join("home"),
        cwd: temp.path().join("cwd"),
        document_bridge: Some(DocumentBridge { executable: env!("CARGO_BIN_EXE_wordcraft").into(), args: vec!["--codex-document-bridge".into()] }),
        timeout: Duration::from_secs(30),
    })
    .unwrap();
    loop {
        match next(&mut runtime) {
            Event::Ready => break,
            Event::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    runtime.send(Request::StartThread).unwrap();
    let thread = loop {
        match next(&mut runtime) {
            Event::Response { request: Request::StartThread, result, .. } => break result["thread"]["id"].as_str().unwrap().to_string(),
            Event::Error(e) => panic!("{e}"),
            _ => {}
        }
    };
    let mut session = Session::new(wordcraft_doc::Document::from_text("Shared text\nPrivate text"));
    session.run("select.text", &json!({"text":"Shared text"})).unwrap();
    let mut profile = CodexProfile::default();
    let context = SharedContext::capture(&session, Scope::Selection, true).unwrap();
    let args = context.metadata();
    profile.share(context);
    for (tool, args, expected) in [
        ("read_context", args.clone(), "Shared text"),
        (
            PROPOSAL_TOOL,
            json!({"documentId":args["documentId"],"revision":args["revision"],"original":"Shared text","replacement":"Proposed text"}),
            "awaiting_user_approval",
        ),
        ("file.save", json!({}), "allowlist"),
        ("read_context", json!({"documentId":0,"revision":0}), "does not match"),
    ] {
        runtime.send(Request::ProbeDocument { thread_id: thread.clone(), tool: tool.into(), arguments: args }).unwrap();
        let result = loop {
            match next(&mut runtime) {
                Event::DocumentRequest { id, thread: requested, tool, arguments, .. } => {
                    assert_eq!(requested, thread);
                    let result = if tool == PROPOSAL_TOOL {
                        profile.propose(&mut session, &thread, "probe-request", &arguments)
                    } else {
                        profile.call(&session, &tool, &arguments)
                    };
                    runtime.document_reply(id, result.to_value()).unwrap();
                }
                Event::Response { request: Request::ProbeDocument { .. }, result, .. } => break result,
                Event::Error(e) => panic!("{e}"),
                _ => {}
            }
        };
        let text = result.to_string();
        assert!(text.contains(expected), "expected {expected}, result shape keys {:?}", result.as_object().map(|o| o.keys().collect::<Vec<_>>()));
        assert!(!text.contains("Private text"));
    }
    assert_eq!(session.doc.final_text(wordcraft_doc::StoryRef::Body), "Shared text\nPrivate text");
    assert_eq!(session.codex_requests()[0]["state"], "pending");
    for tool in [INSERTION_TOOL, PARAGRAPHS_TOOL] {
        let mut session = Session::new(wordcraft_doc::Document::from_text(""));
        let context = SharedContext::capture(&session, Scope::Document, true).unwrap();
        let metadata = context.metadata();
        let mut profile = CodexProfile::default();
        profile.share(context);
        let arguments = json!({"documentId":metadata["documentId"],"revision":metadata["revision"],
            "startParagraph":0,"endParagraph":0,"original":"","replacement":"First paragraph.\nSecond paragraph."});
        runtime.send(Request::ProbeDocument { thread_id: thread.clone(), tool: tool.into(), arguments }).unwrap();
        loop {
            match next(&mut runtime) {
                Event::DocumentRequest { id, thread: requested, tool: requested_tool, arguments, .. } => {
                    assert_eq!(requested, thread);
                    assert_eq!(requested_tool, tool);
                    let result = profile.propose_tool(&mut session, &thread, "draft-probe", &requested_tool, &arguments);
                    runtime.document_reply(id, result.to_value()).unwrap();
                }
                Event::Response { request: Request::ProbeDocument { .. }, result, .. } => {
                    assert!(result.to_string().contains("awaiting_user_approval"));
                    break;
                }
                Event::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        assert_eq!(session.doc.final_text(wordcraft_doc::StoryRef::Body), "");
        session.approve_codex("draft-probe").unwrap();
        assert_eq!(session.doc.final_text(wordcraft_doc::StoryRef::Body), "First paragraph.\nSecond paragraph.");
        assert_eq!(session.doc.body.len(), 2);
    }
    drop(runtime);
    // The temporary profile remains separate; no global settings or credential copying.
    let config = std::fs::read_to_string(temp.path().join("home/config.toml")).unwrap();
    assert!(!config.contains("http://") && !config.contains("127.0.0.1"));
}

/// Exercise discovery through the actual model request, not the direct MCP probe.
#[test]
#[ignore = "requires installed Codex 0.162.1 and local loopback"]
fn installed_runtime_advertises_only_qualified_document_tools() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    let fixture = std::thread::spawn(move || {
        for sequence in 0..3 {
            let deadline = Instant::now() + Duration::from_secs(40);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(pair) => break pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            assert!((1..=8 * 1024 * 1024).contains(&length));
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            if sequence > 0 {
                // Inspect only synthetic tool output, never headers or prompts.
                let output = request["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| {
                        item["type"] == "custom_tool_call_output" && item["call_id"] == if sequence == 1 { "catalogue" } else { "forbidden" }
                    })
                    .unwrap();
                tx.send(output.clone()).unwrap();
            }
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
            let item = if sequence == 0 {
                json!({"type":"custom_tool_call","id":"catalogue","call_id":"catalogue","name":"exec", "input":
                    "text(JSON.stringify({names:ALL_TOOLS.map(t=>t.name),process:typeof process,require:typeof require,fetch:typeof fetch,Deno:typeof Deno,patch:typeof tools.apply_patch,shell:typeof tools.exec_command}));"})
            } else if sequence == 1 {
                // Even an injected direct call to an unadvertised writer must fail closed.
                json!({"type":"custom_tool_call","id":"forbidden","call_id":"forbidden","name":"apply_patch", "input":
                    "*** Begin Patch\n*** Add File: forbidden.txt\n+must not be written\n*** End Patch"})
            } else {
                json!({"type":"message","id":"done","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done.","annotations":[]}]})
            };
            for (kind, mut value) in [
                ("response.created", json!({"response":{"id":"fixture","status":"in_progress"}})),
                ("response.output_item.done", json!({"output_index":0,"item":item})),
                (
                    "response.completed",
                    json!({"response":{"id":"fixture","status":"completed","output":[item],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
                ),
            ] {
                value["type"] = json!(kind);
                write!(stream, "event: {kind}\ndata: {value}\n\n").unwrap();
                stream.flush().unwrap();
            }
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let binary = std::env::var_os("WORDCRAFT_CODEX_BINARY").map(std::path::PathBuf::from).unwrap_or_else(|| "/opt/homebrew/bin/codex".into());
    let mut runtime = Runtime::start(Launch {
        binary,
        home: temp.path().join("home"),
        cwd: temp.path().join("cwd"),
        document_bridge: Some(DocumentBridge { executable: env!("CARGO_BIN_EXE_wordcraft").into(), args: vec!["--codex-document-bridge".into()] }),
        timeout: Duration::from_secs(30),
    })
    .unwrap();
    loop {
        match next(&mut runtime) {
            Event::Ready => break,
            Event::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    runtime.send(Request::ProbeModelThread { base_url: format!("http://127.0.0.1:{port}/v1") }).unwrap();
    let thread = loop {
        match next(&mut runtime) {
            Event::Response { request: Request::ProbeModelThread { .. }, result, .. } => break result["thread"]["id"].as_str().unwrap().to_owned(),
            Event::Error(e) => panic!("{e}"),
            _ => {}
        }
    };
    runtime.send(Request::StartTurn { thread_id: thread, text: "Read the synthetic shared selection.".into() }).unwrap();
    let output = rx.recv_timeout(Duration::from_secs(40)).unwrap();
    let forbidden = rx.recv_timeout(Duration::from_secs(40)).unwrap();
    assert!(!temp.path().join("cwd/forbidden.txt").exists());
    assert!(
        forbidden.to_string().contains("Unknown tool") || forbidden.to_string().contains("rejected") || forbidden.to_string().contains("not allowed"),
        "unadvertised write must be refused"
    );
    drop(runtime);
    fixture.join().unwrap();
    let catalogue = output["output"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|part| part["text"].as_str())
        .find_map(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .expect("code-mode host must execute the catalogue probe");
    let mut names = catalogue["names"].as_array().unwrap().iter().map(|name| name.as_str().unwrap()).collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "mcp__wordcraft__context_status",
            "mcp__wordcraft__find_text",
            "mcp__wordcraft__inspect_formatting",
            "mcp__wordcraft__propose_insertion",
            "mcp__wordcraft__propose_paragraphs",
            "mcp__wordcraft__propose_selection",
            "mcp__wordcraft__read_context",
            "mcp__wordcraft__read_selection"
        ]
    );
    for capability in ["process", "require", "fetch", "Deno", "patch", "shell"] {
        assert_eq!(catalogue[capability], "undefined", "unexpected capability: {capability}");
    }
}
