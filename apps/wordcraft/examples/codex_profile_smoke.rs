//! Native T3 contract smoke using the real WordApp Session, without user files or preferences.
//! Ends its own test process after three rendered frames (avoids macOS window-drop abort).
use serde_json::json;
use wordcraft_engine::Session;
use wordcraft_mcp::codex_profile::{CodexProfile, READ_TOOLS, Scope, SharedContext};
use wordcraft_ui_egui::{Services, WordApp};

struct Smoke {
    app: WordApp,
    frames: usize,
}
impl Smoke {
    fn verify(&mut self) -> Result<(), String> {
        let session = &mut self.app.session;
        session.run("select.text", &json!({"text":"Shared paragraph"})).map_err(|e| e.to_string())?;
        let shared = SharedContext::capture(session, Scope::Selection, true)?;
        let mut args = shared.metadata();
        args["query"] = json!("paragraph");
        let mut profile = CodexProfile::default();
        profile.share(shared);
        profile.handle(session, json!({"id":1,"method":"initialize"}));
        profile.handle(session, json!({"method":"notifications/initialized"}));
        for name in READ_TOOLS {
            let result = profile.call(session, name, &args);
            if result.is_error || result.to_value().to_string().contains("Private paragraph") {
                return Err("Read scope failed in native WordApp.".into());
            }
        }
        for command in session.registry.all() {
            if !profile.call(session, command.id, &args).is_error {
                return Err("An unlisted command was allowed.".into());
            }
        }
        session.run("text.insert", &json!({"text":"Human replacement"})).map_err(|e| e.to_string())?;
        if !profile.call(session, "read_context", &args).is_error {
            return Err("Stale shared context was allowed.".into());
        }
        Ok(())
    }
}
impl eframe::App for Smoke {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        if self.frames >= 3 {
            let result = self.verify();
            match result {
                Ok(()) => println!("PASS: native WordApp, five reads, selection privacy, registry denylist, stale revision"),
                Err(ref error) => eprintln!("FAIL: {error}"),
            }
            std::process::exit(if result.is_ok() { 0 } else { 1 });
        }
        ctx.request_repaint();
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        self.frames = self.frames.saturating_add(1);
    }
}
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("WordCraft document tool smoke").with_active(false).with_inner_size([900.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "WordCraft document tool smoke",
        options,
        Box::new(|_| {
            let session = Session::new(wordcraft_doc::Document::from_text("Shared paragraph\nPrivate paragraph"));
            Ok(Box::new(Smoke { app: WordApp::new(session, Services::default()), frames: 0 }))
        }),
    )
}
