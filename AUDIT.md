# Repository Audit — `xisf2png`

- **Repository:** `https://github.com/ntmb-fb/xisf2png.git`
- **Audited commit:** `23186d7` (`main`, post-`v0.9.1` + PR #1; pulled from `4304943..23186d7`)
- **Date:** 2026-11-26
- **Toolchain used:** `rustc 1.100.0` / `cargo 1.100.0` (macOS)
- **Scope:** All source files in `src/`, `build.rs`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/`, `README.md`, and repository hygiene. (`astropng-core v0.2.1` lives in a separate repository and was not audited internally.)

---

## Executive Summary

`xisf2png` is a compact, clean, well-structured Rust wrapper (CLI + `egui`/`eframe` GUI + OS shell integration) over `astropng-core`. It compiles cleanly on macOS with only one `dead_code` warning, but the audit surfaced **several real bugs, CI supply-chain risks, zero automated tests, and licensing/packaging gaps**:

- **High (3):** Unverified floating `continuous` binaries executed in a release job that holds `contents: write`; Linux AppImage desktop integration writes an ephemeral `/tmp/.mount_*` path that breaks on exit; single-instance TCP handoff ignores the ACK byte (breaking standalone fallback) and has multi-user/unbounded-read security issues.
- **Medium (9):** Broken CLI `--help` formatting; literal `&&` rendered on a GUI button; `file_mode` + `png_only` state lockup; Windows release GUI swallows stdout for scriptable `--*-context-menu` flags; missing graceful shutdown / Ctrl-C cancellation risking partial file writes; positional CLI typos creating spurious output directories; Linux `.desktop` `Exec=` escaping & unset-`HOME` bugs; missing `LICENSE` file and DejaVu license notice in release archives; zero automated tests and no PR/push CI workflow.
- **Low / Maintenance (10):** Unused direct `image` dependency; GUI crates not feature-gated for CLI-only/musl builds; macOS `dead_code` warning; single-instance empty-arg focus omission; `auto_zoom` rounding sensitivity vs manual zoom; stale GUI summary wording; unvirtualized log rendering; branch-deleter SHA-only match; green release log purging; minor doc/metadata drift.

---

## Remediation Status

### Fixed in working tree

- **H1 (Release CI hardening):** Scoped workflow-level `permissions` to `contents: read` with `contents: write` only on `release`; pinned `appimagetool` (`1.9.1`) and `linuxdeploy` (`1-alpha-20251107-1`) with `sha256sum` verification in `.github/workflows/release.yml`.
- **H2 (Linux AppImage `.desktop` path):** Updated `exe_path()` in `src/bin/gui/shell.rs` to prefer `$APPIMAGE` over `current_exe()`.
- **H3 (Single-instance IPC):** Switched Unix handoff in `src/bin/gui/instance.rs` to a per-user `0700`-directory Unix domain socket with stale-socket recovery, strict `b'k'` ACK checking, NUL-delimited raw path framing, a 1 MiB payload cap, and clean standalone fallback (no dummy socket/thread).
- **M1 (CLI `--help` & synopsis):** Fixed missing `\n\` continuations on `--no-ask`/`--png-only`, aligned the `Usage:` synopsis, and updated `--no-ask` text in `src/main.rs`.
- **M2 (GUI button label):** Fixed literal `"Resize && stamp PNGs"` to `"Resize & stamp PNGs"` in `src/bin/gui/main.rs`.
- **M3 (`file_mode` + `png_only` lockup):** Guarded `resize4k` and `App::options()` using `effective_png_only` (`!file_mode && self.png_only`) and `!file_mode && self.recursive`.
- **M4 (Windows GUI console output):** Added `AttachConsole(ATTACH_PARENT_PROCESS)` before printing for `--*-context-menu*`, `--version`, and `--help` in `src/bin/gui/main.rs`.
- **M5 (GUI graceful shutdown):** Implemented `eframe::App::on_exit` to set `cancel`, clear `name_prompt`, and join the worker thread.
- **M6 (CLI argument parsing):** Switched to `args_os()`, added `--` end-of-options and `--concurrency=N`, and rejected missing files with known image extensions instead of treating them as `output_dir`.
- **M7 (Linux `.desktop` escaping & `HOME` check):** Implemented FreeDesktop `Exec=` quoting/escaping and returned an explicit error when neither `XDG_DATA_HOME` nor an absolute `HOME` is set.
- **M9 (Automated tests & PR CI):** Added 13 unit tests across `src/main.rs`, `src/bin/gui/main.rs`, `src/bin/gui/instance.rs`, and `src/bin/gui/shell.rs`, and added `.github/workflows/ci.yml` (`fmt`, CLI-only check, `clippy -D warnings`, `test`).
- **L1–L8, L10:** Removed unused `image` dependency from `Cargo.toml` / `Cargo.lock` / `README.md`; gated GUI deps behind a default `gui` feature (`--no-default-features --bin xisf2png`); fixed macOS `dead_code` warning; focused existing window on empty-arg handoff; added 4 px tolerance to `auto_zoom`; fixed GUI folder-drop exit from `file_mode`, per-file remove button, deferred drops while running, `RunSummary`, and `--version`/`--help`; verified exact path/command in `is_installed()`; matched `(head.ref, head.sha)` in `.github/workflows/delete-merged-branches.yml`; documented AppImage in `README.md`, completed AppImage `.desktop` metadata, and added `.DS_Store` to `.gitignore`.

### Left open for user decision

- **M8 (Licensing & DejaVu notice):** Choose an open-source license, add `LICENSE` + `Cargo.toml` `license` metadata, and bundle `LICENSE-DejaVu.txt` in release archives.
- **M5 (CLI `Ctrl-C` handler):** Wire a signal handler (e.g., via `ctrlc` or OS signal FFI) to `AtomicBool` in `src/main.rs` if graceful mid-batch cancellation on `SIGINT` is desired.
- **H1 (GitHub Action SHA pinning):** Optionally pin third-party Actions (`actions/checkout`, `dtolnay/rust-toolchain`, `softprops/action-gh-release`, `actions/github-script`) to full 40-char commit SHAs.
- **L9 (`purge-green-runs.yml`):** Decide whether to exclude `"Release"` from automatic green-run log deletion.
- **L10 (Identifier consistency):** Decide whether `CFBundleIdentifier` (`io.github.peterbuitho.xisf2png`) and Windows `CompanyName` (`peterbuitho (open source)`) should stay as-is or align with `ntmb-fb`.

---

## Verification Results

### Baseline (pre-fix, commit `23186d7`)

| Check | Result | Notes |
| --- | --- | --- |
| `git pull` | Fast-forwarded `4304943..23186d7` | Pulled `v0.9.1` and PR #1 (`purge-green-runs.yml`, `delete-merged-branches.yml`). |
| `cargo check --offline --locked --all-targets` | **Pass (1 warning)** | `dead_code`: `exe_path` unused on macOS (`src/bin/gui/shell.rs:22`). |
| `cargo test --offline --locked` | **Pass (0 tests)** | 0 unit tests, 0 integration tests, 0 doc-tests across all targets. |
| `cargo clippy --offline --locked --all-targets -- -W clippy::all -W clippy::pedantic` | **9 warnings** | 0 `clippy::all` warnings; 1 `dead_code` + 8 `clippy::pedantic` warnings (`too_many_lines`, `struct_excessive_bools`, `bool_to_int_with_if`, `wildcard_imports`, `doc_markdown`, `cast_precision_loss`). |
| `cargo run --bin xisf2png -- --help` | **Defect reproduced** | `--no-ask` and `--png-only` lines are visibly misaligned due to missing `\n\`. |
| `cargo-audit` / `cargo-deny` | Not installed locally | Recommend adding `cargo-audit` / `cargo-deny` to CI. |

### Post-fix (working tree)

| Check | Result | Notes |
| --- | --- | --- |
| `cargo check --offline --locked --all-targets` | **Pass (0 warnings)** | Clean build of library, CLI, GUI, and tests. |
| `cargo check --offline --locked --no-default-features --bin xisf2png` | **Pass (0 warnings)** | CLI builds without `eframe`, `rfd`, or `winreg`. |
| `cargo test --offline --locked` | **Pass (13 passed, 0 failed)** | 5 tests in `src/main.rs`, 8 tests in `src/bin/gui/main.rs`, 0 doc-tests. |
| `cargo clippy --offline --locked --all-targets -- -D warnings` | **Pass (0 warnings)** | Zero compiler or clippy warnings across all targets. |
| `cargo run --offline --locked --bin xisf2png -- --help` | **Pass** | `--no-ask` and `--png-only` alignment fixed. |
| `cargo run --offline --locked --bin xisf2png-gui -- --version` | **Pass** | Prints `xisf2png-gui 0.9.2`. |
| `cargo fmt --check` | **Pass** | Exit code 0. |

---

## Findings

### High Severity

#### H1. Release workflow downloads and executes unverified floating `continuous` binaries with `contents: write`
- **Where:** `.github/workflows/release.yml:15-16`, `.github/workflows/release.yml:165-167`
- **Issue:**
  1. `permissions: contents: write` is declared at the workflow top level instead of scoped solely to the `release` job. Every matrix `build` job receives a write-capable `GITHUB_TOKEN`.
  2. The `Build AppImage` step runs `curl` against mutable `continuous` tags for both `appimagetool` and `linuxdeploy` without pinning a release/commit or verifying SHA-256 checksums, then `chmod +x` and executes them.
  3. GitHub Actions (`actions/checkout@v7`, `dtolnay/rust-toolchain@stable`, `softprops/action-gh-release@v3`, `actions/github-script@v8`) are pinned to mutable tags rather than immutable commit SHAs.
- **Impact:** Compromise of either upstream `continuous` release or a mutable Action tag yields code execution with write access to the repository and release assets.
- **Recommendation:**
  - Set top-level `permissions: contents: read` and put `permissions: contents: write` only on the `release` job.
  - Pin `appimagetool` and `linuxdeploy` to specific release versions (or verified commits) and verify their SHA-256 digests before `chmod +x`.
  - Pin third-party Actions to full 40-char commit SHAs (with version comments).

#### H2. Linux AppImage desktop integration registers an ephemeral FUSE mount path
- **Where:** `src/bin/gui/shell.rs:22-24`, `src/bin/gui/shell.rs:144-159`, `.github/workflows/release.yml:160-189`
- **Issue:** `release.yml` ships `xisf2png-gui-linux-x86_64.AppImage`. Inside an AppImage, `std::env::current_exe()` resolves to `/tmp/.mount_xisf2pXXXXXX/usr/bin/xisf2png-gui`, which vanishes as soon as the app exits. `shell::install()` writes that path into `~/.local/share/applications/xisf2png.desktop`.
- **Impact:** Clicking **"Install 'Open with xisf2png' desktop entry"** from the official Linux AppImage produces a broken `.desktop` entry immediately after closing the app.
- **Recommendation:** In `exe_path()` on Linux, prefer `std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())` before falling back to `std::env::current_exe()`.

#### H3. Single-instance TCP handoff ignores ACK verification and has local IPC security flaws
- **Where:** `src/bin/gui/instance.rs:18-19`, `src/bin/gui/instance.rs:27-99`
- **Issues:**
  1. **Broken standalone fallback (`instance.rs:60-67`):** `forward()` ignores the result of `stream.read(&mut ack)` and never checks `ack == [b'k']`. If any unrelated local service listens on `127.0.0.1:47811` and accepts TCP, `write_all` succeeds, `forward()` returns `true`, and `xisf2png-gui` exits silently instead of falling back to standalone mode.
  2. **Multi-user & unauthenticated localhost port (`instance.rs:18, 28`):** Binding a fixed unauthenticated port on `127.0.0.1` allows any local process (or another user on a shared host) to inject arbitrary file paths into the GUI and steal window focus. On a multi-user machine, User B launching `xisf2png-gui` forwards User B's file paths to User A's running window and exits.
  3. **Unbounded read / head-of-line blocking (`instance.rs:76-97`):** `serve()` reads lines with no byte/line cap (a local client streaming data without closing can OOM the GUI) and handles connections sequentially on one thread (a stalled client blocks legitimate handoffs for 2s+).
  4. **Lossy UTF-8 / newline framing (`instance.rs:57-58, 84-90`):** `to_string_lossy()` corrupts non-UTF-8 Unix paths, and filenames containing `\n` split into bogus entries.
- **Recommendation:**
  - Verify `matches!(stream.read(&mut ack), Ok(1)) && ack[0] == b'k'` before returning `true` in `forward()`.
  - Prefer a per-user IPC mechanism (Unix domain socket in `$XDG_RUNTIME_DIR` / `$TMPDIR` with `0600` perms on Unix; per-user Named Pipe on Windows), or at minimum store a per-user random port + auth token in a `0600` runtime file.
  - Cap incoming payload size (e.g., `reader.take(1024 * 1024)`) and encode paths safely (length-prefixed or NUL-delimited bytes).

---

### Medium Severity

#### M1. Broken `--help` formatting and incomplete usage synopsis in CLI
- **Where:** `src/main.rs:181-183`, `src/main.rs:201-203`
- **Issue:** Lines 201–203 in `usage()` are missing trailing `\n\` continuations, causing literal indentation spaces to appear in `--help` output for `--no-ask` and `--png-only` (verified by running `xisf2png --help`). Additionally, the `Usage:` synopsis lines omit `[--font <file>]` (and `--no-ask` / `-j N` on lines 182–183), and the `--no-ask` description does not reflect `v0.9.1` asking when lookups fail.
- **Recommendation:** Add `\n\` to lines 201–203, align the synopsis with `README.md:84-86`, and update the `--no-ask` description.

#### M2. GUI button renders literal `"Resize && stamp PNGs"`
- **Where:** `src/bin/gui/main.rs:682`
- **Issue:** `"Resize && stamp PNGs"` escapes `&` as if for Win32/Qt mnemonics, but `egui` renders text literally, displaying a double ampersand (`&&`) on the primary action button in PNG-only mode.
- **Recommendation:** Change to `"Resize & stamp PNGs"`.

#### M3. GUI `file_mode` + `png_only` state conflict locks `resize4k` checkbox
- **Where:** `src/bin/gui/main.rs:273-276`, `src/bin/gui/main.rs:582-597`
- **Issue:** If the user ticks **PNG only** and then adds/drops explicit files (`file_mode = true`), the `png_only` checkbox is disabled while remaining `true`. Because `resize4k`'s enabled state checks `!self.png_only` (not `!(self.png_only && !file_mode)`), the user is locked out of toggling `resize4k`, and `App::options()` still passes `png_only: true` and `recursive: self.recursive` to `run_asking`.
- **Recommendation:** Guard `resize4k` with `!(self.png_only && !file_mode)`, and in `App::options()` pass `png_only: !file_mode && self.png_only` and `recursive: !file_mode && self.recursive`.

#### M4. Windows release GUI silently drops stdout for scriptable CLI flags
- **Where:** `src/bin/gui/main.rs:5`, `src/bin/gui/main.rs:21-38`, `README.md:72-73`
- **Issue:** `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` detaches the Windows release binary from its parent console. Running `xisf2png-gui --install-context-menu`, `--uninstall-context-menu`, or `--context-menu-status` from cmd/PowerShell prints nothing to the terminal unless stdout is explicitly piped/redirected.
- **Recommendation:** On Windows, attach to the parent console via `AttachConsole(ATTACH_PARENT_PROCESS)` before printing in the `--*-context-menu*` flag branch (or move/expose these flags on the console `xisf2png` binary as well).

#### M5. No graceful shutdown on window close (GUI) or Ctrl-C / SIGINT (CLI)
- **Where:** `src/main.rs:118`, `src/bin/gui/main.rs:338-364`
- **Issues:**
  1. `src/main.rs:118` passes `&AtomicBool::new(false)`, so `Ctrl-C` terminates worker threads immediately without letting in-flight file writes complete.
  2. `App` in `src/bin/gui/main.rs` has no `on_exit` / `Drop` hook to set `job.cancel = true` and join the worker thread when the window closes.
- **Impact:** Abrupt exit mid-write can leave truncated `.png` files, corrupt existing PNGs during in-place `--png-only` edits, or partially write `names.txt`.
- **Recommendation:** In the GUI, implement `eframe::App::on_exit` (or `Drop` for `Job`) to signal `cancel`, dismiss any pending `name_prompt` (`None`), and join the worker thread. In the CLI, install a lightweight Ctrl-C handler (e.g., `ctrlc`) wired to the `AtomicBool` cancel flag.

#### M6. CLI positional argument typos silently create directories, and CLI panics on non-UTF-8 args
- **Where:** `src/main.rs:11`, `src/main.rs:79-106`
- **Issues:**
  1. `xisf2png a.xisf typo.xisf` treats non-existent `typo.xisf` as `output_dir` (because `!Path::new("typo.xisf").is_file()`), creating a folder named `typo.xisf/` and writing `typo.xisf/a.png` with exit code `0`. Same if the first file has a typo (`xisf2png typo.xisf b.xisf`).
  2. There is no `--` end-of-options separator, so paths starting with `-` (e.g., `-20C_dark.fits`) are rejected as unknown options.
  3. `--concurrency=N` is rejected (while `--font=FILE` and `-jN` are accepted).
  4. `std::env::args()` panics on Unix if any path argument is non-UTF-8 (whereas `gui/instance.rs:103` correctly uses `args_os()`).
- **Recommendation:** Parse with `args_os()`, support `--` and `--concurrency=N`, and reject arguments that look like missing files (e.g., have a known image extension when `opts.files` is already non-empty, or warn/error when an input path doesn't exist rather than promoting it to `output_dir`).

#### M7. Linux `.desktop` `Exec=` escaping and unset-`HOME` fallback bugs
- **Where:** `src/bin/gui/shell.rs:114-121`, `src/bin/gui/shell.rs:148-159`
- **Issues:**
  1. `Exec="{}" %F` does not escape `"`, `` ` ``, `$`, `\`, or `%` (`%%`) as required by the FreeDesktop Desktop Entry Specification. Installing the binary in a path containing those characters produces a malformed or misbehaving `.desktop` file.
  2. When both `XDG_DATA_HOME` and `HOME` are unset, `data_home()` falls back to `PathBuf::from("").join(".local/share")` — a relative path in the current working directory.
- **Recommendation:** Implement FreeDesktop `Exec` quoting/escaping and return an explicit `Err` from `data_home()` if neither `XDG_DATA_HOME` nor a non-empty `HOME` is set.

#### M8. Missing `LICENSE` file, missing `Cargo.toml` license metadata, and missing DejaVu license notice in release archives
- **Where:** Repo root, `Cargo.toml`, `build.rs:20-23`, `README.md:326-328`, `.github/workflows/release.yml:90-158`
- **Issue:**
  1. `build.rs` declares `"open source"`, yet there is no `LICENSE` file in the repository and no `license` field in `Cargo.toml`. Without an explicit license file, default "all rights reserved" copyright applies.
  2. The binaries embed DejaVu Sans Condensed Bold (via `astropng-core`), whose Bitstream Vera / DejaVu license requires reproducing the copyright and license text in binary distributions. Release archives currently bundle only `README.md`, not `LICENSE-DejaVu.txt`.
- **Recommendation:** Add a `LICENSE` file, populate `license` and `repository` in `Cargo.toml`, and include `LICENSE` and `LICENSE-DejaVu.txt` in all release archives.

#### M9. Zero automated tests and no PR/push CI workflow
- **Where:** Repository-wide, `.github/workflows/`
- **Issue:** `cargo test` executes `0` tests across all targets, and there is no CI workflow running on pushes or pull requests (`release.yml` only triggers on `v*` tags / manual dispatch and only runs `xisf2png --version`). Several bugs found here (`usage()` formatting, `instance::forward()` ACK check, `shell.rs` escaping, `App::options()` `file_mode` interaction) are directly preventable with fast unit tests.
- **Recommendation:** Add unit tests for CLI arg parsing / help output, `instance.rs` handoff protocol, `shell.rs` `.desktop` formatting, and `App` state transitions, plus a lightweight `ci.yml` running `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` on PRs and pushes to `main`.

---

### Low Severity & Maintainability

#### L1. Unused direct dependency `image` in `Cargo.toml`
- **Where:** `Cargo.toml:21`, `README.md:322`
- **Issue:** `image` is declared in `[dependencies]` and listed in `README.md`, but no file in `xisf2png`'s `src/` imports or calls it (`astropng-core` owns image processing, and `eframe::icon_data::from_png_bytes` uses `eframe`'s own `image` dependency).
- **Recommendation:** Remove `image` from `Cargo.toml` and update the `README.md` dependency table.

#### L2. GUI crates (`eframe`, `rfd`, `winreg`) are not feature-gated
- **Where:** `Cargo.toml:15-28`, `.github/workflows/release.yml:54-70`
- **Issue:** Because `eframe`, `rfd`, and `winreg` are unconditional `[dependencies]`, building only the CLI (`--bin xisf2png`, including static `musl` builds) still resolves and compiles GUI dependencies.
- **Recommendation:** Gate GUI dependencies behind a default `gui` Cargo feature (`required-features = ["gui"]` on `xisf2png-gui`) so CLI builds can use `--no-default-features --bin xisf2png` (faster CI builds and no `libwayland-dev`/`libxkbcommon-dev` needed for CLI-only builds).

#### L3. Compiler `dead_code` warning on macOS
- **Where:** `src/bin/gui/shell.rs:22`
- **Issue:** `fn exe_path()` is unused on macOS, producing a default compiler warning on every macOS build.
- **Recommendation:** Annotate with `#[cfg(not(target_os = "macos"))]`.

#### L4. Launching a second GUI instance with no arguments does not focus the existing window
- **Where:** `src/bin/gui/instance.rs:93`
- **Issue:** `serve()` only sends `ViewportCommand::Focus` when `!paths.is_empty()`. Re-launching `xisf2png-gui` without files silently exits without raising an already-open (possibly minimized/occluded) window.
- **Recommendation:** Send `ViewportCommand::Focus` (and `request_repaint()`) even when `paths` is empty.

#### L5. `auto_zoom` exact float equality can fight manual `Ctrl +/-` zoom
- **Where:** `src/bin/gui/main.rs:127-146`
- **Issue:** `auto_zoom` derives physical monitor pixels via `(monitor * native_ppp * current).round()` and compares with exact equality (`state.applied_for == Some(physical)`). When a user manually zooms (`Ctrl +/-`), integer/float rounding of `monitor` can shift `physical` by ≥1 px, causing `auto_zoom` to reset the user's manual zoom two frames later.
- **Recommendation:** Compare against `state.applied_for` with a small pixel tolerance (e.g., `<= 4.0` px per axis) before re-triggering auto-zoom.

#### L6. Minor GUI UX rough edges
- **Where:** `src/bin/gui/main.rs`
- **Issues:**
  - **Dropping a folder in `file_mode` (`main.rs:286-294`):** Updates `input_dir` silently while keeping the UI in `file_mode` (where `input_dir` is hidden). Either clear `self.files` when a folder is dropped or inform the user.
  - **Incoming drops/IPC mutate form while running (`main.rs:471`):** `collect_incoming()` mutates `self.files`/`self.input_dir` even while the form is disabled during a batch.
  - **Stale summary wording (`main.rs:692-695`):** Changing `png_only` or files after a run retroactively changes the displayed summary's verb (`Converted` ↔ `Processed`) and `"No ... files found."` text.
  - **No per-file removal or path deduplication (`main.rs:290, 504-521`):** Users cannot remove individual files from the list (only `Clear`), and `!self.files.contains(&p)` does not deduplicate equivalent paths.
  - **Unvirtualized log list (`main.rs:727-765`):** Rendering all rows every frame can slow down for large batches (hundreds/thousands of subs); `ScrollArea::show_rows` or capping displayed lines helps.
  - **No `--help`/`--version` on `xisf2png-gui` (`main.rs:21-38`):** `xisf2png-gui --version` exits with code 1 and an error message.

#### L7. `is_installed()` does not detect moved binaries or partial Windows installs
- **Where:** `src/bin/gui/shell.rs:47-51`, `src/bin/gui/shell.rs:132-134`
- **Issue:** On Linux, `is_installed()` only checks whether `xisf2png.desktop` exists; on Windows, it only checks `.xisf`. If the user moves the binary, the UI reports it as installed instead of offering to update the path.

#### L8. `delete-merged-branches.yml` matches by commit SHA only
- **Where:** `.github/workflows/delete-merged-branches.yml:43, 59`
- **Issue:** Checking `!mergedShas.has(b.commit.sha)` without matching the branch name could delete an unrelated new branch that was just created at a previously merged PR's tip commit before any new commits are pushed. Also, `pull_request` events paginate all historical PRs (`state: "all"`) even though only `state: "open"` + the event's PR SHA are needed.
- **Recommendation:** Match `(head.ref, head.sha)` pairs (or verify the branch name matches a merged PR's `head.ref`).

#### L9. `purge-green-runs.yml` deletes `Release` workflow build logs
- **Where:** `.github/workflows/purge-green-runs.yml:17`
- **Issue:** Automatically deleting logs of green `Release` runs discards build, codesign, and AppImage packaging logs for published binaries, making post-release verification and regression debugging harder.
- **Recommendation:** Consider excluding `"Release"` from automatic log deletion (keeping it for housekeeping workflows like `"Delete merged branches"`).

#### L10. Packaging, documentation, and repo hygiene minor items
- **AppImage omitted from `README.md` (`README.md:20-26`):** `release.yml` publishes `xisf2png-gui-linux-x86_64.AppImage`, but the install table only documents the four `.zip`/`.tar.gz` archives.
- **AppImage `.desktop` file incomplete (`release.yml:173-180`):** Missing `%F`, `Photography;`, and `MimeType=image/fits;application/fits;image/x-fits;application/x-xisf;`.
- **Standalone fallback creates an unused socket/thread (`instance.rs:35`):** `None.or_else(|| TcpListener::bind(("127.0.0.1", 0)).ok())` binds an unreachable ephemeral port and spawns an idle thread.
- **Identifier consistency (`release.yml:138`, `build.rs:20-23`):** `CFBundleIdentifier` is `io.github.peterbuitho.xisf2png` and Windows `CompanyName` is `peterbuitho (open source)`, while the GitHub repo is `ntmb-fb/xisf2png` (flagging in case `ntmb-fb` was intended everywhere).
- **`.gitignore` missing `.DS_Store`:** An untracked `.DS_Store` exists in the repo root; adding `.DS_Store` to `.gitignore` avoids accidental commits.
- **Duplicate transitive crates in `Cargo.lock`:** Normal for `eframe`/`winit`/`rfd`, though `base64` (`0.22.1` in `astropng-core` vs `0.23.1` in `ureq`) can be unified in `astropng-core` when convenient.

---

## Suggested Priority Plan

1. **Security & CI hardening:** Fix `release.yml` permissions scoping, pin & checksum-verify `appimagetool`/`linuxdeploy`, pin Actions by SHA, and add a `ci.yml` (`cargo test`, `cargo clippy`, `cargo fmt`, `cargo-audit`).
2. **Correctness quick wins:** Fix `instance.rs` ACK check + empty-args focus, `shell.rs` `$APPIMAGE` + `Exec=` escaping + macOS `#[cfg]` warning, `src/main.rs` `--help` `\n\`, and `src/bin/gui/main.rs` `"Resize & stamp PNGs"` + `file_mode`/`png_only` gating.
3. **Reliability & IPC:** Add graceful shutdown (`on_exit` in GUI, Ctrl-C handler in CLI), move single-instance IPC to per-user socket/pipe with bounded reads, and tighten CLI positional arg parsing.
4. **Packaging, licensing & build hygiene:** Add `LICENSE` + `Cargo.toml` license metadata, bundle `LICENSE-DejaVu.txt` in release archives, remove unused `image` dep, feature-gate GUI deps, document the AppImage in `README.md`, and add unit tests.
