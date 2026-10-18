//! Native owner of the runtime. UI actions are typed; protocol values stay in this host.
use std::path::PathBuf;
use std::time::Duration;
use wordcraft_assistant::native::{DocumentBridge, Launch, Runtime};
use wordcraft_assistant::{Event, Notice, Request};
use wordcraft_mcp::codex_profile::{CodexProfile, PROPOSAL_TOOL, Scope, SharedContext};
use wordcraft_mcp::tools::ToolResult;
use wordcraft_ui_egui::{WordApp, assistant::Action};

pub struct Host {
    runtime: Option<Runtime>,
    turn: Option<String>,
    starting_turn: bool,
    stop_requested: bool,
    completed: Vec<String>,
    pub profile: CodexProfile,
    pub thread: Option<String>,
    root: Option<PathBuf>,
    bridge: Option<PathBuf>,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            runtime: None,
            turn: None,
            starting_turn: false,
            stop_requested: false,
            completed: Vec::new(),
            profile: CodexProfile::default(),
            thread: None,
            root: wordcraft_control_key::settings_dir().map(|p| p.join("codex")),
            bridge: std::env::current_exe().ok(),
        }
    }
}
impl Host {
    pub fn shutdown(&mut self) {
        self.profile.revoke();
        self.thread = None;
        self.turn = None;
        self.starting_turn = false;
        self.stop_requested = false;
        self.runtime = None;
    }
    pub fn share(&mut self, app: &WordApp, scope: Scope) -> Result<(), String> {
        if !app.has_document() {
            return Err("Open a document before sharing context.".into());
        }
        if self.thread.is_none() {
            return Err("Start a conversation before sharing context.".into());
        }
        self.profile.share(SharedContext::capture(&app.session, scope, true)?);
        Ok(())
    }
    pub fn send(&self, request: Request) -> Result<(), String> {
        self.runtime.as_ref().ok_or("Start Codex first.")?.send(request).map_err(|e| e.to_string())
    }
    fn submit(&mut self, app: &mut WordApp, text: String) -> Result<(), String> {
        let thread_id = self.thread.clone().ok_or("Start a conversation first.")?;
        self.send(Request::StartTurn { thread_id, text: text.clone() })?;
        self.turn = None;
        self.starting_turn = true;
        self.stop_requested = false;
        app.assistant.turn = None;
        app.assistant.last_prompt = text.clone();
        app.assistant.pending = true;
        app.assistant.error = None;
        app.assistant.activity = "Waiting for Codex…".into();
        app.assistant.push_message("You", text);
        app.assistant.composer.clear();
        app.assistant.focus_composer = true;
        Ok(())
    }
    fn stop(&mut self, app: &mut WordApp) -> Result<(), String> {
        self.stop_requested = true;
        self.profile.revoke();
        app.assistant.activity = "Stopping response…".into();
        if let (Some(thread_id), Some(turn_id)) = (&self.thread, &self.turn) {
            let _ = app.session.deny_codex(&format!("{thread_id}:{turn_id}"));
            self.send(Request::Interrupt { thread_id: thread_id.clone(), turn_id: turn_id.clone() })?;
        } else if !self.starting_turn {
            self.shutdown();
            app.assistant.enabled = false;
            app.assistant.thread = None;
            app.assistant.pending = false;
            app.assistant.signed_in = None;
        }
        Ok(())
    }
    fn begin_turn(&mut self, app: &mut WordApp, id: Option<&str>) {
        let Some(id) = id.filter(|id| !id.is_empty() && id.len() <= 128) else { return };
        if !self.starting_turn || self.completed.iter().any(|old| old == id) {
            return;
        }
        self.starting_turn = false;
        self.turn = Some(id.to_owned());
        app.assistant.turn = Some(id.to_owned());
        app.assistant.activity = "Codex is responding…".into();
        if self.stop_requested {
            let _ = self.stop(app);
        }
    }
    fn start(&mut self, app: &mut WordApp) -> Result<(), String> {
        self.shutdown();
        app.assistant.enabled = false;
        app.assistant.thread = None;
        app.assistant.pending = false;
        app.assistant.signed_in = None;
        app.assistant.error = None;
        let root = self.root.clone().ok_or("WordCraft's settings folder is unavailable.")?;
        let executable = self.bridge.clone().ok_or("WordCraft's executable could not be found.")?;
        let binary = detect(&app.assistant.binary).ok_or("Codex was not found. Install Codex 0.162.1 or select its executable.")?;
        self.runtime = Some(
            Runtime::start(Launch {
                binary,
                home: root.join("home"),
                cwd: root.join("workspace"),
                document_bridge: Some(DocumentBridge { executable, args: vec!["--codex-document-bridge".into()] }),
                timeout: Duration::from_secs(30),
            })
            .map_err(|e| e.to_string())?,
        );
        app.assistant.enabled = true;
        app.assistant.signed_in = None;
        app.assistant.error = None;
        app.assistant.status = "Starting Codex…".into();
        Ok(())
    }
    pub fn tick(&mut self, app: &mut WordApp, ctx: &egui::Context) {
        while let Some(action) = app.assistant.actions.pop_front() {
            let result = match action {
                Action::Connect => self.start(app),
                Action::Disconnect => {
                    self.shutdown();
                    app.assistant.enabled = false;
                    app.assistant.thread = None;
                    app.assistant.pending = false;
                    app.assistant.turn = None;
                    app.assistant.activity.clear();
                    app.assistant.signed_in = None;
                    app.assistant.status = "Codex is disabled. Document sharing is off.".into();
                    Ok(())
                }
                Action::NewThread => {
                    self.profile.revoke();
                    self.thread = None;
                    app.assistant.thread = None;
                    self.send(Request::StartThread)
                }
                Action::Resume(thread_id) => {
                    self.profile.revoke();
                    self.thread = None;
                    app.assistant.thread = None;
                    self.send(Request::ResumeThread { thread_id })
                }
                Action::Send(text) => self.submit(app, text),
                Action::Retry => self.submit(app, app.assistant.last_prompt.clone()),
                Action::Stop => self.stop(app),
                Action::Account => self.send(Request::Account),
                Action::ShareSelection => self.share(app, Scope::Selection),
                Action::ShareDocument => self.share(app, Scope::Document),
                Action::Revoke => {
                    self.profile.revoke();
                    Ok(())
                }
            };
            if let Err(error) = result {
                app.assistant.status = error.clone();
                app.assistant.error = Some(error);
                app.assistant.pending = self.turn.is_some() || self.starting_turn;
            }
        }
        for _ in 0..128 {
            let Some(event) = self.runtime.as_mut().and_then(Runtime::poll) else { break };
            self.event(app, event);
            if self.runtime.is_none() {
                break;
            }
        }
        app.assistant.sharing = match self.profile.context() {
            Some(context) if context.is_current(&app.session) => {
                format!("Sharing {} · revision {} · {} bytes", context.scope.name(), context.revision, context.metadata()["bytes"])
            }
            Some(_) => "Shared context is stale. Share again to include your latest edits.".into(),
            None => "Document sharing is off.".into(),
        };
        if self.runtime.is_some() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
    fn event(&mut self, app: &mut WordApp, event: Event) {
        match event {
            Event::Ready => {
                app.assistant.status = "Codex connected. Checking sign-in…".into();
                if let Err(error) = self.send(Request::Account) {
                    app.assistant.status = error;
                }
            }
            Event::Response { request: Request::Account, result, .. } => {
                let signed_in = result.get("account").is_some_and(|a| !a.is_null());
                app.assistant.signed_in = Some(signed_in);
                app.assistant.status =
                    if signed_in { "Codex connected and signed in." } else { "Codex connected. Sign in to the WordCraft profile to continue." }
                        .into();
            }
            Event::Response { request: Request::StartThread | Request::ResumeThread { .. }, result, .. } => {
                self.profile.revoke();
                self.thread = result.pointer("/thread/id").and_then(|v| v.as_str()).filter(|s| !s.is_empty() && s.len() <= 128).map(str::to_owned);
                if let Some(thread) = &self.thread {
                    app.assistant.remember_thread(thread.clone());
                    if let Some(turns) = result.pointer("/thread/turns").and_then(|v| v.as_array()) {
                        for item in turns.iter().filter_map(|turn| turn.get("items").and_then(|v| v.as_array())).flatten() {
                            match item.get("type").and_then(|v| v.as_str()) {
                                Some("agentMessage") => {
                                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                        app.assistant.push_message("Codex", text.to_owned());
                                    }
                                }
                                Some("userMessage") => {
                                    if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                                        let text = content
                                            .iter()
                                            .filter(|c| c.get("type").and_then(|v| v.as_str()) == Some("text"))
                                            .filter_map(|c| c.get("text").and_then(|v| v.as_str()))
                                            .collect::<Vec<_>>()
                                            .join("\n");
                                        if !text.is_empty() {
                                            app.assistant.push_message("You", text);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    app.assistant.status = "Conversation ready.".into();
                } else {
                    app.assistant.pending = false;
                    app.assistant.error = Some("Codex returned an invalid conversation.".into());
                }
            }
            Event::Response { request: Request::StartTurn { thread_id, .. }, result, .. } if Some(thread_id.as_str()) == self.thread.as_deref() => {
                self.begin_turn(app, result.pointer("/turn/id").and_then(|v| v.as_str()));
            }
            Event::DocumentRequest { id, thread, turn, tool, arguments } => {
                let result = if self.thread.as_deref() == Some(thread.as_str()) && !self.stop_requested && (turn.is_none() || turn == self.turn) {
                    if tool == PROPOSAL_TOOL {
                        if !self.stop_requested && turn.is_some() && turn == self.turn {
                            let request = format!("{thread}:{}", turn.as_deref().unwrap_or(""));
                            let result = self.profile.propose(&mut app.session, &thread, &request, &arguments);
                            if !result.is_error {
                                app.assistant.activity = "A proposed edit is waiting for your approval.".into();
                            }
                            result
                        } else {
                            ToolResult::error("The originating turn is no longer active.")
                        }
                    } else {
                        self.profile.call(&app.session, &tool, &arguments)
                    }
                } else {
                    ToolResult::error("The conversation changed; share context in the active conversation.")
                };
                if result.is_error {
                    app.assistant.activity = "Document action refused. Refresh sharing or choose a supported selection.".into();
                }
                if let Some(runtime) = &self.runtime
                    && let Err(error) = runtime.document_reply(id, result.to_value())
                {
                    app.assistant.status = error.to_string();
                }
            }
            Event::Notification { kind, params }
                if params.get("threadId").and_then(|v| v.as_str()) == self.thread.as_deref() && self.thread.is_some() =>
            {
                if kind == Notice::TurnStarted {
                    self.begin_turn(app, params.pointer("/turn/id").and_then(|v| v.as_str()));
                }
                let turn = params.get("turnId").or_else(|| params.pointer("/turn/id")).and_then(|v| v.as_str());
                if self.turn.is_none() || turn != self.turn.as_deref() {
                    return;
                }
                match kind {
                    Notice::Delta if !self.stop_requested => {
                        if let (Some(id), Some(text)) = (params.get("itemId").and_then(|v| v.as_str()), params.get("delta").and_then(|v| v.as_str()))
                        {
                            app.assistant.stream(id, text, false);
                        }
                    }
                    Notice::ItemStarted | Notice::ItemCompleted => {
                        if params.pointer("/item/type").and_then(|v| v.as_str()) == Some("agentMessage")
                            && kind == Notice::ItemCompleted
                            && !self.stop_requested
                        {
                            if let (Some(id), Some(text)) =
                                (params.pointer("/item/id").and_then(|v| v.as_str()), params.pointer("/item/text").and_then(|v| v.as_str()))
                            {
                                app.assistant.stream(id, text, true);
                            }
                        } else if params.pointer("/item/type").and_then(|v| v.as_str()) == Some("mcpToolCall") {
                            app.assistant.activity =
                                if kind == Notice::ItemStarted { "Codex is checking shared document context…" } else { "Document tool finished." }
                                    .into();
                        }
                    }
                    Notice::TurnCompleted => {
                        if let Some(id) = self.turn.take() {
                            self.completed.push(id);
                        }
                        if self.completed.len() > 100 {
                            self.completed.remove(0);
                        }
                        app.assistant.turn = None;
                        app.assistant.pending = false;
                        let status = params.pointer("/turn/status").and_then(|v| v.as_str());
                        app.assistant.activity = match status {
                            Some("completed") => "Response complete.",
                            Some("interrupted") => "Response stopped. Share context again before continuing.",
                            _ => "The response failed. Check connection and sign-in, then retry.",
                        }
                        .into();
                        if status == Some("failed") {
                            app.assistant.error = Some(app.assistant.activity.clone());
                        }
                    }
                    Notice::Error => {
                        app.assistant.error = Some("Codex encountered a problem. Stop or retry after the response ends.".into());
                    }
                    _ => {}
                }
            }
            Event::Error(error) => {
                if self.turn.is_none() {
                    app.assistant.pending = false;
                    self.starting_turn = false;
                }
                app.assistant.status = error.to_string();
                app.assistant.error = Some(error.to_string());
            }
            Event::Stopped => {
                self.shutdown();
                app.assistant.enabled = false;
                app.assistant.thread = None;
                app.assistant.pending = false;
                app.assistant.turn = None;
                app.assistant.activity.clear();
                app.assistant.signed_in = None;
                if app.assistant.error.is_none() {
                    app.assistant.status = "Codex disconnected. Restart to reconnect.".into();
                }
            }
            _ => {}
        }
    }
}
fn detect(selected: &str) -> Option<PathBuf> {
    if !selected.trim().is_empty() {
        let path = PathBuf::from(selected.trim());
        return path.is_absolute().then_some(path).filter(|p| p.is_file());
    }
    let name = if cfg!(target_os = "windows") { "codex.exe" } else { "codex" };
    let mut candidates = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d| d.join(name)).collect::<Vec<_>>()).unwrap_or_default();
    #[cfg(target_os = "macos")]
    candidates.extend([PathBuf::from("/opt/homebrew/bin/codex"), PathBuf::from("/usr/local/bin/codex")]);
    candidates.into_iter().find(|p| p.is_absolute() && p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Instant;
    fn until(host: &mut Host, app: &mut WordApp, predicate: impl Fn(&WordApp) -> bool) {
        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !predicate(app) {
            host.tick(app, &ctx);
            assert!(Instant::now() < deadline, "host did not reach expected state: {}", app.assistant.status);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    #[ignore = "requires built wordcraft-codex-fixture"]
    fn codex_conversation_actions_share_revoke_and_send() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("wordcraft-codex-fixture");
        let binary = temp.path().join("codex");
        std::fs::copy(fixture, &binary).unwrap();
        std::fs::write(temp.path().join("mode"), "normal").unwrap();
        let mut host = Host { root: Some(temp.path().join("profile")), bridge: Some(binary.clone()), ..Default::default() };
        let mut app =
            WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::from_text("Selected text\nPrivate text")), Default::default());
        app.execute("ui.assistantConnect", json!({"binary":binary})).unwrap();
        until(&mut host, &mut app, |a| a.assistant.signed_in == Some(false));
        app.execute("ui.assistantNew", json!({})).unwrap();
        until(&mut host, &mut app, |a| a.assistant.thread.is_some());
        app.execute("select.text", json!({"text":"Selected text"})).unwrap();
        app.execute("ui.assistantShareSelection", json!({})).unwrap();
        host.tick(&mut app, &egui::Context::default());
        assert!(app.assistant.sharing.contains("selection"));
        app.execute("text.insert", json!({"text":"Human edit"})).unwrap();
        host.tick(&mut app, &egui::Context::default());
        assert!(app.assistant.sharing.contains("stale"));
        app.execute("ui.assistantShareDocument", json!({})).unwrap();
        host.tick(&mut app, &egui::Context::default());
        assert!(app.assistant.sharing.contains("document"));
        app.execute("ui.assistantResume", json!({"thread":"fixture-thread"})).unwrap();
        until(&mut host, &mut app, |a| !a.assistant.pending);
        assert!(host.profile.context().is_none());
        assert_eq!(app.assistant.recent.len(), 1);
        app.execute("ui.assistantSend", json!({"text":"Explain this passage"})).unwrap();
        host.tick(&mut app, &egui::Context::default());
        assert_eq!(app.assistant.messages[0].text, "Explain this passage");
        assert!(app.assistant.composer.is_empty());
        assert!(app.execute("ui.assistantSend", json!({"text":"Duplicate"})).is_err());
        host.send(Request::Interrupt { thread_id: "fixture-thread".into(), turn_id: "fixture-turn".into() }).unwrap();
        until(&mut host, &mut app, |a| !a.assistant.pending);
        app.execute("ui.assistantDisconnect", json!({})).unwrap();
        until(&mut host, &mut app, |a| !a.assistant.enabled);
        assert!(app.assistant.thread.is_none() && host.profile.context().is_none());
    }
    fn stream_host() -> (Host, WordApp) {
        let mut app =
            WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::from_text("Shared text\nHuman paragraph")), Default::default());
        app.assistant.enabled = true;
        app.assistant.remember_thread("thread".into());
        app.assistant.pending = true;
        let mut host = Host { thread: Some("thread".into()), starting_turn: true, ..Default::default() };
        host.begin_turn(&mut app, Some("turn"));
        (host, app)
    }
    fn notice(host: &mut Host, app: &mut WordApp, kind: Notice, params: serde_json::Value) {
        host.event(app, Event::Notification { kind, params });
    }
    #[test]
    fn codex_stream_order_completion_late_events_and_disconnect() {
        let (mut host, mut app) = stream_host();
        app.assistant.actions.push_back(Action::ShareSelection);
        host.tick(&mut app, &egui::Context::default());
        assert!(app.assistant.pending, "a sharing error cannot unlock an active turn");
        for (thread, turn, delta) in
            [("other", "turn", "private"), ("thread", "old", "stale"), ("thread", "turn", "hello "), ("thread", "turn", "world")]
        {
            notice(&mut host, &mut app, Notice::Delta, json!({"threadId":thread,"turnId":turn,"itemId":"item","delta":delta}));
        }
        assert_eq!(app.assistant.messages.len(), 1);
        assert_eq!(app.assistant.messages[0].text, "hello world");
        let completed = json!({"threadId":"thread","turnId":"turn","item":{"id":"item","type":"agentMessage","text":"hello world!"}});
        notice(&mut host, &mut app, Notice::ItemCompleted, completed.clone());
        notice(&mut host, &mut app, Notice::ItemCompleted, completed);
        notice(&mut host, &mut app, Notice::Delta, json!({"threadId":"thread","turnId":"turn","itemId":"item","delta":"duplicated"}));
        assert_eq!(app.assistant.messages[0].text, "hello world!");
        notice(&mut host, &mut app, Notice::TurnCompleted, json!({"threadId":"thread","turn":{"id":"turn","status":"completed"}}));
        assert!(!app.assistant.pending);
        host.starting_turn = true;
        app.assistant.pending = true;
        host.begin_turn(&mut app, Some("next"));
        notice(&mut host, &mut app, Notice::TurnCompleted, json!({"threadId":"thread","turn":{"id":"turn","status":"completed"}}));
        assert!(app.assistant.pending, "old completion cannot finish a new turn");
        notice(&mut host, &mut app, Notice::Delta, json!({"threadId":"thread","turnId":"next","itemId":"item","delta":"next reply"}));
        assert_eq!(app.assistant.messages.len(), 2);
        host.event(&mut app, Event::Stopped);
        assert!(!app.assistant.enabled && !app.assistant.pending && app.assistant.thread.is_none());
    }
    #[test]
    fn codex_protocol_only_proposes_and_user_approval_pauses_autosave() {
        let (mut host, mut app) = stream_host();
        app.execute("select.text", json!({"text":"Shared text"})).unwrap();
        host.share(&app, Scope::Selection).unwrap();
        let mut args = host.profile.context().unwrap().metadata();
        args["original"] = json!("Shared text");
        args["replacement"] = json!("Revised text");
        for (thread, turn, tool) in [
            ("other", "turn", PROPOSAL_TOOL),
            ("thread", "old", PROPOSAL_TOOL),
            ("thread", "turn", "ui.assistantApprove"),
            ("thread", "turn", "file.save"),
        ] {
            host.event(
                &mut app,
                Event::DocumentRequest { id: json!(1), thread: thread.into(), turn: Some(turn.into()), tool: tool.into(), arguments: args.clone() },
            );
        }
        assert_eq!(app.session.codex_requests().as_array().unwrap().len(), 0);
        notice(
            &mut host,
            &mut app,
            Notice::ItemCompleted,
            json!({"threadId":"thread","turnId":"turn","item":{"type":"fileChange","patch":"replace all"}}),
        );
        host.event(
            &mut app,
            Event::DocumentRequest { id: json!(2), thread: "thread".into(), turn: Some("turn".into()), tool: PROPOSAL_TOOL.into(), arguments: args },
        );
        assert_eq!(app.session.doc.final_text(wordcraft_doc::StoryRef::Body), "Shared text\nHuman paragraph");
        assert_eq!(app.session.codex_requests()[0]["state"], "pending");
        app.autosave = true;
        app.session.autosave = true;
        app.execute("ui.assistantApprove", json!({"request":"thread:turn"})).unwrap();
        assert_eq!(app.session.doc.final_text(wordcraft_doc::StoryRef::Body), "Revised text\nHuman paragraph");
        assert!(!app.autosave && !app.session.autosave);
    }
}
