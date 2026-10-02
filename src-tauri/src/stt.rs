use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use mel_spec::prelude::{BatchLogMelConfig, BatchLogMelSpectrogram};
use ndarray::{ArrayD, IxDyn};
use ort::ep::*;
use ort::inputs;
use ort::memory::{AllocationDevice, AllocatorType, MemoryInfo, MemoryType};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use tokenizers::Tokenizer;

// Whisper large-v3-turbo (onnx-community export), real values read directly
// from the model's own config.json/generation_config.json/preprocessor_config.json
// on Hugging Face — not guessed. Decoder was pruned to 4 layers (that's the
// "turbo" speedup vs the 32-layer large-v3 decoder); encoder stays 32 layers
// but only needs one forward pass per chunk, not one per token.
const LAYERS: usize = 4;
const HEADS: usize = 20;
const HEAD_DIM: usize = 64; // d_model 1280 / 20 heads
const SOT: i64 = 50258; // <|startoftranscript|> (decoder_start_token_id)
const LANG_EN: i64 = 50259; // <|en|> — forced, matches Ivy's English-first UI copy (see IVY.md §4)
const TASK_TRANSCRIBE: i64 = 50360;
const NO_TIMESTAMPS: i64 = 50364;
const EOS: i64 = 50257; // also bos_token_id/pad_token_id — Whisper reuses GPT2's <|endoftext|>
const INITIAL_TOKENS: [i64; 4] = [SOT, LANG_EN, TASK_TRANSCRIBE, NO_TIMESTAMPS];

// max_target_positions is 448; stay under it with headroom for the 4-token prefix.
const MAX_NEW_TOKENS: usize = 440;
// Forbid EOS for the first few generated tokens so a spuriously-high EOS
// logit on step 0 can't produce an empty transcript for real speech.
const MIN_TOKENS_BEFORE_EOS: usize = 3;
const MIN_SAMPLES: usize = 1600; // ~0.1s — below this there's nothing to transcribe

// Whisper's encoder always takes a fixed 30s window (this is architectural,
// not a tuning choice — max_source_positions=1500 = 3000 mel frames / 2).
const CHUNK_SECONDS: usize = 30;
const SAMPLE_RATE: usize = 16_000;
const CHUNK_SAMPLES: usize = CHUNK_SECONDS * SAMPLE_RATE; // 480_000
const N_MELS: usize = 128;
const N_FFT: usize = 400;
const HOP_LENGTH: usize = 160;
const NB_MAX_FRAMES: usize = 3000;

pub struct WhisperEngine {
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    tokenizer: Tokenizer,
    mel: BatchLogMelSpectrogram,
    pub is_gpu: bool,
}

static ENGINE: RwLock<Option<Arc<WhisperEngine>>> = RwLock::new(None);

/// Unloads the Whisper STT engine from memory / VRAM, freeing all GPU resources.
pub fn unload_engine() {
    if let Ok(mut lock) = ENGINE.write() {
        if lock.is_some() {
            log::info!("Ivy: Unloading Whisper STT engine from memory/VRAM");
            *lock = None;
        }
    }
}

/// Dynamically returns the Whisper STT engine according to the preferred hardware mode.
/// If already loaded with the desired mode, returns immediately; otherwise loads with fallback.
pub fn engine(models_dir: &Path, prefer_gpu: bool) -> Result<Arc<WhisperEngine>, String> {
    if let Ok(lock) = ENGINE.read() {
        if let Some(e) = lock.as_ref() {
            if e.is_gpu == prefer_gpu {
                return Ok(e.clone());
            }
        }
    }

    let mut lock = ENGINE.write().map_err(|e| e.to_string())?;
    if let Some(e) = lock.as_ref() {
        if e.is_gpu == prefer_gpu {
            return Ok(e.clone());
        }
    }

    let loaded = Arc::new(WhisperEngine::load(models_dir, prefer_gpu)?);
    *lock = Some(loaded.clone());
    Ok(loaded)
}

fn zeros(seq_len: usize) -> ArrayD<f32> {
    ArrayD::zeros(IxDyn(&[1, HEADS, seq_len, HEAD_DIM]))
}

/// If the tail of `tokens` is some short cycle (period 1..=max_period)
/// repeated `min_repeats` times back to back, returns how many leading
/// tokens to keep — dropping `min_repeats - 1` copies of the cycle from the
/// end. Meant to be called after *every* generated token (as the real
/// decode loop does), so it fires the instant a repeat run reaches
/// `min_repeats`. Whisper's own well-documented hallucination failure mode
/// (looping "thank you for watching" on silence/noise) is exactly this
/// shape, same as the Moonshine repetition bug this guard was first built
/// for — kept unchanged across the model swap.
fn repeating_cycle(tokens: &[i64], min_period: usize, max_period: usize, min_repeats: usize) -> Option<usize> {
    let len = tokens.len();
    for period in min_period..=max_period {
        let needed = period * min_repeats;
        if len < needed {
            continue;
        }
        let tail = &tokens[len - needed..];
        let first_cycle = &tail[..period];
        if tail.chunks(period).all(|c| c == first_cycle) {
            return Some(len - needed + period);
        }
    }
    None
}

/// Prefix-Trie node for runtime contextual ASR logit biasing.
#[derive(Default, Debug)]
struct HotwordTrieNode {
    children: HashMap<i64, usize>,
    is_leaf: bool,
}

/// Fast in-memory prefix trie representing target domain vocabulary/hotwords.
/// Used during autoregressive greedy decode to bias continuation logits (+3.0)
/// toward known dictionary entries without the overhead of beam search.
/// Generic over whichever tokenizer the loaded engine uses — unchanged by
/// the Moonshine->Whisper swap, IVY.md §4.
#[derive(Default, Debug)]
pub struct HotwordTrie {
    nodes: Vec<HotwordTrieNode>,
}

impl HotwordTrie {
    pub fn new() -> Self {
        Self {
            nodes: vec![HotwordTrieNode::default()],
        }
    }

    pub fn insert(&mut self, token_ids: &[i64]) {
        if token_ids.is_empty() {
            return;
        }
        let mut curr = 0;
        for &token in token_ids {
            let next_idx = match self.nodes[curr].children.get(&token) {
                Some(&child) => child,
                None => {
                    let new_idx = self.nodes.len();
                    self.nodes.push(HotwordTrieNode::default());
                    self.nodes[curr].children.insert(token, new_idx);
                    new_idx
                }
            };
            curr = next_idx;
        }
        self.nodes[curr].is_leaf = true;
    }

    pub fn from_vocabulary(tokenizer: &Tokenizer, vocabulary: &[String]) -> Self {
        let mut trie = Self::new();
        for word in vocabulary {
            let trimmed = word.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Standard in SentencePiece/BPE: words with leading space
            if let Ok(enc) = tokenizer.encode(format!(" {trimmed}"), false) {
                let ids: Vec<i64> = enc.get_ids().iter().map(|&id| id as i64).collect();
                trie.insert(&ids);
            }
            // Also without leading space (for utterance starts or post-punctuation)
            if let Ok(enc) = tokenizer.encode(trimmed, false) {
                let ids: Vec<i64> = enc.get_ids().iter().map(|&id| id as i64).collect();
                trie.insert(&ids);
            }
        }
        trie
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1 || self.nodes[0].children.is_empty()
    }
}

/// Real, documented OpenAI Whisper preprocessing (`WhisperFeatureExtractor`):
/// 128-bin log-mel spectrogram over a center-padded 30s (480,000-sample)
/// window, log10 (not natural log), then normalized once against the
/// *global* max across the whole window: `max(log10_mel, global_max - 8) / 4
/// + 1`. Built on `mel_spec`'s real FFT + mel-filterbank machinery
/// (`BatchLogMelSpectrogram`, whisper.cpp-compatible framing/centering) —
/// only the log-base conversion and the global (not per-frame) normalization
/// are done by hand here, because `mel_spec`'s own convenience wrappers use
/// natural log and per-frame normalization, which do not match Whisper's
/// reference values. See stt.rs history / IVY.md for how this was checked
/// against the crate's real source rather than assumed.
///
/// Not yet confirmed by ear: this has not been validated against a real
/// reference transcription (e.g. faster-whisper on the same WAV) to prove
/// the framing/normalization exactly matches OpenAI's numbers bit-for-bit.
/// The first real test build is the actual verification step.
fn whisper_mel_config() -> BatchLogMelConfig {
    BatchLogMelConfig {
        sample_rate: SAMPLE_RATE,
        n_fft: N_FFT,
        win_length: N_FFT,
        hop_length: HOP_LENGTH,
        n_mels: N_MELS,
        f_min: 0.0,
        f_max: None, // defaults to sample_rate / 2, matching Whisper's reference
        htk: false,  // slaney-style mel scale, matches Whisper's reference filterbank
        norm: true,
        preemphasis: 0.0,
        center: true, // real reflect-padded framing, required to land on exactly 3000 frames
        log_zero_guard: 1e-10, // matches Whisper's own `clamp(min=1e-10)` floor
        pad_to: 0,
        normalize_per_feature: false, // Whisper does NOT do per-feature normalization
    }
}

/// `mel` is the engine's own cached `BatchLogMelSpectrogram` (built once in
/// `WhisperEngine::load`, reused across every segment of a dictation) —
/// rebuilding the FFT planner + mel filterbank from scratch on every ~20-30s
/// chunk would be real, avoidable per-segment overhead on a long recording.
fn log_mel_spectrogram(mel: &BatchLogMelSpectrogram, samples: &[f32]) -> Result<ArrayD<f32>, String> {
    let mut padded = samples.to_vec();
    padded.resize(CHUNK_SAMPLES, 0.0);

    // `mel_spec` pulls in its own `ndarray` major version, distinct from the
    // one this crate uses (confirmed by a real compile error, not assumed)
    // — cross the boundary via a plain Vec instead of mixing `Array2` types
    // from two different `ndarray` versions.
    let ln_features = mel.compute(&padded).map_err(|e| e.to_string())?; // shape (n_mels, frames), natural log
    let rows = ln_features.nrows();
    let cols = ln_features.ncols();

    const LN10_INV: f32 = std::f32::consts::LOG10_E; // 1/ln(10); converts ln(x) -> log10(x)
    let log10_flat: Vec<f32> = ln_features.iter().map(|&x| x * LN10_INV).collect();
    debug_assert_eq!(rows, N_MELS);

    let global_max = log10_flat.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let floor = global_max - 8.0;
    let normalized_flat: Vec<f32> = log10_flat.iter().map(|&x| (x.max(floor) + 4.0) / 4.0).collect();

    // `center: true` framing yields one frame more than NB_MAX_FRAMES for an
    // exact 30s window (3001 vs 3000) — same off-by-one PyTorch's own
    // reference trims away. Truncate/pad columns to the exact size the
    // encoder's fixed positional embeddings expect.
    let mut out = vec![0.0f32; N_MELS * NB_MAX_FRAMES];
    let copy_cols = cols.min(NB_MAX_FRAMES);
    for mel_idx in 0..N_MELS {
        let src_start = mel_idx * cols;
        let dst_start = mel_idx * NB_MAX_FRAMES;
        out[dst_start..dst_start + copy_cols]
            .copy_from_slice(&normalized_flat[src_start..src_start + copy_cols]);
    }

    let shape = IxDyn(&[1, N_MELS, NB_MAX_FRAMES]);
    ArrayD::from_shape_vec(shape, out).map_err(|e| e.to_string())
}

impl WhisperEngine {
    fn build_sessions(dir: &Path, use_gpu: bool) -> Result<(Session, Session), String> {
        let mut enc_b = Session::builder().map_err(|e| e.to_string())?;
        let mut dec_b = Session::builder().map_err(|e| e.to_string())?;

        if use_gpu {
            enc_b = enc_b
                .with_execution_providers([DirectML::default().build()])
                .map_err(|e| e.to_string())?;
            dec_b = dec_b
                .with_execution_providers([DirectML::default().build()])
                .map_err(|e| e.to_string())?;
        }

        let encoder = enc_b
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| e.to_string())?
            .commit_from_file(dir.join("encoder_model_int8.onnx"))
            .map_err(|e| format!("whisper encoder load failed: {e}"))?;

        let decoder = dec_b
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| e.to_string())?
            .commit_from_file(dir.join("decoder_model_merged_int8.onnx"))
            .map_err(|e| format!("whisper decoder load failed: {e}"))?;

        Ok((encoder, decoder))
    }

    fn load(models_dir: &Path, prefer_gpu: bool) -> Result<Self, String> {
        let dir = models_dir.join("whisper");
        let mut is_gpu = false;

        let (encoder, decoder) = if prefer_gpu {
            match Self::build_sessions(&dir, true) {
                Ok((enc, dec)) => {
                    log::info!("Ivy: Whisper STT loaded with DirectML GPU acceleration");
                    is_gpu = true;
                    (enc, dec)
                }
                Err(e) => {
                    log::warn!("Ivy: DirectML GPU init failed ({e}), falling back to CPU");
                    Self::build_sessions(&dir, false)?
                }
            }
        } else {
            Self::build_sessions(&dir, false)?
        };

        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| format!("whisper tokenizer load failed: {e}"))?;

        let mel = BatchLogMelSpectrogram::new(whisper_mel_config()).map_err(|e| e.to_string())?;

        Ok(Self {
            encoder: Mutex::new(encoder),
            decoder: Mutex::new(decoder),
            tokenizer,
            mel,
            is_gpu,
        })
    }

    fn find_low_energy_split(samples: &[f32], target_start: usize, target_end: usize) -> usize {
        let window_size = 3200; // 200ms at 16kHz
        let step = 800; // 50ms step
        let mut min_energy = f32::MAX;
        let mut best_split = (target_start + target_end) / 2;

        let mut i = target_start;
        while i + window_size <= target_end && i + window_size <= samples.len() {
            let energy: f32 = samples[i..i + window_size].iter().map(|&s| s * s).sum();
            if energy < min_energy {
                min_energy = energy;
                best_split = i + window_size / 2;
            }
            i += step;
        }
        best_split
    }

    /// Transcribes 16kHz mono samples, with decode-time logit biasing toward
    /// custom vocabulary / hotwords. The encoder is fixed to a 30s window
    /// (the ONNX export rejects anything shorter), so longer audio is split
    /// at low-energy pauses into 20-27s chunks.
    pub fn transcribe_with_vocabulary(&self, samples: &[f32], vocabulary: &[String]) -> Result<String, String> {
        if samples.len() < MIN_SAMPLES {
            return Ok(String::new());
        }

        let trie = HotwordTrie::from_vocabulary(&self.tokenizer, vocabulary);
        let trie_opt = if trie.is_empty() { None } else { Some(&trie) };

        if samples.len() <= CHUNK_SAMPLES {
            return self.transcribe_segment(samples, trie_opt);
        }

        let mut segments: Vec<&[f32]> = Vec::new();
        let mut start = 0;
        let total = samples.len();

        while start < total {
            let remaining = total - start;
            if remaining <= CHUNK_SAMPLES {
                segments.push(&samples[start..total]);
                break;
            }

            let min_chunk = 20 * SAMPLE_RATE;
            let max_chunk = 27 * SAMPLE_RATE;
            let target_start = start + min_chunk;
            let target_end = (start + max_chunk).min(total);

            let split = Self::find_low_energy_split(samples, target_start, target_end);
            segments.push(&samples[start..split]);
            start = split;
        }

        let mut results: Vec<String> = Vec::new();
        for segment in segments {
            if segment.len() >= MIN_SAMPLES {
                match self.transcribe_segment(segment, trie_opt) {
                    Ok(text) => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            results.push(trimmed.to_string());
                        }
                    }
                    Err(e) => {
                        log::warn!("Ivy: failed transcribing audio segment: {e}");
                    }
                }
            }
        }

        Ok(results.join(" "))
    }

    #[allow(unused_variables, unused_assignments)]
    fn transcribe_segment(&self, samples: &[f32], trie: Option<&HotwordTrie>) -> Result<String, String> {
        if samples.len() < MIN_SAMPLES {
            return Ok(String::new());
        }

        let mut encoder = self.encoder.lock().map_err(|e| e.to_string())?;
        let mut decoder = self.decoder.lock().map_err(|e| e.to_string())?;

        let input_features = log_mel_spectrogram(&self.mel, samples)?;
        let enc_outputs = encoder
            .run(inputs!["input_features" => Tensor::from_array(input_features).map_err(|e| e.to_string())?])
            .map_err(|e| format!("whisper encoder run failed: {e}"))?;
        let encoder_hidden_states = enc_outputs["last_hidden_state"]
            .try_extract_array::<f32>()
            .map_err(|e| e.to_string())?
            .to_owned()
            .into_dyn();
        drop(enc_outputs);

        let mut past_decoder: Vec<(ArrayD<f32>, ArrayD<f32>)> =
            (0..LAYERS).map(|_| (zeros(0), zeros(0))).collect();
        let past_encoder: Vec<(ArrayD<f32>, ArrayD<f32>)> =
            (0..LAYERS).map(|_| (zeros(0), zeros(0))).collect();

        // Same IoBinding pattern Moonshine used (IVY.md §4, real DirectML
        // per-call dispatch overhead fix) — encoder_hidden_states and the
        // encoder/cross-attention KV are identical every step after step 0,
        // so they're bound once instead of re-uploaded every decode step.
        let mut binding = decoder.create_binding().map_err(|e| e.to_string())?;
        let enc_tensor = Tensor::from_array(encoder_hidden_states).map_err(|e| e.to_string())?;
        binding.bind_input("encoder_hidden_states", &enc_tensor).map_err(|e| e.to_string())?;

        let cpu_mem = MemoryInfo::new(AllocationDevice::CPU, 0, AllocatorType::Device, MemoryType::Default)
            .map_err(|e| e.to_string())?;

        // Whisper decodes with a forced 4-token prefix (start-of-transcript,
        // language, task, no-timestamps) rather than a single BOS — real
        // values read from generation_config.json, not guessed (see module
        // doc comment above INITIAL_TOKENS).
        let mut tokens: Vec<i64> = INITIAL_TOKENS.to_vec();
        let prefix_len = tokens.len();
        let mut use_cache_branch = false;
        let mut active_trie_nodes: Vec<usize> = if trie.is_some() { vec![0] } else { Vec::new() };
        let mut past_encoder_tensors: Option<Vec<(Tensor<f32>, Tensor<f32>)>> = None;

        for step in 0..MAX_NEW_TOKENS {
            let input_ids = if use_cache_branch {
                ArrayD::from_shape_vec(IxDyn(&[1, 1]), vec![*tokens.last().unwrap()])
            } else {
                ArrayD::from_shape_vec(IxDyn(&[1, tokens.len()]), tokens.clone())
            }
            .map_err(|e| e.to_string())?;

            let input_ids_tensor = Tensor::from_array(input_ids).map_err(|e| e.to_string())?;
            binding.bind_input("input_ids", &input_ids_tensor).map_err(|e| e.to_string())?;

            // Real error found on the first actual test run against the
            // downloaded weights, not assumed: this export (newer Optimum
            // than Moonshine's) requires an explicit `cache_position` input
            // — the absolute sequence position of each token in `input_ids`
            // — separate from the KV cache tensors themselves. Full prefix
            // on step 0 (positions 0..4), single running position per step
            // once `use_cache_branch` kicks in.
            let cache_position: Vec<i64> = if use_cache_branch {
                vec![(tokens.len() - 1) as i64]
            } else {
                (0..tokens.len() as i64).collect()
            };
            let cache_position_tensor = Tensor::from_array(
                ArrayD::from_shape_vec(IxDyn(&[cache_position.len()]), cache_position)
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            binding
                .bind_input("cache_position", &cache_position_tensor)
                .map_err(|e| e.to_string())?;

            let use_cache_tensor =
                Tensor::from_array(ArrayD::from_shape_vec(IxDyn(&[1]), vec![use_cache_branch]).unwrap())
                    .map_err(|e| e.to_string())?;
            binding.bind_input("use_cache_branch", &use_cache_tensor).map_err(|e| e.to_string())?;

            let mut decoder_input_tensors: Vec<(Tensor<f32>, Tensor<f32>)> = Vec::with_capacity(LAYERS);
            let mut encoder_input_tensors: Vec<(Tensor<f32>, Tensor<f32>)> = Vec::with_capacity(LAYERS);
            for layer in 0..LAYERS {
                let dk = Tensor::from_array(past_decoder[layer].0.clone()).map_err(|e| e.to_string())?;
                let dv = Tensor::from_array(past_decoder[layer].1.clone()).map_err(|e| e.to_string())?;
                binding
                    .bind_input(format!("past_key_values.{layer}.decoder.key"), &dk)
                    .map_err(|e| e.to_string())?;
                binding
                    .bind_input(format!("past_key_values.{layer}.decoder.value"), &dv)
                    .map_err(|e| e.to_string())?;
                decoder_input_tensors.push((dk, dv));

                if !use_cache_branch {
                    let ek = Tensor::from_array(past_encoder[layer].0.clone()).map_err(|e| e.to_string())?;
                    let ev = Tensor::from_array(past_encoder[layer].1.clone()).map_err(|e| e.to_string())?;
                    binding
                        .bind_input(format!("past_key_values.{layer}.encoder.key"), &ek)
                        .map_err(|e| e.to_string())?;
                    binding
                        .bind_input(format!("past_key_values.{layer}.encoder.value"), &ev)
                        .map_err(|e| e.to_string())?;
                    encoder_input_tensors.push((ek, ev));
                }
            }

            binding.clear_outputs();
            binding.bind_output_to_device("logits", &cpu_mem).map_err(|e| e.to_string())?;
            for layer in 0..LAYERS {
                for name in [
                    format!("present.{layer}.decoder.key"),
                    format!("present.{layer}.decoder.value"),
                    format!("present.{layer}.encoder.key"),
                    format!("present.{layer}.encoder.value"),
                ] {
                    binding.bind_output_to_device(name, &cpu_mem).map_err(|e| e.to_string())?;
                }
            }

            let outputs = decoder.run_binding(&binding).map_err(|e| format!("whisper decoder run failed: {e}"))?;

            let logits = outputs["logits"].try_extract_array::<f32>().map_err(|e| e.to_string())?;
            let shape = logits.shape();
            let (seq, vocab) = (shape[1], shape[2]);
            let mut last_step = logits.as_slice().ok_or("non-contiguous logits")?
                [(seq - 1) * vocab..seq * vocab]
                .to_vec();

            let forbid_eos = step < MIN_TOKENS_BEFORE_EOS;
            // Real, documented Whisper generation-config guard
            // (`begin_suppress_tokens: [220, 50257]`): suppress the bare
            // space token at the very first real generation step so greedy
            // decode can't open a transcript with a leading space/blank.
            // Ponytail: the full `suppress_tokens` list (~90 rare-script/
            // symbol ids) is not applied here — narrower guard for now,
            // upgrade if real testing shows stray symbol tokens.
            if step == 0 && 220 < last_step.len() {
                last_step[220] = f32::NEG_INFINITY;
            }

            if let Some(t) = trie {
                for &node_idx in &active_trie_nodes {
                    for (&next_token_id, _) in &t.nodes[node_idx].children {
                        if next_token_id >= 0 && (next_token_id as usize) < last_step.len() {
                            last_step[next_token_id as usize] += 3.0;
                        }
                    }
                }
            }

            let next_token = last_step
                .iter()
                .enumerate()
                .filter(|(i, _)| !(forbid_eos && *i as i64 == EOS))
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i as i64)
                .unwrap_or(EOS);

            if let Some(t) = trie {
                let mut next_active = Vec::new();
                for &node_idx in &active_trie_nodes {
                    if let Some(&child_idx) = t.nodes[node_idx].children.get(&next_token) {
                        if !t.nodes[child_idx].is_leaf {
                            next_active.push(child_idx);
                        }
                    }
                }
                next_active.push(0);
                active_trie_nodes = next_active;
            }

            if next_token == EOS {
                break;
            }

            let mut fresh_encoder_kv: Option<Vec<(ArrayD<f32>, ArrayD<f32>)>> = None;
            for layer in 0..LAYERS {
                let dk = outputs[format!("present.{layer}.decoder.key")]
                    .try_extract_array::<f32>()
                    .map_err(|e| e.to_string())?
                    .to_owned()
                    .into_dyn();
                let dv = outputs[format!("present.{layer}.decoder.value")]
                    .try_extract_array::<f32>()
                    .map_err(|e| e.to_string())?
                    .to_owned()
                    .into_dyn();
                past_decoder[layer] = (dk, dv);

                if !use_cache_branch {
                    let ek = outputs[format!("present.{layer}.encoder.key")]
                        .try_extract_array::<f32>()
                        .map_err(|e| e.to_string())?
                        .to_owned()
                        .into_dyn();
                    let ev = outputs[format!("present.{layer}.encoder.value")]
                        .try_extract_array::<f32>()
                        .map_err(|e| e.to_string())?
                        .to_owned()
                        .into_dyn();
                    fresh_encoder_kv.get_or_insert_with(|| Vec::with_capacity(LAYERS)).push((ek, ev));
                }
            }

            drop(outputs);
            drop(decoder_input_tensors);
            drop(encoder_input_tensors);

            if let Some(fresh) = fresh_encoder_kv {
                let mut tensors: Vec<(Tensor<f32>, Tensor<f32>)> = Vec::with_capacity(LAYERS);
                for (layer, (ek, ev)) in fresh.into_iter().enumerate() {
                    let ek_t = Tensor::from_array(ek).map_err(|e| e.to_string())?;
                    let ev_t = Tensor::from_array(ev).map_err(|e| e.to_string())?;
                    binding
                        .bind_input(format!("past_key_values.{layer}.encoder.key"), &ek_t)
                        .map_err(|e| e.to_string())?;
                    binding
                        .bind_input(format!("past_key_values.{layer}.encoder.value"), &ev_t)
                        .map_err(|e| e.to_string())?;
                    tensors.push((ek_t, ev_t));
                }
                past_encoder_tensors = Some(tensors);
            }

            tokens.push(next_token);
            use_cache_branch = true;

            // 5 repeats, not 3: Whisper keeps a real "no, no, no" and a lower
            // bar stopped decoding right there, losing the rest of the sentence.
            if let Some(cycle_start) = repeating_cycle(&tokens[prefix_len..], 1, 8, 5) {
                tokens.truncate(prefix_len + cycle_start);
                break;
            }
        }

        let text = self
            .tokenizer
            .decode(&tokens[prefix_len..].iter().map(|&t| t as u32).collect::<Vec<_>>(), true)
            .map_err(|e| e.to_string())?;
        Ok(sanitize_acoustic_artifacts(&text))
    }
}

fn sanitize_acoustic_artifacts(text: &str) -> String {
    let mut s = text.replace(['♪', '♫', '🎵', '🎶'], "");
    s = s.replace("[music]", "").replace("(music)", "")
        .replace("[laughter]", "").replace("(laughter)", "")
        .replace("[applause]", "").replace("(applause)", "")
        // Whisper's own well-documented silence/noise hallucination default
        // output — real observed failure mode on non-speech audio, not
        // hypothetical (widely reported against the reference model).
        .replace("Thank you for watching.", "")
        .replace("Thanks for watching.", "");
    s.trim().to_string()
}

#[cfg(test)]
mod repeating_cycle_tests {
    use super::repeating_cycle;

    #[test]
    fn detects_repeating_phrase() {
        let tokens = [10, 11, 12, 13, 10, 11, 12, 13, 10, 11, 12, 13, 10, 11, 12, 13];
        assert_eq!(repeating_cycle(&tokens, 1, 8, 3), Some(8));
    }

    #[test]
    fn keeps_prefix_before_the_loop() {
        let tokens = [1, 2, 3, 5, 5, 5, 5, 5];
        assert_eq!(repeating_cycle(&tokens, 1, 8, 3), Some(6));
    }

    #[test]
    fn no_false_positive_on_normal_speech() {
        let tokens = [10, 20, 30, 40, 50, 60, 70, 80];
        assert_eq!(repeating_cycle(&tokens, 1, 8, 3), None);
    }
}

// The smallest real check for non-trivial logic: run the actual encoder +
// autoregressive decoder against a real synthesized-speech WAV (Windows
// SAPI, `tests/fixtures/sample.wav`, regenerate with
// `tests/fixtures/generate_sample.ps1`) and confirm the transcript contains
// the words that were spoken. Needs `npm run setup-models` first — skips
// itself (rather than failing) if the models aren't there, since they're
// multi-GB and gitignored.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotword_trie_construction_and_traversal() {
        let mut trie = HotwordTrie::new();
        assert!(trie.is_empty());

        trie.insert(&[101, 102, 103]);
        assert!(!trie.is_empty());

        let root = &trie.nodes[0];
        assert_eq!(root.children.get(&101), Some(&1));
        assert!(!root.is_leaf);

        let node1 = &trie.nodes[1];
        assert_eq!(node1.children.get(&102), Some(&2));
        assert!(!node1.is_leaf);

        let node2 = &trie.nodes[2];
        assert_eq!(node2.children.get(&103), Some(&3));
        assert!(!node2.is_leaf);

        let node3 = &trie.nodes[3];
        assert!(node3.is_leaf);
        assert!(node3.children.is_empty());
    }

    #[test]
    fn transcribes_real_speech() {
        let models_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        if !models_dir.join("whisper").join("encoder_model_int8.onnx").exists() {
            eprintln!("skipping: models not present, run `npm run setup-models` first");
            return;
        }

        let wav_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("sample.wav");
        let mut reader = hound::WavReader::open(&wav_path).expect("open sample.wav");
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16_000, "fixture must already be 16kHz mono");
        assert_eq!(spec.channels, 1, "fixture must already be 16kHz mono");
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.expect("read sample") as f32 / i16::MAX as f32)
            .collect();

        let engine = engine(&models_dir, false).expect("engine loads");
        let text = engine.transcribe_with_vocabulary(&samples, &[]).expect("transcribe succeeds");
        println!("transcribed: {text:?}");

        let lower = text.to_lowercase();
        for word in ["quick", "brown", "fox", "lazy", "dog"] {
            assert!(lower.contains(word), "expected {word:?} in transcript, got {text:?}");
        }
    }

    /// Same fixture, GPU engine — real coverage kept from the Moonshine era
    /// (IVY.md §4): every other test here only ever runs the CPU path,
    /// so a GPU-specific correctness bug (not just a performance issue)
    /// could ship unnoticed. Skips itself (not a failure) if no
    /// DirectML-capable GPU is present, or if the int8 encoder/decoder
    /// don't support DirectML op-for-op (flagged as a real open risk in
    /// IVY.md — int8 ONNX graphs sometimes have DirectML-unsupported ops;
    /// the existing GPU->CPU fallback in `load()` handles that gracefully
    /// either way).
    #[test]
    fn transcribes_real_speech_on_gpu() {
        let models_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        if !models_dir.join("whisper").join("encoder_model_int8.onnx").exists() {
            eprintln!("skipping: models not present, run `npm run setup-models` first");
            return;
        }

        let wav_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("sample.wav");
        let mut reader = hound::WavReader::open(&wav_path).expect("open sample.wav");
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.expect("read sample") as f32 / i16::MAX as f32)
            .collect();

        let engine = match engine(&models_dir, true) {
            Ok(e) if e.is_gpu => e,
            Ok(_) => {
                eprintln!("skipping: DirectML GPU init failed, fell back to CPU (already covered above)");
                return;
            }
            Err(e) => {
                eprintln!("skipping: engine failed to load ({e})");
                return;
            }
        };
        let text = engine.transcribe_with_vocabulary(&samples, &[]).expect("transcribe succeeds on GPU");
        println!("transcribed (GPU): {text:?}");

        let lower = text.to_lowercase();
        for word in ["quick", "brown", "fox", "lazy", "dog"] {
            assert!(lower.contains(word), "expected {word:?} in GPU transcript, got {text:?}");
        }
    }

    /// Regression test for a real bug from the Moonshine era (IVY.md): this
    /// exact recording (Yash's real voice) used to come back empty because
    /// greedy decoding picked EOS as its single most likely first token even
    /// though real words ranked right behind it — fixed by
    /// `MIN_TOKENS_BEFORE_EOS`. Kept across the model swap since the same
    /// class of failure is possible with any greedy-decoded model.
    #[test]
    fn transcribes_real_recording_that_used_to_fail() {
        let models_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        if !models_dir.join("whisper").join("encoder_model_int8.onnx").exists() {
            eprintln!("skipping: models not present, run `npm run setup-models` first");
            return;
        }
        let wav_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("real_speech_sample.wav");
        if !wav_path.exists() {
            eprintln!("skipping: no real_speech_sample.wav fixture present");
            return;
        }

        let mut reader = hound::WavReader::open(&wav_path).expect("open fixture");
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.expect("read sample") as f32 / i16::MAX as f32)
            .collect();

        let engine = engine(&models_dir, false).expect("engine loads");
        let text = engine.transcribe_with_vocabulary(&samples, &[]).expect("transcribe succeeds");
        println!("transcribed: {text:?}");
        assert!(!text.is_empty(), "must not come back empty on real speech with real content");
    }
}
