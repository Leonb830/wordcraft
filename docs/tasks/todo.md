# WordCraft in-app Codex integration — task checklist

Ordered implementation checklist corresponding to [plan.md](plan.md). Working implementation checklist in the public fork `Leonb830/wordcraft`, branch `codex/wordcraft-vnext`. Base refreshed at `eae242176108d328c5dd81fbc10370c51ab90706` on 2026-10-16. The original supplied planning artifacts remain unchanged.

## Foundation

- [x] **T0 — Reconcile assistant work and record ADR** (2–4 h)
  - [x] Re-check and compare open PR #815, PR #178, and current `main`.
  - [x] Select the implementation base and explicitly preserve existing CLI/MCP/external-agent behavior.
  - [x] Record Codex app-server, runtime, auth, desktop/WASM scope, and additive migration decisions.
  - **Checkpoint:** implementation base and ownership of overlapping files are documented.

- [x] **T1 — Validate app-server contract and runtime availability** (4–6 h)
  - [x] Prototype supported handshake, thread/turn lifecycle, streamed events, interruption, and tool approval.
  - [x] Establish protocol/version compatibility and supported auth/runtime behavior.
  - [x] Decide installed-runtime detection versus managed/pinned runtime distribution.
  - **Checkpoint:** compatibility matrix and working in-app integration proof.

## Product implementation

- [x] **T2 — Add Codex client and process lifecycle** (6–10 h; depends on T1)
  - [x] Implement typed stdio JSONL client and bounded event/request handling.
  - [x] Add startup, cancellation, error, shutdown, and child cleanup behavior.
  - [x] Keep native process code out of WASM.
  - **Checkpoint:** protocol tests pass using a fake app-server; no orphan process on shutdown.

- [x] **T3 — Define narrow WordCraft document tools** (4–6 h; depends on T0, T2)
  - [x] Add active-document read tools and selection context.
  - [x] Enforce a server-side allowlist; exclude general files, shell, UI controls, screenshots, save/export.
  - [x] Keep the existing MCP profile and behavior unchanged.
  - **Checkpoint:** allowlist tests pass and excluded tool classes are denied.

- [x] **T4 — Add approval and request-scoped edit review** (6–10 h; depends on T0, T3)
  - [x] Gate every document write behind explicit user approval.
  - [x] Tag edits by Codex author, thread/request, and document revision.
  - [x] Add request-scoped accept/reject/undo if existing review APIs cannot isolate them.
  - [x] Reject or refresh stale proposals without modifying newer user work.
  - **Checkpoint:** engine tests show Codex review leaves unrelated user changes untouched.

- [x] **T5 — Host the service in the native app** (4–6 h; depends on T2, T3)
  - [x] Connect service lifecycle to `WordApp` and native app startup/shutdown.
  - [x] Add runtime detection/management and in-app status/error reporting.
  - [x] Verify coexistence with existing control server, CLI, and MCP.
  - **Checkpoint:** native service starts, stops, and recovers without changing current agent paths.

- [ ] **T6 — Build assistant pane and conversation basics** (6–10 h; depends on T2, T5)
  - [ ] Add Assistant to the existing side-pane framework.
  - [ ] Add composer, new/resume thread flow, and selection/document scope indicator.
  - [ ] Add empty, loading, missing-runtime, and unsupported-target states.
  - **Checkpoint:** a user can start/resume a thread and send a prompt in-app.

- [ ] **T7 — Add streaming, action cards, approvals, and stop** (5–8 h; depends on T4, T6)
  - [ ] Render streamed response and tool progress.
  - [ ] Add action preview/approval cards connected to T4's edit gate.
  - [ ] Add stop, retry, disconnect, and recoverable error behavior.
  - **Checkpoint:** no UI or protocol event can bypass the write approval path.

- [ ] **T8 — Add auth, thread references, settings, and privacy controls** (4–7 h; depends on T1, T5, T6)
  - [ ] Implement selected supported sign-in/status behavior.
  - [ ] Persist only necessary thread references and UI settings.
  - [ ] Add disable/clear-reference controls and verify logs exclude content/secrets.
  - **Checkpoint:** signed-out, expired-auth, resume, and clear states work.

- [ ] **T9 — Complete runtime distribution and platform behavior** (4–8 h; depends on T1, T5, T8)
  - [ ] Implement runtime packaging or discovery decision for supported native platforms.
  - [ ] Keep unsupported targets usable and the WASM build intact.
  - [ ] Add package/version checks and actionable recovery instructions.
  - **Checkpoint:** native support matrix is verified and `cargo xtask wasm` passes.

## Release

- [ ] **T10 — User setup and help documentation** (2–3 h; depends on T2–T9)
  - [ ] Document runtime setup, sign-in, context sharing, approval/review, supported targets, and troubleshooting.
  - [ ] Keep CLI/MCP docs accurate and describe how the in-app assistant relates to those paths.
  - **Checkpoint:** a new user can follow the docs without a terminal chat workflow.

- [ ] **T11 — Release verification and roadmap update** (1–2 h; depends on T2–T10)
  - [ ] Update architecture and roadmap/gap status.
  - [ ] Run `cargo xtask ci` and native end-to-end checks.
  - [ ] Recheck CLI/MCP and external-agent workflows.
  - **Checkpoint:** tested release candidate with no deleted existing integration paths.

**Rough total:** 48–80 engineering hours, subject to T0/T1 decisions and PR reuse. This is a planning estimate, not a committed schedule.

## Verification evidence

### T0 — complete (2026-10-16)

- `gh api user --jq '{login: .login}'`: authenticated as Leonb830.
- `gh api --method POST repos/storytold/wordcraft/forks -f name=wordcraft -F default_branch_only=false`: created public Leonb830/wordcraft; fork=true, private=false, parent=storytold/wordcraft.
- `git clone https://github.com/Leonb830/wordcraft.git work/wordcraft`: exit 0, full clone.
- `git switch -c codex/wordcraft-vnext`: created dedicated branch. Upstream push URL set to DISABLED.
- `gh api repos/storytold/wordcraft/commits/main`: eae242176108d328c5dd81fbc10370c51ab90706, unchanged from plan.
- `gh pr view 815 --repo storytold/wordcraft --json body,files,reviews,mergeable,mergeStateStatus` and the same command for 178: both OPEN, unmerged, no reviews, CONFLICTING/DIRTY. Heads pinned in ADR.
- `git fetch upstream main pull/815/head:refs/remotes/upstream/pr-815 pull/178/head:refs/remotes/upstream/pr-178`: exit 0.
- `git log --oneline upstream/main..upstream/pr-815` and corresponding pr-178: inspected commits.
- `git diff --stat upstream/main...upstream/pr-815`: 26 files, +3578/-15; corresponding pr-178: 59 files, +9479/-129.
- Inspected PR #815 assistant lib/guard contracts and #178 chat_gate allowlist against main session and layers. ADR records incompatibilities, reuse boundaries, file ownership, runtime/auth defaults and owners.
- `git rev-parse --is-shallow-repository`: false.
- `git merge-base --is-ancestor eae242176108d328c5dd81fbc10370c51ab90706 HEAD`: exit 0.
- `git diff --check`: exit 0.
- Reviewed ADR against checked-in AGENTS.md, architecture and layer rules. Shared craftrules returns 404; user explicitly authorized using checked-in rules alone.
- No product code changed. Runtime and product verification belong to the subsequent tasks and are not claimed here.

### T1 — complete protocol qualification (2026-10-16)

- `codex --version`: codex-cli 0.162.1.
- `codex app-server generate-json-schema --out work/codex-spike/schema`: exit 0; stable v2 bundle preserved in `codex-spike/schema/0.162.1.json`.
- Commands below ran from `docs/tasks/codex-spike`, using task-local Rust 1.95.0, CARGO_HOME and RUSTUP_HOME.
- `cargo test`: exit 0, 4 tests passed (partial/malformed/oversized framing, version rejection, missing binary).
- `cargo clippy --all-targets -- -D warnings`: exit 0. Initial prototype lint failures were corrected before this successful rerun.
- `cargo run`: exit 0. Real Codex stable handshake, synthetic thread start/resume, two streamed deltas and completed turn, restricted advertised tool classes, MCP approval round trip, interruption acknowledgement, child-exit error and cleanup.
- `cargo run -- --auth-check`: exit 0. Isolated signed-out state, managed browser login URL origin verified without disclosure, login cancellation, isolated logout, cleanup.
- Separate read-only normal-profile account probe: account present, type chatgpt, requiresOpenaiAuth=true; credentials were neither read nor copied.
- `approval_policy=untrusted` failed at runtime despite appearing in generated schema; qualification uses on-request plus independent document approval.
- Compatibility matrix and exact-version installed-runtime recommendation: `codex-spike/README.md`. This is a protocol proof with local mock inference, not a hosted inference or product end-to-end test. Dedicated-profile sign-in and live native flow remain required in T5/T8/T11.

### Repository baseline before product changes

- Installed Rust 1.95.0, rustfmt, clippy and wasm32-unknown-unknown under this task's `work/toolchain`; shell profile and global Codex configuration unchanged.
- `cargo xtask ci`: exit 1 on unchanged product source. fmt passed; clippy stopped at existing `crates/proof/src/affix.rs:288` (`clippy::collapsible_match`). Tests/assets/layers/wasm were skipped by xtask after that failure. This is not a passing CI result. Revisit the baseline warning before final release verification.

### T2 — complete (2026-10-17)

- Added L5 `wordcraft-assistant`, fixed request enum, bounded JSONL transport/queues, exact runtime version check, timeouts, error redaction and native-only process worker. UI remains free of process I/O.
- Dedicated WordCraft profile refuses the global Codex home and symlinked config, disables broad tool features, clears inherited credential environment variables, and never logs stderr/protocol payloads.
- `cargo test -p wordcraft-assistant --features test-support`: exit 0; 2 unit + 6 process integration tests pass. Initial test startup timeout was increased from 500 ms to 3 s for cold copied macOS executable startup; blocked-write shutdown still has a 2 s assertion.
- Fixtures cover partial output, malformed/truncated/oversized messages, unknown binary, unsupported version, handshake/request timeout, child exit, duplicate response, thread start/resume, streamed text, interrupt, non-reflection of sensitive error text, and shutdown while child stdin is blocked. `/bin/kill -0` confirms exited test children are reaped.
- `cargo clippy -p wordcraft-assistant --all-targets --features test-support -- -D warnings`: exit 0 after corrections.
- `cargo check -p wordcraft-assistant --target wasm32-unknown-unknown --lib`: exit 0.
- `cargo xtask layers`: exit 0; 21 crates, no violations.
- Repository CI remains separately recorded; focused success does not imply workspace CI success.

### T3 — complete (2026-10-17)

- Added a separate `CodexProfile` with five read tools, explicit host-only consent, captured selection/document scope, document identity/revision checks, revocation and context/query limits. Broad MCP dispatch and resource reads are never called. No write schemas exist yet.
- `cargo test -p wordcraft-mcp`: exit 0; 16 unit tests and 5 integration tests passed, including all existing MCP/automation-root tests and 5 new scope/contract tests.
- `cargo run -p wordcraft --example codex_profile_smoke`: exit 0; actual native WordApp rendered three frames, then verified all five reads, private text exclusion, rejection of every engine registry command, and stale-revision rejection against its live Session. Synthetic data only, no user files/preferences. The test exits its own process to avoid the known macOS window-drop issue.
- `cargo clippy -p wordcraft-mcp --all-targets --no-deps -- -D warnings`: exit 0.
- `cargo fmt --all --check`: exit 0.
- `cargo xtask ci`: exit 1; formatting passes, unchanged baseline `crates/proof/src/affix.rs:288` Clippy error remains; later CI stages skipped.
- Excluded file/shell/UI/screenshot/save/export tool classes and resource routes denied in tests; original MCP default surface unchanged. Native test uses the separate in-process profile; runtime service wiring belongs to T5.

### T4 — complete (2026-10-17)

- Product code is restricted to the four planned files. Proposals do not mutate the document; host-only approve/deny UI commands check identity and revision again. Approved text gets fresh Codex insertion/deletion revision IDs, one undo step, and document custom-property audit metadata with thread/request/base revision. User author and track-changes preferences are preserved; save remains manual.
- Initial mutation scope is bounded plain-text replacement within one paragraph. Objects, existing overlapping revisions, protected documents, content-control stories and stale proposals are refused. This conservative scope avoids pretending that arbitrary engine commands are safe for Codex.
- `cargo test -p wordcraft-engine --lib codex_`: exit 0, 8 tests passed; approve/deny/replay, stale/document replacement, ordinary undo, scoped accept/reject/undo, two separate requests, unrelated human tracked changes including the same paragraph, accepted undo and overlap rejection.
- `cargo test -p wordcraft-engine --lib hostile_params_never_panic`: exit 0, 1 test passed (all registered commands, including the new scoped review commands).
- `WORDCRAFT_CODEX_REVIEW_SHOT=<task outputs>/codex-review.png cargo test --locked -p wordcraft-ui-egui --lib codex_cards`: exit 0, 4 tests passed; approval/accept/undo, deny/stale, reject, and full WordApp screenshot. Screenshot inspected: original/proposed text, revision, manual-save notice and explicit buttons visible without clipping.
- `cargo clippy -p wordcraft-engine -p wordcraft-ui-egui --all-targets --no-deps -- -D warnings -A clippy::nonminimal_bool`: exit 0. The allowance isolates pre-existing boolean warnings in `ui-egui/src/objects.rs:248`; the unmodified strict command failed there. A new test-module ordering lint was corrected.
- `cargo xtask ci` remains a separate baseline failure, not a passing full-workspace check. Final pre-commit result recorded below.
- Review state is session-local; persisted document metadata identifies the request, and tracked changes remain Codex-authored. Undo after acceptance refuses changes to the affected paragraph rather than restoring an old whole-document snapshot.
- Verification was delayed by iCloud evicting task-local toolchain/build files. Download requests and Finder Keep Downloaded were applied only to this task's toolchain and checkout. The disposable registry index was preserved as `index-icloud-backup` and refreshed. Stalled intermediate builds were terminated; the final test run above completed successfully.
- Final pre-commit `cargo xtask ci`: exit 1; fmt passes, Clippy fails on unchanged `proof/src/affix.rs:288` and `doc/src/math_linear.rs:559`; later stages skipped. An interrupted-formatting check was fixed and rerun before this result.


### T5 — complete (2026-10-17)

- Native host starts only on an explicit UI action, detects the pinned installed runtime, reports connection/sign-in/errors, and revokes sharing on disconnect/thread change. Both ordinary exit and the special macOS termination path stop and reap the runtime.
- Added a dedicated MCP helper using stable elicitation over private stdio. Only five read tools are exposed; the host checks active thread and shared document identity/revision. No socket, control key, global configuration or context-file bridge is used.
- `cargo test -p wordcraft-assistant --features test-support`: exit 0; 2 unit and 8 integration tests, including dedicated/foreign-server routing and rejection of unsolicited replies.
- `cargo test -p wordcraft --test codex_bridge -- --ignored`: exit 0; actual installed Codex 0.162.1 and actual WordCraft helper complete a read round trip, exclude private text, deny file.save, and refuse mismatched context. No hosted inference or copied credentials.
- `cargo build -p wordcraft-cli`: exit 0.
- `cargo test -p wordcraft --test codex_host -- --ignored`: exit 0; actual macOS WordCraft window with isolated profile. Verified no automatic startup, missing runtime, signed-out status, disable/restart, killed child recovery, existing authenticated control and CLI MCP edits while Codex runs, native close, child reaping and control-key removal. The first harness run failed on a repeated synthetic selection; the harness was corrected before this successful rerun.
- `cargo clippy -p wordcraft -p wordcraft-assistant --all-targets --no-deps -- -D warnings -A clippy::nonminimal_bool`: exit 0; allowance only for the already identified upstream boolean lint.
- `cargo xtask layers`: exit 0, 21 crates with no violations. `git diff --check`: exit 0.
- Pre-commit `cargo xtask ci`: exit 1; fmt passes, baseline Clippy warnings in proof/affix.rs, doc/math_linear.rs and xtask's boolean expression stop CI before subsequent stages. Full CI remains outstanding for T11.
- Authentication UI and hosted conversations remain T8/T11 work; this task verifies signed-out status and lifecycle without using the normal Codex account.
