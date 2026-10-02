# Security Policy

IVY Transcriber is an offline, local-first voice dictation application for Windows designed from the ground up for strict privacy, zero telemetry, and zero network exposure. We treat security and user privacy as primary architectural invariants.

---

## Supported Versions

Only the latest release of IVY Transcriber receives active security patches.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1.0 | :x:                |

---

## Reporting a Vulnerability

If you discover a security vulnerability or privacy leak in IVY Transcriber, please disclose it responsibly so we can protect our users before details become public.

### Preferred Method: GitHub Private Vulnerability Reporting
Please report vulnerabilities directly through GitHub's built-in **Private Vulnerability Reporting**:
1. Navigate to the repository's **Security** tab.
2. Click on **Advisories** and select **Report a vulnerability**.
3. Provide detailed reproduction steps, proof of concept (PoC), and affected components.

### If GitHub Advisories Are Unavailable
Open a regular GitHub issue containing only a request for a private channel — no
reproduction details, no proof of concept — and a maintainer will follow up to
arrange disclosure.

### Response SLA & Disclosure Timeline
- **Initial Response:** Within 24–48 hours confirming receipt of the report.
- **Triage & Assessment:** Within 72 hours with an initial severity classification.
- **Remediation & Patch:** Typically within 7–14 days depending on complexity.
- **Public Disclosure:** Coordinated with the reporter after a patched release is published.

---

## Core Security & Privacy Guarantees

IVY Transcriber enforces the following architectural security invariants:

1. **100% Offline & Air-Gapped Operation:**
   - The application executes zero telemetry, zero analytics, and zero cloud API requests during transcription.
   - All AI models (Voxtral Mini 3B multimodal GGUF via Vulkan/CPU, and fallback Whisper ONNX via DirectML) run 100% locally.

2. **Daily Sensitive Data Auto-Purge:**
   - Voice recordings (`.wav`) and session transcription records (`history.json`) are automatically purged after 24 hours.
   - Manual "Clear History" immediately wipes all raw voice audio files and transcription records from disk.

3. **Decoupled Zero-Knowledge Lifetime Statistics:**
   - User progress (total words dictated, words per minute, day streaks, daily activity counts) is stored in a decoupled `stats.json`.
   - `stats.json` contains **zero audio files, zero words, and zero transcription text**, storing only mathematical aggregates so user productivity progress is preserved even when sensitive transcripts are wiped.

4. **In-Memory Audio Buffer Zeroization:**
   - Audio buffers in RAM are actively zeroized (`0.0f32` overwrite) when transcription completes or when recording is cancelled, preventing sensitive spoken speech from lingering in memory.

5. **Path Traversal & IPC Isolation:**
   - All audio export and file operations enforce strict path canonicalization within the designated application sandbox.
   - Session IDs are validated against strict alphanumeric allowlists to prevent directory traversal attacks.

6. **Focus-Safe Keystroke Injection:**
   - Win32 synthetic paste (`SendInput`) verifies that the original target application still holds foreground focus before injecting keystrokes.
   - If focus changed, the transcript is held in a safe buffer (`MANUAL_PASTE_TEXT`, accessible via `Alt+V`) and never pasted into unknown windows.
   - Clipboard contents are backed up and restored with verification to avoid overwriting newer user copies.

7. **Non-Activating Overlay Protection:**
   - The floating indicator window uses `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` and `SW_SHOWNA` to guarantee it never steals focus or interrupts secure credential entry.

---

## Safe Harbor Policy

We consider security research conducted under this policy to be:
- **Authorized** under applicable anti-hacking laws.
- **Exempt** from anti-circumvention claims under the DMCA.
- **Lawful**, provided researchers act in good faith, do not compromise the privacy of other users, and do not disrupt system availability.
