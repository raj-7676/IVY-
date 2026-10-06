# Ivy

**Talk, and clean text appears wherever your cursor is. Fully offline, free and open source, for Windows.**

Hold a key, speak naturally, let go. Ivy turns your speech into clean, ready-to-send text and pastes it
into whatever you were typing in: email, chat, documents, code editors, browsers, anything.

Everything happens on your own computer. No account, no subscription, no internet connection, no
cloud. Your voice never leaves your PC.

**[Download Ivy for Windows](https://github.com/raj-7676/IVY-/releases/latest)**

![Ivy's main window](docs/ivy-main.png)

While you talk, a small bar shows the time, the tone in use and the app you're typing into:

![Ivy's dictation bar](docs/ivy-capsule.png)

---

## What makes Ivy different

**It writes what you *meant*, not just what you said.**
People change their minds mid-sentence. Most dictation apps type every word, and you fix it by
hand. Ivy understands self-corrections and writes the final version. The kind of thing it handles:

| You say | Ivy types |
|---|---|
| "Let's meet on Tuesday, sorry, I mean Thursday." | Let's meet on Thursday. |
| "Book a table for four, actually make it six." | Book a table for six. |
| "Tell Sam the call is at 3, no wait, 4 PM." | Tell Sam the call is at 4 PM. |
| "Um, so, I I think we should, uh, ship it." | I think we should ship it. |

It also drops fillers (um, uh), removes stutters and repeated words, adds punctuation and capital
letters, and writes numbers, money, times and dates the way you'd type them.

**One small model does all of it.** Ivy's speech model hears and cleans up in a single pass. There's
no second AI rewriting your text afterwards, so it stays fast and keeps your words.

**Truly offline.** The app makes zero network calls. The model is installed from the download
folder, so Ivy never phones home, not even for updates.

**Built for real voices.** The model was trained on many hours of real people speaking (noisy rooms,
cheap microphones, many accents) with human-written transcripts, plus thousands of dictation
paragraphs full of corrections.

---

## Features

- **Hold to talk:** hold **Alt + Space** (or **Ctrl + Shift**, your choice in Settings), speak, release.
- **Hands-free:** double-tap the key to start recording, tap once more to stop. Good for long dictations (up to 5 minutes; a "10 s left" badge warns you).
- **Three tones:**
  - **Casual:** texting style. Your words as spoken, no full stops, one sentence per line.
  - **Standard:** clean, normal writing. Slang is written out.
  - **Professional:** formal. No contractions, slang, chat words or exclamation marks.
- **Per-app tones:** add apps to a tone (for example Slack to Casual and Outlook to Professional), and Ivy switches automatically.
- **Touch Up:** after a paste, one click fixes spelling mistakes. It only corrects misspelled words and never rewrites your sentences.
- **Personal dictionary:** teach Ivy names, brands and jargon so it spells them your way.
- **Snippets:** say a short trigger phrase and Ivy pastes a saved block of text (an address, a signature, a template).
- **Smart number formatting:** "500 rupees" becomes ₹500. Big round amounts are written the readable way, for example "18 lakhs", "2.5 crores" or "2 million".
- **Quick keys:** **Alt + V** pastes your last dictation again. **Alt + B** undoes the last paste.
- **History:** your recent dictations, with their audio, are kept on your PC so you can replay or copy them.
- **Pause:** turn Ivy off for an hour (or up to 24 hours) from the title bar or the tray icon.
- **GPU or CPU:** runs on any graphics card (NVIDIA, AMD or Intel) or on the processor alone. On battery it switches to CPU to save power.
- **Plays nice with games:** when a game or full-screen video is in front, Ivy goes to sleep, frees the graphics card and leaves your keys to the game. If another program is working the graphics card hard, Ivy moves to the CPU until it calms down.
- **Quiet microphone friendly:** Ivy boosts soft recordings automatically.

## Privacy

- **No internet, ever.** Speech recognition and cleanup run entirely on your PC.
- **Automatic clean-up:** recordings and transcripts are deleted after 24 hours. **Clear all** in History deletes them immediately.
- **Not saved in your clipboard history:** Ivy's pastes are kept out of Windows' clipboard history (Win + V) and clipboard cloud sync.
- **Memory wiped:** audio in memory is overwritten with zeros once your dictation is done.
- **Your stats stay, your words don't:** streaks and word counts are stored as plain numbers, separate from your transcripts.
- **Uninstall asks first:** the uninstaller offers to delete all Ivy data from your PC.

See [SECURITY.md](SECURITY.md) for the security details.

---

## Installing

Ivy installs **FitGirl-repack style**: a small setup file plus the model files, all in one folder.

1. Open the **[latest release](https://github.com/raj-7676/IVY-/releases/latest)** and download **all** of these files into the **same folder**:
   - `Ivy_0.1.4_x64-setup.exe` (the installer)
   - `ivy-lite-Q8_0.gguf` (the speech model, 1.8 GB)
   - `mmproj-ivy-lite-f16.gguf` (the part of the model that listens, 0.6 GB)
2. Run `Ivy_0.1.4_x64-setup.exe`. It installs Ivy, copies the model in, and asks whether to use your graphics card (GPU) or processor (CPU).
3. Ivy opens with a short setup wizard that tests your microphone. Then hold **Alt + Space** and talk.

The model is one model stored as two files, because GitHub allows at most 2 GB per file.
After installing, you can delete the downloaded folder.

**"Windows protected your PC"?** Ivy is new and not code-signed yet, so Windows SmartScreen may warn
you. Click **More info**, then **Run anyway**. All the code is here, so anyone can check what it does.

### System requirements

- Windows 10 or 11, 64-bit (tested on Windows 11). Ivy uses Microsoft Edge WebView2, which Windows 11 already has; on an older Windows 10 the installer may download it once.
- About 3 GB of free disk space
- A microphone
- Optional: a graphics card with Vulkan support (most NVIDIA, AMD and Intel GPUs) for faster results

### How fast is it?

Measured on a laptop with an RTX 4060:

| Length of speech | GPU | CPU only |
|---|---|---|
| 5 seconds | about 0.3 s | about 2-3 s |
| 30 seconds | about 0.5 s | about 5-10 s |
| 1 minute | about 2 s | about 10-20 s |

---

## Building from source

**Prerequisites:**

- [Rust](https://rustup.rs/) (stable) and the [Tauri v2 prerequisites for Windows](https://v2.tauri.app/start/prerequisites/) (MSVC Build Tools, WebView2)
- [Node.js](https://nodejs.org/) 18+
- CMake, Ninja, and LLVM (for `libclang.dll`) on `PATH`, needed to build the `llama.cpp` bindings. Visual Studio 2022 Build Tools usually ship CMake/Ninja under `...\Common7\IDE\CommonExtensions\Microsoft\CMake\`; LLVM installs with `winget install LLVM.LLVM`.
- The [Vulkan SDK](https://vulkan.lunarg.com/) (sets `VULKAN_SDK`); `llama.cpp` is built with its Vulkan GPU backend.
- Windows' 260-character path limit can break the nested Vulkan shader build. Point Cargo at a short folder first:
  `$env:CARGO_TARGET_DIR = "C:\ivytgt"` (PowerShell) or `set CARGO_TARGET_DIR=C:\ivytgt` (cmd).

```bash
npm install
npm run setup-models   # downloads Ivy's model (~2.4 GB) from the GitHub release, once
npm run tauri dev      # real hotkey, real transcription, dev build
```

To build a release:

```bash
npx tauri build --no-bundle   # just the app exe, in Cargo's target folder
npm run package-installer     # the full release folder: setup exe + the two model files
```

Always build through `npm run tauri dev` / `npx tauri build`. A bare `cargo build` skips Tauri's
pipeline, and the exe will look for the dev server instead of the bundled interface.

Tests: `cargo test --lib -- --test-threads=1` inside `src-tauri`.

More detail: [ARCHITECTURE.md](ARCHITECTURE.md) (how it fits together), [RULEBOOKS.md](RULEBOOKS.md)
(the formatting and tone rules), [IVY.md](IVY.md) (full engineering notes).

---

## License

Ivy is **MIT-licensed**: use it, change it, share it or build on it, including commercially. See
[LICENSE](LICENSE).

The speech model is a fine-tune of Qwen3-ASR-1.7B (Apache 2.0). Credits for the model, libraries and
data are in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
