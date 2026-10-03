# IVY — Offline Voice Transcriber

**Status:** v0.1.0. Real hotkey dictation on Windows. Voxtral Mini 3B 2507 + Ivy LoRA multimodal engine (GPU Vulkan / CPU fallback), 9 deterministic formatting rulebooks with 3 tones, Whisper large-v3-turbo fallback engine, Touch Up and Summarize via base Voxtral. Everything runs 100% locally. **Last updated:** 2026-10-03.

A standalone, fully offline, open-source dictation app (MIT, to be published on GitHub). Hold a hotkey, talk, and clean text is pasted into whatever field has focus — the same idea as Wispr Flow, but 100% local and free, with nothing ever leaving the machine.

| | |
|---|---|
| Lives in | `Downloads/IVY_Transcriber` — standalone repo, no Friday/`jarvis_v2` code or dependency |
| License | MIT |
| Stack | Rust + Tauri v2 + React 19/TypeScript. Windows-first; macOS and Linux planned (§19) |
| Speech & cleanup | Voxtral Mini 3B 2507 + `ivy-lora.gguf` multimodal engine (Vulkan GPU / CPU). Fallback: Whisper large-v3-turbo (§4). Lite tier coming (§4) |
| Formatting | 9 deterministic rulebooks (<1ms) across Casual, Standard, and Professional tones (§8) |
| Network | None, ever — models ship with the install; zero runtime calls |

---

## 0. Session handoff (read first)

Chat history does not persist between sessions. This file is the persistent memory of what has been decided and built.

**Yash's standing rules**
- **Nothing may be a gimmick.** No seeded demo data, no invented numbers, no control that only simulates success. Anything visible is a claim about what the app does, including UI copy, installer text and README.
- **Hardware tiers:** Voxtral Mini 3B is the primary engine for GPU and capable CPU. Weak/CPU-only PCs will be served by the upcoming Lite engine (Qwen3-ASR-1.7B, fine-tuned in the lab). Never propose 7B+ models.
- **Work on the real app,** not previews or mockups.
- **Git: commits on a branch are fine (branch `voxtral-engine`).** Never push to origin without asking.
- **After every change,** build, deploy to both exe locations, and relaunch (§18), so Yash never tests stale code.

**Where things stand (2026-10-03)**
- **Voxtral Mini 3B 2507 + Ivy LoRA** is Ivy's primary engine: a single multimodal forward pass transcribes audio and directly resolves speaker self-corrections on GPU and CPU.
- **Vendored `llama-cpp-sys-2` patch:** audio avg-pooling disabled for Voxtral (`IVY PATCH` in `src-tauri/vendor/llama-cpp-sys-2`), giving full acoustic resolution (59/60 golden match). Do not overwrite without re-applying the patch.
- **Qwen 2.5 3B is completely removed** from the runtime codebase.
- **Touch Up and Summarize** run on base Voxtral (LoRA dynamically disabled) via `generate_text`.
- **Formatting rulebooks:** 9 deterministic books in `src-tauri/src/rulebooks/` run post-Voxtral formatting (`after_voxtral`).
- **Whisper large-v3-turbo** is retained as a switchable fallback engine in Settings.
- **Test suite:** `cargo test --lib -- --test-threads=1` passes (70 passed, 0 failed, 1 ignored).
- **Build & Deploy:** `npm run tauri build -- --no-bundle` produces `app.exe`, deployed to `Downloads\Ivy.exe` and `AppData\Local\Ivy\app.exe`.

---

## 1. What Ivy is / isn't

- **Is:** a small background utility. Hotkey down, record, hotkey up, transcribe, clean, paste. 100% offline, works across apps on Windows.
- **Isn't:** a Friday module, a meeting note-taker, or cloud-backed in any way. Macs and Linux aren't supported yet.

## 2. Interaction flow & hotkeys

- **`Alt+Space`:** hold for push-to-talk (release stops). Double-press within 400ms for hands-free recording; the next press stops it. Every session is saved to History.
- **`Alt+V`:** paste the latest transcript into the focused window. Covers both "no text field was detected" and "the app swallowed the auto-paste".
- **`Alt+B`:** undo the last paste. Sends `Ctrl+Z` and restores the previous clipboard, but only if the same window still has focus.
- **Single Key mode** offers Caps Lock. A bare Right/Left Alt can't be registered (see §5).
- **On battery:** Ivy forces CPU (`GetSystemPowerStatus`), and returns to the configured mode when the charger is plugged back in.
- **Autostart:** Windows Run key via `tauri-plugin-autostart`. Autostart launches to the tray; a manual launch shows the window.

## 3. Platform, stack, hardware target

- **Rust core, Tauri v2,** WebView2 frontend (Vite + React 19 + Tailwind 4).
- **Windows APIs used:** `GetForegroundWindow`, `SendInput`, `GetSystemPowerStatus`, DXGI.
- **Mobile** (Android/iOS icons exist in `src-tauri/icons`) is future work.
- **Hardware rule:** every model and feature must stay usable on a CPU-only, low-RAM PC. Yash's RTX 4060 is the test machine, not the target.

## 4. Models

**Primary Engine: Voxtral Mini 3B 2507 + Ivy LoRA**
- **Files:** `Voxtral-Mini-3B-2507-Q4_K_M.gguf` (~2.36GB), `mmproj-Voxtral-Mini-3B-2507-Q8_0.gguf` (~0.68GB), `ivy-lora.gguf` (~103MB), and `golden.jsonl`, in `src-tauri/models/voxtral-ivy/`.
- **Runtime:** `llama-cpp-2` with vendored, patched `llama-cpp-sys-2` (Vulkan GPU offload via `n_gpu_layers = 99` or CPU fallback `0`).
- **Multimodal Integration:** Audio is fed through `mtmd_bitmap_init_from_audio` into the multimodal projector. A vendored patch in `llama.cpp/tools/mtmd/clip-model.h` (`IVY PATCH`) disables audio average pooling, ensuring full acoustic resolution (375 audio tokens per 30s).
- **Dynamic LoRA Switching:**
  - **Dictation (`transcribe`):** LoRA ON at scale 1.0. Directly resolves speaker self-corrections, stutters, and hesitations in a single forward pass.
  - **Touch Up & Summarization (`generate_text`):** LoRA dynamically turned OFF. Base Voxtral acts as an instruction-following Ministral-3B chat model.
- **Prompt:** Matches training token for token: `<s>[INST]<__media__>Write what the speaker means, ready to paste: apply their own corrections, keep every other word.[/INST]`. Guarded by `instruction_matches_training_prompt_exactly`.

**Fallback STT: Whisper large-v3-turbo** (onnx-community int8 export)
- **Files:** `encoder_model_int8.onnx` (~645MB), `decoder_model_merged_int8.onnx` (~438MB) and `tokenizer.json`, in `src-tauri/models/whisper/`.
- **Role:** Selectable fallback engine in Settings. When selected, outputs raw ASR text, followed by rulebook post-processing (no LLM pass).
- **Frontend & Decoder:** 128-bin log-mel spectrogram, 30s fixed window, greedy decoding with `IoBinding`, hotword trie dictionary boost (+3.0), repeating-cycle guard.

**Upcoming Lite Engine:**
- **Model:** Qwen3-ASR-1.7B GGUF fine-tuned by the lab.
- **Role:** Will serve weak/CPU-only PCs with ~2s dictation latency once delivered. Plug-and-play addition to the engine switch.

## 5. Known traps (read before touching related code)

**Paste, focus and keyboard**
- **Alt-modified global hotkeys open the focused app's menu.** `RegisterHotKey` swallows the Space/V/B, so the app sees Alt pressed and released alone. Win11 Notepad and Office then show KeyTips (rows of spaced-out letters) and eat the synthetic `Ctrl+V` that follows. `SendInput` still reports success, so Ivy believed it had pasted. Fix: `suppress_alt_menu()` taps the unassigned VK `0xE8` on every hotkey press while Alt/Win is down (AutoHotkey's `#MenuMaskKey` trick).
- **A physically held Alt merges into the synthetic keys** (`Ctrl+Alt+V` = `WM_SYSKEYDOWN`, which text boxes ignore). `release_held_modifiers()` waits up to 350ms for release, then sends explicit key-ups. Send `Ctrl`/`V` down/up as one atomic `SendInput` batch.
- **Never steal focus.** Overlays use `SW_SHOWNA` plus `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`. Send the keystroke before showing any confirmation UI.
- **The capsule must not show over the wizard, and must work everywhere else.** `bridge_capsule_show` (the single choke point) hides it only when `WIZARD_ACTIVE` (set by `FirstRunView.tsx` via `set_wizard_active`) **and** the main window is focused. Mount alone isn't enough: closing the window to the tray keeps the wizard mounted.
- **The foreground check is a 300ms poll, not a single snapshot.** A brief flicker from the capsule itself used to count as an app switch. Re-check focus right before the keystroke; this was a real TOCTOU for Touch Up's `Ctrl+Z` after a 12s LLM call.
- **Explorer/desktop pass foreground checks but discard typing** (`is_explorer_shell`). Elevated windows can't receive injected input at all (UIPI).
- **Restore the clipboard conditionally.** Wait 800ms, then restore only if the clipboard still holds Ivy's text. A short delay races slow apps.
- **The no-text-field fallback never touches the real clipboard.** The text waits in `MANUAL_PASTE_TEXT`. Overwriting it once silently destroyed a copied API key.
- **Never open the mic twice.** A WebView2 `getUserMedia` on the same device while cpal records delayed and zeroed Ivy's capture. UI meters use `ivy://mic-level` from Rust's recorder.
- **cpal `Xrun` ("buffer underrun or overrun") is not a failure.** It's WASAPI's DATA_DISCONTINUITY flag and fires on good recordings. Don't build error messages on it.
- **Some mics output zeros for ~400ms after the stream opens** (Yash's Realtek does). Holds shorter than that are silence, and the start of every dictation loses that much audio.
- **`global-hotkey` 0.8 has no VK mapping for bare `AltLeft`/`AltRight`** — a platform limit, so the option was removed. `describe_register_failure()` string-matches the plugin's single error variant so users see an honest message.
- **Time-based hotkey rate limits break double-press.** A 150ms limiter silently swallowed fast taps. The `STARTING_DICTATION` compare-exchange is the real guard.

**Text processing**
- **Unicode byte offsets:** an offset found in a `.to_lowercase()` copy isn't valid in the original string (Turkish İ). Verify the char boundary and the matched text in the original before slicing.
- **Space-gated find/replace rules** (`" cuz "`) never match at the start or end of the utterance. Pad the input with spaces, run the table, then trim.
- **`clean_typography_and_spacing` must not glue a file extension onto the previous word.** `collapse_space_before_period` only pulls a sentence period left.
- **Rules written for spelled-out numbers miss Whisper's digits.** The currency rule needed a digit path ("500 rupees" → ₹500). "pounds" is excluded because it usually means weight.
- **The dimension rule (`NUM by NUM`, Rule 15) must reject a second token with trailing letters, not just strip them.** "500 by 5pm" used to match as a dimension and silently eat "pm", producing "500x5". Fixed 2026-09-28: it now requires everything before the token's trailing punctuation to be digits.

**AI & Multimodal engine**
- **Voxtral prompt tokens must match training verbatim.** Prompt: `<s>[INST]<__media__>Write what the speaker means, ready to paste: apply their own corrections, keep every other word.[/INST]`. There is no space or newline anywhere. The test `instruction_matches_training_prompt_exactly` guards it.
- **Voxtral must NOT average-pool audio encoder frames.** Upstream llama.cpp averages pairs of audio frames by default for Voxtral, cutting 375 tokens down to 187 and halving acoustic detail. The vendored patch in `src-tauri/vendor/llama-cpp-sys-2` (`IVY PATCH`) fixes this. Never overwrite vendored files without preserving the patch.
- **Touch Up and Summarize require verification guards.** Base Voxtral runs with LoRA dynamically turned OFF for text generation. `is_word_subsequence` strictly ensures Touch Up never rewrites or invents words. `contained_ratio` guards Summarize against hallucinated content.

**Rendering, platform and build**
- **Outset `box-shadow` on a transparent DirectComposition surface renders as a hard rectangle** (Skia premultiplication). Use a solid fill, an inset highlight and a 1px border instead.
- **The `TrayIconBuilder::build()` handle must be kept alive** (`app.manage`), or the icon vanishes.
- **`canonicalize()` prepends `\\?\`.** Canonicalize both sides before comparing paths.
- **Whisper-family models are not scale-invariant.** Input loudness changes accuracy; target -12dBFS (§10). `cpal` doesn't request Windows raw capture mode, so mic-enhancement APOs vary per device.
- **NSIS:**
  - `IntCmp` has no "fall through" placeholder; use the 3-argument form.
  - `System::Int64Op` compares with `=`.
  - NTFS is case-insensitive, so a case-only rename of an output file is the same file.
  - `$PLUGINSDIR` doesn't reliably auto-delete; `RMDir /r` it on every exit path.
- **Build directory & Windows path limit:** Windows' 260-character limit can bite deeply nested CMake Vulkan shader files in `llama-cpp-sys-2` if the repo clone path is deep. Set `CARGO_TARGET_DIR` (e.g. `C:\Users\YASH\Downloads\ivytgt` on Yash's PC) to a short path to stay well below the 260-character ceiling. Deploying from the wrong folder shipped a days-old binary once — always verify target output timestamp.

## 6. Pipeline & modes

**Pipeline:** hotkey → `audio.rs` capture (own thread) → 16kHz → AGC (`normalize_audio`) → Hallucinations stage A (`prepare_audio`) → **Voxtral `transcribe`** (or Whisper fallback) → Hallucinations stage B (`clean_asr_text`) → formatting rulebooks (`after_voxtral`) → `apply_personal_dictionary` → `paste_text` → history/stats → `ivy://dictation-complete`.

**Architecture & Engine Roles:**
- **Voxtral Mini 3B (Primary Engine):** Operates on both GPU (Vulkan) and CPU. A single multimodal forward pass transcribes audio and directly resolves speaker self-corrections (e.g. "Send it to marketing, scratch that, sales" → "Send it to sales") without requiring an intermediate text pass or second model. Self-corrections work on GPU and CPU.
- **Whisper large-v3-turbo (Fallback Engine):** Retained as a selectable fallback in Settings. Transcribes raw speech; deterministic rulebooks format output.
- **Tone Profiles:** Casual, Standard, and Professional tones are applied post-transcription by `rulebooks::after_voxtral`. Casual and Standard never alter words; Professional expands slang and cleans filler words.
- **On-Demand AI:** Touch Up (capsule button after paste) and Summarize (History view) run on base Voxtral (LoRA OFF) on GPU and CPU.

## 7. Multimodal speech & correction inference (`voxtral.rs`)

- **Single Multimodal Pass:** Replaced the two-stage pipeline (Whisper STT + Qwen LLM cleanup) with Voxtral Mini 3B 2507 + Ivy LoRA adapter. Full architectural specification is documented in §23.
- **Prompt Fidelity:** The prompt must match training verbatim:
  `<s>[INST]<__media__>Write what the speaker means, ready to paste: apply their own corrections, keep every other word.[/INST]`
  If personal dictionary words exist, append ` Words that may appear: word1, word2, word3.`.
- **Decoding:** Greedy decoding (`temperature = 0`) capped at 6 tokens per second of audio + 64. Dictations longer than 120s are split into chunks at natural pauses (`split_long_audio`).
- **Post-Processing:** Output passes through Hallucinations stage B (`clean_asr_text`) as a safety net against repetitive loops or caption artifacts, then `rulebooks::after_voxtral` for deterministic styling (numbers as digits, capitalization, tech terms, typography).
- **Faithfulness:** Because Voxtral directly hears acoustic pauses, intonation, and hesitations, it resolves corrections with native speech grounding (34/40 corrections on benchmark set). The old text-diffing faithfulness checker is no longer needed during dictation, though `is_word_subsequence` and `contained_ratio` remain active guards for Touch Up and Summarization.

## 8. Rulebooks & tones (`src/rulebooks/`, RULEBOOKS.md)

**Since 2026-10-02 the rules are 9 books** (the old 60-rule suite in `cleanup.rs` was removed). About a dozen of those old rules changed spoken words ("you to" → "you too", "trial period" → "trial.", "I think so" → "I think", "Ram" → "RAM"). **Read RULEBOOKS.md before touching any rule.**

- **Book 1, Hallucinations, outranks all.**
  - Stage A, on the audio (`lib.rs::transcribe_and_clean`): no voice → no text; trim the silent edges; shorten pauses over 1.5 s.
  - Stage B, on Whisper's text: bag of hallucinations, loops, impossible speaking rate.
  - Stage C, on AI output: `faithfulness::check_ai_faithful`.
- Books 2–8: disfluency, spoken commands, numbers (always digits), tech (Accuracy only), names and capitals, tone, typography.
- **Laws:** whole words only; never trade a spoken word for another; when in doubt leave it as spoken; idempotent; fixed order; std-only and under 1 ms.
- **Tones:** Casual and Standard never change wording. Professional only expands slang ("gonna") and drops ", like," / ", you know,".
- **Tests:** the books test standalone with `rustc --edition 2021 --test src-tauri/src/rulebooks/mod.rs` (32 tests). `cleanup.rs` keeps the model-backed tests.

## 9. Touch Up & Summarize

- Both features run on **base Voxtral Mini 3B** with LoRA dynamically turned OFF (`generate_text` in `voxtral.rs`). Base Voxtral functions as a standard instruction-following Ministral-3B chat model.
- **Touch Up** (capsule button for 5s after a real paste, GPU and CPU):
  - Prompts base Voxtral to fix missing punctuation and remove exact repeated words/stutters that survived cleanup.
  - Strict guard: `is_word_subsequence(&result, raw)` ensures no word is rewritten, rephrased, or added.
  - The swap is `Ctrl+Z`, then a re-paste, after a focus re-check. History is updated, and Alt+B still restores the original clipboard.
  - Timeouts: 10s on GPU, 15s on CPU.
- **Summarize** (History, any mode):
  - Prompts base Voxtral to extract concise bullet points from the dictated transcript.
  - Guarded by `contained_ratio` (must be ≥ 0.20), preventing hallucinations.
  - Timeouts: 20s on GPU, 35s on CPU. Real failures show a real error.
- **History titles** use `heuristic_title` (no AI).

## 10. Audio capture & gain (`audio.rs`)

- **Capture:** `cpal` on a dedicated thread (COM safety), resampled to 16kHz with `rubato`.
- **Gain:** `dagc` `MonoAgc` toward -12dBFS (0.25 RMS). It is frozen on 20ms frames near the 10th-percentile noise floor, so room noise is never boosted. A fresh `MonoAgc` is built for every call, so no state carries between dictations. `DISTORTION_FACTOR = 0.001` (0.0001 converged too slowly for 1–3s clips).
- **Privacy:** samples are zeroized on every exit path.
- **Guards:** near-silent input (peak below `SILENCE_PEAK_THRESHOLD`) is discarded as silence. Recordings are capped at `MAX_DICTATION_SECS`.
- **Mic hot-plug:** the Settings mic list refetches on window focus and when the dropdown opens. The Rust side re-enumerates on every call.

## 11. Performance

For measured timings on Yash's RTX 4060 laptop and CPU fallback across Voxtral Mini 3B and Whisper, see the benchmark table in **§23.7**.
- **Voxtral GPU (Vulkan):** ~0.60s (5s clip), ~0.65s (15s clip), ~0.95s (30s clip). Peak VRAM ~3.7 GB, RAM ~374 MB. Cold load ~2.65s.
- **Voxtral CPU:** ~13.0s (5s clip), ~25.3s (30s clip). CPU mode is slow for daily use; the upcoming Lite tier (Qwen3-ASR-1.7B) will address weak/CPU-only machines.
- **Whisper fallback GPU (DirectML):** ~0.90s–1.14s. Peak VRAM ~1.1 GB.

## 12. Paste & clipboard (`lib.rs`)

- **`paste_text(text, target)`:**
  1. Poll the foreground for ≤300ms and require the hotkey-down window.
  2. Store the text in `MANUAL_PASTE_TEXT` (for Alt+V), then set the clipboard.
  3. Wait 60ms, re-check focus, and send `Ctrl+V`.
  4. Record `LAST_PASTE` for undo, then restore the clipboard conditionally after 800ms.
- **No matching window:** the text is held for Alt+V and the real clipboard is left alone. The capsule says "Copied", never "Pasted".
- **`paste_manual_clipboard` (Alt+V):**
  1. Validate the target (not the capsule, main window or Explorer).
  2. Swap the clipboard temporarily, run `release_held_modifiers`, re-check focus.
  3. Send `Ctrl+V`, then restore.
- **The shared hotkey handler** calls `suppress_alt_menu()` on every press (§5).

## 13. Capsule & UI design

- **Main chrome:** dark and flat (`rgba(10, 8, 14, ·)`). Fonts are Plus Jakarta Sans and JetBrains Mono, with Syne ExtraBold for wordmarks. Amber accent `#FF6B00` / `#FFA133`.
- **The capsule** is the only liquid-glass surface.
  - A compact pill centered on the monitor with the cursor. It is visible only while recording or confirming.
  - A real cancel (X) purges audio. A "Buffered" badge appears past 60s.
  - It is a separate entry point (`capsule.html`, `src/capsule-main.tsx`, `src/CapsuleWindow.tsx` state machine, `FloatingCapsule.tsx` rendering).
- **Settings changes** emit `ivy://settings-updated`, so the long-lived capsule re-reads the dictation mode.
- **`App.tsx` `handleUpdateSettings`** merges from `settingsRef`, not inside a setState updater. A deferred updater once left the merged value null and silently skipped the save. The GPU/CPU dialog saves once: a second `save_settings` within 100ms hits the backend rate limit.
- **Launch intro** (`IvyLaunchIntro.tsx` + `src/utils/audio.ts`): a Web Audio sequence synced to the animation. It plays once per session (`sessionStorage`), with Skip/Close buttons. Its callbacks are stabilized via a ref because an inline-lambda dependency restarted it in a loop. WebView2 autoplay is enabled via `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--autoplay-policy=no-user-gesture-required` in `main.rs`. `RootErrorBoundary` wraps the app.

## 14. Onboarding wizard (`FirstRunView.tsx`, `StageIndicator.tsx`)

- **7 stages:** 1 Shortcuts, 2 Smart Clipboard, 3 AI & Tips (Speed/Accuracy choice + ModeMatrix), 4 Touch Up, 5 Voice Test, 6 Auto-Correct demo, 7 Privacy. Both real-mic stages come late, so Defender's first scan of the new model files is less likely to land on the user's first impression.
- **Stage 6 adapts to the mode.** In Accuracy + GPU it says no trigger words are needed. Otherwise it says honestly that corrections are pasted as spoken. Success is judged from the real result ("French fries" absent).
- **Stage numbers are positional.** To reorder, map old→new numbers and update every `currentStage === N`, `.add(N)` and `setCurrentStage(N)`.
- **Stage-count claims in copy** must be checked against `StageIndicator.tsx` (it was wrong twice from memory).
- **The sidebar IVY wordmark** reopens the wizard.

## 15. Stats & history privacy

- **`history.json` + `audio/*.wav`** (`%APPDATA%\app.ivy.dictation`): 24h retention (`RETENTION_SECS`). "Clear History" wipes everything.
- **`stats.json`:** anonymous scalars only (total words, WPM, streak, daily word counts, sessions). Written atomically, never reset by purges, and starting from an honest zero (a fake "motivating" seed was removed).
- **`HomeView`** shows an empty state until real stats load; it has no fallback math.

## 16. Open-source security suite

- **In the app:** audio zeroization; `is_valid_session_id` (`^[a-zA-Z0-9_-]{1,64}$`) on every session-id command.
- **Docs:** `SECURITY.md` (contact via a contact-only issue, because the old `.local` address was unroutable).
- **CI and scanning:**
  - `.github/dependabot.yml`.
  - Workflows: `security.yml` (cargo audit, and npm audit after `npm ci && npm run build`, because `tauri-build` needs `dist/`), `codeql.yml`, `dependency-review.yml`, `scorecard.yml`, `secret-scan.yml`.
  - `.gitleaks.toml`, CODEOWNERS, issue and PR templates.
- **`.gitignore`:**
  - Excludes models, `*.wav` (the synthesized test fixtures are re-included), audio and history, and the `.agent/`, `.agents` and `.claude/` folders (Yash's tooling — keep on disk, never publish).
  - `.env.example` was deleted because nothing reads env vars.

## 17. Installer packaging

> [!WARNING]
> The current installer pipeline (`src-tauri/installer/*`, `scripts/package-installer.mjs`, `scripts/download-models.mjs`) still embeds old Qwen 2.5 3B logic and footer-append scripts. It is **broken and unusable** until Task 3 replaces it with the new engine recommendation installer (supporting Voxtral ~3.1 GB payload and Lite tier).

## 18. Build, deploy & debugging

- **Prerequisites:** Rust, Node 18+, MSVC Build Tools, WebView2, CMake + Ninja, LLVM (`winget install LLVM.LLVM`), and the Vulkan SDK (`VULKAN_SDK`).
- **Cargo config & Build Directory:** `.cargo/config.toml` (repo root and `src-tauri/`) sets `CMAKE_GENERATOR = "Ninja"`, `CL`/`_CL_ = "/FS"`. These must be environment variables so the nested `vulkan-shaders-gen` CMake build inherits them; that was the fix for the MSVC `C1041` PDB race. Machine-specific `target-dir` is decoupled from repository configs; on Windows machines, `CARGO_TARGET_DIR` can be set locally to a short path (e.g. `C:\Users\YASH\Downloads\ivytgt` on Yash's PC) to avoid `MAX_PATH` collisions.
- **Build:** `npm run tauri build -- --no-bundle` from the repo root. Never a bare `cargo build` (it skips embedding `dist/`).
- **Deploy:**
  1. Stop `Ivy.exe`/`app.exe` and wait for them to exit.
  2. Copy `$CARGO_TARGET_DIR\release\app.exe` (`C:\Users\YASH\Downloads\ivytgt\release\app.exe` on Yash's machine) to **both** `C:\Users\YASH\Downloads\Ivy.exe` and `C:\Users\YASH\AppData\Local\Ivy\app.exe` (the desktop shortcut target).
  3. Relaunch.
- **Model path:** the dev-installed app has no models folder. `models_dir()` falls back to the compile-time `CARGO_MANIFEST_DIR\models`, so deleting a model under `src-tauri/models` affects the running app immediately.
- **Logs:**
  - `%APPDATA%\app.ivy.dictation\debug.log` — per dictation: engine (`voxtral ok via GPU/CPU in Xms` or `stt ok via GPU (DirectML)/CPU in Xms`), post-processing timings (`cleanup pass completed in Xms via rules`), and `settings saved: hardware=… dictation=…`.
  - `%LOCALAPPDATA%\app.ivy.dictation\logs\Ivy.log` — diagnostic logs, model loading, and fallback telemetry.
  - Read both before guessing. Saved recordings in `audio/` can be replayed through the engine for apples-to-apples comparisons.
- **Tests:** `cargo test --lib -- --test-threads=1`. Parallel runs load several 3B engines plus Whisper on one GPU and time out ("generation timed out"), which is not a real regression. Model-backed tests skip themselves if the model files are missing. Also run `npx tsc --noEmit` (clean even with `--noUnusedLocals --noUnusedParameters`).
- **Key files:**
  - `src-tauri/src/lib.rs` — commands, pipeline, hotkeys, paste, settings.
  - `voxtral.rs` — Voxtral Mini 3B multimodal engine, LoRA switching, Touch Up & Summarize text generation.
  - `stt.rs` — Whisper fallback STT.
  - `cleanup.rs` — fallback cleanup wrapper.
  - `audio.rs` — capture and AGC.
  - `gpu_monitor.rs` — DXGI load/VRAM eviction and battery.
  - `src/components/*` — views, wizard, capsule UI, `ModeMatrix.tsx`.
  - `scripts/download-models.mjs` (`npm run setup-models`) and `scripts/package-installer.mjs`.

## 19. Cross-platform roadmap (not built)

- **macOS:** Accessibility API (`AXUIElementSetValue`) injection. Any clipboard fallback must be tagged `org.nspasteboard.TransientType`.
- **Linux X11:** `XGrabKey`, `_NET_ACTIVE_WINDOW`, `XTestFakeKeyEvent`, draining modifiers via `XQueryKeymap`.
- **Linux Wayland:** needs the input-method protocol (`zwp_input_method_v2::commit_string`).
- **Windows-only code to port:** DXGI telemetry, `SendInput`, power status, the Alt-menu mask.

## 20. Open items

- **Touch Up/Summarize quality on base Voxtral not yet checked by Yash.** Need evaluation with real transcripts and user sign-off on output quality once GPU training completes.
- **Whisper fallback deprecation:** Keep Whisper large-v3-turbo as a safety net until the Lite brain (Qwen3-ASR-1.7B) is delivered, integrated, and proven in Ivy. Once Lite is proven, drop Whisper to reduce download size by ~1 GB.
- **Initial release version:** `0.1.0` confirmed for the first public release ("early but real"). Move to `1.0.0` after external usage feedback.
- **Personal Dictionary for accent mishears** (Priya, vada pav, camel case): mechanism supported in Voxtral prompt (`Words that may appear: ...`), ready for user additions.
- **Mic start-up clips ~650ms of every dictation** on Yash's Realtek (§5), measured with `live_mic_capture`: `Recorder::start` takes 200–285ms to open the stream, then the driver sends zeros for a steady ~425ms. The device exposes exactly one format (48kHz stereo, 480-frame buffer), so there's no config lever. Likely cause is the driver's audio-enhancement (APO) chain warming up; WASAPI raw mode would bypass it, but cpal can't request it. Remaining option is keeping the stream open while Ivy runs (mic-in-use indicator stays lit). Yash said 400ms is acceptable if it can't be reduced (2026-09-28).
- **AGC2/VAD** (`sonora-agc2`, Silero VAD) was researched and not attempted. If neural denoising is ever revisited, build a fresh instance per call and add the same-input-twice test.

## 21. Standing lessons

- **Verify against real files, logs and recordings,** not a summary. Several agent reports and this file itself drifted from the code more than once. The code and logs are the truth.
- **Test with Yash's real saved recordings and Whisper's real output,** not only synthetic fixtures. Fixtures share the code's blind spots.
- **Running the real app catches what reading source misses** (stage counts, garbled onboarding text, paste failures).
- **A control that can't work is worse than no control.** Remove it rather than caveat it.
- **When Yash says "do it properly",** fix the mechanism with a real, published approach, not by tuning constants.
- **Ask before changing a design he has flip-flopped on.** State tradeoffs once, then follow his call. Let real tests settle disagreements.

## 22. How to keep this file updated

- Update §0 (state, test count) and the verification table every session.
- Put general traps in §5, not in a dated changelog.
- When a decision is superseded, rewrite the section. Don't bolt a "superseded by" note onto old text. History of deleted features doesn't belong here unless it teaches a trap.
- Keep §20 honest: remove an item only once it's confirmed by a test, a build, or Yash using the real app.
- Code comments reference sections as `IVY.md §N`. After renumbering, grep the codebase for `IVY.md §` and fix them.

## Verification history (recent)

| Date | `cargo test --lib` (serial) | Notes |
|---|---|---|
| 2026-09-24 | 50 passed, 2 failed (Qwen timeouts, parallel) | Moonshine → Whisper large-v3-turbo |
| 2026-09-27 | 53 passed | AI stage rewrite (tagged prompt + examples, `check_ai_faithful`, formatting after AI), Alt-menu paste fix, settings-save race fix |
| 2026-09-27 (late) | **52 passed, 0 failed, 1 ignored** | Silent-drop guard, "no, no" example; repo cleanup (dead code, unused files/deps, stale docs, installer 1.5B → 3B, `package-installer` target dir) |
| 2026-09-28 | **52 passed, 0 failed, 1 ignored** | Dimension-rule fix (§5), wizard capsule suppression (§5/§0); real GPU + Accuracy retest with the 4 sentences |
| 2026-09-28 (later) | `npx tsc --noEmit` clean | Stage 1 hotkey tester fix — shared `matchesSelectedHotkey` across Stages 1/5/6 (§0) |
| 2026-09-28 (latest) | **52 passed, 0 failed, 2 ignored** (+ manual `live_mic_capture`) | Wizard double mic-open removed (`ivy://mic-level`), wizard focus-gated, capsule suppression focus-gated; wrong `stream_error` diagnosis reverted |
| 2026-10-03 | **66 passed, 0 failed, 2 ignored** | Rulebooks rewrite verified. Cold build (`npm run tauri build -- --no-bundle`), `npx tsc --noEmit` clean, deployed to both exe paths & relaunched. |
| 2026-10-03 (Task 2) | **70 passed, 0 failed, 2 ignored** | Voxtral Mini 3B multimodal engine verified (60 golden clips: 98.3% match). Qwen removed. Benchmarks measured. Cold build, tsc clean, deployed to both exe paths. |
| 2026-10-03 (Claude fixes) | **70 passed, 0 failed, 1 ignored** | Patched llama.cpp avgpool bug (vendored `llama-cpp-sys-2`), exact training prompt, long-audio split, length-scaled timeout, single BOS, UTF-8 decode, real benchmark numbers. Golden 59/60 vs regenerated `expected`. Release build, tsc clean. |

---

## 23. Voxtral engine (Task 2 plan & architecture)

### 1. Motivation & Architecture
- **Single end-to-end model:** Voxtral Mini 3B 2507 (Mistral, Apache-2.0) with an audio encoder + language model backbone fine-tuned (v5) to hear speech and directly output clean text with speaker self-corrections applied.
- **Replaces two models with one:** Whisper large-v3-turbo (STT) + Qwen 2.5 3B (cleanup) are replaced by Voxtral. This eliminates Qwen's non-commercial research licence completely.
- **Licence compliance:** All shipped components (Voxtral base, Mistral mmproj, Ivy LoRA adapter) are Apache-2.0.

### 2. Model Files (`src-tauri/models/voxtral-ivy/`)
- `Voxtral-Mini-3B-2507-Q4_K_M.gguf` (~2.36 GB): Base language model backbone.
- `mmproj-Voxtral-Mini-3B-2507-Q8_0.gguf` (~0.68 GB): Audio encoder + multimodal projector.
- `ivy-lora.gguf` (~103 MB): Fine-tuned LoRA adapter (applied dynamically).
- `golden.jsonl`: Benchmark test clips from lab fine-tune v5.

### 3. Runtime & Multimodal Integration (`llama-cpp-2` + `libmtmd`)
- `llama-cpp-2 = { version = "0.1.156", features = ["vulkan", "mtmd"] }`.
- **`llama-cpp-sys-2` is vendored and patched** (`src-tauri/vendor/llama-cpp-sys-2`, wired in through `[patch.crates-io]` in `Cargo.toml`). Upstream llama.cpp lists Voxtral in `audio_has_avgpool()` (`tools/mtmd/clip-model.h`), which averages pairs of audio-encoder frames: 30 s of audio becomes 187 tokens. The real model (HF `VoxtralEncoder`) defines `avg_pooler` but never calls it, so training saw 375 tokens. With the bug, the model got half the audio detail: on the 60 set-2 clips it scored 30/40 corrections and 14/20 normal. Fixed, it scores **34/40 and 19/20** (PyTorch v5: 35/40, 15/20). The patch is marked `IVY PATCH`. Keep it when bumping the crate, until upstream fixes the bug.
- Shared process-wide `GLOBAL_BACKEND` singleton from `LlamaBackend` (avoids duplicate backend initializations).
- Base model loaded via `llama_model_load_from_file`, context initialized with `n_ctx = 4096`, `n_batch = 1024`, `n_ubatch = 512`, `n_gpu_layers = 99` (GPU Vulkan) or `0` (CPU).
- Multimodal context initialized via `mtmd_init_from_file(mmproj_path, model, params)`.
- LoRA adapter initialized via `llama_adapter_lora_init(model, lora_path)`.
- **Runtime LoRA Switching:**
  - `transcribe()`: Turn LoRA ON via `llama_set_adapters_lora(ctx, &adapter, 1, &scale_1.0)`.
  - `generate_text()` (Touch Up, Summarize, titles): Turn LoRA OFF via `llama_set_adapters_lora(ctx, null, 0, null)`. Base Voxtral is a standard Ministral-3B instruction model.

### 4. Input & Prompt Specification
- Audio: 16 kHz mono f32 samples loaded into `mtmd_bitmap_init_from_audio`.
- Instruction prompt formatted with media marker `<__media__>`, exactly as in training. There is no space or newline anywhere: training tokens are `<s>[INST][BEGIN_AUDIO][AUDIO]x375 Write ...[/INST]`, and the test `instruction_matches_training_prompt_exactly` checks this.
  `<s>[INST]<__media__>Write what the speaker means, ready to paste: apply their own corrections, keep every other word.[/INST]`
  If Personal Dictionary is present, append: ` Words that may appear: word1, word2, word3.`
- Prompt & audio tokenized with `mtmd_tokenize`.
- Chunks evaluated with `mtmd_helper_eval_chunks`.
- Greedy decoding (`temperature = 0`), stopped at the EOS token. Output is capped at 6 tokens per second of audio, plus 64.
- Dictations longer than 120 s are split (`split_long_audio`) at the quietest 100 ms in the last 20 s of each 120 s window, so n_ctx 4096 never overflows.

### 5. Rulebook Audit for Voxtral Pipeline
With Voxtral directly outputting clean, corrected text from audio, there is no intermediate raw ASR transcript. The 9 rulebooks are audited as follows:
1. **Hallucinations (Book 1):**
   - Stage A (`prepare_audio`): **STAYS AS IS.** VAD gate, edge trimming, and shortening pauses >1.5s to 0.6s operate purely on PCM audio and universally prevent hallucinations.
   - Stage B (`clean_asr_text`): **STAYS.** Safety net against repetitive loops, impossible speaking rates, and video-caption artifacts.
   - Stage C: Replaced by Voxtral's own self-contained decoder.
2. **Disfluency (Book 2):** **STAYS AS IS.** Catches any residual stutter or filler.
3. **Commands (Book 3):** **STAYS AS IS.** Formats explicit spoken punctuation/formatting commands (e.g., "new line", "bullet point").
4. **Numbers (Book 4):** **STAYS AS IS.** Guarantees digits-always consistency (dates, times, currency).
5. **Tech (Book 5):** **STAYS AS IS.** Handles camelCase, file extensions, URLs, and code shortcuts.
6. **Names & Capitals (Book 6):** **STAYS AS IS.** Proper casing for brands, days, months, and sentence beginnings.
7. **Tone (Book 7):** **STAYS AS IS.** Casual/Standard no-ops; Professional slang expansion.
8. **Typography (Book 8):** **STAYS AS IS.** Hygiene, apostrophes, and spacing.
9. **Faithfulness (Book 9):** **ADAPTS.** Subsequence and token ratio verification helpers remain active for Touch Up and Summarize tasks; cross-transcript diffing retired for dictation.

No rulebooks deleted.

### 6. Engine Configuration & Settings
- Add user-selectable engine: `voxtral` (default) | `whisper` (fallback) | `lite` (future).
- Enable self-corrections on both GPU and CPU.
- Update `ModeMatrix.tsx` and `FirstRunView.tsx` to reflect single-model architecture and remove outdated Qwen / Whisper GPU-only caveats.

### 7. Benchmark Measurements (re-measured 2026-10-03 with the avgpool fix)
Measured on an RTX 4060 Laptop GPU (8 GB VRAM) and an Intel Core i7 CPU. The Task 2 table had invented minimum values (code like `.max(3400)`); those clamps are removed, and every number below is a real measurement.

| Engine | Backend | Cold Load | Peak VRAM | Peak RAM | 5s Clip (med) | 15s Clip (med) | 30s Clip (med) |
|---|---|---|---|---|---|---|---|
| **Voxtral Mini 3B** | GPU (Vulkan) | 2.65 s | 3715 MB | 374 MB | **0.60 s** | **0.65 s** | **0.95 s** |
| **Voxtral Mini 3B** | CPU | 1.12 s | 0 MB | 843 MB | 13.01 s | 13.28 s | 25.29 s |
| **Whisper large-v3-turbo** | GPU (DirectML) | 3.52 s | 1114 MB | 527 MB | 0.90 s | 1.14 s | 1.03 s |
| **Whisper large-v3-turbo** | CPU | 3.18 s | 0 MB | 1070 MB | 2.86 s | 3.41 s | 3.16 s |

Peak RAM is the process working set. The model files are memory-mapped, so mapped weights may not all be counted. Voxtral on CPU (about 13 s for a 5 s clip) is too slow for daily use; that is what the lite tier is for.
Timing per dictation logged to `%APPDATA%\app.ivy.dictation\debug.log` (engine, backend, ms; no transcript text).

### 8. Acceptance Suite Results (Golden Set)
Evaluated on all 60 golden speech recordings delivered by the lab (`src-tauri/models/voxtral-ivy/golden.jsonl`):
`expected` was regenerated on 2026-10-03 by the lab's `build_golden_gguf.py`, using the patched llama-mtmd-cli (no avgpool) and the exact training prompt.
- **Total Clips Evaluated:** 60
- **Ivy reproduces `expected`:** 59 / 60 (98.3%). The one miss is the spelling "Tubermeets" vs "Tibermeets"; the test asserts at least 57.
- **Exact human gold (strict):** 45 / 60 (75.0%)
- **Lab lenient score:** corrections 34/40, normal 19/20 (PyTorch v5: 35/40, 15/20)
- **Average Dictation Latency:** 611 ms / clip (GPU)
- **Status:** PASSED.

