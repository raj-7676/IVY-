# Third-party notices

Ivy's own code is under the MIT License with the Commons Clause condition: free to use, change and share,
not for sale (see [LICENSE](LICENSE)). It is built on the open-source work below. Everything listed allows
free use, changes and redistribution, as long as the notices are kept.

## The speech model (shipped in the release)

`ivy-lite-Q8_0.gguf` and `mmproj-ivy-lite-f16.gguf` are a fine-tuned version of
**Qwen3-ASR-1.7B** by the Qwen team, Alibaba Cloud, licensed under the **Apache License 2.0**
(full text: [LICENSES/Apache-2.0.txt](LICENSES/Apache-2.0.txt)).

Changes made by the Ivy project: the model was fine-tuned with LoRA on real human speech (with
human-made transcripts) and scripted self-correction paragraphs, so that it writes what the speaker
means. The fine-tune was merged into the weights and converted to GGUF (Q8_0 language model,
F16 audio projector) for llama.cpp. The fine-tuned model is distributed under the Apache 2.0 license with the
Commons Clause condition (free to use, not for sale; [LICENSES/IVY-MODEL-LICENSE.txt](LICENSES/IVY-MODEL-LICENSE.txt)).
The original Qwen3-ASR-1.7B stays under the plain Apache 2.0 license from its authors.

## Bundled in the installer

Setup carries Microsoft's **Visual C++ Redistributable for Visual Studio 2015-2022 (x64)**, unmodified and
signed by Microsoft, and runs it only on PCs that lack a recent enough Visual C++ runtime. It is
redistributed under the Visual Studio redistribution terms; its own license is shown and accepted when it
installs.

## Intro film (shown on the first-run download screen)

Music ("House 02") and sound effects from [Mixkit](https://mixkit.co), used under the Mixkit License.
Voice-over generated with [ElevenLabs](https://elevenlabs.io).

## Libraries and data

| Component | Used for | License |
|---|---|---|
| [llama.cpp](https://github.com/ggml-org/llama.cpp) (via the `llama-cpp-2` / `llama-cpp-sys-2` crates, vendored with a small patch) | Running the model on GPU (Vulkan) or CPU | MIT |
| [Tauri](https://tauri.app) | Desktop app framework and installer | MIT or Apache 2.0 |
| [React](https://react.dev), [Vite](https://vite.dev), [Tailwind CSS](https://tailwindcss.com), [Motion](https://motion.dev), [Lucide](https://lucide.dev) | User interface | MIT / ISC |
| [cpal](https://github.com/RustAudio/cpal) | Microphone capture | Apache 2.0 |
| SymSpell English frequency dictionary by Wolf Garbe | Touch Up spell-check word list | MIT ([src-tauri/data/en-80k.LICENSE.txt](src-tauri/data/en-80k.LICENSE.txt)) |
| Plus Jakarta Sans, Syne, JetBrains Mono (via Fontsource) | Fonts | SIL Open Font License 1.1 |

The complete list of Rust and npm dependencies, with versions, is in `src-tauri/Cargo.lock` and
`package-lock.json`. Each package's own license file is included in its source.
