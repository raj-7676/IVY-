# IVY — Offline Voice Transcriber

**Status:** v0.1.0. Real hotkey dictation on Windows. Whisper large-v3-turbo STT, a 50+ rule cleanup engine with 3 tones, and Qwen 2.5 3B for live AI cleanup (GPU + Accuracy), Touch Up and Summarize. Everything runs locally. **Last updated:** 2026-09-28.

A standalone, fully offline, open-source dictation app (MIT, to be published on GitHub). Hold a hotkey, talk, and clean text is pasted into whatever field has focus — the same idea as Wispr Flow, but 100% local and free, with nothing ever leaving the machine.

| | |
|---|---|
| Lives in | `Downloads/IVY_Transcriber` — standalone repo, no Friday/`jarvis_v2` code or dependency |
| License | MIT |
| Stack | Rust + Tauri v2 + React 19/TypeScript. Windows-first; macOS and Linux planned (§19) |
| Speech to text | Whisper large-v3-turbo, int8 ONNX, via `ort`. DirectML GPU with automatic CPU fallback (§4) |
| Cleanup | 50+ deterministic rules (§8), plus Qwen 2.5 3B via `llama-cpp-2`/Vulkan in GPU + Accuracy mode (§7) |
| Network | None, ever — models ship with the install; zero runtime calls |

---

## 0. Session handoff (read first)

Chat history does not persist between sessions. This file is the persistent memory of what has been decided and built.

**Yash's standing rules**
- **Nothing may be a gimmick.** No seeded demo data, no invented numbers, no control that only simulates success. Anything visible is a claim about what the app does, including UI copy, installer text and README.
- **Target the weakest PC, not Yash's RTX 4060 laptop.** Ivy is open source for Windows, macOS and Linux. Never propose a bigger model (7B+) because it "fits on 8GB". Improve quality through prompting, guards and rules first.
- **Work on the real app,** not previews or mockups.
- **Git: nothing is committed.** Never commit or push without asking.
- **After every change,** build, deploy to both exe locations, and relaunch (§18), so Yash never tests stale code.

**2026-10-02 / 2026-10-03: rulebooks rewrite (Yash asked: clean rulebooks + a separate Hallucinations book, research-backed).**
- New `src-tauri/src/rulebooks/` (9 books) and RULEBOOKS.md.
- `cleanup.rs` went from 3,524 to 592 lines.
- `lib.rs` runs Hallucinations stages A and B around Whisper.
- `ModeMatrix.tsx` text was updated.
- Cold build succeeded: `npm run tauri build -- --no-bundle` produced `C:\Users\YASH\Downloads\ivytgt\release\app.exe`.
- `npx tsc --noEmit` clean.
- `cargo test --lib -- --test-threads=1`: **66 passed, 0 failed, 2 ignored**.
- Deployed to both `C:\Users\YASH\Downloads\Ivy.exe` and `C:\Users\YASH\AppData\Local\Ivy\app.exe`, and relaunched.
- A backup of the replaced files is in `D:\Dev\CODE\ivy_backup_2026-10-02_before_rulebooks`.

**Where things stand (2026-09-28)**
- STT is Whisper large-v3-turbo. The cleanup LLM is Qwen 2.5 3B only. Moonshine, Phi-4-mini and Qwen 1.5B are all deleted.
- GPU + Accuracy mode runs the AI stage described in §7. The old deterministic self-correction system (`resolve_self_corrections`, marker lists) was deleted. Only the AI resolves corrections now.
- `cargo test --lib -- --test-threads=1`: **52 passed, 0 failed, 1 ignored**.
- The latest exe is deployed to both locations and relaunched.
- **GPU + Accuracy retest (2026-09-28) with the 4 sentences — 3/4 clean, 1 real bug found and fixed:**
  - France question: kept as transcribed (Qwen dropped a clause, `check_ai_faithful` guard rejected it, rule pipeline pasted it verbatim — guard worked as designed).
  - Biryani/lemonade cross-sentence correction: still not resolved by the 3B model, matches the known limit (§20) — expected, not a bug.
  - "Send it to marketing, scratch that, sales": Qwen resolved it correctly to "Send it to sales."
  - Samosa/money sentence: found a real formatting bug. `format_ordinals_fractions_dimensions` (`cleanup.rs`, Rule 15) matched "500 by 5pm" as a dimension pair, silently eating the "pm" and producing "500x5" instead of leaving the time alone. Root cause: the dimension rule only checked that token 2's *numeric-stripped core* was numeric, not that the whole non-trailing-punctuation part of the token was numeric — so a token like "5pm" (numeric prefix + letters, no punctuation) slipped through. **Fixed:** the rule now requires everything before the trailing punctuation to be digits, so "5pm"/"5ft"/"5kg" no longer match. Regression check added next to the existing dimension test.
- **Wizard/capsule fix (2026-09-28):** the floating capsule overlay (a separate always-on-top OS window) was popping up over every onboarding stage, including Stage 5 Voice Test, overlapping the wizard's own recording UI. Added `WIZARD_ACTIVE` (an `AtomicBool` in `lib.rs`) gated inside `bridge_capsule_show` — the single choke point every capsule-show call already routed through — so no call site needed touching. `FirstRunView.tsx` calls the new `set_wizard_active` command on mount/unmount. Stage 5/6 still run real dictations through the real hotkey pipeline; only the visible overlay window is suppressed while the wizard is open.
- **Stage 5 "instant loading"/silent capture (2026-09-28).** Every wizard capture came back `peak=0.0000`. Findings, each verified:
  - The mic and cpal are fine: `cargo test --lib live_mic_capture -- --ignored --nocapture` (standalone, outside the app) records real room noise.
  - cpal's "A buffer underrun or overrun occurred" (`Xrun`) is WASAPI's non-fatal DATA_DISCONTINUITY flag. It fires on good captures too. An earlier fix this session wrongly treated it as the cause and added a "microphone glitched" message; that was reverted. It is now logged as a warning only.
  - This Realtek mic outputs **pure zeros for the first ~400ms** after a stream opens (standalone: 400ms → peak 0, 700ms → real signal). Any short hold is silence.
  - In the wizard, the stream opened ~250ms later than standalone, because Stage 5/6 also opened the same mic via WebView2 `getUserMedia` (echo cancellation on) for the visualizer, at the same instant. **Fixed:** the wizard no longer calls `getUserMedia` in the app; Rust emits `ivy://mic-level` (peak of the latest buffer, ~20Hz) from its own recorder and the visualizer uses that.
  - The wizard stayed mounted on Stage 5 after the window was closed to the tray, so it kept reacting to every Alt+Space everywhere (opening the second mic stream) and `WIZARD_ACTIVE` kept the capsule hidden app-wide. **Fixed:** wizard hotkey handlers require `document.hasFocus()`, its completion handlers ignore dictations it didn't start, and `bridge_capsule_show` suppresses the capsule only when `WIZARD_ACTIVE` **and** the main window is focused.
- **Stage 1 hotkey tester bug (2026-09-28):** the tactile "press your shortcut" tester in Stage 1 lit up for *any* of Alt/Space/Control/CapsLock, ignoring which of the 3 options (`Alt + Space`, `Caps Lock`, `Ctrl + Space`) was actually selected — picking Caps Lock but pressing plain Alt "worked", picking Ctrl + Space but pressing plain Control "worked". Root cause: Stage 1's checker (`FirstRunView.tsx`) was a separate, looser condition than Stage 5/6's real `triggerMatch`, which already gated correctly on `selectedHotkey`. **Fixed** by making all three stages share one `matchesSelectedHotkey` expression instead of Stage 1 having its own — this can't drift apart again because there's only one check left.
- **Next:** Yash confirms the capsule stays gone through the whole wizard, that Stage 1 now only lights up for the actually-selected key, and retests Stage 5 — if it fails again, `debug.log`/`Ivy.log` will now say plainly whether it was silence or a stream glitch.
- **Repo cleanup (2026-09-27):** removed dead code, 4 unused npm packages, unused images and assets, old logs, the stale Sep 23 `release/Ivy_Setup.exe` (it still had Moonshine and 1.5B inside), and the stale `src-tauri/target`. Fixed the installer scripts' 1.5B paths and the stale model names in README, ARCHITECTURE, SECURITY and the privacy page. Nothing is committed.

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

**STT: Whisper large-v3-turbo** (onnx-community int8 export)
- **Files:** `encoder_model_int8.onnx` (~645MB), `decoder_model_merged_int8.onnx` (~438MB) and `tokenizer.json`, in `src-tauri/models/whisper/`.
- **Why it replaced Moonshine v2 base (2026-09-24):** Moonshine hit an accuracy ceiling on accented speech, and Whisper is multilingual (Hindi loanwords).
- **Parakeet was rejected:** it is English-only, and its transducer decoder can't use the hotword trie.
- **Language:** English is forced (`<|en|>`), using the 4-token prefix `SOT, EN, TRANSCRIBE, NOTIMESTAMPS`.
- **Frontend:** a 128-bin log-mel spectrogram via `mel_spec`'s `BatchLogMelSpectrogram`. Whisper's log10 and global normalization are applied by hand, because the crate's convenience path uses natural log and per-frame normalization.
- **Encoder:** fixed at 3000 frames (a 30s window). The ONNX export rejects anything shorter ("Got invalid dimensions for input: input_features", tested). Audio longer than 30s is split at low-energy pauses into 20–27s chunks.
- **Decoder:**
  - Greedy, using `IoBinding`. Encoder states and the cross-attention KV are bound once per segment, and a `cache_position` input is required.
  - The Personal Dictionary becomes a `HotwordTrie` that adds a +3.0 logit boost.
  - EOS is forbidden for the first 3 tokens.
  - A repeating-cycle guard (period 1–8, **5** repeats) stops hallucination loops. It is 5 rather than 3 so a real "no, no, no" doesn't cut the sentence off.
  - Output is sanitized: "Thank you for watching." and music/laughter tags are removed.
- **Whisper cleans up speech on its own:** it adds punctuation, writes numbers as digits ("500", "5 pm"), and sometimes drops repeated words. On Yash's own recordings it kept "No, no," in 2 of 3 takes.

**Cleanup LLM: Qwen 2.5 3B-Instruct Q4_K_M**
- **File:** `src-tauri/models/qwen2.5-3b/qwen2.5-3b-instruct-q4_k_m.gguf` (2,104,932,768 bytes).
- **Runtime:** `llama-cpp-2 = { version = "0.1.156", features = ["vulkan"] }`, used through a process-wide `LlamaBackend` singleton.
- **GPU offload is verified:** 37/37 layers on the RTX 4060. The engine reloads when the requested CPU/GPU mode differs from the cached one.
- **3B was chosen over 1.5B by Yash** (context understanding matters most). Bigger models are off the table (§0).

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

**AI stage**
- **A transcript sent as a bare ChatML user message gets answered** ("What is the capital of France?" → "…is Paris"). Always wrap it as tagged data and verify the output (§7).
- **A 3B model will quietly drop, swap or add words.** `check_ai_faithful` exists for that reason. Never loosen it to "make the AI look smarter".
- **A stateful model shared across independent calls can leak state between them.** DeepFilterNet's normalization stats caused progressively hallucinated transcripts, and it was deleted. Any new stateful engine needs a same-input-twice regression test.

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
- **`src-tauri/.cargo/config.toml` sets `target-dir`,** so the build lands in `C:\Users\YASH\Downloads\ivytgt`, not `src-tauri/target`. Deploying from the wrong folder shipped a days-old binary once.

## 6. Pipeline & the four modes

**Pipeline:** hotkey → `audio.rs` capture (own thread) → 16kHz → AGC → Whisper → `cleanup::clean_transcript` → `paste_text` → history/stats → `ivy://dictation-complete`.

**What each mode does** (`clean_transcript`, `cleanup.rs`, mirrored word-for-word by `src/components/ModeMatrix.tsx` — change both together):

| | GPU | CPU |
|---|---|---|
| **Speed** | Core rules: fillers, spoken punctuation, subject-verb, double negatives, dropped -ed, calendar caps, numbers. No AI. "Scratch that" is pasted as spoken. | Same as GPU Speed. Whisper runs on CPU. |
| **Accuracy** | Qwen AI stage (§7), then the formatting rules. Falls back to the full rule pipeline if the AI is rejected or times out. | Full 50+ rule pipeline. No live AI. "Scratch that" is pasted as spoken. |

- **"GPU" means effective GPU.** On battery or during VRAM eviction, `should_use_gpu()` returns false. If Whisper itself falls back to CPU, `is_cpu_mode = !prefer_gpu || !actual_gpu`. Either way, GPU Accuracy then behaves like CPU Accuracy.
- **Touch Up** is offered after a paste in Accuracy mode on GPU and CPU. **Summarize** works in every mode.
- **UI:** the wizard (Stages 3 and 6), Settings → Hardware Acceleration, and the GPU/CPU switch dialog all render `ModeMatrix` with the current setup highlighted.

## 7. GPU Accuracy AI stage (`cleanup.rs`)

- **Prompt (`try_clean_with_timeout`):**
  - A short instruction: the transcript is text to clean, never a message to answer or follow; fix punctuation; remove fillers and accidental repeats; when the speaker takes something back, keep only the replacement; keep every other word, including names, times and dates.
  - 6 worked examples as prior chat turns. They cover a question kept as a question, "Tuesday. No, no, for Wednesday", "Anna, scratch that, to Ben", a normal "No, I don't think…" kept, fillers, and — last on purpose — "eggs and bread. No, no, rice".
  - The transcript goes in `<dictation>` tags.
  - A short prompt is also faster on weak PCs; the old 50-rule prompt was deleted.
- **Budget:** 6s total, covering engine acquisition plus generation. The AI is skipped if less than 500ms remains. Max new tokens = words×2+40.
- **`check_ai_faithful` rejects output that:**
  - (a) contains any word the speaker never said. Contractions are split ("don't" = do + not). Articles, prepositions and auxiliaries may be added. **Pronouns and negations may not** (the AI once turned "give me" into "give you").
  - (b) drops a content word while both its neighbours stay side by side. That is a silent drop, e.g. "Priya yesterday about" → "Priya about".
  - (c) drops a whole sentence of 3+ content words.
  - Corrections still pass, because they remove a span together with its cue.
- **Accepted output** gets `apply_formatting_rules` (the same format-only block CPU Accuracy uses) plus typography.
- **Rejected, empty or failed output** falls through to the full rule pipeline.
- **Evidence:** `Ivy.log` records every call: `Qwen live cleanup pass - input: … raw LLM output: …` and then `accepted` or `rejected (reason)`.
- **Real 3B results on Yash's own Whisper output:**
  - The France question is kept, not answered.
  - "₹500 … 5 PM IST" is produced.
  - "Send it to marketing, scratch that, send it to sales." → "Send it to sales."
  - Known limit: "…I want lemonade. No, no, I want watermelon juice." is **not** resolved (left as spoken), even with a matching example.
  - A Whisper "verbatim prompt" (`<|startofprev|>` plus disfluent text) had zero effect on real recordings and was removed.

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

- **Touch Up** (capsule button for 5s after a real paste, Accuracy mode, GPU and CPU):
  - Qwen may only fix punctuation and remove exact repeats. `is_word_subsequence` rejects any substituted or invented word.
  - The swap is `Ctrl+Z`, then a re-paste, after a focus re-check. History is updated, and Alt+B still restores the original clipboard.
  - Timeouts are 10s on GPU and 15s on CPU. These were tuned for 1.5B and are unverified for 3B on CPU.
- **Summarize** (History, any mode): bullet points, guarded by `contained_ratio`. Timeouts are 20s GPU and 35s CPU. Real failures show a real error.
- **History titles** use `heuristic_title` (no AI).

## 10. Audio capture & gain (`audio.rs`)

- **Capture:** `cpal` on a dedicated thread (COM safety), resampled to 16kHz with `rubato`.
- **Gain:** `dagc` `MonoAgc` toward -12dBFS (0.25 RMS). It is frozen on 20ms frames near the 10th-percentile noise floor, so room noise is never boosted. A fresh `MonoAgc` is built for every call, so no state carries between dictations. `DISTORTION_FACTOR = 0.001` (0.0001 converged too slowly for 1–3s clips).
- **Privacy:** samples are zeroized on every exit path.
- **Guards:** near-silent input (peak below `SILENCE_PEAK_THRESHOLD`) is discarded as silence. Recordings are capped at `MAX_DICTATION_SECS`.
- **Mic hot-plug:** the Settings mic list refetches on window focus and when the dropdown opens. The Rust side re-enumerates on every call.

## 11. Performance (measured 2026-09-27, Yash's RTX 4060 laptop)

| Stage | GPU | CPU |
|---|---|---|
| Whisper encoder (fixed 30s window) | ~770ms | ~3.3s |
| Whisper total, 6–13s clip | ~1.0–1.3s | ~3.7–4.2s |
| Qwen 3B live pass (warm) | ~0.7s | ~2.2s (not used live) |
| Rules | ~1ms | ~1ms |
| History save + paste | ~0.1s | ~0.1s |

- **Totals:** GPU Speed is about 1.2s and GPU Accuracy about 1.9s. CPU is about 4s.
- **CPU cost is almost all Whisper's encoder** chewing a fixed 30s window, even for a 3-second clip. The export can't take less. No artificial delays exist beyond the 60ms pre-paste settle and the ≤300ms focus poll.
- **Unexplored levers:** a smaller Whisper for CPU mode (faster, less accurate), an fp16 encoder for DirectML (faster on GPU, larger download), or a custom export with a dynamic encoder length. All are Yash's call. On a weak PC, CPU mode will be much slower than 4s.

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

- **Single file:** Qwen's ~2GB GGUF plus the Whisper-bearing installer exceed NSIS's 2GB data cap. A 7-Zip SFX was rejected (the modern 7-Zip no longer ships `7zSD.sfx`, and old binaries are an unpatched security risk).
- **`npm run package-installer`:**
  1. Asks `cargo metadata` for the real target dir.
  2. Runs `tauri build`.
  3. Compiles `src-tauri/installer/wrapper.nsi` (embeds the real installer).
  4. Appends the GGUF plus a 16-byte footer (magic + little-endian length) to produce `…/release/bundle/nsis/Ivy_Setup.exe`.
- **At install time:** the wrapper streams the GGUF out next to the real installer and runs it. `hooks.nsh` then:
  - copies the GGUF to `$INSTDIR\models\qwen2.5-3b\`;
  - asks GPU or CPU and writes `hardware_preference.txt` (read and deleted once by `load_settings`; silent installs default to GPU);
  - removes the GGUF on uninstall, and asks before deleting app data.
- **Not rebuilt or tested since the Whisper + 3B switch.** Rebuild and do a real install before any release.

## 18. Build, deploy & debugging

- **Prerequisites:** Rust, Node 18+, MSVC Build Tools, WebView2, CMake + Ninja, LLVM (`winget install LLVM.LLVM`), and the Vulkan SDK (`VULKAN_SDK`).
- **Cargo config:** `.cargo/config.toml` (repo root and `src-tauri/`) sets `CMAKE_GENERATOR = "Ninja"`, `CL`/`_CL_ = "/FS"`. These must be environment variables so the nested `vulkan-shaders-gen` CMake build inherits them; that was the fix for the MSVC `C1041` PDB race.
- **Build:** `npm run tauri build -- --no-bundle` from the repo root. Never a bare `cargo build` (it skips embedding `dist/`).
- **Deploy:**
  1. Stop `Ivy.exe`/`app.exe` and wait for them to exit.
  2. Copy `C:\Users\YASH\Downloads\ivytgt\release\app.exe` to **both** `C:\Users\YASH\Downloads\Ivy.exe` and `C:\Users\YASH\AppData\Local\Ivy\app.exe` (the desktop shortcut target).
  3. Relaunch.
- **Model path:** the dev-installed app has no models folder. `models_dir()` falls back to the compile-time `CARGO_MANIFEST_DIR\models`, so deleting a model under `src-tauri/models` affects the running app immediately.
- **Logs:**
  - `%APPDATA%\app.ivy.dictation\debug.log` — per dictation: engine (`stt ok via GPU/CPU in Xms`), `cleanup pass … via Qwen AI|rules (gpu|cpu, mode)`, and `settings saved: hardware=… dictation=…`.
  - `%LOCALAPPDATA%\app.ivy.dictation\logs\Ivy.log` — Qwen's exact input and output, plus the accept/reject reason.
  - Read both before guessing. Saved recordings in `audio/` can be replayed through Whisper for apples-to-apples comparisons.
- **Tests:** `cargo test --lib -- --test-threads=1`. Parallel runs load several 3B engines plus Whisper on one GPU and time out ("generation timed out"), which is not a real regression. Model-backed tests skip themselves if the model files are missing. Also run `npx tsc --noEmit` (clean even with `--noUnusedLocals --noUnusedParameters`).
- **Key files:**
  - `src-tauri/src/lib.rs` — commands, pipeline, hotkeys, paste, settings.
  - `stt.rs` — Whisper.
  - `cleanup.rs` — rules, AI stage, Touch Up, Summarize.
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

- **Retest GPU + Accuracy** with the 4 sentences (biryani, samosa, France, rename) and confirm in `Ivy.log`.
- **Cross-sentence corrections** ("…lemonade. No, no, I want watermelon juice.") aren't resolved by 3B. Whisper also sometimes deletes the "no, no" itself. There's no fix yet within the weakest-hardware constraint.
- **CPU speed (~4s here, slower on weak PCs)** comes from the fixed 30s encoder window. The levers are in §11 and need Yash's decision.
- **`target-dir = "C:/Users/YASH/Downloads/ivytgt"`** is a machine-specific absolute path in a repo meant to be published. It must become portable before release. Test with a clean build first, because the short path may be what keeps the Vulkan shader build under Windows' 260-character path limit.
- **Touch Up/Summarize timeouts** were tuned for 1.5B, not re-measured for 3B on CPU.
- **Rebuild and install-test the single-file installer** (§17).
- **Personal Dictionary for accent mishears** (Priya, vada pav, camel case): the mechanism exists (hotword trie), but Yash hasn't added the words yet.
- **Mic start-up clips ~650ms of every dictation** on Yash's Realtek (§5), measured with `live_mic_capture`: `Recorder::start` takes 200–285ms to open the stream, then the driver sends zeros for a steady ~425ms. The device exposes exactly one format (48kHz stereo, 480-frame buffer), so there's no config lever. Likely cause is the driver's audio-enhancement (APO) chain warming up; WASAPI raw mode would bypass it, but cpal can't request it. Remaining option is keeping the stream open while Ivy runs (mic-in-use indicator stays lit). Yash said 400ms is acceptable if it can't be reduced (2026-09-28).
- **AGC2/VAD** (`sonora-agc2`, Silero VAD) was researched and not attempted. If neural denoising is ever revisited, build a fresh instance per call and add the same-input-twice test.
- **Jev-style "decision" models, tested 2026-09-28** (offline, scratchpad venv, CPU, never touching Ivy's code). Jev (TypeSafe) is closed and cloud-only, so it's ruled out. The test: cue-gated self-corrections. Code finds a cue ("no, no", "scratch that", "sorry", "I mean", "actually") with a phrase before it. The model picks: keep the earlier phrase, keep the later phrase, or keep both. 13 cases (8 real corrections, 5 lookalikes), each asked in both option orders, 26 questions total. Random guessing scores ~9/26.
  - **Laya** (Convai, Apache 2.0, ModernBERT): English 7/26 at ~170ms and 2GB RAM; multilingual 9/26 at ~70ms. Useless zero-shot, as its own docs warn (~0.36 zero-shot). It would need fine-tuning on thousands of labelled examples (4–5h on 2×T4).
  - **Qwen 2.5 3B (already in Ivy), asked as a decision via next-token logits** (one forward pass, no generation): **20/26**, ~1.3s/question on CPU. On GPU, prompt eval alone runs ~100ms. It resolves the biryani/lemonade cross-sentence correction in both orders, which the rewrite prompt can't. Misses: two swapped-order corrections (inconsistent answers, so no action if both orders must agree), and one lookalike it wrongly deletes consistently: "I love the design. No, I mean it, it's really good." Shipping would need both-order agreement, deterministic deletion, and a cue list that excludes emphasis ("I mean it"), plus a bigger test set first.
  - **Not tested:** OpenJev Verdict 2.0 (151M ModernBERT, claims 77% and better than Laya) and Kev (Qwen2.5-0.5B + 38MB LoRA). Running their GitHub code was blocked by the permission classifier; Yash can allow it.
  - Test scripts: session scratchpad `laya_test.py`, `pair_test.py`, `qwen_logprob_test.py` (temporary folder; recreate if needed).
  - **Follow-up in progress (2026-09-28): training our own decision model.** Yash wants a Jev-style non-LLM model added *alongside* Qwen, runnable on any laptop. Work happens in a sandbox so this repo is untouched: full copy at `C:\Users\YASH\Downloads\IVY_Transcriber_lab` (own build dir `ivytgt_lab`, app id `app.ivy.dictation.lab`) and training in `C:\Users\YASH\Downloads\IVY_decision_lab`. First baseline (ModernBERT-base, 1 epoch on the 4060): 34/40 corrections fixed and 1/40 lookalikes wrongly edited on a hand-written held-out test. **Full details, data sources, scripts and next steps are in the lab copy's IVY.md, "Decision-model experiment — progress".**

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
