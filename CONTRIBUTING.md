# Contributing to Ivy

Thanks for helping. Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

Open an [issue](https://github.com/raj-7676/IVY-/issues/new/choose) with:
- what you said (or roughly), what Ivy typed, and what you expected
- GPU or CPU mode, your Windows version and graphics card
- the app you were dictating into

Please never attach recordings of other people, and remove anything private from logs.
Security problems go through [SECURITY.md](SECURITY.md), not public issues.

## Changing the code

1. Follow "Building from source" in the [README](README.md).
2. Keep changes small and focused; one fix or feature per pull request.
3. Run the checks before opening the pull request:
   ```bash
   npx tsc --noEmit
   cd src-tauri && cargo test --lib -- --test-threads=1
   ```
4. Text-formatting rules live in `src-tauri/src/rulebooks/` and are documented in [RULEBOOKS.md](RULEBOOKS.md). Every rule change needs a test.

## Ground rules

- Ivy is offline. Pull requests that add network calls, telemetry or accounts won't be merged.
- Nothing fake in the UI: every number and message must reflect what the app really does.
- By contributing, you agree your work is released under Ivy's license (MIT with the Commons Clause condition, [LICENSE](LICENSE)).
