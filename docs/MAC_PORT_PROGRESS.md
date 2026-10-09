# Ivy for Mac: progress and handoff

Written 2026-10-10 by Claude (on Yash's Windows PC) for whoever picks this up next, most likely Claude running on
the test MacBook. You have none of the Windows session's memory, so everything you need is here. Read it all before
changing anything.

## 1. What this is

**Ivy** is Yash's (Yashvanraj's) fully offline dictation app: hold a key, speak, let go, and clean text is typed
where the cursor is. One speech model (Ivy lite, a fine-tuned Qwen3-ASR-1.7B, run with llama.cpp), formatting
rulebooks, history, tones. Rust + Tauri v2 backend, React 19 + TypeScript + Tailwind 4 frontend.

- **Windows:** v0.2.8 is released and used by Yash's friends. It lives on the `main` branch. Don't touch it.
- **Mac:** this port, built overnight on 2026-10-10. Branch `macos-port` of https://github.com/raj-7676/IVY- . It is
  a **test build**, version 0.2.8 like Windows. There is **no GitHub release** for it, and there won't be until it works.
  Yash zips the `.dmg` and gives it to a friend with an Apple-silicon Mac.

## 2. How to work with Yash (his standing rules)

- Explain in plain, non-technical words. End every block of work with a short "what, why, result" summary.
- **Nothing may be a gimmick:** no invented numbers, no control that only pretends to work, and no UI copy that
  claims something the app doesn't do. A control that can't work is removed rather than caveated.
- **One model only.** Don't propose other or bigger models.
- **Git:** commits are authored as `Yashvanraj` (`git -c user.name=Yashvanraj commit ...`). **Never** add a Claude
  co-author line or a "Generated with Claude Code" line to commits, PRs or releases. Push only the `macos-port`
  branch. Never push or merge into `main`, and never publish a GitHub release unless Yash says so.
- **Don't break Windows.** Every Mac change sits behind `#[cfg(target_os = "macos")]` (Rust) or `IS_MAC` (frontend).
  On Windows, the suite is `cargo test --lib -- --test-threads=1` in `src-tauri` (70 passed, 0 failed, 2 ignored on
  this branch).
- **Verify live before claiming a fix.** A UI fix isn't done until the new build runs on the Mac and Yash (or you)
  saw it work.
- Long jobs: keep a live progress note in a file and don't saturate the machine.
- Quality over speed: pick the more accurate option after measuring.

## 3. Where the code is

- GitHub: `git clone -b macos-port https://github.com/raj-7676/IVY-.git` (the repo is public).
- On Yash's Windows PC: worktree `D:\Dev\CODE\IVY_Transcriber-mac` (branch `macos-port`); the Windows repo is
  `D:\Dev\CODE\IVY_Transcriber` (`main`). The lab's running notes are in
  `C:\Users\YASH\Downloads\IVY_decision_lab\LAB_PROGRESS.md` (newest ">>> RESUME HERE" section at the end).
- Engineering notes: `IVY.md` (§19 = what the Mac port does, §5 = traps, including the macOS ones). Tester guide:
  `docs/MAC_TESTING.md`.

Commits on `macos-port` (newest first):

| Commit | What |
|---|---|
| a339900 | Keep llama.cpp's default attention; document the virtual-GPU finding |
| 7b297f4 | Crash guard for the GPU check; only overlay clicks hand the front back to your app |
| 4ea7e92 | Native Mac window buttons (title bar overlay); experiment with flash attention, reverted in a339900 |
| eda1e6b | Menu bar icon as a template silhouette of the logo |
| 32b9dac | Metal self-check with CPU fallback, hotkey-conflict banner, App Nap off, docs |
| 983951b | Main port: native integration, Mac UI, model check in CI |
| 2eca971 | Platform-gated dependencies, Mac CI |

## 4. What the Mac port does (file map)

**Backend**
- `src-tauri/src/macos.rs`: everything Mac-only. Covers:
  - Which app is in front (`frontmost_pid`; Accessibility API, NSWorkspace fallback), its name, bundle file name and
    focused window title.
  - Finder's focused element role, for the "is there a text box" check.
  - Cmd+V and Cmd+Z as Core Graphics events carrying only Command (`press_cmd`), and waiting for Option to be let go.
  - The Control + Shift key state.
  - Permissions: Accessibility (`is_trusted`, `prompt_trust`) and Microphone (`mic_permission`), plus opening System
    Settings.
  - Activating an app (`activate`, `bring_to_front`).
  - Running apps for the Tone screen.
  - The chip's name.
  - Opting out of App Nap.
  - The capsule overlay window (`capsule_setup`, `capsule_show`, main thread only).
- `src-tauri/src/lib.rs`: `#[cfg(target_os = "macos")]` branches next to each Windows twin:
  - Foreground helpers, `has_text_focus`, `paste_text`, `paste_manual_clipboard`, `touch_up_transcript`,
    `retry_transcription(from_capsule)`, `cancel_dictation`.
  - Capsule setup, show and position (below the menu bar).
  - `models_dir` (the model lives in `~/Library/Application Support/app.ivy.dictation/models`).
  - `should_use_gpu`: always Metal unless the Metal check failed.
  - `metal_check`, `remember_metal_crash` and the marker files.
  - Commands: `get_permissions`, `request_permission`, `list_running_apps`, `get_hotkey_problem`.
  - Dock reopen (`RunEvent::Reopen`).
  - Menu bar template icon (`icons/tray-mac.png`).
- `src-tauri/src/modifier_hotkey.rs`: the Control + Shift watcher reads the Mac keyboard state.
- `src-tauri/src/update.rs`: on a Mac, the update is the release's `Ivy_<version>_aarch64.dmg`, checksum-verified,
  then opened. Ivy quits so the new app can be dragged into Applications.
- `src-tauri/src/gpu_monitor.rs`: Windows-only GPU sharing is off on a Mac; it reports the chip's name.
- Config:
  - `tauri.conf.json`: `macOSPrivateApi`, macOS 13+, ad-hoc signing (`signingIdentity: "-"`), `Entitlements.plist`.
  - `tauri.macos.conf.json`: main window with a native title bar overlay; traffic lights at x 18, y 22.
  - `Info.plist`: microphone text.
  - `Entitlements.plist`: audio-input, needed under the hardened runtime.

**Frontend**
- `src/utils/platform.ts`: `IS_MAC`, `DEVICE` ("PC"/"Mac"), `MOD` (Ctrl/Cmd), and `keyLabel()`. Saved shortcuts stay
  "Alt + Space"; on screen they read "⌥ Option + Space".
- `src/components/MacPermissions.tsx`: live permission rows (polled every 1.5 s) with the one button that moves
  each forward.
- Wizard step 3 on a Mac is "Allow Ivy" (permissions) instead of "GPU or CPU". Yash decided a Mac has one chip, so
  there's no GPU/CPU choice and no GPU sharing anywhere on a Mac.
- Settings on a Mac:
  - A Permissions section.
  - "Runs on your Mac's chip" with the chip's name, instead of the GPU/CPU switch, GPU sharing and the load bar.
  - Mac wording for Launch at Startup and Updates.
- Main window:
  - Header starts after the native traffic lights (`pl-[88px]`).
  - HUD glass for "Background blur".
  - An Accessibility banner, and a banner when the shortcut didn't register (Raycast, Alfred and ChatGPT all default
    to Option + Space).
- Tone: "Add app" lists the running apps (an `.app` can't go through a file picker).
- Mac key names and Mac wording everywhere; intro sound as AAC (old WebKit can't decode Ogg Opus).

**CI:** `.github/workflows/macos.yml` on GitHub's `macos-14` runners:
- Unit tests, then an ad-hoc-signed `Ivy_0.2.8_aarch64.dmg` artifact (14 days).
- A model check: downloads the 2.4 GB model, transcribes `src-tauri/tests/fixtures/*.wav`, and compares the text
  with the Windows output.

## 5. Status (2026-10-10 morning)

- Windows: unaffected. 70 passed, 0 failed, 2 ignored.
- Mac on GitHub:
  - Unit tests: 69 passed, 0 failed.
  - The `.dmg` builds. Checked from Windows: arm64 binary; Info.plist has the mic text and `LSMinimumSystemVersion`
    13.0; ad-hoc signature with the hardened runtime and the audio-input entitlement.
- **The model on the Mac CPU writes exactly the Windows text** for both test clips:
  - "The quick brown fox jumps over the lazy dog."
  - "Delete requirements.rs, we don't need it anymore."
- **Metal is unverified on real hardware.** GitHub's virtual Macs have an "Apple Paravirtual device" GPU of the
  Apple5 family, without simdgroup reduction or matrix multiply (every real M-series GPU is Apple7+). There,
  llama.cpp's fallback kernels write "!!!!", with flash attention on or off.
- So the app checks Metal itself, once per run, on a known clip:
  - A pass means dictation runs on the GPU.
  - A fail means dictation runs on the CPU, and Settings says so.
  - A crash during the check leaves a marker file, and that build stays on the CPU at the next start.
- Nothing has run on a real Mac yet.

## 6. First things to check on the real Mac (in this order)

1. **Does it open?** If not, run it from Terminal to see errors: `/Applications/Ivy.app/Contents/MacOS/app`. Crash
   reports are in Console.app or `~/Library/Logs/DiagnosticReports/` (files named `app-*.ips`).
2. **Ivy's own log:** `~/Library/Application Support/app.ivy.dictation/debug.log`. After the model download, look for:
   - `Metal check passed: dictating on the Mac's GPU` (good), or
   - `Metal check failed (heard "...")` (the GPU path is broken on real hardware; investigate `lite.rs` on Metal), or
   - `Ivy stopped during its last GPU check` (a crash inside Metal last time).

   Tauri's log is in `~/Library/Logs/app.ivy.dictation/`.
3. **Permissions:** wizard step 3. Accessibility and Microphone should both turn green within 2 s of allowing.
4. **Dictation:**
   - Hold ⌥ Option + Space in TextEdit or Notes, speak, and let go. Text appears; the capsule says "Pasted to TextEdit".
   - Then try a browser, Slack and VS Code.
5. Everything else on the checklist in `docs/MAC_TESTING.md`.

## 7. Building and testing on the Mac itself

```bash
xcode-select --install                     # Apple's compilers
brew install cmake ninja node              # Homebrew: https://brew.sh
curl https://sh.rustup.rs -sSf | sh        # Rust
git clone -b macos-port https://github.com/raj-7676/IVY-.git ivy && cd ivy
npm ci && npm run build                    # tauri-build needs dist/
cd src-tauri && cargo test --lib -- --test-threads=1
```

The decisive check is the model on the **real** Apple GPU (about 2.4 GB download):

```bash
mkdir -p ~/ivy-models/ivy-lite && cd ~/ivy-models/ivy-lite
base=https://github.com/raj-7676/IVY-/releases/download/v0.2.6
curl -fL -o mmproj-ivy-lite-f16.gguf "$base/mmproj-ivy-lite-f16.gguf"
curl -fL -o ivy-lite-Q8_0.gguf "$base/ivy-lite-Q8_0.gguf"
shasum -a 256 *.gguf   # 07ed1cc9...fc00 mmproj, da50c4dc...95ed model
cd -   # back to src-tauri
IVY_MODELS_DIR=~/ivy-models IVY_REQUIRE_METAL=1 cargo test --lib model_matches_reference -- --ignored --nocapture
```

`IVY_REQUIRE_METAL=1` makes the Metal result count. Expect "Metal" lines with the two reference sentences.

Running the app from source: `npm run tauri dev` from the repo root (debug builds read the model from
`src-tauri/models/ivy-lite/`, so copy or link the two files there). Caution: run from Terminal, the Accessibility
and Microphone permissions belong to **Terminal**, not Ivy. To build the disk image:
`npx tauri build --bundles app,dmg` (output in `src-tauri/target/release/bundle/dmg/`).

## 8. Known limits and risks

- **Ad-hoc signing** (no $99 Apple account; Yash, 2026-10-10):
  - First launch needs "Open Anyway" in System Settings › Privacy & Security.
  - Every new build is a new app to macOS's permission system, so Accessibility must be switched off and on again.
- A click on the capsule makes Ivy the active app (a Tauri window can't be a non-activating panel). Touch Up, Retry
  and X hand the front back; any other click on the capsule leaves Ivy in front.
- Only Finder is asked whether a text box has focus; every other app is assumed to have one.
- Cmd+V uses the QWERTY key position; a plain Dvorak layout would send another letter.
- Option + Space may be taken by Raycast, Alfred or ChatGPT. The main window shows a banner; pick Control + Shift
  in Settings.
- Run Ivy from Applications. Started from the disk image or Downloads, "launch at login" points at a temporary copy.
- Not done on a Mac: Intel Macs, notarization, the full-screen sleep and GPU-load rules, Caps Lock as a key.
- Not verified on a real Mac yet: everything. GitHub can't run a microphone, hotkeys or another app to paste into.

## 9. When you change something

1. Make the change behind the platform switch. Keep Windows identical.
2. On the Mac: `cargo test --lib -- --test-threads=1`, then `npx tsc --noEmit` and `npm run build` in the repo root.
3. Commit as Yashvanraj (no Claude attribution) and push `macos-port`. GitHub builds a new `.dmg` (Actions tab →
   "macOS Build" → artifact `Ivy-macOS-AppleSilicon`), or build it locally with `npx tauri build --bundles app,dmg`.
4. Update this file, `IVY.md` §19/§5 and `docs/MAC_TESTING.md` when behaviour changes.
5. Tell Yash in plain words what changed and what to try.
