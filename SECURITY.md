# Security Policy

Ivy is an offline voice dictation app for Windows. It hears your voice, turns it into text and types
that text into other apps, so we take security and privacy seriously. This page explains how to report
a problem, what is in scope, and what Ivy does and does not protect against.

## Supported versions

Only the latest release gets security fixes. Please update before reporting.

| Version | Supported |
|---|---|
| 0.2.3 (latest) | Yes |
| Older | No |

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report privately through GitHub: open the repository's **Security** tab, then **Report a vulnerability**.
Only the maintainer can see the report.

A good report includes:
- the Ivy version and your Windows version
- what an attacker can do, and what they need first (for example "another program on the same PC")
- the steps to reproduce it
- a proof of concept, if you have one

If private reporting is not available, open an issue that only asks for a private contact. Put no
details in it.

### What happens next

Ivy is maintained by one person, so these are honest targets, not guarantees:

- We confirm we received your report within **7 days**.
- We tell you whether we accept it, and how serious we think it is, within **14 days**.
- We aim to release a fix within **90 days**. Serious problems are fixed first.
- We publish a GitHub Security Advisory after the fix is out, and credit you unless you prefer not.

Please give us the chance to fix the problem before you share details publicly. If we stop
responding, you may disclose after 90 days.

## Scope

**In scope**
- The Ivy app and its installer from this repository and its [Releases](https://github.com/raj-7676/IVY-/releases)
- Anything that makes Ivy send data over the network, keep recordings or text longer than promised,
  type text into the wrong window, or run code it shouldn't
- How Ivy loads its speech model files
- The rule files and scripts in this repository

**Out of scope**
- Problems that need an attacker who already controls your Windows account or has admin rights
  (they can read your files and keystrokes without Ivy)
- Copies of Ivy downloaded from anywhere other than this repository
- Bugs in llama.cpp, Tauri, WebView2 or Windows itself. Please report those to their projects; tell us
  too if Ivy needs to update.
- Wrong transcriptions, unless they cause a security problem
- Missing hardening with no real attack, and reports produced only by automated scanners

## How Ivy protects you

- **No network, except one model download.** Ivy sends nothing anywhere: no telemetry, no analytics, no
  accounts, no automatic updates. Its only connection is downloading its speech model, once, when the model
  isn't installed yet (since 0.2.5; before that, setup downloaded it): HTTPS requests to this repository's
  release, which carry nothing but the file name, the byte range and Ivy's version number. The model must match SHA-256
  fingerprints built into Ivy, or it is deleted. To keep Ivy fully offline, put the model files next to the
  setup file: setup copies them in (checked against the same fingerprints) and Ivy never connects.
  The app's Content Security Policy only allows local content. (Before 0.1.5, the main window linked
  to Google Fonts, so it opened a connection to fonts.googleapis.com at startup. No text or audio
  was sent. Fixed in 0.1.5.) Ivy's window runs in Microsoft Edge WebView2, which is part of Windows.
  WebView2 itself can contact Microsoft for its own services, as set by your Windows settings. That
  traffic carries nothing from Ivy.
- **Short-lived data.** Recordings and transcripts are deleted after 24 hours. **Clear all** in History
  deletes them at once. Lifetime stats (word counts, streaks) are stored separately as plain numbers,
  with no text or audio.
- **Audio in memory is wiped.** Ivy's audio buffers, including the boosted and resampled copies, are
  overwritten with zeros when a dictation finishes or is cancelled. The speech engine's own working
  memory inside llama.cpp is freed but not wiped.
- **Typing into the right window.** Before pasting, Ivy checks the window you were dictating into still
  has focus. If it doesn't, the text is held back (Alt + V pastes it) instead of going somewhere else.
  Your previous clipboard (text, an image or copied files) is restored afterwards.
- **Kept out of clipboard history.** Text Ivy puts on the clipboard is marked so Windows leaves it out
  of clipboard history (Win + V) and cloud clipboard sync, and clipboard managers that respect the
  Windows flag skip it.
- **No focus stealing.** The dictation bar never takes keyboard focus, so it cannot interrupt typing in
  password boxes.
- **Safe file handling.** Recording IDs are checked against a strict allow-list and file paths are
  confirmed to stay inside Ivy's own folder.
- **Uninstall asks.** The uninstaller offers to delete all Ivy data.

## What Ivy does not protect against

Being clear about limits is part of security:

- **Other programs on your PC.** For up to 24 hours, recordings and transcripts are stored unencrypted
  in `%LOCALAPPDATA%\app.ivy.dictation`, readable by anything running as your Windows user. Clear History
  if that matters to you. (Local, not Roaming, AppData: a roaming profile never copies them to a server.)
- **Clipboard readers.** Ivy pastes through the clipboard for a moment. A program that reads the
  clipboard directly, ignoring the Windows "don't record" flag, could see the text during that moment.
- **Tampered model files.** Ivy loads its model with llama.cpp. Several llama.cpp bugs have let a
  specially crafted model file crash or take over the program that loads it (for example CVE-2024-25664
  to 25666, CVE-2025-49847, CVE-2026-27940). Only use the model files from this repository's Releases, and
  check them against `SHA256SUMS.txt` (see below).
- **Unsigned app.** Ivy is not code-signed yet, so Windows cannot confirm who made the installer. Only
  download it from this repository's Releases page.

## Check your download

Each release includes `SHA256SUMS.txt`. In PowerShell, inside the folder with your downloads:

```powershell
Get-FileHash .\* -Algorithm SHA256 | Format-Table Hash, Path
```

Every hash must match the line for that file in `SHA256SUMS.txt`. If one doesn't, delete the files and
download them again from the Releases page.

## How the code is checked

Every change pushed to `main` is checked automatically:
- **CodeQL** static analysis of the interface code, the Rust core and the CI workflows
- Every CI action is pinned to an exact commit, so a hijacked action tag can't change what runs
- **Gitleaks** scan for passwords, keys and tokens accidentally committed
- **cargo audit** and **npm audit** for known vulnerabilities in dependencies
- **Dependency Review** on pull requests, and **OpenSSF Scorecard**
- **Dependabot** suggests dependency updates monthly; security alerts arrive as soon as GitHub knows

Ivy vendors its own copy of `llama-cpp-sys-2` (with a small documented patch), so llama.cpp security
fixes are applied by updating that copy, not automatically.

## Good-faith research

If you make a good-faith effort to follow this policy, we will consider your research authorized, we
will not take legal action against you, and we will work with you to understand and fix the problem.
Please only test on your own computer and your own data, never on other people's.

There is no paid bug bounty. We are grateful for every report and will credit you in the advisory.
