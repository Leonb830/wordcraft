# ADR 0001: Add a native Codex assistant

Date: 2026-10-16
Status: Accepted architecture; Codex 0.162.1 protocol qualified by T1 (local mock inference)
Owner: the WordCraft fork maintainer (Leonb830); implementation and verification are recorded in `docs/tasks/todo.md`.

## Base and reconciliation (T0)

Work is confined to the public GitHub fork `Leonb830/wordcraft`, on
`codex/wordcraft-vnext`. The base is upstream main
`eae242176108d328c5dd81fbc10370c51ab90706`, refreshed on 2026-10-16.
The clone contains full history; upstream pushes are disabled locally.

Neither relevant PR is merged or approved. Both report merge conflicts with main:

| Work | Inspected head | Decision |
| --- | --- | --- |
| #815, assistant-agent | `8f36148639dc1369ef4d9b7963669bbf3d3b1e02` | Keep its generic provider work independent. Reuse its L5 assistant/L6 pane/native host separation and pane integration approach when implementing T2/T6. Do not import its HTTP agent loop as the Codex runtime. |
| #178, pr/chat | `4f375f3c81d4dde11d644b3e40312808bce1db80` | Keep external-agent chat independent. Preserve current CLI/MCP/control behavior and leave integration seams available for a later upstream merge. Do not import its broad agent tool profile. |

Compared each PR with `git log upstream/main..upstream/pr-N` and
`git diff --stat upstream/main...upstream/pr-N`, and inspected the assistant
`lib.rs`/`guard.rs` and chat `chat_gate.rs` contracts. #815 adds 26 changed files
(3,578 insertions, 15 deletions); #178 adds 59 (9,479 insertions, 129 deletions).
No PR commits are adopted wholesale. This avoids importing unaccepted provider,
chat, and unrelated documentation changes into the current implementation base.
Any later direct code adaptation must retain attribution and identify its source.

#815's deletion/style thresholds do not approve every write. Its session author
is changed for the duration of a request; Codex attribution must instead be
scoped to the actual mutation so concurrent user edits remain user-authored.
#178 permits page rendering and broad review commands, including acceptAll and
rejectAll. Those policies cannot be used as the Codex tool boundary.

Overlapping files belong to these ordered tasks: assistant protocol/service crate
(T2), MCP profile (T3), session/review/approval model (T4), native host and Services
(T5), panes/ribbon/icons/assistant UI (T6), event/approval presentation (T7),
settings/auth (T8), packaging (T9). Existing provider work is not overwritten;
existing command dispatch and control integrations are retained. No deletion of
an existing feature is planned or authorized by this ADR.

## Connection, runtime and authentication

Use Codex app-server over private child stdio, behind typed Rust commands/events.
No terminal chat UI, `codex exec`, experimental dynamic tools, or WebSocket
transport dependency. The native app owns OS services; egui never speaks JSON-RPC
or spawns a process. The engine stays below assistant/MCP and UI. Register any
new crate with the layer checker and keep native process modules out of WASM.
Web editing stays usable with an explicit assistant-unavailable state.

Default runtime policy: detect a separately installed, explicitly selected Codex
binary, validate an exact tested version, and provide setup/recovery in the pane.
Do not silently download executables. T1 owns selecting the tested version and
qualification matrix; the fork maintainer owns future pinned-runtime packaging.
A managed installer is deferred until platform qualification and update ownership
are established. Installed-runtime detection is the v1 distribution choice.

Default authentication: supported app-server-managed ChatGPT browser sign-in,
initiated from the pane, with account status and cancellation. This is a local,
open-source desktop app. The runtime owns credentials and refresh; WordCraft
does not persist credentials or implement external token sharing. T1 must verify
the installed runtime's supported flow before product implementation. A separate
WordCraft runtime home/config is required; never rewrite global Codex settings
or expose unrelated configured MCP servers, plugins, skills, or tools.

## Document and tool policy

Use a dedicated allowlisted WordCraft MCP profile, not experimental dynamic tools
or the existing unrestricted MCP registry. Native host marshals requests onto
the session/UI thread. Explicit scope and initial content-sharing consent apply
before document text leaves WordCraft. Selection reads must not silently broaden
to the full document. Tools include revision and request identity.

Every mutation waits for explicit approval and rechecks the document revision.
Attribute only the approved mutation to Codex, group by thread/request, and
provide scoped review/undo without resolving unrelated human revisions. Normal
save remains manual. No shell, general filesystem, screenshots, window-control,
save/export, or unlisted tools may be available to the model. T1 must demonstrate
that the runtime itself can enforce that restriction; a read-only sandbox or a
prompt alone is insufficient. If the supported runtime cannot do this, stop T1
and escalate instead of weakening the boundary.

## Rules and evidence

Read repository AGENTS.md, ROADMAP.md, docs/gaps.md, docs/roadmap.md,
docs/architecture.md, and xtask/src/layers.rs. No local plan/ directory exists.
The referenced shared craftrules repository and AGENTS.md return HTTP 404 under
the authenticated account; no local copy was found. The user explicitly authorized
proceeding with WordCraft's checked-in rules on 2026-10-16.

The architecture preserves the command registry, Rust-only implementation,
no-panic rule, layering, asset attribution, manual save, and WASM requirements.
No runtime or application test success is implied by this architecture decision.

Official contract reference inspected on 2026-10-16:
https://learn.chatgpt.com/docs/app-server
(the developers.openai.com/codex/app-server URL redirects there).

## Revision history

| Date | Change |
| --- | --- |
| 2026-10-16 | T0: reconciled main and both unmerged PRs; selected additive native app-server architecture and installed-runtime/auth defaults. |

T1 qualification: see [compatibility matrix](../tasks/codex-spike/README.md). The supported version is exactly 0.162.1 until another version is qualified. Managed browser auth start/cancel and isolated sign-out work. Hosted inference with the dedicated profile remains a product integration check.

## T5 native document relay (2026-10-17)

The dedicated MCP helper runs as `wordcraft --codex-document-bridge`, before GUI,
logging or preference initialization. Its five allowlisted read tools relay requests
through standard MCP form elicitation and the stable app-server
`mcpServer/elicitation/request` route. The owning native host checks the thread and
current shared document identity/revision before replying. Tool arguments travel in
`_meta.wordcraftCall`; the result is a schema-defined string in the elicitation
response. This round trip was qualified against installed Codex 0.162.1.

This keeps active document data in private stdio pipes: there is no listening
socket, control-key reuse, context file, global MCP registration or broad Backend.
Unknown servers and tools are declined. The ordinary control server and CLI MCP
remain independently available to external agents. The helper is a separate
process so the assistant runtime never gains access to the editor's unrestricted
command dispatcher. Write proposals will be added only through the T4 gate in T7.


## T7 proposal scope and manual save (2026-10-18)

The relay now additionally exposes `propose_selection`. It only queues one preview
per turn for the exact originally shared selection in one paragraph. The host
checks originating thread/turn and the profile checks consent, document identity,
revision and exact original text. The engine adds object/protection/revision guards.
Explicit UI approval is the sole application path; there is no approval tool.

Approval pauses existing AutoSave for the session. This is necessary because an
already enabled AutoSave could otherwise persist an approved edit on the next UI
tick despite the integration's manual-save promise. The card explains the pause.
The user can save normally or explicitly turn AutoSave back on. No other existing
integration or save command is removed.
