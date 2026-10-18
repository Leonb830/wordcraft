//! Real native window, installed runtime and existing control/MCP coexistence.
#![cfg(target_os = "macos")]
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
fn descendants(pid: u32) -> Vec<u32> {
    let out = Command::new("/usr/bin/pgrep").args(["-P", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|s| s.parse().ok()).collect()
}
fn alive(pid: u32) -> bool {
    Command::new("/bin/kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().unwrap().success()
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(40);
    while !predicate() {
        assert!(Instant::now() < deadline, "native host timeout");
        std::thread::sleep(Duration::from_millis(30));
    }
}
struct Control {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    key: String,
}
impl Control {
    fn call(&mut self, method: &str, params: Value) -> Value {
        writeln!(self.writer, "{}", json!({"id":1,"key":self.key,"method":method,"params":params})).unwrap();
        self.writer.flush().unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        let result: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(result["ok"], true, "control operation failed: {method} {params} {result}");
        result["result"].clone()
    }
    fn execute(&mut self, command: &str, params: Value) -> Value {
        self.call("engine.execute", json!({"command":command,"params":params}))
    }
    fn status(&mut self) -> Value {
        self.execute("ui.assistantStatus", json!({}))
    }
}
#[test]
#[ignore = "requires macOS window session, installed Codex 0.162.1 and built wordcraft-cli"]
fn native_runtime_restart_crash_shutdown_and_existing_mcp() {
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
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let key_path = wordcraft_control_key::key_path(temp.path(), port);
    wait(|| key_path.exists());
    let writer = TcpStream::connect(("127.0.0.1", port)).unwrap();
    writer.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    let mut control = Control { reader: BufReader::new(writer.try_clone().unwrap()), writer, key: std::fs::read_to_string(&key_path).unwrap() };
    assert_eq!(control.status()["enabled"], false);
    assert!(!temp.path().join("codex").exists(), "runtime starts only on request");
    control.execute("ui.assistantConnect", json!({"binary":"/missing/wordcraft-codex"}));
    wait(|| control.status()["status"].as_str().is_some_and(|s| s.contains("not found")));
    assert_eq!(control.status()["enabled"], false);
    for cycle in 0..3 {
        control.execute("ui.assistantConnect", json!({"binary":"/opt/homebrew/bin/codex"}));
        wait(|| control.status()["signedIn"] == false);
        assert_eq!(control.status()["enabled"], true);
        let children = descendants(app.0.id());
        assert_eq!(children.len(), 1, "one owned runtime");
        control.execute("text.insert", json!({"text":"Native coexistence"}));
        assert!(control.call("document.inspect", json!({})).to_string().contains("Native coexistence"));
        if cycle == 0 {
            control.execute("ui.assistant", json!({"open":true}));
            control.execute("ui.assistantNew", json!({}));
            wait(|| control.status()["thread"].is_string());
            let thread = control.status()["thread"].clone();
            control.execute("ui.assistantShareDocument", json!({}));
            wait(|| control.status()["sharing"].as_str().is_some_and(|s| s.starts_with("Sharing document")));
            control.execute("ui.assistantResume", json!({"thread":thread}));
            wait(|| control.status()["pending"] == false);
            assert_eq!(control.status()["sharing"], "Document sharing is off.");
            // Codex does not persist a zero-turn thread yet. Verify the recovery state, rather than claiming a successful resume.
            assert!(control.status()["status"].as_str().is_some_and(|s| s.contains("rejected")));
            control.execute("ui.assistantNew", json!({}));
            wait(|| control.status()["status"] == "Conversation ready.");
            if let Ok(path) = std::env::var("WORDCRAFT_CODEX_NATIVE_SHOT") {
                control.call("ui.screenshot", json!({"path":path}));
            }
            let cli = std::path::Path::new(env!("CARGO_BIN_EXE_wordcraft")).with_file_name("wordcraft-cli");
            let mut mcp = Owned(
                Command::new(cli)
                    .args(["mcp", "--connect", &format!("127.0.0.1:{port}")])
                    .env("WORDCRAFT_PROFILE_DIR", temp.path())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let mut input = mcp.0.stdin.take().unwrap();
            for msg in [
                json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute","arguments":{"command":"text.insert","params":{"text":"MCP coexistence"}}}}),
                json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"execute","arguments":{"command":"document.inspect","params":{}}}}),
            ] {
                writeln!(input, "{msg}").unwrap();
            }
            drop(input);
            let output = BufReader::new(mcp.0.stdout.take().unwrap()).lines().map(|s| s.unwrap()).collect::<Vec<_>>();
            assert!(mcp.0.wait().unwrap().success());
            assert!(output.iter().any(|s| s.contains("MCP coexistence")));
            assert!(control.call("document.inspect", json!({})).to_string().contains("MCP coexistence"));
            control.execute("ui.assistantDisconnect", json!({}));
            wait(|| control.status()["enabled"] == false);
            wait(|| children.iter().all(|p| !alive(*p)));
        } else if cycle == 1 {
            assert!(Command::new("/bin/kill").args(["-KILL", &children[0].to_string()]).status().unwrap().success());
            wait(|| control.status()["enabled"] == false);
            assert!(control.status()["status"].as_str().unwrap().contains("stopped"));
        } else {
            control.execute("file.close", json!({"force":true}));
            wait(|| app.0.try_wait().unwrap().is_some());
            wait(|| children.iter().all(|p| !alive(*p)));
            assert!(!key_path.exists());
        }
    }
}

#[test]
#[ignore = "requires macOS window session and built wordcraft-codex-fixture"]
fn native_slow_stream_repeated_turns_stop_and_retry() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_wordcraft")).with_file_name("wordcraft-codex-fixture");
    let binary = temp.path().join("codex");
    std::fs::copy(fixture, &binary).unwrap();
    std::fs::write(temp.path().join("mode"), "slow").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut app = Owned(
        Command::new(env!("CARGO_BIN_EXE_wordcraft"))
            .args(["--sample", "--control", &port.to_string()])
            .env("WORDCRAFT_PROFILE_DIR", temp.path().join("profile"))
            .env("WORDCRAFT_NO_PREFS", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let key = wordcraft_control_key::key_path(&temp.path().join("profile"), port);
    wait(|| key.exists());
    let writer = TcpStream::connect(("127.0.0.1", port)).unwrap();
    writer.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    let mut control = Control { reader: BufReader::new(writer.try_clone().unwrap()), writer, key: std::fs::read_to_string(key).unwrap() };
    control.execute("ui.assistant", json!({"open":true}));
    control.execute("ui.assistantConnect", json!({"binary":binary}));
    wait(|| control.status()["signedIn"] == false);
    control.execute("ui.assistantNew", json!({}));
    wait(|| control.status()["thread"].is_string());
    for count in [2, 4] {
        control.execute("ui.assistantSend", json!({"text":"Synthetic slow response"}));
        wait(|| control.status()["turn"].is_string());
        wait(|| control.status()["messageCount"] == count);
        assert_eq!(control.status()["pending"], true, "a partial response is visible before completion");
        wait(|| control.status()["pending"] == false);
    }
    control.execute("ui.assistantSend", json!({"text":"Stop this synthetic response"}));
    wait(|| control.status()["turn"].is_string());
    control.execute("ui.assistantStop", json!({}));
    wait(|| control.status()["pending"] == false);
    assert_eq!(control.status()["sharing"], "Document sharing is off.");
    control.execute("ui.assistantRetry", json!({}));
    wait(|| control.status()["turn"].is_string());
    wait(|| control.status()["pending"] == false);
    if let Ok(path) = std::env::var("WORDCRAFT_CODEX_STREAM_SHOT") {
        control.call("ui.screenshot", json!({"path":path}));
    }
    control.execute("file.close", json!({"force":true}));
    wait(|| app.0.try_wait().unwrap().is_some());
}
