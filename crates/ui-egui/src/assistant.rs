//! Host-only proposal approval and request review. No runtime messages can invoke these actions.
use crate::WordApp;
use serde_json::json;

#[derive(Default)]
pub struct State {
    pub review_open: bool,
    pub error: Option<String>,
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
    ui.add(egui::Label::new("Approve each proposal before it changes your document. Save remains manual.").wrap());
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
                ui.label(format!("Codex · {id} · {state}"));
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
}
