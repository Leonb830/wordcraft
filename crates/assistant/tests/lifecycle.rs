#![cfg(all(not(target_arch = "wasm32"), feature = "test-support"))]
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wordcraft_assistant::{
    Error, Event, Notice, Request,
    native::{Launch, Runtime},
};

fn fixture(mode: &str) -> (tempfile::TempDir, Launch) {
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join(if cfg!(windows) { "codex.exe" } else { "codex" });
    std::fs::copy(env!("CARGO_BIN_EXE_wordcraft-codex-fixture"), &binary).unwrap();
    std::fs::write(temp.path().join("mode"), mode).unwrap();
    let launch =
        Launch { binary, home: temp.path().join("home"), cwd: temp.path().join("cwd"), document_bridge: None, timeout: Duration::from_secs(3) };
    (temp, launch)
}
fn event(runtime: &mut Runtime) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(event) = runtime.poll() {
            return event;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for fixture")
}
#[test]
fn handshake_thread_stream_resume_interrupt_and_cleanup() {
    let (_temp, launch) = fixture("normal");
    let mut runtime = Runtime::start(launch).unwrap();
    assert!(matches!(event(&mut runtime), Event::Ready));
    for request in [Request::StartThread, Request::ResumeThread { thread_id: "fixture-thread".into() }] {
        runtime.send(request).unwrap();
        assert!(matches!(event(&mut runtime), Event::Response { .. }));
    }
    runtime.send(Request::StartTurn { thread_id: "fixture-thread".into(), text: "synthetic".into() }).unwrap();
    assert!(matches!(event(&mut runtime), Event::Response { .. }));
    for _ in 0..2 {
        assert!(matches!(event(&mut runtime), Event::Notification { kind: Notice::Delta, .. }));
    }
    runtime.send(Request::Interrupt { thread_id: "fixture-thread".into(), turn_id: "fixture-turn".into() }).unwrap();
    assert!(matches!(event(&mut runtime), Event::Response { .. }));
    assert!(matches!(event(&mut runtime), Event::Notification { kind: Notice::TurnCompleted, .. }));
    let start = Instant::now();
    drop(runtime);
    assert!(start.elapsed() < Duration::from_secs(2));
    #[cfg(unix)]
    {
        let pid = std::fs::read_to_string(_temp.path().join("pid")).unwrap();
        assert!(!std::process::Command::new("/bin/kill").args(["-0", pid.trim()]).stderr(std::process::Stdio::null()).status().unwrap().success());
    }
}
#[test]
fn malformed_oversized_exit_and_unsupported_are_structured() {
    for (mode, expected) in [
        ("invalid", Error::Protocol),
        ("truncated", Error::Protocol),
        ("oversize", Error::MessageLimit),
        ("exit", Error::ChildExited),
        ("unsupported", Error::UnsupportedVersion),
        ("hang", Error::Timeout),
    ] {
        let (_temp, launch) = fixture(mode);
        let mut runtime = Runtime::start(launch).unwrap();
        match event(&mut runtime) {
            Event::Error(error) => assert_eq!(error, expected, "{mode}"),
            other => panic!("{mode}: {other:?}"),
        }
    }
}
#[test]
fn unknown_binary_returns_structured_error() {
    let (_temp, mut launch) = fixture("normal");
    launch.binary = PathBuf::from("/wordcraft-deliberately-missing-codex");
    let mut runtime = Runtime::start(launch).unwrap();
    assert!(matches!(event(&mut runtime), Event::Error(Error::MissingRuntime)));
}
#[test]
fn duplicate_response_and_request_timeout_fail_closed() {
    for (mode, expected) in [("duplicate", Error::Protocol), ("timeout", Error::Timeout)] {
        let (_temp, launch) = fixture(mode);
        let mut runtime = Runtime::start(launch).unwrap();
        assert!(matches!(event(&mut runtime), Event::Ready));
        runtime.send(Request::Account).unwrap();
        if mode == "duplicate" {
            assert!(matches!(event(&mut runtime), Event::Response { .. }));
        }
        match event(&mut runtime) {
            Event::Error(error) => assert_eq!(error, expected),
            other => panic!("{other:?}"),
        }
    }
}
#[test]
fn blocked_stdin_cannot_block_shutdown() {
    let (_temp, launch) = fixture("blocked-write");
    let mut runtime = Runtime::start(launch).unwrap();
    assert!(matches!(event(&mut runtime), Event::Ready));
    for _ in 0..16 {
        runtime.send(Request::StartTurn { thread_id: "t".into(), text: "x".repeat(60_000) }).unwrap();
    }
    std::thread::sleep(Duration::from_millis(50));
    let start = Instant::now();
    drop(runtime);
    assert!(start.elapsed() < Duration::from_secs(2));
    #[cfg(unix)]
    {
        let pid = std::fs::read_to_string(_temp.path().join("pid")).unwrap();
        assert!(!std::process::Command::new("/bin/kill").args(["-0", pid.trim()]).stderr(std::process::Stdio::null()).status().unwrap().success());
    }
}
#[test]
fn raw_provider_errors_are_not_reflected() {
    let (_temp, launch) = fixture("normal");
    let mut runtime = Runtime::start(launch).unwrap();
    assert!(matches!(event(&mut runtime), Event::Ready));
    runtime.send(Request::Login).unwrap();
    match event(&mut runtime) {
        Event::Error(error) => assert!(!error.to_string().contains("SENSITIVE")),
        other => panic!("{other:?}"),
    }
}
