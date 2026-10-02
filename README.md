# Ivy

A standalone, fully offline, open-source voice dictation app for Windows.

Hold a hotkey, talk, get clean text pasted wherever your cursor is — the
same idea as Wispr Flow, but 100% local: no account, no subscription, no
network call, ever. Speech-to-text and cleanup both run on your own
machine.

- **Hotkey:** hold `Alt + Space` (customizable in Settings), speak, release.
- **Speech-to-text:** OpenAI's Whisper large-v3-turbo (int8 ONNX), running locally via ONNX Runtime (DirectML GPU acceleration with automatic CPU fallback).
- **Cleanup:** 50+ instant deterministic rules across Casual, Standard, and Professional tones (stutters, spoken grammar, code/math/dates/URLs/currency formatting). In Accuracy mode on a GPU, a local Qwen 2.5 3B model also reads every dictation to resolve corrections like "scratch that" — behind a guard that falls back to the rules if it ever adds or drops words. Qwen also powers on-demand Touch Up and History Summarization on GPU and CPU.
- **History & Privacy:** every dictation and its audio are kept strictly locally and automatically purged after 24 hours (daily retention). Sensitive voice audio and chat history can also be manually purged at any time.
- **Progress Without Compromise:** user productivity metrics (day streaks, words dictated, words per minute, and the 14-day activity chart) are decoupled from sensitive transcripts and stored as anonymous scalar aggregates (`stats.json`). Clearing your chat or voice history never wipes your streak or sets your stats back to zero.
- **Network:** none. The packaged app makes zero network calls at runtime — models are bundled at build/install time, not fetched on launch.

## Security & Privacy

IVY is engineered under a zero-trust, zero-cloud architecture:
- **Zero Cloud Leakage:** All speech recognition (Whisper ONNX) and AI cleanup/summarization (Qwen 2.5 3B GGUF) run entirely in-process on your local CPU/GPU.
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

```bash
npm install
npm run setup-models   # downloads the real STT + cleanup models, ~3.1GB, once
npm run tauri dev      # real hotkey, real transcription, dev build
```

To build a release binary or installer:

```bash
npm run build
npx tauri build --no-bundle   # produces release/app.exe in Cargo's target folder
# or, for the full NSIS installer + companion model file:
npm run package-installer
```

Never run a bare `cargo build` in `src-tauri/` for a binary you intend to
actually use — it skips Tauri's build pipeline and the resulting exe will
try to load the dev server URL instead of the bundled frontend. Always go
through `npm run tauri dev` / `npx tauri build`.

## License

MIT — see [LICENSE](LICENSE).
