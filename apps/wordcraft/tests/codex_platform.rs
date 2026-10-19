//! Portable native packaging smoke: no account or inference required.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "requires native window session and WORDCRAFT_CODEX_BINARY"]
fn native_platform_runtime_and_editor_smoke() {
    let binary = std::path::PathBuf::from(std::env::var_os("WORDCRAFT_CODEX_BINARY").expect("set the installed pinned Codex binary"));
    assert!(binary.is_absolute() && binary.is_file());
    assert!(json!(binary).is_string());
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut app = Owned(
        Command::new(env!("CARGO_BIN_EXE_wordcraft"))
            .args(["--sample", "--control", &port.to_string()])
            .env("WORDCRAFT_PROFILE_DIR", temp.path())
            .env("WORDCRAFT_NO_PREFS", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let key_path = wordcraft_control_key::key_path(temp.path(), port);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !key_path.exists() {
        assert!(app.0.try_wait().unwrap().is_none(), "native window exited");
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut writer = TcpStream::connect(("127.0.0.1", port)).unwrap();
    writer.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    let mut reader = BufReader::new(writer.try_clone().unwrap());
    let key = std::fs::read_to_string(key_path).unwrap();
    let mut call = |command: &str, params: Value| {
        writeln!(writer, "{}", json!({"id":1,"key":key,"method":"engine.execute","params":{"command":command,"params":params}})).unwrap();
        writer.flush().unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        reply["result"].clone()
    };
    assert_eq!(call("ui.assistantStatus", json!({}))["enabled"], false);
    call("ui.assistantConnect", json!({"binary":binary}));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = call("ui.assistantStatus", json!({}));
        if status["signedIn"] == false {
            break;
        }
        assert!(Instant::now() < deadline, "runtime failed: {status}");
        std::thread::sleep(Duration::from_millis(50));
    }
    call("ui.assistantNew", json!({}));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = call("ui.assistantStatus", json!({}));
        if status["thread"].is_string() {
            break;
        }
        assert!(Instant::now() < deadline, "thread failed: {status}");
        std::thread::sleep(Duration::from_millis(50));
    }
    call("text.insert", json!({"text":"Portable native editing smoke"}));
    call("ui.assistantShareDocument", json!({}));
    std::thread::sleep(Duration::from_millis(200));
    assert!(call("ui.assistantStatus", json!({}))["sharing"].as_str().unwrap().starts_with("Sharing document"));
    call("ui.assistantDisconnect", json!({}));
    call("file.close", json!({"force":true}));
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
}
