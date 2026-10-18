//! Opt-in installed-runtime qualification, never uses the normal Codex profile or hosted inference.
use serde_json::json;
use std::time::{Duration, Instant};
use wordcraft_assistant::{
    Event, Request,
    native::{DocumentBridge, Launch, Runtime},
};
use wordcraft_engine::Session;
use wordcraft_mcp::codex_profile::{CodexProfile, PROPOSAL_TOOL, Scope, SharedContext};
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
    drop(runtime);
    // The temporary profile remains separate; no global settings or credential copying.
    let config = std::fs::read_to_string(temp.path().join("home/config.toml")).unwrap();
    assert!(!config.contains("http://") && !config.contains("127.0.0.1"));
}
