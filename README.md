# Ivy

A standalone, fully offline, open-source voice dictation app for Windows.

Hold a hotkey, talk, get clean text pasted wherever your cursor is — the
same idea as Wispr Flow, but 100% local: no account, no subscription, no
network call, ever. Speech-to-text and cleanup both run on your own
machine.

- **Hotkey:** hold `Alt + Space` (customizable in Settings), speak, release.
- **One model:** Ivy lite, Qwen3-ASR-1.7B (Apache-2.0) fine-tuned by the Ivy lab to hear speech and write clean text in one pass, with your self-corrections ("no wait", "sorry, I mean") already applied. Runs through `llama.cpp` on your graphics card (Vulkan: NVIDIA, AMD, Intel) or on any CPU. A 1-minute dictation takes about 2 s on GPU and 10-20 s on CPU.
- **Tones:** Casual (your words as spoken), Standard (slang written out), Professional (no contractions, slang, chat words or exclamation marks), chosen per app. Deterministic rulebooks also format spoken commands, numbers (big round amounts as "18 lakhs" / "2 million"), tech terms and typography, plus your personal dictionary.
- **Touch Up:** after a paste, one click fixes misspelled words (offline dictionary; never rephrases).
- **History & Privacy:** every dictation and its audio are kept strictly locally and automatically purged after 24 hours (daily retention). Sensitive voice audio and chat history can also be manually purged at any time.
- **Progress Without Compromise:** user productivity metrics (day streaks, words dictated, words per minute, and the 14-day activity chart) are decoupled from sensitive transcripts and stored as anonymous scalar aggregates (`stats.json`). Clearing your chat or voice history never wipes your streak or sets your stats back to zero.
- **Network:** none. The packaged app makes zero network calls at runtime — the model ships next to the installer and is copied in at install time, never fetched on launch.

## Security & Privacy

IVY is engineered under a zero-trust, zero-cloud architecture:
- **Zero Cloud Leakage:** All speech recognition (the Ivy lite GGUF model) runs entirely in-process on your local CPU/GPU.
- **Audio RAM Zeroization:** Raw PCM audio sample buffers in memory (`Vec<f32>`) are actively overwritten with zeros (`fill(0.0)`) upon completion or cancellation to prevent residual audio in unallocated memory.
- **Daily Auto-Purge:** Audio recordings (`.wav`) and session text transcripts are automatically deleted after 24 hours.
- **Strict IPC Validation:** Native Tauri IPC handlers validate all session identifiers to prevent directory traversal attacks.
- **Automated Open-Source Audits:** Continuous integration runs `cargo audit`, `npm audit`, Dependabot automated dependency scanning, and GitHub CodeQL static analysis.
- For our vulnerability disclosure program and threat model, see [**`SECURITY.md`**](SECURITY.md).

## Building from source

**Prerequisites:**

- [Rust](https://rustup.rs/) (stable) and the [Tauri v2 prerequisites for Windows](https://v2.tauri.app/start/prerequisites/) (MSVC Build Tools, WebView2)
- [Node.js](https://nodejs.org/) 18+
- CMake, Ninja, and LLVM (for `libclang.dll`) on `PATH` — needed to build `llama.cpp` bindings. If you have Visual Studio 2022 Build Tools installed, CMake/Ninja usually already ship under `...\Common7\IDE\CommonExtensions\Microsoft\CMake\`; LLVM can be installed with `winget install LLVM.LLVM`.
- The [Vulkan SDK](https://vulkan.lunarg.com/) (sets `VULKAN_SDK`) — `llama.cpp` is built with its Vulkan GPU backend.
- *Windows path length recommendation:* The nested Vulkan shader build in `llama.cpp` generates deeply nested files. If your clone path is deep, you may encounter Windows' 260-character path limit (`MAX_PATH`). To ensure clean builds, point Cargo's build directory to a short path before building:
  `$env:CARGO_TARGET_DIR = "C:\ivytgt"` (PowerShell) or `set CARGO_TARGET_DIR=C:\ivytgt` (cmd).

```bash
npm install
npm run setup-models   # downloads Ivy's lite model (~2.4 GB) from the GitHub release, once
npm run tauri dev      # real hotkey, real transcription, dev build
```

To build a release binary or installer:

```bash
npm run build
npx tauri build --no-bundle   # produces release/app.exe in Cargo's target folder
# or, for the release folder (setup exe + the two model files, each under GitHub's 2 GB limit):
npm run package-installer
```

Never run a bare `cargo build` in `src-tauri/` for a binary you intend to
actually use — it skips Tauri's build pipeline and the resulting exe will
try to load the dev server URL instead of the bundled frontend. Always go
through `npm run tauri dev` / `npx tauri build`.

## Installing

Download every file from the release into one folder (the setup exe plus `ivy-lite-Q8_0.gguf` and
`mmproj-ivy-lite-f16.gguf`) and run the setup. It copies the model in and asks whether to run on GPU or CPU.

## License

MIT — see [LICENSE](LICENSE). The model is Qwen3-ASR-1.7B (Apache-2.0) fine-tuned by the Ivy lab. Touch Up's
word list is SymSpell's English frequency dictionary (MIT, `src-tauri/data/en-80k.LICENSE.txt`).
