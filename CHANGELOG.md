# Changelog

All notable changes to Ivy. Downloads are on the [Releases page](https://github.com/raj-7676/IVY-/releases).

## 0.2.8 (2026-10-09)

- **Fixed: a black window when opening Ivy after a restart.** Ivy starts hidden with Windows; opening it from the Start menu or a shortcut then showed only a black screen, until you minimized and restored it. It now opens normally every time.
- **Fixed: the Close button on the Intro film did nothing.** The title bar sat on top of it and took the click. Close and Esc both work now.
- **Tray "Dictate Now" ends a pause.** It used to start recording while the title bar still said Paused.
- Esc now closes the GPU/CPU dialog, the Clear-all dialog, the History "More options" menu and the microphone list; clicking outside the microphone list closes it too.
- An app can be in only one Tone mode: adding it to a mode takes it out of the others (before, it could sit in two and Casual silently won).
- Dictionary: words are limited to 100 characters as you type (a longer one used to show an error), and "ivy" isn't added again when "IVY" is already there.
- The empty History page shows your real dictation key instead of always "Alt+Space".

## 0.2.7 (2026-10-08)

- **Keep your history longer if you want.** History has a new **Keep for** choice: 24 hours (still the default), 2, 3, 5 or 7 days. It covers both transcripts and recordings. Choosing a shorter time deletes anything older straight away; **Clear all** still deletes everything at once. Thanks to a friend's idea for people who reuse what they dictated.
- **Check for updates** in Settings. Ivy asks GitHub for the latest version only when you press it; if there's a newer one, **Download & install** fetches it, checks its fingerprint, installs it (keeping your data, model and settings) and reopens Ivy. Nothing checks by itself in the background.
- The Setup guide on Home is now one slim line instead of a big card.
- **Intro** button in the title bar, next to Pause: watch Ivy's intro film any time (people who installed offline or updated never saw it). Press Close or Esc to stop it.

## 0.2.6 (2026-10-08)

- **New license: free to use, not for sale.** Ivy is now under the MIT License with the Commons Clause condition, and Ivy's fine-tuned speech model under Apache 2.0 with the same condition. Anyone may use Ivy for free, including at work, change it and share it; nobody may sell it or a paid product built mainly on it. The source code stays public. Earlier releases were withdrawn, and the model now downloads from this release.
- **Intro film on first start.** While Ivy downloads its speech model for the first time, a 74-second film plays full screen, after the launch animation: what Ivy does, how to use it, and why not to skip the setup wizard. It plays once, can't be skipped, and stops the moment you close or minimize Ivy (it carries on where it was when you open Ivy again). When it ends, the setup wizard opens as before. People updating from an earlier version never see it.
- Setup now shows the publisher (Yashvanraj) in Windows' installed apps list. The installer is about 30 MB bigger because of the film.

## 0.2.5 (2026-10-08)

- Ivy now downloads its speech model itself, like other apps that fetch their content after install. Setup is a normal installer again: no command window, done in seconds. On first start Ivy shows a "Getting Ivy ready" screen with the download (size, speed, time left), only that one time; when it's done, the setup wizard opens by itself. It downloads in 32 MB pieces, 8 at a time, picks up after a dropped connection by itself, and keeps what it has if you close it. If it gives up, a **Retry** button carries on where it stopped. The model is checked against its SHA-256 fingerprint before Ivy uses it.
- Dictating before the model is ready shows "Speech model still downloading · 45%" instead of doing nothing.
- The one-line PowerShell install now just fetches and runs the setup file; Ivy downloads the model the same way.
- Offline install is unchanged: put the two model files next to the setup file and Ivy never goes online.

## 0.2.4 (2026-10-07)

- Installing on a slow or unstable connection works now. The 2.4 GB model used to download over one connection, and a drop stopped the install (`curl: (18) end of response with ... bytes missing`). Both the one-line command and setup now download it in 32 MB pieces, 8 at a time, and fetch any failed piece again by themselves. Running them again keeps the finished pieces.
- When setup downloads the model itself, its window now says what it is doing ("Ivy setup: downloading the speech model") instead of showing a bare download meter.

## 0.2.3 (2026-10-07)

- No more intro sound when Windows starts. With launch-at-startup on, Ivy starts hidden, but its intro animation (with sound) still played in the background. The intro now waits until you first open Ivy's window, then plays once.
- The capsule no longer says "Pasted" when there was nowhere to paste. Ivy used to press Ctrl + V in whatever window was in front, even a dock or the desktop. Now it pastes only when the window in front has a text field focused; if Windows can't tell, Ivy pastes as before. Alt + V (paste again) is unchanged.

## 0.2.2 (2026-10-07)

- Ivy now starts on a fresh Windows too. Its speech engine needs Microsoft's Visual C++ runtime, which most PCs have but a fresh Windows doesn't (Ivy then failed with "MSVCP140.dll was not found"). Setup now carries Microsoft's official installer and runs it only when the runtime is missing or too old; Windows asks for permission once. Setup grew from 10 MB to 34 MB.

## 0.2.1 (2026-10-07)

- One-file install: download just `Ivy_0.2.1_x64-setup.exe` and run it. If the speech model isn't next to it, setup downloads it (2.4 GB, with a progress window; run setup again to resume an interrupted download). Updates keep the model you already have.
- Or install with one command in PowerShell: `irm https://raw.githubusercontent.com/raj-7676/IVY-/main/install.ps1 | iex`.
- Every model file, copied or downloaded, is checked against its SHA-256 fingerprint before Ivy uses it.

## 0.2.0 (2026-10-07)

- Alt + B (undo the last paste) is removed. The app you dictated into already undoes a paste with its own Ctrl + Z.
- Your recordings, transcripts and settings now live in Local AppData (`%LOCALAPPDATA%\app.ivy.dictation`) instead of Roaming. On a work PC with a roaming profile, Windows copies Roaming AppData to a server when you sign out; Local AppData never leaves the PC. Ivy moves your existing data over once, the first time it starts.
- "Also delete your Ivy data?" at uninstall now removes everything, including the app log and WebView2's cache.
- History and the capsule show the program you dictated into ("Brave", "Notepad"), never its window title, which could hold a document name or page text. Per-app tones still recognise browser tabs like Gmail.
- The launch-at-startup entry quotes Ivy's path, so a Windows user name with a space can't make Windows look for a different program first.

## 0.1.5 (2026-10-07)

- Snippets now work anywhere in a sentence: "Send it to my email address" types your saved address in place of the trigger. Pick triggers you'd never say by accident; Alt + B undoes a paste.
- Privacy fix: the main window no longer links to Google Fonts. In 0.1.4 this made the window open a connection to fonts.googleapis.com at startup (no text or audio was sent). Ivy's fonts were already bundled, so nothing looks different.
- Security update: Tauri 2.11.6, which fixes GHSA-w28w-mhc8-qvjv (one window could read IPC data meant for another). Both of Ivy's windows only load Ivy's own local code, so this was hard to exploit in Ivy.
- WebView2's optional background traffic (experiment configs, component updates, connection telemetry) is switched off. WebView2 itself still opens one connection to Microsoft at startup that no setting turns off; nothing from Ivy goes through it (see SECURITY.md).
- Your clipboard is safer: a copied image or files now come back after a dictation (before, they were lost), and if you switch windows at the moment of pasting, your old clipboard is restored right away instead of the transcript being left on it.
- The installer no longer asks GPU or CPU; the setup wizard already does (the installer's answer was also being forgotten).
- The greeting follows the clock while Ivy stays open (morning, afternoon, evening, night).
- Alt + B now undoes a paste even if you keep holding Alt after Alt + V.
- Glass opacity and background blur now really work: lower the opacity and the desktop shows through, and "Background blur" (now an on/off switch using Windows' own Acrylic blur) frosts it. Before, a solid layer behind the window hid the desktop, so neither setting changed anything you could see.
- The launch intro is smooth (full refresh rate, no freezes): the model now loads right after it instead of during it. It has a new, quieter sound made for it: a soft fold tap as each piece of the logo lands (I on the left, Y on the right) and a warm chord as it settles. Click the Ivy logo at the top left to replay it.
- The capsule's sound wave is drawn at your screen's full resolution, so it's sharp instead of blurry on scaled displays.
- The main window's edge is a plain thin border now; the orange line along the top and left edges is gone.
- Hardening: every copy of the audio in memory is wiped, settings have size limits, two unused app permissions are removed, and a release build never looks for the model in a build-machine folder.

## 0.1.4 (2026-10-07)

First public release.

- One speech model: Qwen3-ASR-1.7B fine-tuned for dictation. It hears you and writes clean text in one pass, with self-corrections applied. Runs on any Vulkan GPU or on CPU.
- Three tones (Casual, Standard, Professional), with per-app tones.
- Touch Up spell-check, personal dictionary, Snippets.
- Hold-to-talk or hands-free (double-tap), Alt + Space or Ctrl + Shift. Alt + V pastes the last dictation again, Alt + B undoes the last paste.
- Pause (1 to 24 hours), 5-minute dictation cap with a warning.
- Smart GPU sharing: sleeps during full-screen games and videos, moves to CPU when another program keeps the GPU busy.
- Quiet-microphone boost.
- Privacy: no network calls; recordings and transcripts deleted after 24 hours; pastes kept out of Windows clipboard history.
- FitGirl-style installer: setup exe plus the model files in one folder.
