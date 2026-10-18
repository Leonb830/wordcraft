//! Native owner of the runtime. UI actions are typed; protocol values stay in this host.
use std::path::PathBuf;
use std::time::Duration;
use wordcraft_assistant::native::{DocumentBridge, Launch, Runtime};
use wordcraft_assistant::{Event, Notice, Request};
use wordcraft_mcp::codex_profile::{CodexProfile, Scope, SharedContext};
use wordcraft_mcp::tools::ToolResult;
use wordcraft_ui_egui::{WordApp, assistant::Action};

pub struct Host {
    runtime: Option<Runtime>,
    pub profile: CodexProfile,
    pub thread: Option<String>,
    root: Option<PathBuf>,
    bridge: Option<PathBuf>,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            runtime: None,
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
                Action::Send(text) => {
                    let result = self
                        .thread
                        .clone()
                        .ok_or("Start a conversation first.".into())
                        .and_then(|thread_id| self.send(Request::StartTurn { thread_id, text: text.clone() }));
                    if result.is_ok() {
                        app.assistant.push_message("You", text);
                        app.assistant.composer.clear();
                        app.assistant.focus_composer = true;
                    }
                    result
                }
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
                app.assistant.pending = false;
            }
        }
        for _ in 0..128 {
            let Some(event) = self.runtime.as_mut().and_then(Runtime::poll) else { break };
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
                    self.thread =
                        result.pointer("/thread/id").and_then(|v| v.as_str()).filter(|s| !s.is_empty() && s.len() <= 128).map(str::to_owned);
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
                Event::DocumentRequest { id, thread, tool, arguments } => {
                    let result = if self.thread.as_deref() == Some(thread.as_str()) {
                        self.profile.call(&app.session, &tool, &arguments)
                    } else {
                        ToolResult::error("The conversation changed; share context in the active conversation.")
                    };
                    if let Some(runtime) = &self.runtime
                        && let Err(error) = runtime.document_reply(id, result.to_value())
                    {
                        app.assistant.status = error.to_string();
                    }
                }
                Event::Notification { kind: Notice::TurnCompleted, params }
                    if params.get("threadId").and_then(|v| v.as_str()) == self.thread.as_deref() =>
                {
                    app.assistant.pending = false;
                }
                Event::Error(error) => {
                    app.assistant.pending = false;
                    app.assistant.status = error.to_string();
                    app.assistant.error = Some(error.to_string());
                }
                Event::Stopped => {
                    self.shutdown();
                    app.assistant.enabled = false;
                    app.assistant.thread = None;
                    app.assistant.pending = false;
                    app.assistant.signed_in = None;
                    if app.assistant.error.is_none() {
                        app.assistant.status = "Codex disconnected. Restart to reconnect.".into();
                    }
                    break;
                }
                _ => {}
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
}
