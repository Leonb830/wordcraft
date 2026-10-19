//! Bounded stdio transport. Only the native application owns a Runtime.
use crate::{Error, Event, MAX_MESSAGE, Notice, RUNTIME_VERSION, Request};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const QUEUE: usize = 128;
/// A dedicated stdio MCP bridge owned by WordCraft; never a user-supplied registry.
#[derive(Clone, Debug)]
pub struct DocumentBridge {
    pub executable: PathBuf,
    pub args: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Launch {
    pub binary: PathBuf,
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub document_bridge: Option<DocumentBridge>,
    pub timeout: Duration,
}
enum Input {
    Request(Request),
    DocumentReply { id: Value, result: Value },
}
/// Dropping the owner stops and reaps the process, even if a queue is full.
pub struct Runtime {
    requests: SyncSender<Input>,
    events: Receiver<Event>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    disconnected: bool,
}
impl Runtime {
    pub fn start(launch: Launch) -> Result<Self, Error> {
        let (tx, requests) = mpsc::sync_channel(QUEUE);
        let (events, rx) = mpsc::sync_channel(QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("wordcraft-codex".into())
            .spawn(move || {
                if let Err(error) = run(launch, requests, &events, &shutdown) {
                    let _ = events.try_send(Event::Error(error));
                }
                let _ = events.try_send(Event::Stopped);
            })
            .map_err(|_| Error::MissingRuntime)?;
        Ok(Self { requests: tx, events: rx, stop, worker: Some(worker), disconnected: false })
    }
    pub fn send(&self, request: Request) -> Result<(), Error> {
        request.params()?;
        self.requests.try_send(Input::Request(request)).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => Error::Backpressure,
            mpsc::TrySendError::Disconnected(_) => Error::ChildExited,
        })
    }
    /// Only replies to a pending request from the dedicated WordCraft MCP relay.
    pub fn document_reply(&self, id: Value, result: Value) -> Result<(), Error> {
        if serde_json::to_vec(&result).map_err(|_| Error::Protocol)?.len() > MAX_MESSAGE / 2 {
            return Err(Error::MessageLimit);
        }
        self.requests.try_send(Input::DocumentReply { id, result }).map_err(|_| Error::Backpressure)
    }
    pub fn poll(&mut self) -> Option<Event> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) if !self.disconnected => {
                self.disconnected = true;
                Some(Event::Stopped)
            }
            Err(TryRecvError::Disconnected) => None,
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn emit(events: &SyncSender<Event>, event: Event) -> Result<(), Error> {
    events.try_send(event).map_err(|_| Error::Backpressure)
}
fn write(input: &mut impl Write, value: Value) -> Result<(), Error> {
    let bytes = serde_json::to_vec(&value).map_err(|_| Error::Protocol)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(Error::MessageLimit);
    }
    input.write_all(&bytes).and_then(|_| input.write_all(b"\n")).and_then(|_| input.flush()).map_err(|_| Error::ChildExited)
}
pub fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>, Error> {
    let mut bytes = Vec::new();
    let n = Read::by_ref(reader).take((MAX_MESSAGE + 1) as u64).read_until(b'\n', &mut bytes).map_err(|_| Error::Protocol)?;
    if n == 0 {
        return Ok(None);
    }
    if n > MAX_MESSAGE {
        return Err(Error::MessageLimit);
    }
    if bytes.last() != Some(&b'\n') {
        return Err(Error::Protocol);
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|_| Error::Protocol)
}
fn version(launch: &Launch, stop: &AtomicBool) -> Result<(), Error> {
    let mut child = OwnedChild(
        Command::new(&launch.binary)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::MissingRuntime)?,
    );
    let deadline = Instant::now() + launch.timeout.min(Duration::from_secs(5));
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(Error::ChildExited);
        }
        if let Some(status) = child.0.try_wait().map_err(|_| Error::ChildExited)? {
            if !status.success() {
                return Err(Error::UnsupportedVersion);
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        thread::sleep(Duration::from_millis(10));
    }
    let mut text = String::new();
    child.0.stdout.take().ok_or(Error::Protocol)?.take(256).read_to_string(&mut text).map_err(|_| Error::Protocol)?;
    if text.trim() != format!("codex-cli {RUNTIME_VERSION}") {
        return Err(Error::UnsupportedVersion);
    }
    Ok(())
}
fn prepare(launch: &Launch) -> Result<(), Error> {
    // The host must supply a WordCraft-owned home. Never accept the normal Codex home.
    if !launch.home.is_absolute() || !launch.cwd.is_absolute() {
        return Err(Error::Configuration);
    }
    if let Some(home) = std::env::var_os("HOME")
        && launch.home == PathBuf::from(home).join(".codex")
    {
        return Err(Error::Configuration);
    }
    if std::env::var_os("CODEX_HOME").is_some_and(|home| launch.home.as_os_str() == home) {
        return Err(Error::Configuration);
    }
    let globals = [
        std::env::var_os("CODEX_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")),
        std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".codex")),
    ];
    let globals = globals.into_iter().flatten().filter_map(|p| std::fs::canonicalize(p).ok()).collect::<Vec<_>>();
    for path in [&launch.home, &launch.cwd] {
        let actual = path.ancestors().find_map(|p| std::fs::canonicalize(p).ok()).ok_or(Error::Configuration)?;
        if globals.iter().any(|p| actual.starts_with(p)) {
            return Err(Error::Configuration);
        }
    }
    std::fs::create_dir_all(&launch.home).map_err(|_| Error::Configuration)?;
    std::fs::create_dir_all(&launch.cwd).map_err(|_| Error::Configuration)?;
    let actual = std::fs::canonicalize(&launch.home).map_err(|_| Error::Configuration)?;
    let globals = [
        std::env::var_os("CODEX_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")),
        std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".codex")),
    ];
    if globals.into_iter().flatten().filter_map(|p| std::fs::canonicalize(p).ok()).any(|p| actual.starts_with(p))
        || std::fs::symlink_metadata(launch.home.join("config.toml")).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(Error::Configuration);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launch.home, std::fs::Permissions::from_mode(0o700)).map_err(|_| Error::Configuration)?;
    }
    let mut config = String::from(
        "approval_policy = \"on-request\"\nsandbox_mode = \"read-only\"\nweb_search = \"disabled\"\ncli_auth_credentials_store = \"file\"\n[history]\npersistence = \"none\"\n[analytics]\nenabled = false\n[otel]\nlog_user_prompt = false\n",
    );
    config.push_str(include_str!("restricted-features.toml"));
    if let Some(bridge) = &launch.document_bridge {
        let command = serde_json::to_string(&bridge.executable).map_err(|_| Error::Configuration)?;
        let args = serde_json::to_string(&bridge.args).map_err(|_| Error::Configuration)?;
        config.push_str(&format!("\n[mcp_servers.wordcraft]\ncommand = {command}\nargs = {args}\nrequired = true\n"));
    }
    std::fs::write(launch.home.join("config.toml"), config).map_err(|_| Error::Configuration)
}
fn run(launch: Launch, requests: Receiver<Input>, events: &SyncSender<Event>, stop: &AtomicBool) -> Result<(), Error> {
    version(&launch, stop)?;
    prepare(&launch)?;
    let mut command = Command::new(&launch.binary);
    command.args(["app-server", "--listen", "stdio://"]).env_clear();
    for name in ["PATH", "HOME", "USERPROFILE", "USER", "TMPDIR", "TEMP", "TMP", "SystemRoot", "LOCALAPPDATA", "APPDATA", "LANG"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("RUST_LOG", "off")
        .env("CODEX_HOME", &launch.home)
        .current_dir(&launch.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = OwnedChild(command.spawn().map_err(|_| Error::MissingRuntime)?);
    let mut input = child.0.stdin.take().ok_or(Error::Protocol)?;
    let (outgoing, writes) = mpsc::sync_channel(QUEUE);
    let writer = thread::spawn(move || {
        while let Ok(value) = writes.recv() {
            if write(&mut input, value).is_err() {
                break;
            }
        }
    });
    let output = child.0.stdout.take().ok_or(Error::Protocol)?;
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let reader = thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let value = read_message(&mut reader);
            let finished = !matches!(value, Ok(Some(_)));
            if tx.try_send(value).is_err() || finished {
                break;
            }
        }
    });
    let result = pump(&outgoing, requests, events, stop, rx, launch.timeout);
    drop(outgoing);
    drop(child);
    let _ = reader.join();
    let _ = writer.join();
    result
}
fn pump(
    input: &SyncSender<Value>,
    requests: Receiver<Input>,
    events: &SyncSender<Event>,
    stop: &AtomicBool,
    rx: Receiver<Result<Option<Value>, Error>>,
    timeout: Duration,
) -> Result<(), Error> {
    enqueue(
        input,
        json!({"id":0,"method":"initialize","params":{"clientInfo":{"name":"wordcraft","title":"WordCraft","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false}}}),
    )?;
    let init_deadline = Instant::now() + timeout;
    let mut ready = false;
    let mut next = 1u64;
    let mut pending = BTreeMap::<u64, (Request, Instant)>::new();
    let mut document_pending = BTreeMap::<String, Instant>::new();
    while !stop.load(Ordering::Acquire) {
        if !ready && Instant::now() >= init_deadline {
            return Err(Error::Timeout);
        }
        if pending.values().any(|(_, deadline)| Instant::now() >= *deadline) || document_pending.values().any(|deadline| Instant::now() >= *deadline)
        {
            return Err(Error::Timeout);
        }
        if ready {
            match requests.try_recv() {
                Ok(Input::DocumentReply { id, result }) => {
                    if document_pending.remove(&id.to_string()).is_none() {
                        return Err(Error::Protocol);
                    }
                    enqueue(input, json!({"id":id,"result":{"action":"accept","content":{"result":result.to_string()}}}))?;
                }
                Ok(Input::Request(request)) => {
                    if pending.len() >= QUEUE {
                        return Err(Error::Backpressure);
                    }
                    let params = request.params()?;
                    enqueue(input, json!({"id":next,"method":request.method(),"params":params}))?;
                    pending.insert(next, (request, Instant::now() + timeout));
                    next = next.checked_add(1).ok_or(Error::Protocol)?;
                }
                Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => {}
            }
        }
        let value = match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(Ok(Some(value))) => value,
            Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Error::ChildExited),
            Ok(Err(error)) => return Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        };
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            if let Some(id) = value.get("id") {
                if method == "mcpServer/elicitation/request" {
                    let params = value.get("params").unwrap_or(&Value::Null);
                    if params.get("serverName").and_then(Value::as_str) == Some("wordcraft")
                        && params.get("mode").and_then(Value::as_str) == Some("form")
                        && let Some(call) = params.pointer("/_meta/wordcraftCall")
                        && let (Some(thread), Some(tool)) = (params.get("threadId").and_then(Value::as_str), call.get("name").and_then(Value::as_str))
                        && tool.len() <= 128
                        && thread.len() <= 128
                        && (id.is_number() || id.is_string())
                    {
                        if document_pending.len() >= 32 || document_pending.insert(id.to_string(), Instant::now() + timeout).is_some() {
                            return Err(Error::Backpressure);
                        }
                        emit(
                            events,
                            Event::DocumentRequest {
                                id: id.clone(),
                                thread: thread.into(),
                                turn: params.get("turnId").and_then(Value::as_str).filter(|s| s.len() <= 128).map(str::to_owned),
                                tool: tool.into(),
                                arguments: call.get("arguments").cloned().unwrap_or(Value::Null),
                            },
                        )?;
                        continue;
                    }
                }
                // Runtime requests cannot authorize document edits. All writes use WordCraft's gate.
                let result = match method {
                    "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => Some(json!({"decision":"decline"})),
                    "mcpServer/elicitation/request" => Some(json!({"action":"decline","content":null})),
                    "item/tool/requestUserInput" => Some(json!({"answers":{}})),
                    _ => None,
                };
                enqueue(
                    input,
                    match result {
                        Some(result) => json!({"id":id,"result":result}),
                        None => json!({"id":id,"error":{"code":-32601,"message":"unsupported request"}}),
                    },
                )?;
            } else if ready && let Some(kind) = Notice::from_method(method) {
                emit(events, Event::Notification { kind, params: value.get("params").cloned().unwrap_or(Value::Null) })?;
            }
            continue;
        }
        let id = value.get("id").and_then(Value::as_u64).ok_or(Error::Protocol)?;
        if id == 0 && !ready {
            if value.get("result").is_none() {
                return Err(Error::Protocol);
            }
            enqueue(input, json!({"method":"initialized","params":{}}))?;
            ready = true;
            emit(events, Event::Ready)?;
            continue;
        }
        let Some((request, _)) = pending.remove(&id) else { return Err(Error::Protocol) };
        if value.get("error").is_some() {
            emit(events, Event::Error(Error::Rejected))?;
        } else {
            emit(events, Event::Response { id, request, result: value.get("result").cloned().ok_or(Error::Protocol)? })?;
        }
    }
    Ok(())
}

fn enqueue(input: &SyncSender<Value>, value: Value) -> Result<(), Error> {
    input.try_send(value).map_err(|_| Error::Backpressure)
}
