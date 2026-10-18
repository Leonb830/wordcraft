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
    loaded: bool,
    settings_enabled: bool,
    settings_value: Option<serde_json::Value>,
    login_cancelled: bool,
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
            loaded: false,
            settings_enabled: std::env::var_os("WORDCRAFT_NO_PREFS").is_none(),
            settings_value: None,
            login_cancelled: false,
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
    pub fn persist(&mut self, app: &mut WordApp) {
        if !self.settings_enabled {
            return;
        }
        let Some(root) = &self.root else { return };
        let value = crate::codex_settings::value(&app.assistant);
        if self.settings_value.as_ref() == Some(&value) {
            return;
        }
        match crate::codex_settings::save(root, &value) {
            Ok(()) => self.settings_value = Some(value),
            Err(error) => {
                app.assistant.error = Some(error.into());
                self.settings_enabled = false;
            }
        }
    }
    fn clear_auth(app: &mut WordApp) {
        app.assistant.auth_pending = false;
        app.assistant.login_url = None;
        app.assistant.login_id = None;
        app.assistant.open_login = false;
    }
    fn clear_references(&mut self, app: &mut WordApp) {
        self.shutdown();
        Self::clear_auth(app);
        app.assistant.enabled = false;
        app.assistant.signed_in = None;
        app.assistant.thread = None;
        app.assistant.turn = None;
        app.assistant.pending = false;
        app.assistant.recent.clear();
        app.assistant.messages.clear();
        app.assistant.composer.clear();
        app.assistant.resume.clear();
        app.assistant.last_prompt.clear();
        app.assistant.activity.clear();
        app.assistant.status = "Thread references cleared. Codex is disabled.".into();
        if let Some(requests) = app.session.codex_requests().as_array() {
            for request in requests {
                if request["state"] == "pending"
                    && let Some(id) = request["request"].as_str()
                {
                    let _ = app.session.deny_codex(id);
                }
            }
        }
    }
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
        Self::clear_auth(app);
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
        if !self.loaded {
            self.loaded = true;
            if self.settings_enabled
                && let Some(root) = &self.root
            {
                crate::codex_settings::load(root, &mut app.assistant);
            }
            self.settings_value = Some(crate::codex_settings::value(&app.assistant));
        }
        while let Some(action) = app.assistant.actions.pop_front() {
            let auth_action = matches!(action, Action::SignIn | Action::CancelSignIn | Action::SignOut);
            let result = match action {
                Action::SignIn => {
                    let started = if self.runtime.is_none() { self.start(app) } else { Ok(()) };
                    started.and_then(|()| {
                        self.login_cancelled = false;
                        app.assistant.auth_pending = true;
                        app.assistant.open_login = true;
                        app.assistant.login_url = None;
                        self.send(Request::Login)
                    })
                }
                Action::CancelSignIn => {
                    self.login_cancelled = true;
                    let result =
                        if let Some(login_id) = app.assistant.login_id.clone() { self.send(Request::CancelLogin { login_id }) } else { Ok(()) };
                    Self::clear_auth(app);
                    app.assistant.auth_pending = result.is_ok() && self.runtime.is_some();
                    app.assistant.status = "Cancelling sign-in…".into();
                    result
                }
                Action::SignOut => {
                    if self.turn.is_some() {
                        let _ = self.stop(app);
                    }
                    self.profile.revoke();
                    self.thread = None;
                    self.turn = None;
                    self.starting_turn = false;
                    app.assistant.thread = None;
                    app.assistant.turn = None;
                    app.assistant.pending = false;
                    Self::clear_auth(app);
                    self.send(Request::Logout)
                }
                Action::ClearReferences => {
                    self.clear_references(app);
                    Ok(())
                }
                Action::Connect => self.start(app),
                Action::Disconnect => {
                    self.shutdown();
                    Self::clear_auth(app);
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
                if auth_action {
                    Self::clear_auth(app);
                }
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
        self.persist(app);
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
            Event::Response { request: Request::Login, result, .. } => {
                let id = result.get("loginId").and_then(|v| v.as_str()).filter(|s| !s.is_empty() && s.len() <= 128);
                let url = result
                    .get("authUrl")
                    .and_then(|v| v.as_str())
                    .filter(|s| s.starts_with("https://auth.openai.com/") && s.len() <= 8192 && !s.chars().any(char::is_control));
                if self.login_cancelled {
                    if let Some(login_id) = id {
                        let _ = self.send(Request::CancelLogin { login_id: login_id.into() });
                    }
                    Self::clear_auth(app);
                    app.assistant.auth_pending = id.is_some() && self.runtime.is_some();
                } else if let (Some(id), Some(url), Some("chatgpt")) = (id, url, result.get("type").and_then(|v| v.as_str())) {
                    app.assistant.login_id = Some(id.into());
                    app.assistant.login_url = Some(url.into());
                    app.assistant.status = "Complete secure sign-in in your browser.".into();
                } else {
                    Self::clear_auth(app);
                    app.assistant.error = Some("Codex returned an unsupported sign-in response.".into());
                }
            }
            Event::Response { request: Request::CancelLogin { .. }, .. } => {
                Self::clear_auth(app);
                app.assistant.status = "Sign-in cancelled.".into();
            }
            Event::Response { request: Request::Logout, .. } => {
                Self::clear_auth(app);
                app.assistant.signed_in = Some(false);
                app.assistant.status = "Signed out of WordCraft's Codex profile.".into();
            }
            Event::Notification { kind: Notice::LoginCompleted, params } => {
                let matches = app.assistant.login_id.is_some() && params.get("loginId").and_then(|v| v.as_str()) == app.assistant.login_id.as_deref();
                if matches {
                    Self::clear_auth(app);
                    if params.get("success").and_then(|v| v.as_bool()) == Some(true) {
                        let _ = self.send(Request::Account);
                    } else {
                        app.assistant.status = "Sign-in did not complete. Try again.".into();
                    }
                }
            }
            Event::Notification { kind: Notice::AccountUpdated, params } => {
                if params.get("authMode").is_some_and(|v| v.is_null()) {
                    self.profile.revoke();
                    app.assistant.signed_in = Some(false);
                    app.assistant.status = "Your WordCraft Codex session needs sign-in.".into();
                    if self.turn.is_some() {
                        let _ = self.stop(app);
                    }
                } else {
                    let _ = self.send(Request::Account);
                }
            }
            Event::Response { request: Request::Account, result, .. } => {
                let signed_in = result.get("account").is_some_and(|a| !a.is_null());
                if !signed_in {
                    self.profile.revoke();
                    if self.turn.is_some() {
                        let _ = self.stop(app);
                    }
                }
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
                if app.assistant.auth_pending {
                    Self::clear_auth(app);
                }
                if self.turn.is_none() {
                    app.assistant.pending = false;
                    self.starting_turn = false;
                }
                app.assistant.status = error.to_string();
                app.assistant.error = Some(error.to_string());
            }
            Event::Stopped => {
                self.shutdown();
                Self::clear_auth(app);
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
        let mut host = Host { root: None, thread: Some("thread".into()), starting_turn: true, ..Default::default() };
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
    #[test]
    fn codex_auth_events_cancel_expire_and_clear() {
        let (mut host, mut app) = stream_host();
        let login = |url: &str| Event::Response { id: 1, request: Request::Login, result: json!({"type":"chatgpt","loginId":"login","authUrl":url}) };
        app.assistant.auth_pending = true;
        host.event(&mut app, login("https://auth.openai.com.attacker.invalid/secret"));
        assert!(!app.assistant.auth_pending && app.assistant.login_url.is_none());
        app.assistant.auth_pending = true;
        host.event(&mut app, login("https://auth.openai.com/authorize?synthetic"));
        assert_eq!(app.assistant.login_id.as_deref(), Some("login"));
        notice(&mut host, &mut app, Notice::LoginCompleted, json!({"loginId":"foreign","success":true}));
        assert!(app.assistant.auth_pending);
        notice(&mut host, &mut app, Notice::LoginCompleted, json!({"loginId":"login","success":false,"error":"SECRET"}));
        assert!(!app.assistant.auth_pending && app.assistant.login_url.is_none());
        assert!(!app.assistant.status.contains("SECRET"));
        host.login_cancelled = true;
        host.event(&mut app, login("https://auth.openai.com/authorize?synthetic"));
        assert!(app.assistant.login_url.is_none());
        app.execute("select.text", json!({"text":"Shared text"})).unwrap();
        host.share(&app, Scope::Selection).unwrap();
        notice(&mut host, &mut app, Notice::AccountUpdated, json!({"authMode":null}));
        assert!(host.profile.context().is_none());
        assert_eq!(app.assistant.signed_in, Some(false));
        assert!(host.stop_requested);
        app.assistant.composer = "SECRET".into();
        app.assistant.push_message("You", "SECRET".into());
        host.clear_references(&mut app);
        assert!(app.assistant.recent.is_empty() && app.assistant.messages.is_empty() && app.assistant.composer.is_empty());
        assert!(!app.assistant.enabled && !app.assistant.pending && host.thread.is_none());
        assert_eq!(app.session.doc.final_text(wordcraft_doc::StoryRef::Body), "Shared text\nHuman paragraph");
    }
    #[test]
    #[ignore = "requires installed Codex 0.162.1 and built WordCraft helper"]
    fn codex_real_isolated_auth_start_cancel_logout() {
        let temp = tempfile::tempdir().unwrap();
        let bridge = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("wordcraft");
        let mut host = Host { root: Some(temp.path().join("profile")), bridge: Some(bridge), settings_enabled: true, ..Default::default() };
        let mut app = WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::from_text("Synthetic auth check")), Default::default());
        app.execute("ui.assistantConnect", json!({"binary":"/opt/homebrew/bin/codex"})).unwrap();
        until(&mut host, &mut app, |a| a.assistant.signed_in == Some(false));
        app.execute("ui.assistantSignIn", json!({})).unwrap();
        until(&mut host, &mut app, |a| a.assistant.login_url.is_some());
        assert!(app.assistant.auth_pending);
        app.execute("ui.assistantCancelSignIn", json!({})).unwrap();
        until(&mut host, &mut app, |a| !a.assistant.auth_pending);
        app.assistant.signed_in = None;
        app.execute("ui.assistantSignOut", json!({})).unwrap();
        until(&mut host, &mut app, |a| a.assistant.signed_in == Some(false));
        host.shutdown();
        host.settings_value = None;
        host.persist(&mut app);
        let settings = std::fs::read_to_string(temp.path().join("profile/assistant.json")).unwrap();
        assert!(!settings.contains("authUrl") && !settings.contains("loginId") && !settings.contains("Synthetic auth"));
        assert!(!temp.path().join("profile/home/history.jsonl").exists());
    }
}
