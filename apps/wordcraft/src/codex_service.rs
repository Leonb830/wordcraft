//! Native owner of the runtime. UI actions are typed; protocol values stay in this host.
use std::path::PathBuf;
use std::time::Duration;
use wordcraft_assistant::native::{DocumentBridge, Launch, Runtime};
use wordcraft_assistant::{Event, Request};
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
                    app.assistant.signed_in = None;
                    app.assistant.status = "Codex is disabled. Document sharing is off.".into();
                    Ok(())
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
                app.assistant.status = error;
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
                    self.thread = result.pointer("/thread/id").and_then(|v| v.as_str()).map(str::to_owned);
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
                Event::Error(error) => {
                    app.assistant.status = error.to_string();
                    app.assistant.error = Some(error.to_string());
                }
                Event::Stopped => {
                    self.shutdown();
                    app.assistant.enabled = false;
                    app.assistant.signed_in = None;
                    if app.assistant.error.is_none() {
                        app.assistant.status = "Codex disconnected. Restart to reconnect.".into();
                    }
                    break;
                }
                _ => {}
            }
        }
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
