# Changelog

All notable changes to Ivy. Downloads are on the [Releases page](https://github.com/raj-7676/IVY-/releases).

## 0.1.5 (2026-10-07)

- Snippets now work anywhere in a sentence: "Send it to my email address" types your saved address in place of the trigger. Pick triggers you'd never say by accident; Alt + B undoes a paste.
- Privacy fix: the main window no longer links to Google Fonts. In 0.1.4 this made the window open a connection to fonts.googleapis.com at startup (no text or audio was sent). Ivy's fonts were already bundled, so nothing looks different.
- Security update: Tauri 2.11.6, which fixes GHSA-w28w-mhc8-qvjv (one window could read IPC data meant for another). Both of Ivy's windows only load Ivy's own local code, so this was hard to exploit in Ivy.
- WebView2's optional background traffic (experiment configs, component updates, connection telemetry) is switched off. WebView2 itself still opens one connection to Microsoft at startup that no setting turns off; nothing from Ivy goes through it (see SECURITY.md).
- Your clipboard is safer: a copied image or files now come back after a dictation (before, they were lost), and if you switch windows at the moment of pasting, your old clipboard is restored right away instead of the transcript being left on it.
- The installer no longer asks GPU or CPU; the setup wizard already does (the installer's answer was also being forgotten).
- The greeting follows the clock while Ivy stays open (morning, afternoon, evening, night).
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
