# IVY — Offline Voice Transcriber

**Status:** v0.1.4. Real hotkey dictation on Windows with ONE model: Ivy lite (Qwen3-ASR-1.7B fine-tuned by the lab; GPU via Vulkan or CPU), which hears speech and writes clean text with self-corrections applied. 8 deterministic rulebooks with 3 tones, spell-check Touch Up. Everything runs 100% locally. **Last updated:** 2026-10-06.

A standalone, fully offline, source-available dictation app (MIT + Commons Clause: free to use, not for sale; public on GitHub). Hold a hotkey, talk, and clean text is pasted into whatever field has focus — the same idea as Wispr Flow, but 100% local and free, with nothing ever leaving the machine.

| | |
|---|---|
| Lives in | `Downloads/IVY_Transcriber` — standalone repo, no Friday/`jarvis_v2` code or dependency |
| License | MIT + Commons Clause (free to use, not for sale); model: Apache 2.0 + Commons Clause |
| Stack | Rust + Tauri v2 + React 19/TypeScript. Windows-first; macOS and Linux planned (§19) |
| Speech & cleanup | Ivy lite: Qwen3-ASR-1.7B fine-tuned (Apache-2.0), one GGUF + mmproj, Vulkan GPU or CPU (§4) |
| Formatting | 8 deterministic rulebooks (<1ms) across Casual, Standard, and Professional tones (§8) |
| Network | None, ever — the model ships next to the installer and is copied in at install; zero runtime calls |

---

## 0. Session handoff (read first)

Chat history does not persist between sessions. This file is the persistent memory of what has been decided and built.

**Yash's standing rules**
- **Nothing may be a gimmick.** No seeded demo data, no invented numbers, no control that only simulates success. Anything visible is a claim about what the app does, including UI copy, installer text and README.
- **One model only** (Yash, 2026-10-06): Ivy lite on every PC, GPU or CPU. Voxtral and Whisper were removed after lite won Yash's blind paragraph test (round 2: lite 14 of 17 paragraphs, Voxtral 3). Never propose 7B+ models or a second model.
- **Work on the real app,** not previews or mockups.
- **Git: commits on a branch are fine (branch `lite-only`, from `voxtral-engine`), only when Yash says.** Never push to origin without asking.
- **After every change,** build, deploy to both exe locations, and relaunch (§18), so Yash never tests stale code.

**Where things stand (2026-10-06)**
- **Ivy lite is the only engine** (`lite.rs`). Model files: `src-tauri/models/ivy-lite/` (§4). It applies the speaker's self-corrections in its single pass, so there is no separate correction step and no AI cleanup pass.
- **No Speed/Accuracy mode** (removed 2026-10-06: corrections cost no extra time, so "Speed" could never be faster). The only runtime choice is GPU or CPU.
- **Tones are rules** (book 7): Casual = texting style, Standard = slang written out, Professional = formal (§8). Three modes: the one clicked on the Tone screen applies at once to every app; programs added under a mode (picked as .exe, e.g. brave.exe, matched by the foreground process) always get that mode. `tone_for_label`, 2026-10-06.
- **Touch Up** = spell-check of the pasted text (`spellcheck.rs`), never rephrasing. **Summarize was removed** (lite can't generate from text). History titles are the first words (`heuristic_title`).
- **Big round amounts** are written with the scale word ("18 lakhs", "2 million"), book 4 N8.
- **Installer:** the release is a folder: setup exe + `ivy-lite-Q8_0.gguf` + `mmproj-ivy-lite-f16.gguf`; `hooks.nsh` copies the model in (§17).
- **Test suite:** `cargo test --lib -- --test-threads=1` (see the verification table).

---

## 1. What Ivy is / isn't

- **Is:** a small background utility. Hotkey down, record, hotkey up, transcribe, clean, paste. 100% offline, works across apps on Windows.
- **Isn't:** a Friday module, a meeting note-taker, or cloud-backed in any way. Macs and Linux aren't supported yet.

## 2. Interaction flow & hotkeys

- **`Alt+Space`:** hold for push-to-talk (release stops). Double-press (each tap under 400ms, second press within 600ms of the first release) for hands-free recording; the next press stops it. Every session is saved to History.
- **`Alt+V`:** paste the latest transcript into the focused window. Covers both "no text field was detected" and "the app swallowed the auto-paste".
- **No undo key:** `Alt+B` (undo the last paste) was removed in 0.2.0 at Yash's request; the target app's own `Ctrl+Z` does the same.
- **Single Key mode** offers Caps Lock. A bare Right/Left Alt can't be registered (see §5).
- **On battery:** Ivy forces CPU (`GetSystemPowerStatus`), and returns to the configured mode when the charger is plugged back in.
- **Autostart:** Windows Run key via `tauri-plugin-autostart`. Autostart launches to the tray; a manual launch shows the window.

## 3. Platform, stack, hardware target

- **Rust core, Tauri v2,** WebView2 frontend (Vite + React 19 + Tailwind 4).
- **Windows APIs used:** `GetForegroundWindow`, `SendInput`, `GetSystemPowerStatus`, DXGI.
- **Mobile** (Android/iOS icons exist in `src-tauri/icons`) is future work.
- **Hardware rule:** every model and feature must stay usable on a CPU-only, low-RAM PC. Yash's RTX 4060 is the test machine, not the target.

## 4. Models

**Ivy lite (the only engine)**
- **Base:** Qwen3-ASR-1.7B (Qwen, Apache-2.0): Whisper-style audio encoder + Qwen3 decoder. Fine-tuned by the lab (IVY_decision_lab, `train_lite2.py`) to write what the speaker MEANS: self-corrections applied, fillers dropped, numbers in dictation style. Lite v2 = a 50/50 weight blend of lab rounds 1 and 3 (`blend_lora.py`), merged into the base.
- **Files** (`src-tauri/models/ivy-lite/`, git-ignored): `ivy-lite-Q8_0.gguf` (1.83 GB, language model, fine-tune merged in: no LoRA, no runtime switching), `mmproj-ivy-lite-f16.gguf` (0.64 GB, audio encoder + projector). The golden clips (Yash's voice) were deleted on 2026-10-06 for privacy, so `test_lite_golden_set_gpu` and the quiet-mic test skip themselves; a backup of the model is in `D:\Dev\qasr-gguf\ivy-lite2-*`.
- **Runtime:** `llama-cpp-2` + `mtmd` (projector type `QWEN3A`), Vulkan GPU (`n_gpu_layers = 99`) or CPU (`0`).
- **Prompt** (must match training token for token; `instruction_matches_training_prompt_exactly`):
  `<|im_start|>system\n{INSTRUCTION}<|im_end|>\n<|im_start|>user\n{MEDIA_MARKER}<|im_end|>\n<|im_start|>assistant\nlanguage English<asr_text>`
  with `{INSTRUCTION}` = `Write what the speaker means, ready to paste: apply their own corrections, keep every other word.` plus ` Words that may appear: a, b.` when the personal dictionary has words.
- **Silence:** outputs EMPTY text for silence or room noise (trained on purpose). Empty means "nothing was said".
- **It can't write from text** (no audio in -> empty out), which is why Touch Up is rules and Summarize is gone.

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
- **Rules written for spelled-out numbers miss a model's digits.** The currency rule needed a digit path ("500 rupees" → ₹500). "pounds" is excluded because it usually means weight.
- **The dimension rule (`NUM by NUM`, Rule 15) must reject a second token with trailing letters, not just strip them.** "500 by 5pm" used to match as a dimension and silently eat "pm", producing "500x5". Fixed 2026-09-28: it now requires everything before the token's trailing punctuation to be digits.

**Model engine**
- **The lite prompt must match training token for token** (§4). Don't add `<|audio_start|>`/`<|audio_end|>` yourself: mtmd adds them for `QWEN3A`. Tokenize with special-token parsing ON and BOS OFF (Qwen has no BOS).
- **The lite model ignores prompt changes for style:** with an empty or "word for word" instruction it still applies corrections. A verbatim mode would need the base model, not a prompt.
- **llama.cpp vs PyTorch on long audio:** 100-word greedy outputs never match PyTorch word for word (short clips match 60/60), and the 103 paragraph traps swing about ±4. Checked 2026-10-06: not quantisation (F16/BF16 give the same) and not windowing (mtmd's qwen3a already splits audio into 8 s windows like `n_window_infer`). Judge the GGUF on its own scores.
- **Never give the model the pause-squeezed audio** from `prepare_audio`: squeezing pauses dropped the end of long dictations (Task 6 Bug B). Stage A is only the speech/no-speech gate now.
- **The vendored `llama-cpp-sys-2` keeps the `IVY PATCH`** (no Voxtral audio avg-pooling, `tools/mtmd/clip-model.h`). Voxtral is gone, so it no longer matters, but keep it when bumping the crate unless the vendor is replaced.

**Rendering, platform and build**
- **Outset `box-shadow` on a transparent DirectComposition surface renders as a hard rectangle** (Skia premultiplication). Use a solid fill, an inset highlight and a 1px border instead.
- **The `TrayIconBuilder::build()` handle must be kept alive** (`app.manage`), or the icon vanishes.
- **`canonicalize()` prepends `\\?\`.** Canonicalize both sides before comparing paths.
- **Quiet microphones went EMPTY** (lite on clips peaking at 0.04-0.06). Peak-normalising to 0.7 (gain max 20x) fixed most (§10). `cpal` doesn't request Windows raw capture mode, so mic-enhancement APOs vary per device.
- **NSIS:**
  - `IntCmp` has no "fall through" placeholder; use the 3-argument form.
  - `System::Int64Op` compares with `=`.
  - NTFS is case-insensitive, so a case-only rename of an output file is the same file.
  - `$PLUGINSDIR` doesn't reliably auto-delete; `RMDir /r` it on every exit path.
- **Build directory & Windows path limit:** Windows' 260-character limit can bite deeply nested CMake Vulkan shader files in `llama-cpp-sys-2` if the repo clone path is deep. Set `CARGO_TARGET_DIR` (e.g. `C:\Users\YASH\Downloads\ivytgt` on Yash's PC) to a short path to stay well below the 260-character ceiling. Deploying from the wrong folder shipped a days-old binary once — always verify target output timestamp.

## 6. Pipeline

hotkey → `audio.rs` capture (own thread) → 16kHz → `normalize_audio` (peak 0.7, max 20x) → Hallucinations stage A (speech gate only) → **lite `transcribe`** (full audio; >120 s split at pauses) → Hallucinations stage B (`clean_asr_text`) → `rulebooks::after_model` (commands, tone, numbers, tech, names, typography) → `apply_personal_dictionary` → `paste_text` → history/stats → `ivy://dictation-complete`.

The only runtime choice is **GPU or CPU** (the setup wizard, or Settings → Hardware). On battery, Ivy uses CPU. **Smart GPU sharing** (``gpu_monitor``, every 3 s, Settings switch): (1) a full-screen app in front for 6 s (Windows' own ``SHQueryUserNotificationState``: busy / D3D full screen / presentation; browsers, terminals, editors and Explorer excluded) puts Ivy to sleep: model unloaded, dictation hotkey unregistered so it reaches the game, no overlay; leaving full screen re-registers the hotkey and pre-warms the model. (2) In GPU mode, when OTHER programs keep the GPU (Windows ``GPU Engine`` counter, busiest engine, Ivy's own process excluded) at or above the user's threshold (50-100%) for 6 s, Ivy unloads, dictates on CPU and shows "GPU above your N% limit · Ivy switched to CPU" on the overlay; back to GPU after 15 s at 15 points below the threshold. Both tested 2026-10-06 (92% load: switched, back 39 s after; full-screen window: asleep with Alt+Space free, awake + reloaded 3 s after).

## 7. Speech model (`lite.rs`)

- One `LiteEngine` (cached per GPU/CPU mode; `unload_engine` on a hardware switch or VRAM eviction).
- Greedy decoding, stop at `<|im_end|>`/`<|endoftext|>`, output cap 6 tokens per second of audio + 64, n_ctx 4096.
- Dictations longer than 120 s are split at the quietest 100 ms in the last 20 s of each window (`split_long_audio`). A correction spoken across a cut isn't merged (ponytail note in the code).
- Timeout scales with length: CPU 60 s + 2 s per audio second, GPU 30 s + 0.5 s per audio second.

## 8. Rulebooks & tones (`src/rulebooks/`, RULEBOOKS.md)

**Read RULEBOOKS.md before touching any rule.** 8 books (the disfluency and AI-faithfulness books were removed with the AI cleanup pass; lite removes fillers itself).

- **Book 1, Hallucinations, outranks all.** Stage A: no voice → no text. Stage B on the model's text: bag of hallucinations, loops, impossible speaking rate.
- Books 3–8: spoken commands, numbers (digits; big round amounts as lakhs/crores/million), tech, names and capitals, tone, typography.
- **Laws:** whole words only; never trade a spoken word for another (except the tone's fixed lists); when in doubt leave it as spoken; idempotent; fixed order; std-only and under 1 ms.
- **Tones** (each a step up; research: rule-based formality keeps meaning best):
  - Casual: the speaker's words, texting style: sentence-ending full stops dropped and each sentence on its own line (`casual_endings`, run last); "?", "!", "...", abbreviations and numbers kept.
  - Standard: slang written out ("gonna" → "going to"), ", like," / ", you know," dropped, "off of" → "off".
  - Professional: Standard + contractions written out, chat words → formal ("yeah" → "yes", "btw" → "by the way", "thanks" → "thank you"), an opening "Honestly," / "Basically," dropped, no "!".
  - The Tone screen's samples are the real rule outputs (`tone_screen_samples_are_real`).

## 9. Touch Up

- Capsule button for 7 s after every real paste (GPU and CPU). Spell-check only (`spellcheck.rs`): misspelled plain lowercase words are fixed from SymSpell's 80k English frequency list (MIT); names, acronyms, numbers, emails, file names, contractions and a keep list (lakh, chai, yaar, haan …) are never touched. No typo → nothing is swapped and the capsule says "No typos found".
- The swap is `Ctrl+Z`, then a re-paste, after a focus re-check. History is updated.
- **History titles** use `heuristic_title` (first 4-6 words, no AI). There is no Summarize.

## 10. Audio capture & gain (`audio.rs`)

- **Capture:** `cpal` on a dedicated thread (COM safety), resampled to 16kHz with `rubato`.
- **Gain:** peak-normalisation to 0.7, gain capped at 20x (Task 7, 2026-10-06). Left alone: true silence (peak < 0.01), already-loud audio (peak ≥ 0.7) and flat noise (peak/RMS < 1.6). It replaced the `dagc` AGC: on the lab's quiet clips peak-normalising recovered 4 of 5 empty outputs (`test_task7_quiet_microphones`).
- **Privacy:** samples are zeroized on every exit path.
- **Guards:** near-silent input (peak below `SILENCE_PEAK_THRESHOLD`) is discarded as silence. Recordings are capped at `MAX_DICTATION_SECS`.
- **Mic hot-plug:** the Settings mic list refetches on window focus and when the dropdown opens. The Rust side re-enumerates on every call.

## 11. Performance

Measured on Yash's RTX 4060 laptop GPU and Intel Core i7 (§23):
- **GPU (Vulkan):** 0.22 s (5 s clip), 0.33 s (15 s), 0.34 s (30 s); a 60 s paragraph 1.5-2.2 s. Peak VRAM ~3.7 GB.
- **CPU:** 1.95 s (5 s clip), 3.20 s (15 s), 4.88 s (30 s); a 60 s paragraph 11-22 s (about two thirds of it is the audio encoder).

## 12. Paste & clipboard (`lib.rs`)

- **`paste_text(text, target)`:**
  1. Poll the foreground for ≤300ms and require the hotkey-down window.
  2. Store the text in `MANUAL_PASTE_TEXT` (for Alt+V), then set the clipboard.
  3. Wait 60ms, re-check focus, and send `Ctrl+V`.
  4. Record `LAST_PASTE` (the target window, for Touch Up), then restore the clipboard conditionally after 800ms.
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
- **Settings changes** emit `ivy://settings-updated`, so the long-lived capsule re-reads settings (the manual-paste hotkey it shows).
- **`App.tsx` `handleUpdateSettings`** merges from `settingsRef`, not inside a setState updater. A deferred updater once left the merged value null and silently skipped the save. The GPU/CPU dialog saves once: a second `save_settings` within 100ms hits the backend rate limit.
- **Launch intro** (`IvyLaunchIntro.tsx` + `src/utils/audio.ts`): a Web Audio sequence synced to the animation. It plays once per session (`sessionStorage`), with Skip/Close buttons. Its callbacks are stabilized via a ref because an inline-lambda dependency restarted it in a loop. WebView2 autoplay is enabled via `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--autoplay-policy=no-user-gesture-required` in `main.rs`. `RootErrorBoundary` wraps the app.

## 14. Onboarding wizard (`FirstRunView.tsx`, `StageIndicator.tsx`)

- **7 stages:** 1 Shortcuts, 2 Smart Clipboard, 3 GPU or CPU, 4 Touch Up, 5 Voice Test, 6 Auto-Correct demo, 7 Privacy. Both real-mic stages come late, so Defender's first scan of the new model files is less likely to land on the user's first impression.
- **Stage 6** says no trigger words are needed (the model applies corrections on GPU and CPU). Success is judged from the real result ("French fries" absent).
- **Stage numbers are positional.** To reorder, map old→new numbers and update every `currentStage === N`, `.add(N)` and `setCurrentStage(N)`.
- **Stage-count claims in copy** must be checked against `StageIndicator.tsx` (it was wrong twice from memory).
- **The sidebar IVY wordmark** reopens the wizard.

## 15. Stats & history privacy

- **`history.json` + `audio/*.wav`** (`%LOCALAPPDATA%\app.ivy.dictation`; 0.1.5 and older used Roaming `%APPDATA%`, moved over once at startup by `migrate_roaming_data`): 24h retention (`RETENTION_SECS`). "Clear History" wipes everything.
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
  - Excludes models, `*.wav` (the synthesized test fixtures are re-included), audio and history. Local tool folders are excluded through `.git/info/exclude`, so they never show up in the repo.
  - `.env.example` was deleted because nothing reads env vars.

## 17. Installer packaging

- **The release is a folder, FitGirl-repack style** (Yash, 2026-10-06): `Ivy_<version>_x64-setup.exe` + `ivy-lite-Q8_0.gguf` (1.83 GB) + `mmproj-ivy-lite-f16.gguf` (0.64 GB). The model can't go inside the exe: NSIS caps the payload at 2 GB and GitHub caps every release file at 2 GB.
- `npm run package-installer` (`scripts/package-installer.mjs`) builds the NSIS installer and assembles `release/Ivy-<version>/`. `npm run checksums` hashes exe + gguf files.
- `src-tauri/installer/hooks.nsh`: copies the two model files from next to the setup exe into `$INSTDIR\models\ivy-lite` (warns, non-silent installs only, if they're missing), deletes the model on uninstall, and asks before deleting user data.
- `npm run setup-models` (`scripts/download-models.mjs`) downloads the model from the GitHub release for source builds. **The v0.1.4 release must exist with both gguf files attached for it to work.**

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
  - `%LOCALAPPDATA%\app.ivy.dictation\debug.log` — per dictation: `lite ok via GPU (Vulkan)/CPU in Xms`, and `settings saved: hardware=…`.
  - `%LOCALAPPDATA%\app.ivy.dictation\logs\Ivy.log` — diagnostic logs, model loading, and fallback telemetry.
  - Read both before guessing. Saved recordings in `audio/` can be replayed through the engine for apples-to-apples comparisons.
- **Tests:** `cargo test --lib -- --test-threads=1` (the lite golden, quiet-mic and benchmark tests load the model on the GPU; run them only when the GPU is free). Model-backed tests skip themselves if the model files are missing. Also run `npx tsc --noEmit`.
- **Key files:**
  - `src-tauri/src/lib.rs` — commands, pipeline, hotkeys, paste, settings.
  - `lite.rs` — the speech model engine.
  - `spellcheck.rs` — Touch Up (data: `src-tauri/data/en-80k.txt`).
  - `audio.rs` — capture and gain.
  - `gpu_monitor.rs` — DXGI load/VRAM eviction and battery.
  - `src/components/*` — views, wizard, capsule UI, `ModeMatrix.tsx`.
  - `scripts/download-models.mjs` (`npm run setup-models`) and `scripts/package-installer.mjs`.

## 19. Cross-platform roadmap (not built)

- **macOS:** Accessibility API (`AXUIElementSetValue`) injection. Any clipboard fallback must be tagged `org.nspasteboard.TransientType`.
- **Linux X11:** `XGrabKey`, `_NET_ACTIVE_WINDOW`, `XTestFakeKeyEvent`, draining modifiers via `XQueryKeymap`.
- **Linux Wayland:** needs the input-method protocol (`zwp_input_method_v2::commit_string`).
- **Windows-only code to port:** DXGI telemetry, `SendInput`, power status, the Alt-menu mask.

## 20. Open items

- **GitHub release v0.1.4** needs the two gguf files attached (installer + `setup-models` depend on it). Code signing: SignPath Foundation is free only for OSI open-source licenses, which MIT + Commons Clause is not (2026-10-08); a paid certificate or Azure Trusted Signing instead.
- **Lab:** lite v2 sometimes writes a whole chatty paragraph in lowercase without punctuation (Yash round 2 p20). Cause: 44% of the YouTube-subtitle training keys have no punctuation. Round 5 (lab) retrains without them; swap the model files in if it passes.
- **Personal Dictionary for accent mishears** (place names like Gachibowli): supported in the lite prompt (`Words that may appear: ...`).
- **Mic start-up clips ~650ms of every dictation** on Yash's Realtek (§5), measured with `live_mic_capture`: `Recorder::start` takes 200–285ms to open the stream, then the driver sends zeros for a steady ~425ms. Remaining option is keeping the stream open while Ivy runs (mic-in-use indicator stays lit). Yash said 400ms is acceptable if it can't be reduced (2026-09-28).
- **macOS** after Windows is finished (§19): Metal, Apple signing ($99/yr) + notarization, mic/Accessibility/Input Monitoring permissions.

## 21. Standing lessons

- **Verify against real files, logs and recordings,** not a summary. Several agent reports and this file itself drifted from the code more than once. The code and logs are the truth.
- **Test with Yash's real saved recordings and the model's real output,** not only synthetic fixtures. Fixtures share the code's blind spots.
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
| 2026-10-03 (fixes) | **70 passed, 0 failed, 1 ignored** | Patched llama.cpp avgpool bug (vendored `llama-cpp-sys-2`), exact training prompt, long-audio split, length-scaled timeout, single BOS, UTF-8 decode, real benchmark numbers. Golden 59/60 vs regenerated `expected`. Release build, tsc clean. |
| 2026-10-06 (lite-only) | **61 passed, 0 failed, 1 ignored** (57 CPU + 4 lite GPU: golden 60/60, quiet mic 4/5, benchmark) | Antigravity Tasks 5-7 verified (lite golden 60/60, quiet mic 4/5). Voxtral, Whisper, Speed/Accuracy and Summarize removed; tone rulebooks, spell-check Touch Up, lakhs/crores rule, split-release installer. tsc clean. |

---

## 23. Lite engine measurements

### Benchmark (re-measured 2026-10-06, `test_lite_benchmark_measurements`)
RTX 4060 Laptop GPU (8 GB VRAM) and an Intel Core i7 CPU. Real measurements only.

| Engine | Backend | Cold Load | Peak VRAM | 5s Clip (med) | 15s Clip (med) | 30s Clip (med) |
|---|---|---|---|---|---|---|
| **Ivy lite v2** | GPU (Vulkan) | 2.72 s | 3660 MB | **0.22 s** | **0.33 s** | **0.34 s** |
| **Ivy lite v2** | CPU | 1.16 s | 0 MB | **1.95 s** | **3.20 s** | **4.88 s** |

For comparison, the removed Voxtral Mini 3B took 0.60/0.65/0.95 s on GPU and 13.0/13.3/25.3 s on CPU.

### Golden set
60 set-2 clips (`src-tauri/models/ivy-lite/golden.jsonl`, `expected` = llama-server output with the exact prompt, GPU): **Ivy reproduces 60/60** (`test_lite_golden_set_gpu`, asserts ≥ 57). Lab scores for this GGUF: corrections 34/40, normal 19/20; round 1 paragraph traps 76/103, paragraph WER 11.5% (lite v1 76/103, 13.2%; Voxtral 75/103, 13.3%).

### Yash's blind round 2 (20 new paragraphs in his voice, 2026-10-06)
Best version per paragraph: lite v2 9, lite v1 5, Voxtral 3, none right 3. That decided lite-only.
