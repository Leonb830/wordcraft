# Releasing WordCraft

The canonical recipe is [craftrules `release/playbook.md`](https://github.com/storytold/craftrules/blob/main/release/playbook.md);
this file has the WordCraft specifics.

1. Bump the version on `main`: `cargo xtask version set 0.2.0` (the one source of truth is
   `[workspace.package] version`; the app shows it in `--version` and About, with the commit from
   `WORDCRAFT_BUILD_SHA`).
2. Merge `main` into `release` and push. `.github/workflows/release.yml` builds:

| Platform | Artifacts | Signing |
|---|---|---|
| macOS | universal `WordCraft.app` in a `.dmg`, universal CLI zip | Developer ID + notarization (`APPLE_*` secrets) |
| Windows | x64, x86 and arm64 `.msi` + portable `.zip` | Azure Trusted Signing (`AZURE_*` secrets) |
| Linux | x86_64 and aarch64 `.AppImage` (+ `.zsync` for AppImageUpdate), `.flatpak` bundle, `.deb`, `.rpm`, `.tar.gz`; Flathub manifest | checksums |
| Linux riscv64 | `.tar.gz` (cross-compiled on `ubuntu-24.04`, glibc >= 2.39; CLI smoke-tested under QEMU) | checksums |
| FreeBSD | x86_64 `.tar.gz` | checksums |
| Web | `wordcraft-web-<version>.zip` (static site: `index.html`, wasm, glue) | none |

3. The run drafts a GitHub Release `WordCraft v<version>` with `SHA256SUMS.txt`; review and publish it.

Signing secrets live in the repository's `release` environment (deployment branch: `release` only),
synced from the release manager's machine (`craftrules/release/signing-setup.md`). Missing secrets
produce unsigned builds with a warning, so forks still build.

Local packaging: `packaging/macos/package.sh --arch universal`, `packaging/windows/package.ps1`,
`packaging/linux/package.sh` (then `packaging/linux/flatpak-bundle.sh` for the Flatpak), `packaging/web/package.sh`.

Dry run: `gh workflow run release.yml --ref <branch>` builds the unsigned jobs (version, Linux,
Linux riscv64, Flatpak, FreeBSD, web) on any branch; the signing jobs are refused there by the `release`
environment's branch rule, so no draft release is made.

## Codex runtime qualification

Fork development branch: `codex/wordcraft-vnext`, qualified 2026-10-19. No release
was published. The user-facing setup, privacy and recovery instructions are in
[assistant.md](assistant.md). All six native combinations passed the lifecycle,
preference, real relay and window checks; exact run/job evidence is in
[the T9 checklist](tasks/todo.md#t9--complete-2026-10-19).

The assistant pins **`gpt-6.1-sol`** for new and resumed conversations. The model and
runtime are qualified together: a newer runtime default may omit document tools
unless its isolated dispatch host is installed. Requalify the executed tool
catalogue before changing either.

The in-app assistant discovers a separately installed **Codex 0.162.1** native
executable and matching `codex-code-mode-host` beside it (with `.exe` on Windows).
Neither is bundled, silently downloaded, or upgraded by WordCraft.
Connection settings links to the pinned upstream release and accepts an absolute
executable path. A version mismatch disables only the assistant. The stdio MCP
helper is the same WordCraft binary invoked with `--codex-document-bridge`, so
packagers must preserve that entry point; no additional helper asset is needed.

The upstream release has native binaries for macOS, Windows and Linux on x86_64
and aarch64. `.github/workflows/codex-native.yml` qualifies these six combinations
using fresh profiles, the real pinned runtime, document-relay boundary checks,
and a native-window smoke test (Xvfb on Linux). It does not use account credentials
or publish artifacts/releases. Record the successful run before claiming a
platform is qualified. Local macOS arm64 and WASM verification are also recorded
in [the task checklist](tasks/todo.md).

FreeBSD, Linux riscv64, 32-bit Windows and the web retain editing and saving but
do not expose a working in-app Codex connection. Flatpak's existing sandbox does
not permit the external runtime/network flow: the host reports this and suggests
a native package or AppImage. Do not add host filesystem, network, or process
escape permissions to Flatpak for this feature. Ordinary CLI/MCP packaging is
unchanged. Native packages do not require signing or shipping a Codex binary.

T11 adds executed code-mode catalogue qualification: current signed-in models
require the isolated host. Download its same-target `codex-code-mode-host` archive
from the same release and preserve the required sibling filename. The host is an
external Codex dependency, not a new WordCraft helper asset. General `functions`
and `clock` namespaces are excluded; only the six document MCP tools are callable
inside it. The expanded six-platform run must pass before T11 is complete.
