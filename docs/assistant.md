# Codex inside WordCraft

Status: fork development branch, 2026-10-19. This feature is in
[`Leonb830/wordcraft`, branch `codex/wordcraft-vnext`](https://github.com/Leonb830/wordcraft/tree/codex/wordcraft-vnext).
It is not included in the upstream downloads linked from the README. No fork
release has been published. Build this branch using the README's Cargo command.
Once WordCraft is open, setup and conversations use its Assistant pane and your
browser; no terminal chat command is needed.

The assistant pins **`gpt-6.1-sol`** for new and resumed conversations. The model and
runtime are qualified together: a newer runtime default may omit document tools
unless its isolated dispatch host is installed. Requalify the executed tool
catalogue before changing either.

## Set up

1. Open **Assistant** in WordCraft's title bar, then **Connection settings**.
2. Choose **Get Codex 0.162.1 for your computer**. Download the matching archive
   from the [official pinned release](https://github.com/openai/codex/releases/tag/rust-v0.162.1)
   and extract it. Also download the corresponding
   `codex-code-mode-host-<same target>.tar.gz` from that release. Extract the host
   beside Codex and rename it to `codex-code-mode-host` (`codex-code-mode-host.exe`
   on Windows). A complete official installation may already include the host.
   WordCraft requires exactly **0.162.1** and does not install or
   upgrade it. Keep this executable in a stable location.
3. Enter the extracted executable's **full absolute path** in **Installed Codex
   0.162.1 executable**. You can leave the field blank if that version is already
   on the app's PATH. Choose **Start / restart Codex**.
4. Choose **Sign in with ChatGPT** and finish the secure browser flow. If the
   browser does not open, use **Open secure sign-in page**. Return to WordCraft;
   the dialog reports **Signed in to WordCraft's Codex profile.** Use **Check
   sign-in** to refresh the status. **Cancel sign-in** cancels an unfinished flow.
   An account with Codex access and an internet connection are required.
5. Close the connection dialog. Choose **New conversation**, type in **Message
   to Codex**, and click **Send message** or press Ctrl+Enter (⌘+Enter on macOS).
   Responses and action progress appear in the pane.

Choose the archive for your computer:

| System | CPU | Codex archive |
| --- | --- | --- |
| macOS | Apple silicon | `codex-aarch64-apple-darwin.tar.gz` |
| macOS | Intel | `codex-x86_64-apple-darwin.tar.gz` |
| Windows | ARM64 | `codex-aarch64-pc-windows-msvc.exe.tar.gz` |
| Windows | Intel/AMD 64-bit | `codex-x86_64-pc-windows-msvc.exe.tar.gz` |
| Linux | ARM64 | `codex-aarch64-unknown-linux-musl.tar.gz` |
| Linux | Intel/AMD 64-bit | `codex-x86_64-unknown-linux-musl.tar.gz` |

Keep both executables in the same folder. The isolated host runs only the eight
WordCraft document tools; it has no Node, filesystem or network globals.
Use the executable inside the archive, not the archive itself. On macOS/Linux
it must have executable permission. If your OS blocks an untrusted download,
follow its normal verification and installation process for the official binary.
32-bit Windows, FreeBSD, Linux RISC-V, the browser build and Flatpak do not support
this assistant flow. Document editing and saving remain available. On Linux use
a native package/build or AppImage for Codex; Flatpak permissions are unchanged.

## Choose what to share

**Document sharing is off** initially and after starting/resuming a conversation
or reconnecting. Prompts you type are sent when you click Send, even with sharing off.

- Place the caret on a blank page or between existing characters, then choose
  **Share insertion point** to let Codex propose text there. This shares the
  location and typing format without sending surrounding text.
- Select text in the active document, then choose **Share selection** to permit
  reads and replacement proposals for that captured range, including multiple
  consecutive plain-text paragraphs. Moving the caret does not change the grant.
- Choose **Share document** to permit reads and proposals for consecutive body
  paragraphs. Codex receives paragraph indices and must match their original text.
  If the captured selection is a caret, insertion there is also available.
- Check the scope, revision and size indicator at the top of the pane before
  sending. Changing the document makes the captured context stale; share again
  explicitly to include the new revision. The limit is 256 KiB of shared text;
  whole-document sharing supports up to 1,024 top-level blocks.
- **Stop sharing** revokes further document reads. Previously sent content
  remains in the conversation. Revocation does not erase it from Codex.

For example, select a paragraph, share the selection, and ask “Summarize the
shared selection.” Codex can read, search and inspect permitted text/formatting.
It cannot open other files, run shell commands, operate windows, take screenshots,
save/export the document, or invoke WordCraft's general MCP command registry.

## Approve and review edits

To draft on a blank page, choose **Share insertion point** and ask, for example,
“Draft three paragraphs introducing our community garden.” To revise several
paragraphs, select and share them, or choose **Share document** and identify the
paragraphs to revise. Paragraph breaks in the proposal become real document
paragraphs, including empty paragraphs.

The private MCP tools are `propose_insertion` (captured caret),
`propose_selection` (entire captured selection), and `propose_paragraphs`
(consecutive body paragraphs from shared document context). Each turn can queue
one proposal of up to 64 KiB and 1,024 paragraphs. These tools handle plain text;
objects, protected text, content controls, intervening section breaks and
overlapping tracked changes are refused.

The **Codex edits** card shows the insertion location or original text, proposed
text and document revision. A proposal alone does not change the document:

- **Approve edit** applies it as tracked changes authored by **Codex**, associated
  with the conversation and request. This includes inserted or removed paragraph
  breaks. Approval checks the document revision again.
- **Deny edit** discards that preview without changing the text.
- **Accept request** keeps only that request's changes; **Reject request** removes
  only its changes. These actions leave unrelated human revisions alone.
- **Undo request** reverses that request, including an accepted request while its
  session record is still available. If later edits overlap the affected text,
  the action refuses to overwrite them.

If the document changed while Codex was preparing an edit, deny the stale preview,
share the current context and ask again.

Approval pauses AutoSave and the card tells you to save manually. Use WordCraft's
normal **Save** or **Save As** when ready; Codex never saves the document. You can
explicitly re-enable AutoSave yourself. Save as **DOCX** to retain tracked changes
and the request audit metadata. Formats such as plain text cannot preserve them.

Request review cards and accepted-request undo records last for the current
editing session. After reopening a saved DOCX, Codex attribution, tracked changes
and request metadata remain, but the session's request cards are not restored.
Review remaining tracked changes with WordCraft's normal Review controls.

## Continue, stop or disconnect

**Recent conversations** resumes a saved reference; alternatively enter a
**Conversation ID** and choose **Resume conversation**. After restarting WordCraft,
click **Start Codex** first. The runtime does not start automatically, and sharing
must be granted again. An empty conversation that never completed a turn may not
be available to resume; start a new one if Codex reports it unavailable.

**Stop response** interrupts the active turn and revokes sharing. **Retry message**
resends the last prompt when the current turn has ended; share again first if the
retry needs document access. **Disconnect** or **Disable Codex** stops the runtime.
You can keep editing and save normally.

## Privacy and local storage

WordCraft uses a separate Codex profile; signing into another Codex application
does not sign into WordCraft. **Sign out of WordCraft** affects only this profile.
No global Codex configuration is changed.

Under WordCraft's preference directory, `codex/assistant.json` stores the pane
visibility, executable path and up to 20 conversation IDs. It stores no prompts,
transcripts, credentials or sharing grants. Codex itself owns credentials and
canonical conversation data in `codex/home`; that data supports resume and can
contain prompts, responses and shared content. Treat the profile as private.
Diagnostic logs exclude assistant payloads and credentials.

**Clear thread references** clears WordCraft's list and conversation display and
disconnects. It does not delete Codex's own conversation data, saved document
revisions or request metadata. Disconnecting is not signing out or deleting data.

## Troubleshooting

| What you see | What to do |
| --- | --- |
| Codex not found | Extract the matching archive and set the full executable path in Connection settings. Desktop app PATH can differ from a terminal's PATH. |
| Missing code-mode host | Install the matching 0.162.1 host beside Codex, using the filename above; reconnect. |
| Unsupported version | Select the exact 0.162.1 executable. Keep it separate from a package manager's automatically updated Codex. |
| Signed out / expired authentication | Use Sign in with ChatGPT, then Check sign-in. Reconnect and resume if necessary; grant context again. |
| Browser sign-in fails | Cancel the pending flow, close any other pending Codex sign-in, then start a fresh sign-in from WordCraft. Never paste credentials into a conversation. |
| Runtime stopped / timed out | Use Start / restart Codex, resume the conversation and share again. Editing remains available. |
| Context stale or proposal refused | Share the current insertion point or a plain-text paragraph range without existing tracked changes, objects or section breaks, then request a fresh proposal. |
| Conversation unavailable | Check that you are using the same WordCraft profile; start a new conversation if the old thread cannot be resumed. |
| Response too large | Stop and ask for a shorter response. The pane retains up to 100 messages / 1 MiB and at most 256 KiB per displayed response. |
| Flatpak / unsupported target | Use a supported native build for the assistant; continue editing and saving in the current build. |

External agents still use the existing [CLI/MCP guide](mcp.md) and
[control protocol](control-protocol.md). Those separately configured integrations
have their own permissions; this assistant's limited tool profile does not replace
them. Platform qualification and verification evidence are in the
[release notes](releasing.md#codex-runtime-qualification) and [task checklist](tasks/todo.md).
