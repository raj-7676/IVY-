## Description
<!-- Provide a brief description of what this PR introduces, modifies, or fixes. -->

## Motivation & Context
<!-- Why is this change needed? Closes #[issue_number] -->

## Type of Change
- [ ] 🐛 Bug fix (non-breaking change which fixes an issue)
- [ ] ✨ New feature (non-breaking change which adds functionality)
- [ ] 🔒 Security hardening / Vulnerability remediation
- [ ] ⚡ Performance optimization
- [ ] 📝 Documentation update

## 🛡️ Security & Privacy Checklist (MANDATORY)
*Please verify each of the following architectural invariants before requesting review:*
- [ ] **100% Offline Guarantee:** This change introduces **ZERO** outbound network calls, analytics, telemetry, or external API pings.
- [ ] **Zero Cloud / Local Execution:** All models and processing remain strictly in-process on the local machine.
- [ ] **No Secret Leaks:** No API keys, credentials, local paths, or personal tokens are included.
- [ ] **Audio Memory Hygiene:** Any in-memory audio buffers (`Vec<f32>`) are zeroed out (`zeroize_samples`) upon exit.
- [ ] **IPC Path Safety:** Any new Tauri commands taking file paths or session IDs enforce strict sanitization/validation (`is_valid_session_id`).
- [ ] **Decoupled Privacy:** User progress metrics (`stats.json`) remain anonymous aggregates and never store verbatim speech or audio.

## Testing & Verification
<!-- Describe the tests you ran to verify your changes. -->
- [ ] `cargo test --lib` passes without failures
- [ ] `cargo check` passes with 0 warnings
- [ ] `npm run build` succeeds cleanly
- [ ] Manual verification in Tauri dev environment
