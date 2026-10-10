# Ivy for Linux: progress and handoff

Written 2026-10-10 by Claude (on Yash's Windows PC) for whoever picks this up next, for example Claude running on a
Linux PC. You have none of the Windows session's memory, so everything you need is here. Read it all before
changing anything.

## 1. What this is

**Ivy** is Yash's (Yashvanraj's) fully offline dictation app: hold a key, speak, let go, and clean text is typed
where the cursor is. One speech model (Ivy lite, a fine-tuned Qwen3-ASR-1.7B, run with llama.cpp), formatting
rulebooks, history, tones. Rust + Tauri v2 backend, React 19 + TypeScript + Tailwind 4 frontend.

- **Windows:** v0.2.8 is released and used by Yash's friends. It lives on the `main` branch. Don't touch it.
- **Mac:** branch `macos-port` (`docs/MAC_PORT_PROGRESS.md`), waiting for its first test on a real Mac.
- **Linux:** this port, built on 2026-10-10. Branch `linux-port` of https://github.com/raj-7676/IVY- , started from
  `macos-port` (commit ea08c1d), so it carries the Mac code too and both ports can go into `main` together. It is a
  **test build**, version 0.2.8 like Windows. There is **no GitHub release** for it.
- **Targets:** 64-bit PCs (x86-64) with Ubuntu 22.04 or later, Debian 12, Linux Mint 21 or later, Fedora 37 or later.
  It's built against Ubuntu 22.04's glibc (2.35). Packages: `Ivy_0.2.8_amd64.deb` and `Ivy-0.2.8-1.x86_64.rpm`.

## 2. How to work with Yash (his standing rules)

- Explain in plain, non-technical words. End every block of work with a short "what, why, result" summary.
- **Nothing may be a gimmick:** no invented numbers, no control that only pretends to work, and no UI copy that
  claims something the app doesn't do. A control that can't work is removed rather than caveated.
- **One model only.** Don't propose other or bigger models.
- **Git:** commits are authored as `Yashvanraj` (`git -c user.name=Yashvanraj commit ...`). **Never** add a Claude
  co-author line or a "Generated with Claude Code" line to commits, PRs or releases. Push only the `linux-port`
  branch. Never push or merge into `main`, and never publish a GitHub release unless Yash says so.
- **Don't break Windows.** Every Linux change sits behind `#[cfg(target_os = "linux")]` (Rust) or `IS_LINUX`
  (frontend). On Windows, the suite is `cargo test --lib -- --test-threads=1` in `src-tauri` (70 passed, 0 failed,
  2 ignored on this branch, the same as before the port).
- **Verify live before claiming a fix.** A UI fix isn't done until the new build ran and someone saw it work.
- **Double-check everything** (Yash, after the Mac port turned up many errors): re-read every changed line, run the
  tests on every OS, run the app live.
- Long jobs: keep a live progress note in a file and don't saturate the machine.

## 3. Where the code is

- GitHub: `git clone -b linux-port https://github.com/raj-7676/IVY-.git` (the repo is public).
- On Yash's Windows PC: worktree `D:\Dev\CODE\IVY_Transcriber-linux` (branch `linux-port`). The Windows repo is
  `D:\Dev\CODE\IVY_Transcriber` (`main`), the Mac worktree `D:\Dev\CODE\IVY_Transcriber-mac`. The lab's running
  notes are in `C:\Users\YASH\Downloads\IVY_decision_lab\LAB_PROGRESS.md` (newest ">>> RESUME HERE" at the end).
- Engineering notes: `IVY.md` (§19 = what the Linux port does, §5 = traps, including the Linux ones). Tester guide:
  `docs/LINUX_TESTING.md`.

## 4. How Ivy works on Linux

Linux desktops come in two kinds, and nearly every Linux decision follows from that:

- **X11** (Linux Mint, "Ubuntu on Xorg"): any app may see which window is in front, read the keyboard and press
  keys. Ivy does everything as on Windows.
- **Wayland** (Ubuntu's and Fedora's default): no app may do any of that, nor place a window or keep it above
  others. Ivy still runs as an X11 window there, through XWayland (`main.rs` sets `GDK_BACKEND=x11`), so the capsule
  can float over other apps. It can see and type into other X11 windows, and leaves the rest of the keyboard to its
  helper `ivy-keys` (below).

What that means, piece by piece:

- **Window in front:** `_NET_ACTIVE_WINDOW`; its program from `WM_CLASS` ("Slack", "firefox", "Code"); Ivy's own
  windows by `_NET_WM_PID`; the desktop's icon window (`_NET_WM_WINDOW_TYPE_DESKTOP`) takes no paste, like Explorer.
  On Wayland a native Wayland app reads as `WAYLAND_APP` (-1): it has no name (the capsule says "Pasted"), can't
  match a Tone app list and gets no Touch Up, but the paste still goes ahead.
- **Keys:** the Ctrl + Shift watcher (`modifier_hotkey.rs`) polls `QueryKeymap` on X11, or ivy-keys on Wayland.
  Alt + Space and Alt + V are X11 key grabs (global-hotkey). The Linux default key is **Ctrl + Shift**: Alt + Space
  is the window menu on GNOME, Cinnamon and Xfce, and KRunner on KDE. On Wayland only Ctrl + Shift exists (no app may
  claim a key there); a saved Alt + Space switches to it at start, and there is no Alt + V.
- **Paste:** Shift+Insert, because terminals take it too (Ctrl+V types ^V there). XTest into X11 windows, ivy-keys'
  virtual keyboard into Wayland apps. The text goes on the clipboard and on the selection (some terminals paste the
  selection), marked for clipboard managers to skip, and both go back after 800 ms if they still hold Ivy's text.
  Held modifiers are waited out (350 ms) first, and released synthetically on X11.
- **Touch Up:** Ctrl+Z through XTest, X11 windows only (`DictationComplete.touch_up` tells the capsule).
- **Capsule:** an override-redirect X11 window (`make_window_non_activating`), which no window manager focuses or
  decorates; placed in the work area, below a top panel.
- **GPU:** llama.cpp's Vulkan backend. `linux::gpu()` asks llama.cpp for its devices; software Vulkan (llvmpipe)
  doesn't count. The first GPU load of each run is checked on a known clip (`gpu_check`, shared with the Mac's Metal
  check). A wrong answer, or a crash during the check (`gpu-check.running` found at the next start, then
  `gpu-disabled`), leaves that build on the CPU. The battery rule works as on Windows (`/sys/class/power_supply`).
  The full-screen sleep and GPU-sharing rules read Windows' counters, so they don't exist on Linux. Without a usable
  GPU, Settings and the wizard say so and grey out "GPU".
- **Model files:** `~/.local/share/app.ivy.dictation/models` (the package's own files belong to root).
- **Updates:** `rpm -qf` on Ivy's program decides between the release's `.rpm` and `.deb`. It's downloaded and
  checked against `SHA256SUMS.txt` as on Windows, installed with `pkexec apt-get install -y` or
  `pkexec rpm -U --replacepkgs` (the system's password prompt), then the new Ivy starts. A release without a Linux
  package isn't offered as an update.
- **WebKitGTK:** `WEBKIT_DISABLE_DMABUF_RENDERER=1` (its DMA-BUF renderer leaves the window blank on some NVIDIA
  drivers), unless the user set it.

**ivy-keys** (`src-tauri/linux/ivy-keys`, std and libc only). Yash asked for "what most people do": this is the
pattern of ydotool's and keyd's helpers.

- Installed as `/usr/libexec/ivy-keys`, setgid `input`, by `/usr/libexec/ivy-keys-setup`. The packages' postinst
  runs the setup script, and so does "Fix keyboard access" in Ivy, through pkexec. The udev rule
  `60-ivy-keys.rules` gives `/dev/uinput` to the same group. Users are never added to the group.
- It talks over stdin/stdout: `ready <devices> <denied> <uinput>`, `keys <bits>`, and `paste`, answered `pasted 1`
  or `pasted 0`. It exits when Ivy closes its stdin.
- Any program may run it, so it tells and accepts little:
  - Ctrl, Shift, Alt and Super bits.
  - "Another key or button" only while Ctrl and Shift are both held, which is all the watcher needs to tell its
    key from Ctrl+Shift+T. It never says which key, nor the rhythm of typing.
  - Nothing at all, and no pastes, unless logind's record of the seat (`/run/systemd/seats/seat0`, `ACTIVE_UID`)
    says the user who started it is the one at the screen. SSH sessions and switched-away users get nothing.
  - Its virtual keyboard has two keys: Shift and Insert.
- Ivy runs it for as long as Ivy runs (restarting it with a growing wait), and shows its state (`get_desktop`:
  ready, starting, missing, denied, no-keyboard, no-paste). When it doesn't work, a banner (`LinuxKeyboard.tsx`)
  says why in plain words, with the one fix there is.

## 5. File map

**Backend**
- `src-tauri/src/linux.rs`: everything Linux-only. Covers:
  - The X11 connection, window in front, its class, title and process, the desktop window, the open apps.
  - The keyboard state (X11 or ivy-keys), XTest presses, releasing held modifiers.
  - The clipboard and the selection (one arboard handle is kept for the whole run: arboard empties what it put
    there when its last handle closes).
  - Battery state; the GPU llama.cpp sees.
  - Running ivy-keys and repairing it.
- `src-tauri/src/lib.rs`: `#[cfg(target_os = "linux")]` branches next to each Windows twin:
  - Foreground helpers, `paste_text`, `paste_manual_clipboard`, `touch_up_transcript`, the capsule window,
    `models_dir`, `should_use_gpu`, the GPU check, `get_hardware_status`, `list_running_apps`, the Linux preset
    apps, the hotkey wording.
  - `get_desktop` (all OSes; it differs only on a Linux Wayland desktop) and `request_permission("keyboard")`.
  - `can_claim_keys()`: false on Wayland, so Alt + Space and Alt + V are refused there.
- `src-tauri/src/main.rs`: `GDK_BACKEND` and `WEBKIT_DISABLE_DMABUF_RENDERER`.
- `src-tauri/src/modifier_hotkey.rs`, `gpu_monitor.rs`, `update.rs`: the Linux keyboard, battery, GPU name and
  updates.
- `src-tauri/linux/`: ivy-keys (source, `check.py` live test, its own `Cargo.lock`), `60-ivy-keys.rules`,
  `ivy-keys-setup`, `postinst.sh`, `postrm.sh`.
- `src-tauri/tauri.linux.conf.json`: program name `ivy`, the helper's build before bundling, the `.deb` and `.rpm`
  dependencies and extra files. `Cargo.toml`: Vulkan llama.cpp, x11rb with XTest, gtk (the capsule's window). enigo
  is gone (it was the Linux stand-in and is no longer used anywhere). `.gitattributes` keeps `src-tauri/linux/**` LF.

**Frontend**
- `src/utils/platform.ts`: `IS_LINUX`, `IS_WINDOWS`, and `useDesktop()` (reads `get_desktop` every 2 s on Linux).
- `src/components/LinuxKeyboard.tsx`: the keyboard banner (main window, Settings, wizard step 3).
- Wizard: Ctrl + Shift first on Linux; on Wayland it's the only key, the held-back text waits in History and the
  Touch Up step says it's for some apps only. Step 3 greys out GPU without a usable one.
- Settings: no blur switch (Linux desktops offer apps none), no Alt + V and only Ctrl + Shift on Wayland, GPU
  explanations, no GPU-sharing or load reading (Windows counters), Linux wording for updates and Launch at Startup.
- Tone: "Add app" lists the open apps Ivy can see; Linux names are capitalized for display.
- Capsule: "Pasted" without an app name; "Couldn't paste here — it's saved in History" when there's no Alt + V.
- The intro film moves on if it can't play (a Linux without an H.264 decoder for GStreamer).

**CI:** `.github/workflows/linux.yml` on GitHub's `ubuntu-22.04` runners:
- Unit tests, the helper's tests, then `Ivy_0.2.8_amd64.deb` and `Ivy-0.2.8-1.x86_64.rpm` with `SHA256SUMS-linux.txt`
  as the artifact `Ivy-Linux-x86_64` (14 days).
- A model check: downloads the 2.4 GB model and transcribes `src-tauri/tests/fixtures/*.wav` on the CPU and on
  Vulkan (Mesa's llvmpipe, as the runners have no GPU; `GGML_VK_VISIBLE_DEVICES=0`), compared with the Windows text.

## 6. Status (2026-10-10)

Checked on Yash's PC, in WSL (Ubuntu 24.04):
- Unit tests: Windows 70 passed, 0 failed, 2 ignored (as before the port); Linux 69 passed, 0 failed, 2 ignored;
  the helper's own test passes.
- The `.deb` installs: `/usr/libexec/ivy-keys` comes out `root:input` with setgid, the udev rule is in place and
  `/dev/uinput` belongs to the `input` group. Its dependencies are right.
- **ivy-keys against the real kernel input layer** (`check.py`, with a fake USB keyboard): the key bits, nothing
  for plain typing or Ctrl+T, Shift+Insert from its two-key virtual keyboard, its own keys not read back, exit when
  Ivy closes it. PASS.
- **The installed app on an X11 desktop** (Xvfb + Openbox, keys through XTest, a virtual microphone playing the test
  clip):
  - First start: model download, then the wizard opens by itself.
  - Dictation lands in a text box, and the clipboard comes back afterwards.
  - Alt + V pastes held-back text; Touch Up swaps in the corrected text.
  - The Alt + Space conflict banner shows when another app holds the key.
  - Ctrl + Shift keeps working through all of it.
- **Wayland screens** (WSLg is a Wayland desktop): Ctrl + Shift only, no Alt + V, and the keyboard banner for each
  helper state.

**Not verified yet (needs a real Linux PC):**
- A real GNOME or KDE Wayland session: ivy-keys on real keyboards, pasting into native Wayland apps, XTest through
  XWayland, the capsule over Wayland apps.
- A real GPU through Vulkan (WSL has none; CI uses llvmpipe).
- The tray icon, Launch at Startup, the updater (pkexec), installing the `.rpm` on Fedora.
- A desktop without a compositor: the capsule's see-through corners may show black there.
- One open question: once, in a long WSL session, Ctrl + Shift stopped responding after the test's window manager
  had crashed. Two exact replays later didn't repeat it. If the X connection ever fails, Ivy now writes "Ivy: the X
  server didn't answer the keyboard query" once to its log (`logs/Ivy.log`); look for it if Ctrl + Shift goes dead.

## 7. First things to check on a real Linux PC

Easiest without one: an Ubuntu 24.04 "Try Ubuntu" USB stick (`docs/LINUX_TESTING.md`) on any PC, which gives real
hardware, a real GNOME Wayland desktop and the real GPU, with nothing installed on the disk.

1. **Install:** `sudo apt install ./Ivy_0.2.8_amd64.deb`, then `ls -l /usr/libexec/ivy-keys` shows
   `-rwxr-sr-x 1 root input`.
2. **Ivy's own log:** `~/.local/share/app.ivy.dictation/debug.log`. Look for:
   - `Wayland: ivy-keys Ready { paste: true } (N keyboards and mice)` (good), or `Denied`, `NoKeyboard`, `Missing`.
   - `Vulkan check passed: dictating on the GPU` (good), or `Vulkan check failed (...)`, or
     `Ivy stopped during its last GPU check`.

   Tauri's log is `~/.local/share/app.ivy.dictation/logs/Ivy.log`.
3. **Dictation:** hold Ctrl + Shift in Text Editor (a native Wayland app), speak, let go. Then Firefox, a terminal,
   and an X11 app such as VS Code or Slack (Touch Up shows only there on Wayland).
4. Everything else on the checklist in `docs/LINUX_TESTING.md`, on Wayland and, if possible, on "Ubuntu on Xorg".

## 8. Building and testing on Linux itself

```bash
sudo apt install build-essential curl pkg-config libssl-dev cmake ninja-build \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev \
  libvulkan-dev glslc spirv-headers python3-evdev
curl https://sh.rustup.rs -sSf | sh          # Rust; Node 20 or newer from nodejs.org
git clone -b linux-port https://github.com/raj-7676/IVY-.git ivy && cd ivy
npm ci && npm run build                      # tauri-build needs dist/
cd src-tauri && cargo test --lib -- --test-threads=1
(cd linux/ivy-keys && cargo test && cargo build --release)
sudo python3 linux/ivy-keys/check.py linux/ivy-keys/target/release/ivy-keys   # needs a free /dev/uinput
```

The model check (about 2.4 GB download); on a PC with a GPU its "Vulkan" lines are the real GPU:

```bash
mkdir -p ~/ivy-models/ivy-lite && cd ~/ivy-models/ivy-lite
base=https://github.com/raj-7676/IVY-/releases/download/v0.2.6
curl -fL -o mmproj-ivy-lite-f16.gguf "$base/mmproj-ivy-lite-f16.gguf"
curl -fL -o ivy-lite-Q8_0.gguf "$base/ivy-lite-Q8_0.gguf"
sha256sum *.gguf   # 07ed1cc9...fc00 mmproj, da50c4dc...95ed model
cd -   # back to src-tauri
IVY_MODELS_DIR=~/ivy-models cargo test --release --lib model_matches_reference -- --ignored --nocapture
```

Packages: `npx tauri build --bundles deb,rpm` from the repo root (output in `src-tauri/target/release/bundle/`).
Running from source: `npm run tauri dev`. On Wayland it needs ivy-keys installed, so install a `.deb` once first.

## 9. How it was tested without a Linux PC

All in WSL (Ubuntu 24.04) on Yash's PC, with a copy of the worktree in `~/ivy` (rsync; building on `/mnt/d` is slow):

- **X11 desktop:** `Xvfb :99` plus the Openbox window manager, and Ivy started with
  `env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 DISPLAY=:99 GDK_BACKEND=x11`. A Tk text box was the app to type
  into, `xdotool` (XTest) pressed the keys, and a PulseAudio null sink (`ivymic`, made the default source through
  its monitor) played `src-tauri/tests/fixtures/sample.wav` as the microphone.
- **Wayland screens:** WSLg itself (a Weston-based Wayland desktop).
- **ivy-keys:** `check.py` with a python-evdev keyboard (`sudo modprobe uinput evdev` first in WSL).
- **Traps found there:**
  - WSLg's window manager gives even an override-redirect window the focus, so focus behaviour can't be judged
    there; Xvfb + Openbox behaves like a real desktop.
  - WSLg mounts `/tmp/.X11-unix` read-only.
  - WSL has no keyboards in `/dev/input` (ivy-keys reports NoKeyboard there).
- **Dead ends:** VirtualBox on this PC runs on top of Hyper-V ("NEM" mode) and was too slow to finish installing
  Ubuntu (the VM `IvyLinuxTest` is powered off and can be deleted). A headless Sway inside WSL never saw any input
  device.

## 10. Known limits and risks

- Wayland:
  - Only X11 apps can be told apart (tone lists, "Pasted to X", Touch Up). Native Wayland apps get the mode clicked
    on the Tone screen.
  - Ctrl + Shift is the only key; text that can't be pasted waits in History.
  - Ctrl + Shift and pasting depend on ivy-keys, which needs the package's setup (or "Fix keyboard access").
  - The capsule is an X11 window (override-redirect). Whether it shows above every Wayland window, full-screen ones
    included, is unverified.
- ivy-keys is a trade-off, like ydotool's: any program the user runs may ask it for modifier keys and for a
  Shift+Insert. That's much less than the `input` group itself, which users never get.
- Shift+Insert is the paste key everywhere; an app that binds it to something else would do that instead.
- Not done on Linux: ARM PCs, Flatpak, Snap or AppImage, the Wayland input-method protocol, the full-screen sleep
  and GPU-load rules, Caps Lock as a key.
- Not verified on a real Linux PC yet: see §6.

## 11. When you change something

1. Make the change behind the platform switch. Keep Windows identical.
2. On Linux: `cargo test --lib -- --test-threads=1` in `src-tauri`, the helper's tests and `check.py` if it changed,
   then `npx tsc --noEmit` and `npm run build` in the repo root. On Windows: the same `cargo test`.
3. Commit as Yashvanraj (no Claude attribution) and push `linux-port`. GitHub builds new packages (Actions tab →
   "Linux Build" → artifact `Ivy-Linux-x86_64`).
4. Update this file, `IVY.md` §19/§5 and `docs/LINUX_TESTING.md` when behaviour changes.
5. Tell Yash in plain words what changed and what to try.
