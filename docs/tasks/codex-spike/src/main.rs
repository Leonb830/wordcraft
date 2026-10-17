//! T1 runtime qualification probe. Synthetic data only; no credentials or payload logging.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

type Error = Box<dyn std::error::Error + Send + Sync>;
struct Server {
    child: Child,
    input: Option<ChildStdin>,
    events: Receiver<Value>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            if Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    fn send(&mut self, value: Value) -> Result<(), Error> {
        let input = self.input.as_mut().ok_or("stdin unavailable")?;
        serde_json::to_writer(&mut *input, &value)?;
        input.write_all(b"\n")?;
        input.flush()?;
        Ok(())
    }
    fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, Error> {
        self.send(json!({"id": id, "method": method, "params": params}))?;
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let v = self.events.recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
            if v.get("id") == Some(&json!(id)) {
                if v.get("error").is_some() {
                    return Err(format!("{method}: protocol error (payload withheld)").into());
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err(format!("{method}: timeout").into())
    }
}
fn mcp_fixture() -> Result<(), Error> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut output = std::io::stdout().lock();
    let write = |output: &mut std::io::StdoutLock<'_>, value: Value| -> Result<(), Error> {
        serde_json::to_writer(&mut *output, &value)?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    };
    while let Some(line) = lines.next() {
        let request: Value = serde_json::from_str(&line?)?;
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let result = match request.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"wordcraft-fixture","version":"0.1.0"}})
            }
            Some("tools/list") => {
                json!({"tools":[{"name":"propose_edit","description":"Synthetic approval probe. Does not edit a real document.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}]})
            }
            Some("tools/call") => {
                write(
                    &mut output,
                    json!({"jsonrpc":"2.0","id":10001,"method":"elicitation/create","params":{"message":"Approve the synthetic WordCraft edit?","requestedSchema":{"type":"object","properties":{"approve":{"type":"boolean"}},"required":["approve"]}}}),
                )?;
                let mut approved = false;
                for line in lines.by_ref() {
                    let answer: Value = serde_json::from_str(&line?)?;
                    if answer.get("id") == Some(&json!(10001)) {
                        approved = answer.pointer("/result/action").and_then(Value::as_str) == Some("accept")
                            && answer.pointer("/result/content/approve").and_then(Value::as_bool) == Some(true);
                        break;
                    }
                }
                json!({"content":[{"type":"text","text":if approved {"synthetic edit approved"} else {"synthetic edit denied"}}],"isError":!approved})
            }
            Some("ping") => json!({}),
            _ => {
                write(&mut output, json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"not allowlisted"}}))?;
                continue;
            }
        };
        write(&mut output, json!({"jsonrpc":"2.0","id":id,"result":result}))?;
    }
    Ok(())
}
fn model_fixture(listener: TcpListener, tx: mpsc::Sender<Vec<String>>) -> Result<(), Error> {
    for sequence in 0..2 {
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => return Err(e.into()),
            }
        };
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse()?;
            }
        }
        if length == 0 || length > 8 * 1024 * 1024 {
            return Err("invalid request size".into());
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        let request: Value = serde_json::from_slice(&body)?;
        tx.send(
            request
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|t| t.get("name").or_else(|| t.get("type")).and_then(Value::as_str).unwrap_or("unknown").to_owned())
                .collect(),
        )?;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")?;
        let mut event = |kind: &str, mut value: Value| -> Result<(), Error> {
            value["type"] = json!(kind);
            write!(stream, "event: {kind}\ndata: {value}\n\n")?;
            stream.flush()?;
            Ok(())
        };
        event("response.created", json!({"response":{"id":"resp_fixture","status":"in_progress"}}))?;
        let item = json!({"id":"msg_fixture","type":"message","role":"assistant","content":[]});
        event("response.output_item.added", json!({"output_index":0,"item":item}))?;
        event(
            "response.content_part.added",
            json!({"item_id":"msg_fixture","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        )?;
        event("response.output_text.delta", json!({"item_id":"msg_fixture","output_index":0,"content_index":0,"delta":"Hello "}))?;
        if sequence == 1 {
            std::thread::sleep(Duration::from_secs(4));
            continue;
        }
        std::thread::sleep(Duration::from_millis(100));
        event("response.output_text.delta", json!({"item_id":"msg_fixture","output_index":0,"content_index":0,"delta":"WordCraft"}))?;
        let item = json!({"id":"msg_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Hello WordCraft","annotations":[]}]});
        event("response.output_item.done", json!({"output_index":0,"item":item}))?;
        event(
            "response.completed",
            json!({"response":{"id":"resp_fixture","status":"completed","output":[item],"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3}}}),
        )?;
    }
    Ok(())
}
fn main() -> Result<(), Error> {
    if std::env::args().any(|arg| arg == "--mcp") {
        return mcp_fixture();
    }
    if std::env::args().any(|arg| arg == "--auth-check") {
        return auth_check();
    }
    let root = std::env::current_dir()?;
    let home = root.join("isolated-home");
    let cwd = root.join("empty-workspace");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&cwd)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = mpsc::channel();
    let mock = std::thread::spawn(move || model_fixture(listener, tx));
    let config = std::fs::read_to_string(root.join("restricted.toml"))?.replace("PORT", &port.to_string());
    let config = format!(
        "{config}\n[mcp_servers.wordcraft]\ncommand = {}\nargs = [\"--mcp\"]\nenabled_tools = [\"propose_edit\"]\n",
        serde_json::to_string(&std::env::current_exe()?.to_string_lossy())?
    );
    std::fs::write(home.join("config.toml"), config)?;
    let mut server = spawn_server(&home, &cwd)?;
    server.request(
        1,
        "initialize",
        json!({"clientInfo":{"name":"wordcraft_qualification","version":"0.1.0"},"capabilities":{"experimentalApi":false}}),
    )?;
    server.send(json!({"method":"initialized","params":{}}))?;
    println!("PASS: stable initialize/initialized");
    let account = server.request(2, "account/read", json!({"refreshToken":false}))?;
    println!("isolated account present: {}", account.get("account").is_some_and(|a| !a.is_null()));
    let thread = server.request(3, "thread/start", json!({"cwd":cwd,"approvalPolicy":"on-request","sandbox":"read-only"}))?;
    let id = thread.pointer("/thread/id").and_then(Value::as_str).ok_or("missing thread id")?.to_owned();
    println!("PASS: thread/start");
    server.request(4, "turn/start", json!({"threadId":id,"input":[{"type":"text","text":"Synthetic qualification. Reply Hello WordCraft."}]}))?;
    let names = rx.recv_timeout(Duration::from_secs(45))?;
    println!("Advertised tools: {}", serde_json::to_string(&names)?);
    if names.iter().any(|name| {
        !["request_user_input", "mcp__wordcraft", "list_mcp_resources", "list_mcp_resource_templates", "read_mcp_resource"].contains(&name.as_str())
    }) {
        return Err("unexpected tool advertised".into());
    }
    let mut deltas = 0;
    loop {
        let event = server.events.recv_timeout(Duration::from_secs(20))?;
        if event.get("method") == Some(&json!("item/agentMessage/delta")) {
            deltas += 1;
        }
        if event.get("method") == Some(&json!("turn/completed")) {
            if event.pointer("/params/turn/status") != Some(&json!("completed")) {
                return Err("turn did not complete".into());
            }
            break;
        }
    }
    if deltas < 2 {
        return Err("incremental text was not observed".into());
    }
    println!("PASS: incremental text ({deltas} deltas), turn/completed");
    server.request(5, "thread/resume", json!({"threadId":id}))?;
    println!("PASS: thread/resume");
    // Explicit MCP round trip exercises the stable server approval route without model inference.
    server.send(json!({"id":6,"method":"mcpServer/tool/call","params":{"threadId":id,"server":"wordcraft","tool":"propose_edit","arguments":{}}}))?;
    let mut approved = false;
    loop {
        let event = server.events.recv_timeout(Duration::from_secs(20))?;
        if event.get("method") == Some(&json!("mcpServer/elicitation/request")) {
            server.send(json!({"id":event.get("id"),"result":{"action":"accept","content":{"approve":true}}}))?;
            approved = true;
        }
        if event.get("id") == Some(&json!(6)) {
            if !approved || event.get("error").is_some() {
                println!("MCP response shape: {}", event);
                return Err("MCP approval roundtrip failed".into());
            }
            break;
        }
    }
    println!("PASS: MCP approval round trip");
    let turn = server.request(7, "turn/start", json!({"threadId":id,"input":[{"type":"text","text":"Synthetic interruption qualification."}]}))?;
    let tid = turn.pointer("/turn/id").and_then(Value::as_str).ok_or("missing turn id")?;
    let _ = rx.recv_timeout(Duration::from_secs(30))?;
    server.request(8, "turn/interrupt", json!({"threadId":id,"turnId":tid}))?;
    println!("PASS: turn/interrupt acknowledged");
    server.child.kill()?;
    server.child.wait()?;
    if server.request(9, "account/read", json!({"refreshToken":false})).is_ok() {
        return Err("child exit was not reported".into());
    }
    println!("PASS: child exit returns an error");
    drop(server);
    mock.join().map_err(|_| "fixture failed")??;
    println!("PASS: process cleanup");
    Ok(())
}

fn spawn_server(home: &std::path::Path, cwd: &std::path::Path) -> Result<Server, Error> {
    let version = Command::new(std::env::var_os("WORDCRAFT_CODEX_BINARY").unwrap_or_else(|| "codex".into())).arg("--version").output()?;
    check_version(std::str::from_utf8(&version.stdout)?)?;
    let mut child = Command::new(std::env::var_os("WORDCRAFT_CODEX_BINARY").unwrap_or_else(|| "codex".into()))
        .args(["app-server", "--listen", "stdio://"])
        .env("CODEX_HOME", home)
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let input = child.stdin.take().ok_or("missing stdin")?;
    let output = child.stdout.take().ok_or("missing stdout")?;
    let (etx, events) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        while let Ok(Some(value)) = decode_line(&mut reader, 1024 * 1024) {
            if etx.send(value).is_err() {
                break;
            }
        }
    });
    let server = Server { child, input: Some(input), events };
    Ok(server)
}
fn auth_check() -> Result<(), Error> {
    let root = std::env::current_dir()?;
    let home = root.join("auth-fixture-home");
    let cwd = root.join("empty-workspace");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&cwd)?;
    std::fs::write(
        home.join("config.toml"),
        "cli_auth_credentials_store = \"ephemeral\"\n[features]\napps = false\nplugins = false\nshell_tool = false\n",
    )?;
    let mut server = spawn_server(&home, &cwd)?;
    server.request(1, "initialize", json!({"clientInfo":{"name":"wordcraft_auth_qualification","version":"0.1.0"}}))?;
    server.send(json!({"method":"initialized","params":{}}))?;
    let account = server.request(2, "account/read", json!({"refreshToken":false}))?;
    if account.get("account") != Some(&Value::Null) || account.get("requiresOpenaiAuth") != Some(&json!(true)) {
        return Err("unexpected isolated auth state".into());
    }
    println!("PASS: signed-out state, OpenAI auth required");
    let login = server.request(3, "account/login/start", json!({"type":"chatgpt"}))?;
    let login_id = login.get("loginId").and_then(Value::as_str).ok_or("missing login id")?;
    if !login.get("authUrl").and_then(Value::as_str).is_some_and(|url| url.starts_with("https://auth.openai.com/")) {
        return Err("unexpected login URL origin".into());
    }
    println!("PASS: managed browser login available (URL withheld)");
    server.request(4, "account/login/cancel", json!({"loginId":login_id}))?;
    println!("PASS: login cancellation");
    server.request(5, "account/logout", json!({}))?;
    println!("PASS: isolated sign-out");
    drop(server);
    println!("PASS: auth child cleanup");
    Ok(())
}

fn decode_line(reader: &mut impl BufRead, limit: usize) -> Result<Option<Value>, Error> {
    let mut bytes = Vec::new();
    let n = Read::by_ref(reader).take(limit.saturating_add(1) as u64).read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Ok(None);
    }
    if n > limit {
        return Err("event exceeds limit".into());
    }
    if bytes.last() != Some(&b'\n') {
        return Err("incomplete event".into());
    }
    Ok(Some(serde_json::from_slice(&bytes)?))
}
fn check_version(value: &str) -> Result<(), Error> {
    if value.trim() == "codex-cli 0.162.1" { Ok(()) } else { Err("unsupported Codex version; qualified version is 0.162.1".into()) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_reads_and_multiple_events() {
        let stream = b"{\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"hello\"}}\n{\"method\":\"turn/completed\"}\n";
        let mut reader = BufReader::with_capacity(2, &stream[..]);
        assert_eq!(decode_line(&mut reader, 1024).unwrap().unwrap()["method"], "item/agentMessage/delta");
        assert_eq!(decode_line(&mut reader, 1024).unwrap().unwrap()["method"], "turn/completed");
        assert!(decode_line(&mut reader, 1024).unwrap().is_none());
    }
    #[test]
    fn malformed_truncated_oversized_events_fail() {
        for (data, limit) in [(b"not-json\n".as_slice(), 100), (b"{}".as_slice(), 100), (b"{\"long\":true}\n".as_slice(), 4)] {
            assert!(decode_line(&mut BufReader::new(data), limit).is_err());
        }
    }
    #[test]
    fn unsupported_versions_fail_closed() {
        assert!(check_version("codex-cli 0.162.1\n").is_ok());
        for version in ["", "codex-cli 0.161.0", "codex-cli 0.163.0", "garbage"] {
            assert!(check_version(version).is_err());
        }
    }
    #[test]
    fn missing_binary_returns_error() {
        let missing = std::env::temp_dir().join("wordcraft-deliberately-missing-codex-1f4b8c");
        assert!(Command::new(missing).spawn().is_err());
    }
}
