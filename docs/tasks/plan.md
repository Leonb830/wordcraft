# WordCraft vNext: in-app Codex integration

**Status:** Proposed implementation plan  
**Prepared:** 2026-10-16  
**Starting point:** `storytold/wordcraft` `main` at `eae242176108d328c5dd81fbc10370c51ab90706` (clean inspection checkout)  
**Product direction confirmed:** a Codex agent inside WordCraft's UI.

## Objective

Turn WordCraft's existing agent-drivable editor into a product with a first-party Codex workspace. A user should be able to open a document, start or resume a Codex conversation in WordCraft, share the current document or selection, watch the agent work, approve document changes, and review or undo those changes without leaving the editor or using a terminal workflow.

Keep the current Rust editor, document engine, CLI, MCP server, control channel, and web/WASM build. Add the Codex experience alongside them. Do not remove existing code or replace current agent entry points unless a later implementation proves a specific piece is redundant and documents why.

## What the source already provides

- `crates/engine/src/session.rs` is the shared command entry point for UI, CLI, MCP, and the control channel. Mutating commands participate in undo.
- `crates/ui-egui/src/panes.rs` and `crates/ui-egui/src/lib.rs` provide the existing side-pane UI and `WordApp`/`Session` seam.
- `crates/mcp` exposes document commands through an MCP server. `wordcraft-cli mcp --connect` can drive a running desktop app.
- The desktop app has a loopback control server with a per-window key. The existing MCP surface also includes broad file and UI operations, so it is not an appropriate unrestricted tool set for an in-app writing agent.
- Track Changes and review commands already exist. `review.acceptAll` and `review.rejectAll` affect all tracked changes, so they must not be used to resolve only Codex's edits.
- The repository is Rust-only and requires its WASM build to keep working. The current main branch describes agent integration through CLI/MCP and has no built-in Codex pane.

## Capability map

| Capability | Responsibility | Depends on |
|---|---|---|
| Codex runtime | Start/stop the Codex app-server, negotiate protocol version, manage threads and turns, stream events, interrupt work, report failures | Supported Codex runtime distribution and auth decision |
| WordCraft document tools | Give Codex a narrow, typed view of the active document and selection; expose only approved document actions | Existing `Session`/command APIs; MCP may be reused behind an allowlist |
| Change policy and review | Gate writes, scope them to a request, label them as Codex-authored, protect concurrent human edits, and review/undo them independently | Track Changes and undo APIs; may need request-scoped review support |
| Assistant experience | Conversation list, composer, streaming timeline, context scope, tool/approval cards, stop/retry, settings and empty/error states | Runtime events + document tools |
| Platform and release | Native desktop runtime availability; graceful behavior on unsupported targets; docs and release packaging | Runtime decision; current build/release flow |

## Recommended architecture

### Codex connection

Use the **Codex app-server** as WordCraft's Codex integration surface. It is designed for deep product integration and provides persistent threads, streamed events, tool connections, and approval flows over JSON-RPC-style newline-delimited JSON on stdio. Run it as a managed child process behind a Rust service; stdio is the v1 transport. Do not build the main experience around `codex exec`, and do not depend on the app-server's experimental dynamic-tools feature.

This gives WordCraft an in-app agent UI even though a Codex runtime process may run in the background. It is not a terminal-based user flow. Make runtime availability and sign-in state visible in WordCraft. Before release, decide whether WordCraft detects a compatible installed Codex runtime or installs/manages a pinned runtime for the user; the latter is preferable if the product goal is a one-step setup.

### WordCraft integration boundary

Keep Codex behind a native assistant service and a UI-facing event/command interface. The egui pane should not spawn processes or speak app-server JSON directly. The native app owns process startup and platform services. The WASM target continues to compile and presents a clear unavailable state until a supported browser/server transport is designed.

For document context, expose a purpose-built, allowlisted tool profile. Start with read-only actions such as inspect current document, read selected text, find text, and inspect relevant formatting. Add narrowly scoped write actions only through WordCraft's existing command/session path. Never expose general filesystem access, arbitrary shell execution, unrestricted `file.open`/`file.save`, window clicks, screenshots, or the entire MCP registry to the assistant by default. Do not alter global Codex configuration to register WordCraft tools.

Prefer a typed in-process bridge when it fits the app-server's supported tool mechanism; otherwise use a dedicated WordCraft MCP profile that filters tools and starts a local server for the active document. Reuse the MCP protocol implementation where useful, but do not grant the existing broad MCP server wholesale.

### Edit safety and review

- Share only the active document or selection, with a visible scope indicator and a user-controlled context setting. Ask before sending document content the first time in a session.
- Treat each user request as a change group. Record Codex edits with a stable Codex author identity and request identifier, and guard writes against stale document revisions.
- Ask for approval before mutating the document. Apply accepted edits as tracked changes where supported, and offer request-scoped accept/reject/undo so resolving Codex work cannot accept or reject a user's unrelated changes.
- Keep WordCraft's normal save flow under user control. Do not save, overwrite, or export a document automatically.
- Never use “accept all” or “reject all” for Codex-only review actions. If current APIs cannot isolate Codex's request, add the narrowest engine/UI command needed and tests proving human edits remain untouched.

### Existing assistant/chat work

At planning time, two relevant pull requests are open. Re-check their status and branch contents before implementation:

- PR [#815](https://github.com/storytold/wordcraft/pull/815) proposes an in-app assistant pane, a reusable assistant crate, provider APIs, and a document-edit harness. Treat it as a likely UI/harness foundation. Add a Codex app-server adapter to that foundation if accepted; do not duplicate or overwrite its generic provider work.
- PR [#178](https://github.com/storytold/wordcraft/pull/178) proposes chat with external agents using the CLI/control path. Preserve that use case and its existing command-line/MCP compatibility. It is complementary to the built-in Codex runtime.

The maintainer should first choose whether to build on the PR branch(es), rebase their landed equivalents, or implement against `main`. This plan does not assume either PR has merged.

## User journey for v1

1. The user opens **Assistant** from a persistent WordCraft side pane or keyboard shortcut.
2. WordCraft reports whether Codex is available and signed in. Setup and recovery happen in the pane; the user does not need to launch a terminal command to chat.
3. The user starts or resumes a thread and sees the current context scope (selection or whole active document). Sharing is explicit and can be changed per conversation.
4. The user asks Codex to explain, summarize, proofread, or revise the document. Text and progress stream into the pane. The user can stop a turn.
5. A write is previewed or presented as an approval card. After approval, changes appear as Codex-authored tracked changes grouped to the request. The user can accept/reject that request's edits, undo the request, or continue editing normally.
6. The user saves through WordCraft. On reopening, the assistant can resume the Codex thread; if the runtime or thread is unavailable, the document still opens and the pane explains how to recover.

## Scope

### v1 includes

- Native desktop assistant pane, with a streaming conversation and thread start/resume.
- Codex app-server lifecycle, protocol compatibility check, interruption, and useful startup/auth/version errors.
- Context controls for selected text and the active document.
- Allowlisted WordCraft document tools and explicit approval for writes.
- Codex-authored, request-scoped changes that can be reviewed independently of user changes.
- Runtime/auth status, conversation reference persistence, and privacy-conscious logging.
- Existing CLI, MCP, control channel, document behavior, and WASM compilation retained.
- Release notes and setup/help documentation.

### Not in v1

- A browser-hosted Codex agent or remote WordCraft service. App-server stdio is the supported v1 integration; browser support needs a separate transport and security design.
- General-purpose shell, filesystem, or unrestricted desktop automation for Codex.
- Replacing existing MCP or external-agent chat with Codex.
- Silent background document edits or automatic save.
- A custom cloud agent backend or a new model/provider abstraction that bypasses Codex.

## Ordered implementation plan

Estimates are rough engineering time for planning, not commitments. Each task should land as a small additive change with focused tests and a reviewable checkpoint. Re-estimate after the runtime spike and PR reconciliation.

### T0 — Reconcile existing assistant work and record the decision (2–4 h)

**Work:** Re-check PR #815 and #178 and any merged equivalents. Identify the accepted assistant crate, UI work, edit harness, provider contracts, and ownership of overlapping files. Write a short ADR at the repository's chosen decision-doc location (there is no obvious ADR convention in the inspected tree; `docs/decisions/0001-codex-app-server.md` is a proposed location). Record app-server choice, native-only v1, auth/runtime assumptions, and the additive migration plan.

**Likely files:** `docs/decisions/0001-codex-app-server.md`; update `docs/architecture.md` only if needed.

**Acceptance:** The implementation base is explicit; the plan names which existing PR changes are reused and which remain independent; no existing CLI/MCP/chat surface is removed; the open auth/runtime questions have owners and a chosen default.

**Verify:** Compare the selected base against current `main` and both PRs; review the ADR against `AGENTS.md` layering and Rust/WASM requirements.

**Depends on:** —

### T1 — Validate the app-server contract and runtime availability (4–6 h)

**Work:** Prototype the supported Codex app-server handshake, version/schema matching, thread start/resume, turn start, streamed text/status events, interruption, and one approved tool round trip. Determine the minimum Codex runtime version and auth path. Test missing binary, unsupported version, sign-out, and child exit behavior. Use a generated or versioned protocol schema compatible with the selected runtime.

**Likely files:** scratch prototype initially; then document the chosen protocol/runtime in the ADR. No user-facing feature yet.

**Acceptance:** A written compatibility matrix and a working local proof show that WordCraft can receive incremental events, stop a turn, and expose a tool request without terminal interaction. Runtime packaging/auth has a release-ready recommendation.

**Verify:** Run the prototype against the supported Codex app-server and a fake server fixture for malformed/partial event streams. Do not use experimental dynamic tools as a production dependency.

**Depends on:** T0

### T2 — Add the Codex runtime client and process lifecycle (6–10 h)

**Work:** Add or extend an assistant crate with a typed app-server client and service abstraction. Spawn the child without a shell, keep protocol traffic on stdio, bound message sizes, handle timeouts, serialize requests, route events, interrupt turns, and reap the child on shutdown. Keep secrets out of command arguments, logs, and persisted UI state. Isolate native-only code behind target/platform boundaries.

**Likely files:** `crates/assistant/Cargo.toml`, `crates/assistant/src/lib.rs`, `crates/assistant/src/codex.rs`, new protocol/process modules (or the accepted equivalent from #815).

**Acceptance:** The client starts/resumes a thread, streams events, interrupts, and reports structured errors; shutdown leaves no orphan process; malformed/oversized messages fail safely; non-native builds do not compile native process code.

**Verify:** Unit tests with a fake app-server for protocol ordering, partial lines, invalid JSON, interruption, child exit, message limits, and cleanup; run crate tests and clippy.

**Depends on:** T1

### T3 — Define and implement a narrow WordCraft tool profile (4–6 h)

**Work:** Create a separate Codex tool profile for active-document context. Begin with read-only inspection and selection operations. Add write tool schemas only after T4 defines the policy. Reuse command schemas and MCP implementation where safe, but enforce an allowlist in code rather than relying on prompt instructions. Include document revision and scope in tool calls.

**Likely files:** `crates/mcp/src/tools.rs`, `crates/mcp/src/server.rs`, new `crates/mcp/src/codex_profile.rs` or equivalent assistant bridge, `crates/engine/src/session.rs` only if a narrow read API is missing.

**Acceptance:** The Codex profile can access only the active document and permitted operations; attempts to call file, shell, UI-control, screenshot, save, or unlisted commands are rejected server-side. Existing MCP tools and behavior remain unchanged.

**Verify:** Tool contract tests prove allowlisted calls work and every excluded class is denied. Test against a running app and headless session without broadening the existing MCP default surface.

**Depends on:** T0, T2

### T4 — Add request-scoped approval and review for Codex edits (6–10 h)

**Work:** Route proposed mutations through a user approval gate. Associate edits with author `Codex`, conversation/thread, request ID, and document revision. Use tracked changes and undo grouping where supported. Add narrowly scoped review/undo actions if the current review model cannot isolate a request. Refuse or ask the user to retry if the document changed since the proposal was generated.

**Likely files (keep this task to these four; add tests beside the implementation):** `crates/engine/src/session.rs`, `crates/engine/src/cmd/review.rs`, `crates/ui-egui/src/assistant.rs`, `crates/ui-egui/src/lib.rs`.

**Acceptance:** A user can approve or deny a Codex mutation; approved edits show as Codex-authored changes and can be accepted/rejected/undone by request. A request never resolves unrelated user changes. Stale document revisions cannot silently overwrite new edits. Normal save remains manual.

**Verify:** Engine tests cover approve, deny, stale revision, undo, accept/reject by Codex request, and preservation of unrelated user tracked changes. UI tests cover approval and review controls.

**Depends on:** T0, T3

### T5 — Host the service in the native WordCraft application (4–6 h)

**Work:** Add a native service host and lifecycle wiring. It owns runtime detection or management, auth/status calls, assistant events, active-document context, and shutdown. Keep global Codex configuration unchanged. Decide whether runtime is bundled/managed or discovered, and make the chosen flow understandable inside WordCraft.

**Likely files:** `apps/wordcraft/src/main.rs`, new `apps/wordcraft/src/codex_service.rs`, `apps/wordcraft/Cargo.toml`, `crates/ui-egui/src/lib.rs`.

**Acceptance:** The service starts only when Assistant is used or enabled, communicates only over its private stdio pipes, surfaces missing-runtime/auth errors in the UI, and cleans up on app close. Existing `--control`, MCP, and CLI startup continue to work.

**Verify:** Manual native smoke check for start, sign-in status, child crash, disable/restart, app shutdown, and coexistence with the current MCP/control path.

**Depends on:** T2, T3

### T6 — Build the assistant pane and conversation basics (6–10 h)

**Work:** Add an Assistant entry to the existing pane framework. Implement thread list or recent-thread selector, new/resume conversation, message composer, send state, empty state, and context scope indicator. Keep layout behavior consistent with existing panes and localization patterns.

**Likely files:** `crates/ui-egui/src/panes.rs`, new `crates/ui-egui/src/assistant.rs`, `crates/ui-egui/src/lib.rs`, pane strings/icons as required.

**Acceptance:** A user can open/close the pane, start/resume a thread, send a prompt, see their message, and tell whether the selection or full document is shared. Existing panes still function and the web build can show a clear unsupported-runtime state.

**Verify:** `egui_kittest` interaction checks for pane, composer, context toggle, and empty/error states; manual resize/accessibility/keyboard-navigation review.

**Depends on:** T2, T5

### T7 — Add streaming, tool activity, approval cards, and interruption (5–8 h)

**Work:** Render incremental assistant responses and clear progress states. Show which WordCraft action Codex wants to take and its affected scope. Connect approval cards to T4, and provide stop, retry, and recoverable error states. Ensure waiting for approval cannot leave the UI blocked.

**Likely files:** `crates/ui-egui/src/assistant.rs`, service event types in assistant/app code, `crates/ui-egui/src/lib.rs`.

**Acceptance:** Text appears incrementally; tool activity is understandable; document writes wait for explicit approval; stop interrupts the current turn; runtime and tool errors are visible and recoverable. No event stream can trigger a write without passing the edit gate.

**Verify:** UI tests for delta ordering, approval/deny, stop, duplicate/out-of-order events, and runtime disconnect; manual slow-response and repeated-turn checks.

**Depends on:** T4, T6

### T8 — Add auth, thread references, settings, and privacy controls (4–7 h)

**Work:** Implement the selected supported sign-in flow and runtime/auth status. Persist only WordCraft UI metadata and the Codex thread reference needed to resume. Add user controls to clear local references and disable the integration. Define logging and document-context disclosure behavior; do not copy full document transcripts into WordCraft settings by default.

**Likely files:** assistant service/settings modules, `apps/wordcraft/src`, `crates/ui-egui/src/assistant.rs`, settings and documentation modules selected in T0.

**Acceptance:** Users can see sign-in state, recover from expired auth, resume a thread, disable the feature, and clear WordCraft's local thread references. Document content, credentials, and protocol payloads are absent from logs. Context sharing is visible before the first send.

**Verify:** Test no-auth, expired-auth, sign-out, and clear-history states; inspect logs and persisted settings with synthetic content and credentials.

**Depends on:** T1, T5, T6

### T9 — Runtime distribution, platform behavior, and WASM preservation (4–8 h)

**Work:** Implement the runtime distribution decision from T1/T0 for supported desktop platforms, including version checks and graceful fallback. Keep process spawning out of WASM and preserve its compile. If the app-server is unavailable on a target, show the limitation without affecting editing, save, or the existing web app.

**Likely files:** `apps/wordcraft/Cargo.toml`, platform-specific app/release configuration, `docs/releasing.md`, target-specific assistant modules.

**Acceptance:** Supported native builds can use the same in-app Codex flow; unsupported runtime/platform produces an actionable UI state; `cargo xtask wasm` still passes; no change to current CLI/MCP operation.

**Verify:** Native build and smoke checks per supported OS, `cargo xtask wasm`, and packaging checks for the selected runtime strategy.

**Depends on:** T1, T5, T8

### T10 — User setup and help documentation (2–3 h)

**Work:** Add a user-facing assistant guide and update product, setup, release, and MCP docs for the in-app assistant, runtime requirements, sign-in, context sharing, write approval/review, supported targets, troubleshooting, and continued CLI/MCP usage.

**Likely files (maximum five):** `README.md`, new `docs/assistant.md`, `docs/mcp.md`, `docs/releasing.md`, assistant help strings in `crates/ui-egui/src/assistant.rs`.

**Acceptance:** A new user can set up and use Codex without following CLI chat steps; current CLI/MCP docs remain accurate; runtime/auth limitations and recovery are clear.

**Verify:** Follow the documented setup from a clean user profile and review links/commands against the shipped UI and supported runtime.

**Depends on:** T2–T9

### T11 — Release integration verification and roadmap update (1–2 h)

**Work:** Update architecture and roadmap/gap status, run repository CI, and review the full change for additive scope and dependency boundaries.

**Likely files (maximum four):** `docs/architecture.md`, `docs/gaps.md`, `ROADMAP.md`, plus the relevant release checklist if required.

**Acceptance:** The release documentation reflects the shipped Codex path, existing CLI/MCP paths remain documented, and the final source change does not remove existing integration behavior.

**Verify:** `cargo xtask ci`; native manual end-to-end run through start/resume, stream, document read, approved write, scoped review, save, and restart. Verify CLI MCP still works against both headless and running WordCraft.

**Depends on:** T2–T10

### T11 verification repair scope (2026-10-19)

The required full CI currently stops on pre-existing Rust 1.95 Clippy warnings in
`crates/proof/src/affix.rs`, `crates/doc/src/math_linear.rs`,
`crates/ui-egui/src/objects.rs` and `xtask/src/visual_diff.rs`. T11 includes minimal,
semantics-preserving match/boolean simplifications in those files so the required
workspace gates can run. No existing feature or integration is removed. Any
further gate failure will be diagnosed and recorded before a necessary repair;
focused checks alone cannot satisfy final CI. The next Clippy pass also found a
pre-existing collapsible `w:cr` match in `crates/docx/src/read/story.rs`; include
the same semantics-preserving match-guard repair there. Two identical baseline
match warnings in `crates/docbin/src/officeart.rs` need the same repair.
`apps/wordcraft-cli/src/main.rs` also has an equivalent argument-filter boolean
warning; simplify the expression without changing argument parsing.

The hosted T11 turn exposed missing model-visible document tools despite passing
direct relay probes. Add a real-runtime local model-request regression at that
boundary and repair the qualified model/tool configuration before final E2E;
keep the same restricted tool surface and stable protocol.

## Verification strategy

Do not start by making the full repo CI the feedback loop. Add focused checks with each task, then finish with the repository's required `cargo xtask ci` (fmt, clippy, tests, assets/layer checks, and WASM build) and native end-to-end checks.

- Protocol client: fake app-server fixtures for JSONL, version negotiation, event ordering, interruption, malformed data, bounded messages, and child cleanup.
- Tool boundary: positive tests for allowed document operations and negative tests for file/shell/UI/control actions.
- Document safety: approved vs denied writes, stale revision handling, request grouping, and assurance that user-authored tracked changes remain unchanged.
- UI: pane lifecycle, scope indicator, stream rendering, approval/stop/retry/error handling, keyboard navigation, and non-native unavailable state.
- Integration: Codex signed-in and signed-out flows; runtime missing/unsupported; thread resume; a full edit/review/save cycle; existing CLI/MCP paths.
- Platform: supported native packages and `cargo xtask wasm`.

## Risks and decisions to close in T0/T1

1. **Runtime distribution:** app-server requires a compatible Codex runtime. Detecting an installed runtime is quicker but may undercut the in-app setup goal; managed/pinned delivery offers a smoother experience but adds packaging and update ownership.
2. **Authentication:** select the supported Codex sign-in flow and determine whether product OAuth registration/token sharing is needed. Do not store or mint credentials outside the supported flow.
3. **WASM/browser scope:** v1 is native desktop. A browser experience needs a supported transport and a deliberate server/auth boundary; do not build on experimental app-server WebSocket transport.
4. **PR overlap:** #815 may alter the assistant crate/UI/edit harness; #178 may alter chat/control/CLI. Reconcile before choosing file ownership.
5. **Document concurrency:** the assistant can hold stale context while a person edits. Revision checks and request-scoped review are required before writes ship.
6. **Protocol drift:** pin/test a supported app-server version and surface upgrade incompatibility clearly.

## Success criteria

- A user can complete a Codex conversation from WordCraft's own UI without entering a terminal command.
- The assistant can read only the active document context the user selected and can make a document change only after approval.
- Codex edits are clearly identified and independently reviewable; unrelated user changes are preserved.
- Streaming, interruption, thread resume, auth/runtime errors, and shutdown behave reliably.
- Existing CLI/MCP/control workflows remain usable, native desktop builds work, and the WASM build remains green.

## References

- [WordCraft README](https://github.com/storytold/wordcraft/blob/main/README.md)
- [WordCraft contributor and architecture rules](https://github.com/storytold/wordcraft/blob/main/AGENTS.md)
- [WordCraft MCP guide](https://github.com/storytold/wordcraft/blob/main/docs/mcp.md)
- [WordCraft control protocol](https://github.com/storytold/wordcraft/blob/main/docs/control-protocol.md)
- [WordCraft AI assistant gap](https://github.com/storytold/wordcraft/blob/main/docs/gaps.md)
- [Open assistant PR #815](https://github.com/storytold/wordcraft/pull/815)
- [Open external-agent chat PR #178](https://github.com/storytold/wordcraft/pull/178)
- [OpenAI Codex app-server overview](https://developers.openai.com/codex/app-server)
- [Codex as a platform](https://developers.openai.com/blog/codex-as-a-platform)
- [Codex Sign in with ChatGPT / app-server token sharing](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server)

### T11 live-runtime repair refinement (2026-10-19)

Signed-in model metadata requires code-only dispatch. A legacy direct-tool model
pin is insufficient (GPT-5.4 is unavailable; gpt-reserve also requires code-only
mode once authenticated). Keep experimental code-mode selection disabled; allow
the pinned runtime's standard local code-mode host for model-required dispatch.
Exclude the `functions` and `clock` namespaces from nested dispatch so only the
six WordCraft MCP tools remain. Verify the actual executed catalogue plus absent
Node/require/fetch/Deno/shell/patch APIs, denied injected native write requests,
and hosted read/propose/approve. The protocol stays stable stdio with no dynamic
tools. Add the same-release `codex-code-mode-host` executable to setup and six-OS/CPU
qualification; no automatic install or new WordCraft release asset. Missing-host
warnings must give actionable recovery without logging payloads.

Full CI also exposed missing translations for the new Assistant entry and Codex
review commands. Add those catalogue entries without relaxing the i18n gates.

Full CI on a German macOS environment exposed the existing Breaks-menu tests
assuming English while inheriting the system locale. Set the fixture language
explicitly before rendering; preserve the production locale behavior and assertions.
