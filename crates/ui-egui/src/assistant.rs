//! Host-only proposal approval and request review. No runtime messages can invoke these actions.
use crate::WordApp;
use serde_json::json;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Connect,
    Disconnect,
    Account,
    ShareSelection,
    ShareDocument,
    Revoke,
    NewThread,
    Resume(String),
    Send(String),
    Stop,
    Retry,
    SignIn,
    CancelSignIn,
    SignOut,
    ClearReferences,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub role: &'static str,
    pub id: Option<String>,
    pub complete: bool,
    pub text: String,
}

pub struct State {
    pub login_url: Option<String>,
    pub login_id: Option<String>,
    pub auth_pending: bool,
    pub open_login: bool,
    pub turn: Option<String>,
    pub last_prompt: String,
    pub activity: String,
    pub open: bool,
    pub composer: String,
    pub resume: String,
    pub thread: Option<String>,
    pub recent: Vec<String>,
    pub messages: Vec<Message>,
    pub pending: bool,
    pub sharing: String,
    pub focus_composer: bool,
    pub connection_open: bool,
    pub enabled: bool,
    pub status: String,
    pub binary: String,
    pub signed_in: Option<bool>,
    pub actions: std::collections::VecDeque<Action>,
    pub review_open: bool,
    pub error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            login_url: None,
            login_id: None,
            auth_pending: false,
            open_login: false,
            turn: None,
            last_prompt: String::new(),
            activity: String::new(),
            open: false,
            composer: String::new(),
            resume: String::new(),
            thread: None,
            recent: Vec::new(),
            messages: Vec::new(),
            pending: false,
            sharing: "Document sharing is off.".into(),
            focus_composer: false,
            connection_open: false,
            enabled: false,
            status: if cfg!(target_arch = "wasm32") { "Codex requires the desktop app." } else { "Codex is disabled." }.into(),
            binary: String::new(),
            signed_in: None,
            actions: Default::default(),
            review_open: false,
            error: None,
        }
    }
}

impl State {
    pub fn can_send(&self) -> bool {
        self.enabled && self.thread.is_some() && !self.pending && !self.composer.trim().is_empty() && self.composer.len() <= 64 * 1024
    }
    pub fn remember_thread(&mut self, id: String) {
        self.recent.retain(|old| old != &id);
        self.recent.insert(0, id.clone());
        self.recent.truncate(20);
        self.thread = Some(id);
        self.turn = None;
        self.last_prompt.clear();
        self.activity.clear();
        self.messages.clear();
        self.pending = false;
    }
    pub fn push_message(&mut self, role: &'static str, text: String) {
        self.messages.push(Message { role, text, id: None, complete: true });
        while self.messages.len() > 100 || self.messages.iter().map(|m| m.text.len()).sum::<usize>() > 1024 * 1024 {
            self.messages.remove(0);
        }
    }
    pub fn stream(&mut self, item: &str, text: &str, completed: bool) {
        if item.is_empty() || item.len() > 128 || text.len() > 256 * 1024 {
            self.error = Some("The response exceeded the display limit. Stop and ask for a shorter response.".into());
            return;
        }
        let key = format!("{}:{item}", self.turn.as_deref().unwrap_or(""));
        if let Some(message) = self.messages.iter_mut().find(|m| m.id.as_deref() == Some(key.as_str())) {
            if message.complete {
                return;
            }
            if completed {
                message.text = text.to_owned();
                message.complete = true;
            } else if message.text.len().saturating_add(text.len()) <= 256 * 1024 {
                message.text.push_str(text);
            } else {
                self.error = Some("The response exceeded the display limit. Stop and ask for a shorter response.".into());
            }
        } else {
            self.push_message("Codex", text.to_owned());
            if let Some(message) = self.messages.last_mut() {
                message.id = Some(key);
                message.complete = completed;
            }
        }
        while self.messages.iter().map(|m| m.text.len()).sum::<usize>() > 1024 * 1024 {
            self.messages.remove(0);
        }
    }
}

pub fn pane(app: &mut WordApp, ui: &mut egui::Ui) {
    if crate::panes::header(ui, "Assistant") {
        let _ = app.run("ui.assistant", json!({"open":false}));
        return;
    }
    ui.add(egui::Label::new(&app.assistant.sharing).wrap());
    if !cfg!(target_arch = "wasm32") {
        egui::Panel::bottom("assistant_composer_panel").exact_size(200.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(app.assistant.pending, egui::Button::new(tl!("Stop response"))).clicked() {
                    let _ = app.run("ui.assistantStop", json!({}));
                }
                if ui
                    .add_enabled(
                        !app.assistant.pending && !app.assistant.last_prompt.is_empty() && app.assistant.thread.is_some(),
                        egui::Button::new(tl!("Retry message")),
                    )
                    .clicked()
                {
                    let _ = app.run("ui.assistantRetry", json!({}));
                }
                if ui.button(tl!("Disconnect")).clicked() {
                    let _ = app.run("ui.assistantDisconnect", json!({}));
                }
            });
            if !app.assistant.activity.is_empty() {
                ui.add(egui::Label::new(&app.assistant.activity).wrap());
            }
            ui.separator();
            ui.label(tl!("Message to Codex"));
            let input = ui.add(
                egui::TextEdit::multiline(&mut app.assistant.composer)
                    .id_salt("assistant_composer")
                    .hint_text(tl!("Message to Codex"))
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .char_limit(64 * 1024),
            );
            if std::mem::take(&mut app.assistant.focus_composer) {
                input.request_focus();
            }
            let shortcut = input.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
            if ui.add_enabled(app.assistant.can_send(), egui::Button::new(tl!("Send message"))).clicked() || shortcut && app.assistant.can_send() {
                let _ = app.run("ui.assistantSend", json!({}));
            }
        });
    }
    egui::ScrollArea::vertical().id_salt("assistant_scroll").stick_to_bottom(true).show(ui, |ui| {
        ui.add(egui::Label::new(&app.assistant.status).wrap());
        if ui.button(tl!("Connection settings")).clicked() {
            let _ = app.run("ui.assistantConnection", json!({}));
        }
        if cfg!(target_arch = "wasm32") {
            ui.label(tl!("Open the desktop app to use Codex. You can keep editing here."));
            return;
        }
        if !app.assistant.enabled && ui.button(tl!("Start Codex")).clicked() {
            let _ = app.run("ui.assistantConnect", json!({}));
        }
        ui.separator();
        ui.add_enabled_ui(app.assistant.enabled && !app.assistant.pending, |ui| {
            if ui.button(tl!("New conversation")).clicked() {
                let _ = app.run("ui.assistantNew", json!({}));
            }
            if !app.assistant.recent.is_empty() {
                egui::ComboBox::from_id_salt("assistant_recent").selected_text(tl!("Recent conversations")).show_ui(ui, |ui| {
                    for id in app.assistant.recent.clone() {
                        if ui.selectable_label(false, &id).clicked() {
                            let _ = app.run("ui.assistantResume", json!({"thread":id}));
                        }
                    }
                });
            }
            ui.label(tl!("Conversation ID"));
            ui.add(egui::TextEdit::singleline(&mut app.assistant.resume).hint_text(tl!("Conversation ID")).desired_width(f32::INFINITY));
            if ui.button(tl!("Resume conversation")).clicked() {
                let _ = app.run("ui.assistantResume", json!({"thread":app.assistant.resume}));
            }
        });
        if let Some(id) = &app.assistant.thread {
            ui.add(egui::Label::new(format!("{}: {id}", tl!("Conversation"))).wrap());
        }
        ui.separator();
        ui.strong(tl!("Shared context"));
        ui.add(
            egui::Label::new(tl!(
                "Sharing lets Codex read this content. Changes require your approval. Previously sent content remains in the conversation."
            ))
            .wrap(),
        );
        ui.add_enabled_ui(app.assistant.thread.is_some() && !app.assistant.pending, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, command) in [
                    ("Share selection", "ui.assistantShareSelection"),
                    ("Share document", "ui.assistantShareDocument"),
                    ("Stop sharing", "ui.assistantRevoke"),
                ] {
                    if ui.button(tl!(label)).clicked() {
                        let _ = app.run(command, json!({}));
                    }
                }
            });
        });
        ui.separator();
        if app.assistant.messages.is_empty() {
            ui.add(
                egui::Label::new(tl!("Start or resume a conversation, then ask a question. Share context only when you want Codex to read it."))
                    .wrap(),
            );
        }
        for message in &app.assistant.messages {
            ui.group(|ui| {
                ui.strong(tl!(message.role));
                ui.add(egui::Label::new(&message.text).wrap());
            });
        }
        if app.session.codex_requests().as_array().is_some_and(|r| !r.is_empty()) {
            ui.separator();
            review_cards(app, ui);
        }
        if app.assistant.pending {
            ui.spinner();
            ui.label(tl!("Waiting for Codex…"));
        }
        if let Some(error) = &app.assistant.error {
            ui.add(egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color)).wrap());
        }
    });
}

pub fn connection(app: &mut WordApp, ctx: &egui::Context) {
    if !app.assistant.connection_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Codex connection").default_width(520.0).open(&mut open).resizable(true).show(ctx, |ui| {
        ui.label(&app.assistant.status);
        ui.label(match app.assistant.signed_in {
            Some(true) => "Signed in to WordCraft's Codex profile.",
            Some(false) => "Signed out of WordCraft's Codex profile.",
            None => "Sign-in status has not been checked.",
        });
        ui.label("Sign in with ChatGPT in your browser. Credentials stay in WordCraft's separate Codex profile.");
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!cfg!(target_arch = "wasm32") && !app.assistant.auth_pending, egui::Button::new(tl!("Sign in with ChatGPT"))).clicked()
            {
                let _ = app.run("ui.assistantSignIn", json!({}));
            }
            if ui.add_enabled(app.assistant.auth_pending, egui::Button::new(tl!("Cancel sign-in"))).clicked() {
                let _ = app.run("ui.assistantCancelSignIn", json!({}));
            }
            if ui.add_enabled(app.assistant.enabled, egui::Button::new(tl!("Sign out of WordCraft"))).clicked() {
                let _ = app.run("ui.assistantSignOut", json!({}));
            }
        });
        if let Some(url) = &app.assistant.login_url {
            ui.hyperlink_to(tl!("Open secure sign-in page"), url);
            if std::mem::take(&mut app.assistant.open_login) {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        if app.assistant.auth_pending {
            ui.label(tl!("Complete sign-in in your browser, then return here."));
        }
        ui.separator();
        if ui.button(tl!("Clear thread references")).clicked() {
            let _ = app.run("ui.assistantClearReferences", json!({}));
        }
        ui.add(
            egui::Label::new(tl!(
                "Clearing removes WordCraft's conversation list and disconnects. Codex conversation data remains in its own profile."
            ))
            .wrap(),
        );
        ui.label("Installed Codex 0.162.1 executable (leave blank to detect)");
        ui.text_edit_singleline(&mut app.assistant.binary);
        ui.label("WordCraft uses its own Codex profile. Document sharing is off until you choose a scope.");
        ui.add_enabled_ui(!cfg!(target_arch = "wasm32"), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Start / restart Codex").clicked() {
                    let _ = app.run("ui.assistantConnect", json!({}));
                }
                if ui.button("Disable Codex").clicked() {
                    let _ = app.run("ui.assistantDisconnect", json!({}));
                }
                if ui.button("Check sign-in").clicked() {
                    let _ = app.run("ui.assistantAccount", json!({}));
                }
            });
        });
    });
    if !open {
        app.assistant.connection_open = false;
    }
}

pub fn show_review(app: &mut WordApp, ui: &mut egui::Ui) {
    if !app.assistant.review_open {
        return;
    }
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::Panel::right("codex_review").default_size(320.0).resizable(true).frame(egui::Frame::NONE.fill(t.panel).inner_margin(10)).show(ui, |ui| {
        if crate::panes::header(ui, "Codex edits") {
            let _ = app.run("ui.assistantReview", json!({"open":false}));
            return;
        }
        review_cards(app, ui);
    });
}

pub fn review_cards(app: &mut WordApp, ui: &mut egui::Ui) {
    ui.add(egui::Label::new("Approve each proposal before it changes your document. Approval pauses AutoSave; save manually when ready.").wrap());
    if let Some(error) = &app.assistant.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    let requests = app.session.codex_requests();
    let Some(requests) = requests.as_array() else { return };
    if requests.is_empty() {
        ui.label("No proposed edits.");
    }
    for request in requests {
        let id = request.get("request").and_then(|v| v.as_str()).unwrap_or("");
        let state = request.get("state").and_then(|v| v.as_str()).unwrap_or("");
        ui.push_id(id, |ui| {
            ui.group(|ui| {
                ui.add(egui::Label::new(format!("Codex · {id} · {state}")).wrap());
                ui.label(format!("Document revision {}", request["revision"]));
                ui.label("Original text");
                ui.add(egui::Label::new(request["before"].as_str().unwrap_or("")).wrap());
                ui.label("Proposed text");
                ui.add(egui::Label::new(request["replacement"].as_str().unwrap_or("")).wrap());
                let mut command = None;
                ui.horizontal_wrapped(|ui| {
                    if state == "pending" {
                        if ui.button("Approve edit").clicked() {
                            command = Some("ui.assistantApprove");
                        }
                        if ui.button("Deny edit").clicked() {
                            command = Some("ui.assistantDeny");
                        }
                    } else if state == "applied" {
                        if ui.button("Accept request").clicked() {
                            command = Some("review.acceptCodex");
                        }
                        if ui.button("Reject request").clicked() {
                            command = Some("review.rejectCodex");
                        }
                        if ui.button("Undo request").clicked() {
                            command = Some("review.rejectCodexUndo");
                        }
                    } else if state == "accepted" && ui.button("Undo request").clicked() {
                        command = Some("review.rejectCodexUndo");
                    }
                });
                if let Some(command) = command {
                    app.assistant.error = app.run(command, json!({"request":id})).err();
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};
    use wordcraft_doc::{Document, StoryRef};
    use wordcraft_engine::Session;
    fn app() -> WordApp {
        let mut session = Session::new(Document::from_text("Original sentence"));
        session.run("select.all", &json!({})).unwrap();
        session.propose_codex("thread-1", "request-1", session.document_id(), session.rev(), session.sel.clone(), "Revised sentence").unwrap();
        WordApp::new(session, crate::Services::default())
    }
    #[test]
    fn codex_cards_approve_accept_and_undo() {
        let mut h = Harness::builder().with_size(egui::vec2(500.0, 700.0)).build_ui_state(|ui, app: &mut WordApp| review_cards(app, ui), app());
        assert_eq!(h.state().session.doc.final_text(StoryRef::Body), "Original sentence");
        h.get_by_label("Approve edit").click();
        h.run();
        assert_eq!(h.state().session.doc.final_text(StoryRef::Body), "Revised sentence");
        h.get_by_label("Accept request").click();
        h.run();
        h.get_by_label("Undo request").click();
        h.run();
        assert_eq!(h.state().session.doc.final_text(StoryRef::Body), "Original sentence");
    }
    #[test]
    fn codex_cards_deny_and_stale_approval() {
        let mut h = Harness::builder().with_size(egui::vec2(500.0, 700.0)).build_ui_state(|ui, app: &mut WordApp| review_cards(app, ui), app());
        h.get_by_label("Deny edit").click();
        h.run();
        assert_eq!(h.state().session.doc.final_text(StoryRef::Body), "Original sentence");
        let mut stale = app();
        stale.session.run("text.insert", &json!({"text":"Human edit"})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(500.0, 700.0)).build_ui_state(|ui, app: &mut WordApp| review_cards(app, ui), stale);
        h.get_by_label("Approve edit").click();
        h.run();
        assert!(h.state().assistant.error.is_some());
        assert_eq!(h.state().session.doc.final_text(StoryRef::Body), "Human edit");
    }
    #[test]
    fn codex_cards_reject_request() {
        let mut h = Harness::builder().with_size(egui::vec2(500.0, 700.0)).build_ui_state(|ui, app: &mut WordApp| review_cards(app, ui), app());
        h.get_by_label("Approve edit").click();
        h.run();
        h.get_by_label("Reject request").click();
        h.run();
        assert_eq!(h.state().session.doc.final_text(StoryRef::Body), "Original sentence");
    }
    #[test]
    fn codex_cards_snapshot() {
        let Ok(path) = std::env::var("WORDCRAFT_CODEX_REVIEW_SHOT") else { return };
        let mut state = app();
        state.assistant.review_open = true;
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 850.0)).wgpu().build_ui_state(
            |ui, app: &mut WordApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            state,
        );
        h.input_mut().max_texture_side = Some(8192);
        for _ in 0..8 {
            h.step();
        }
        h.render().unwrap().save(path).unwrap();
    }
    fn conversation_app() -> WordApp {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::from_text("Synthetic document")), Default::default());
        a.assistant.open = true;
        a.assistant.enabled = true;
        a.assistant.remember_thread("synthetic-thread".into());
        a.assistant.status = "Conversation ready.".into();
        a
    }
    fn pane_harness(a: WordApp, width: f32) -> Harness<'static, WordApp> {
        let mut fonts = false;
        let mut h = Harness::builder().with_size(egui::vec2(width, 900.0)).build_ui_state(
            move |ui, a: &mut WordApp| {
                if fonts {
                    pane(a, ui);
                } else {
                    crate::theme::install_fonts(ui.ctx());
                    fonts = true;
                }
            },
            a,
        );
        h.run_steps(3);
        h
    }
    #[test]
    fn codex_auth_connection_controls_and_snapshot() {
        let mut app = conversation_app();
        app.assistant.connection_open = true;
        app.assistant.status = "Codex connected. Sign in to continue.".into();
        let mut h = Harness::builder().with_size(egui::vec2(800.0, 650.0)).build_ui_state(|ui, app: &mut WordApp| connection(app, ui.ctx()), app);
        h.run_steps(3);
        if let Ok(path) = std::env::var("WORDCRAFT_CODEX_AUTH_SHOT") {
            h.render().unwrap().save(path).unwrap();
        }
        h.get_by_label("Sign in with ChatGPT").click();
        h.run_steps(3);
        assert!(h.state().assistant.auth_pending);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::SignIn));
        assert!(h.state_mut().execute("ui.assistantSignIn", json!({})).is_err());
        h.get_by_label("Cancel sign-in").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::CancelSignIn));
        h.get_by_label("Sign out of WordCraft").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::SignOut));
        h.get_by_label("Clear thread references").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::ClearReferences));
    }
    #[test]
    fn codex_pane_composer_context_and_pending_gate() {
        let mut h = pane_harness(conversation_app(), 360.0);
        for (label, action) in
            [("Share selection", Action::ShareSelection), ("Share document", Action::ShareDocument), ("Stop sharing", Action::Revoke)]
        {
            h.get_by_label(label).click();
            h.run_steps(3);
            assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(action));
        }
        h.state_mut().assistant.composer = "Explain the selected text".into();
        h.run_steps(3);
        h.get_by_label("Send message").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::Send("Explain the selected text".into())));
        assert!(h.state().assistant.pending);
        assert!(h.state_mut().execute("ui.assistantSend", json!({})).is_err());
    }
    #[test]
    fn codex_pane_keyboard_send_and_thread_controls() {
        let mut a = conversation_app();
        a.assistant.focus_composer = true;
        let mut h = pane_harness(a, 360.0);
        h.event(egui::Event::Text("A keyboard question".into()));
        h.run_steps(3);
        assert_eq!(h.state().assistant.composer, "A keyboard question");
        h.event(egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.run_steps(3);
        assert!(matches!(h.state_mut().assistant.actions.pop_front(), Some(Action::Send(_))));
        h.state_mut().assistant.pending = false;
        h.run_steps(3);
        h.get_by_label("New conversation").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::NewThread));
        h.state_mut().assistant.pending = false;
        h.state_mut().assistant.resume = "synthetic-thread".into();
        h.run_steps(3);
        h.get_by_label("Resume conversation").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::Resume("synthetic-thread".into())));
    }
    #[test]
    fn codex_pane_empty_error_close_and_narrow_snapshot() {
        let mut a = conversation_app();
        a.assistant.enabled = false;
        a.assistant.thread = None;
        a.assistant.status = "Codex was not found. Install Codex 0.162.1 or select its executable.".into();
        let mut h = pane_harness(a, 280.0);
        assert!(h.query_by_label_contains("Codex was not found").is_some());
        assert!(h.query_by_label("Document sharing is off.").is_some());
        h.get_by_label("Start Codex").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::Connect));
        h.get_by_label("Close").click();
        h.run_steps(3);
        assert!(!h.state().assistant.open);
        let Ok(path) = std::env::var("WORDCRAFT_CODEX_PANE_SHOT") else { return };
        let mut a = conversation_app();
        a.assistant.sharing = "Sharing selection · revision 4 · 128 bytes".into();
        a.assistant.push_message("You", "Please explain this selection.".into());
        let mut h = Harness::builder().with_size(egui::vec2(1100.0, 850.0)).wgpu().build_ui_state(
            |ui, a: &mut WordApp| {
                a.logic(ui.ctx());
                a.ui(ui);
            },
            a,
        );
        h.input_mut().max_texture_side = Some(8192);
        for _ in 0..8 {
            h.step();
        }
        h.render().unwrap().save(path).unwrap();
    }
    #[test]
    fn codex_stream_ui_stop_retry_and_approval_cards() {
        let mut a = conversation_app();
        a.assistant.turn = Some("turn".into());
        a.assistant.last_prompt = "Explain".into();
        a.assistant.pending = true;
        a.assistant.stream("item", "First ", false);
        let mut h = pane_harness(a, 360.0);
        assert!(h.query_by_label("First ").is_some());
        h.state_mut().assistant.stream("item", "second", false);
        h.run_steps(3);
        assert!(h.query_by_label("First second").is_some());
        h.get_by_label("Stop response").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::Stop));
        h.state_mut().assistant.pending = false;
        h.run_steps(3);
        h.get_by_label("Retry message").click();
        h.run_steps(3);
        assert_eq!(h.state_mut().assistant.actions.pop_front(), Some(Action::Retry));
    }
}
