use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;

const LITE_DIR: &str = "ivy-lite";
const MODEL_NAME: &str = "ivy-lite-Q8_0.gguf";
const MMPROJ_NAME: &str = "mmproj-ivy-lite-f16.gguf";

pub struct LiteEngine {
    _backend: Arc<LlamaBackend>,
    model: LlamaModel,
    mtmd_ctx: *mut llama_cpp_sys_2::mtmd_context,
    lctx: *mut llama_cpp_sys_2::llama_context,
    pub is_cpu_mode: bool,
    inference_lock: Mutex<()>,
}

unsafe impl Send for LiteEngine {}
unsafe impl Sync for LiteEngine {}

impl Drop for LiteEngine {
    fn drop(&mut self) {
        unsafe {
            if !self.lctx.is_null() {
                llama_cpp_sys_2::llama_free(self.lctx);
                self.lctx = std::ptr::null_mut();
            }
            if !self.mtmd_ctx.is_null() {
                llama_cpp_sys_2::mtmd_free(self.mtmd_ctx);
                self.mtmd_ctx = std::ptr::null_mut();
            }
        }
    }
}

type CachedEngine = (bool, Arc<LiteEngine>);
static ENGINE: RwLock<Option<CachedEngine>> = RwLock::new(None);

pub fn unload_engine() {
    if let Ok(mut lock) = ENGINE.write() {
        if lock.is_some() {
            log::info!("Ivy: Unloading Lite engine from memory");
            *lock = None;
        }
    }
}

pub fn engine(models_dir: &Path, is_cpu_mode: bool) -> Result<Arc<LiteEngine>, String> {
    if let Ok(lock) = ENGINE.read() {
        if let Some((cached_cpu_mode, e)) = lock.as_ref() {
            if *cached_cpu_mode == is_cpu_mode {
                return Ok(e.clone());
            }
        }
    }

    let mut lock = ENGINE.write().map_err(|e| e.to_string())?;
    if let Some((cached_cpu_mode, e)) = lock.as_ref() {
        if *cached_cpu_mode == is_cpu_mode {
            return Ok(e.clone());
        }
    }

    let loaded = Arc::new(LiteEngine::load(models_dir, is_cpu_mode)?);
    *lock = Some((is_cpu_mode, loaded.clone()));
    Ok(loaded)
}

fn clear_kv_cache(lctx: *mut llama_cpp_sys_2::llama_context) {
    unsafe {
        let mem = llama_cpp_sys_2::llama_get_memory(lctx);
        llama_cpp_sys_2::llama_memory_clear(mem, true);
    }
}

pub fn instruction(dictionary: &[String]) -> String {
    let base = "Write what the speaker means, ready to paste: apply their own corrections, keep every other word.";
    if dictionary.is_empty() {
        base.to_string()
    } else {
        format!("{base} Words that may appear: {}.", dictionary.join(", "))
    }
}

pub fn format_prompt(dictionary: &[String], marker: &str) -> String {
    let inst = instruction(dictionary);
    format!("<|im_start|>system\n{inst}<|im_end|>\n<|im_start|>user\n{marker}<|im_end|>\n<|im_start|>assistant\nlanguage English<asr_text>")
}

impl LiteEngine {
    fn load(models_dir: &Path, is_cpu_mode: bool) -> Result<Self, String> {
        let lite_path = models_dir.join(LITE_DIR);
        let model_path = lite_path.join(MODEL_NAME);
        let mmproj_path = lite_path.join(MMPROJ_NAME);

        if !model_path.exists() {
            return Err(format!("Lite base model not found: {}", model_path.display()));
        }
        if !mmproj_path.exists() {
            return Err(format!("Lite mmproj not found: {}", mmproj_path.display()));
        }

        let backend = crate::voxtral::get_or_init_backend()?;
        let mparams = LlamaModelParams::default().with_n_gpu_layers(if is_cpu_mode { 0 } else { 99 });
        let model = LlamaModel::load_from_file(&backend, &model_path, &mparams)
            .map_err(|e| format!("Lite model load failed: {e}"))?;

        let raw_model: *mut llama_cpp_sys_2::llama_model = unsafe { std::mem::transmute_copy(&model) };

        let mmproj_cstr = CString::new(mmproj_path.to_str().ok_or("invalid mmproj path")?)
            .map_err(|e| e.to_string())?;

        let threads = std::cmp::min(std::thread::available_parallelism().map_or(4, |n| n.get() as i32), 8);
        let mut mtmd_params = unsafe { llama_cpp_sys_2::mtmd_context_params_default() };
        mtmd_params.use_gpu = !is_cpu_mode;
        mtmd_params.print_timings = false;
        mtmd_params.n_threads = threads;

        let mtmd_ctx = unsafe {
            llama_cpp_sys_2::mtmd_init_from_file(mmproj_cstr.as_ptr(), raw_model, mtmd_params)
        };
        if mtmd_ctx.is_null() {
            return Err("failed to initialize mtmd context for Lite".into());
        }

        let mut cparams = unsafe { llama_cpp_sys_2::llama_context_default_params() };
        cparams.n_ctx = 4096;
        cparams.n_batch = 1024;
        cparams.n_ubatch = 512;
        cparams.n_threads = threads;
        cparams.n_threads_batch = threads;

        let lctx = unsafe { llama_cpp_sys_2::llama_new_context_with_model(raw_model, cparams) };
        if lctx.is_null() {
            unsafe { llama_cpp_sys_2::mtmd_free(mtmd_ctx); }
            return Err("failed to initialize llama context for Lite".into());
        }

        log::info!(
            "Ivy: Lite engine loaded (is_cpu_mode={}, threads={})",
            is_cpu_mode,
            threads
        );

        Ok(Self {
            _backend: backend,
            model,
            mtmd_ctx,
            lctx,
            is_cpu_mode,
            inference_lock: Mutex::new(()),
        })
    }

    /// Transcribe 16 kHz mono f32 audio with self-corrections resolved via fine-tuned merged weights.
    pub fn transcribe(
        &self,
        samples_16k_mono: &[f32],
        dictionary: &[String],
        timeout: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let _guard = self.inference_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut parts = Vec::new();
        for chunk in crate::voxtral::split_long_audio(samples_16k_mono) {
            let text = self.transcribe_chunk(chunk, dictionary, deadline)?;
            if !text.is_empty() {
                parts.push(text);
            }
        }
        Ok(parts.join(" "))
    }

    fn transcribe_chunk(&self, samples: &[f32], dictionary: &[String], deadline: Instant) -> Result<String, String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        clear_kv_cache(self.lctx);

        let bitmap = unsafe { llama_cpp_sys_2::mtmd_bitmap_init_from_audio(samples.len(), samples.as_ptr()) };
        if bitmap.is_null() {
            return Err("failed to create audio bitmap from samples".into());
        }

        let marker = unsafe {
            let m = llama_cpp_sys_2::mtmd_default_marker();
            if m.is_null() {
                "<__media__>"
            } else {
                CStr::from_ptr(m).to_str().unwrap_or("<__media__>")
            }
        };
        let user_prompt = format_prompt(dictionary, marker);
        let c_prompt = CString::new(user_prompt).map_err(|e| e.to_string())?;

        let input_text = llama_cpp_sys_2::mtmd_input_text {
            text: c_prompt.as_ptr(),
            text_len: c_prompt.as_bytes().len(),
            add_special: false,
            parse_special: true,
        };

        let chunks = unsafe { llama_cpp_sys_2::mtmd_input_chunks_init() };
        if chunks.is_null() {
            unsafe { llama_cpp_sys_2::mtmd_bitmap_free(bitmap); }
            return Err("failed to init input chunks".into());
        }

        let mut bitmaps = [bitmap as *const llama_cpp_sys_2::mtmd_bitmap];
        let tok_res = unsafe {
            llama_cpp_sys_2::mtmd_tokenize(self.mtmd_ctx, chunks, &input_text, bitmaps.as_mut_ptr(), 1)
        };
        if tok_res != 0 {
            unsafe {
                llama_cpp_sys_2::mtmd_bitmap_free(bitmap);
                llama_cpp_sys_2::mtmd_input_chunks_free(chunks);
            }
            return Err(format!("mtmd_tokenize failed with code {tok_res}"));
        }

        let mut new_n_past: llama_cpp_sys_2::llama_pos = 0;
        let eval_res = unsafe {
            llama_cpp_sys_2::mtmd_helper_eval_chunks(self.mtmd_ctx, self.lctx, chunks, 0, 0, 1024, true, &mut new_n_past)
        };
        unsafe {
            llama_cpp_sys_2::mtmd_bitmap_free(bitmap);
            llama_cpp_sys_2::mtmd_input_chunks_free(chunks);
        }
        if eval_res != 0 {
            clear_kv_cache(self.lctx);
            return Err(format!("mtmd_helper_eval_chunks failed with code {eval_res}"));
        }

        let max_new_tokens = (samples.len() as f32 / 16000.0 * 6.0) as usize + 64;
        let text = self.greedy_decode(new_n_past, max_new_tokens, deadline);
        clear_kv_cache(self.lctx);
        let trimmed = text?.trim()
            .trim_end_matches("<|im_end|>")
            .trim_end_matches("<|endoftext|>")
            .trim()
            .to_string();
        Ok(trimmed)
    }

    fn greedy_decode(&self, mut n_past: llama_cpp_sys_2::llama_pos, max_tokens: usize, deadline: Instant) -> Result<String, String> {
        let raw_model: *mut llama_cpp_sys_2::llama_model = unsafe { std::mem::transmute_copy(&self.model) };
        let vocab = unsafe { llama_cpp_sys_2::llama_model_get_vocab(raw_model) };
        let n_vocab = unsafe { llama_cpp_sys_2::llama_n_vocab(vocab) };
        let mut bytes: Vec<u8> = Vec::new();
        let batch = unsafe { llama_cpp_sys_2::llama_batch_init(1, 0, 1) };
        let mut result = Ok(());

        for _ in 0..max_tokens {
            if Instant::now() > deadline {
                result = Err("Lite generation timed out".to_string());
                break;
            }
            let logits = unsafe { llama_cpp_sys_2::llama_get_logits_ith(self.lctx, -1) };
            if logits.is_null() {
                break;
            }
            let logits_slice = unsafe { std::slice::from_raw_parts(logits, n_vocab as usize) };
            let mut best_id = 0;
            let mut best_logit = f32::NEG_INFINITY;
            for (id, &logit) in logits_slice.iter().enumerate() {
                if logit > best_logit {
                    best_logit = logit;
                    best_id = id as i32;
                }
            }
            // Stop at <|im_end|> (151645), <|endoftext|> (151643), or vocab EOG
            if unsafe { llama_cpp_sys_2::llama_vocab_is_eog(vocab, best_id) } || best_id == 151645 || best_id == 151643 {
                break;
            }

            let mut piece_buf = [0u8; 128];
            let n_chars = unsafe {
                llama_cpp_sys_2::llama_token_to_piece(vocab, best_id, piece_buf.as_mut_ptr() as *mut c_char, piece_buf.len() as i32, 0, false)
            };
            if n_chars > 0 {
                bytes.extend_from_slice(&piece_buf[..n_chars as usize]);
            }

            unsafe {
                *batch.token.offset(0) = best_id;
                *batch.pos.offset(0) = n_past;
                *batch.n_seq_id.offset(0) = 1;
                *(*batch.seq_id.offset(0)).offset(0) = 0;
                *batch.logits.offset(0) = 1;
                let mut b = batch;
                b.n_tokens = 1;
                n_past += 1;
                if llama_cpp_sys_2::llama_decode(self.lctx, b) != 0 {
                    result = Err("llama_decode failed during generation".to_string());
                    break;
                }
            }
        }
        unsafe { llama_cpp_sys_2::llama_batch_free(batch); }
        result.map(|_| String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Generate text for Touch Up, Summarize, and title testing.
    pub fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: usize,
        timeout: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let _guard = self.inference_lock.lock().unwrap_or_else(|e| e.into_inner());

        clear_kv_cache(self.lctx);

        let raw_model: *mut llama_cpp_sys_2::llama_model = unsafe { std::mem::transmute_copy(&self.model) };
        let vocab = unsafe { llama_cpp_sys_2::llama_model_get_vocab(raw_model) };

        let full_prompt = format!("<|im_start|>system\n{system_prompt}<|im_end|>\n<|im_start|>user\n{user_prompt}<|im_end|>\n<|im_start|>assistant\n");
        let c_prompt = CString::new(full_prompt).map_err(|e| e.to_string())?;

        let tokenize = |buf: &mut Vec<llama_cpp_sys_2::llama_token>| unsafe {
            llama_cpp_sys_2::llama_tokenize(vocab, c_prompt.as_ptr(), c_prompt.as_bytes().len() as i32, buf.as_mut_ptr(), buf.len() as i32, false, true)
        };
        let mut tokens: Vec<llama_cpp_sys_2::llama_token> = vec![0; c_prompt.as_bytes().len() + 32];
        let mut n_tokens = tokenize(&mut tokens);
        if n_tokens < 0 {
            tokens.resize((-n_tokens) as usize, 0);
            n_tokens = tokenize(&mut tokens);
            if n_tokens < 0 {
                return Err("failed to tokenize text prompt".into());
            }
        }
        tokens.truncate(n_tokens as usize);
        if tokens.is_empty() {
            return Ok(String::new());
        }

        let prompt_batch = unsafe { llama_cpp_sys_2::llama_batch_init(tokens.len() as i32, 0, 1) };
        let last_idx = tokens.len() - 1;
        for (i, &tok) in tokens.iter().enumerate() {
            unsafe {
                *prompt_batch.token.add(i) = tok;
                *prompt_batch.pos.add(i) = i as llama_cpp_sys_2::llama_pos;
                *prompt_batch.n_seq_id.add(i) = 1;
                *(*prompt_batch.seq_id.add(i)).add(0) = 0;
                *prompt_batch.logits.add(i) = if i == last_idx { 1 } else { 0 };
            }
        }
        let mut pb = prompt_batch;
        pb.n_tokens = tokens.len() as i32;
        let decode_res = unsafe { llama_cpp_sys_2::llama_decode(self.lctx, pb) };
        unsafe { llama_cpp_sys_2::llama_batch_free(prompt_batch); }
        if decode_res != 0 {
            clear_kv_cache(self.lctx);
            return Err(format!("failed to decode text prompt batch: {decode_res}"));
        }

        let text = self.greedy_decode(tokens.len() as llama_cpp_sys_2::llama_pos, max_tokens, deadline);
        clear_kv_cache(self.lctx);
        let trimmed = text?.trim()
            .trim_end_matches("<|im_end|>")
            .trim_end_matches("<|endoftext|>")
            .trim()
            .to_string();
        Ok(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::path::PathBuf;
    use serde::Deserialize;

    #[test]
    fn instruction_matches_training_prompt_exactly() {
        assert_eq!(
            instruction(&[]),
            "Write what the speaker means, ready to paste: apply their own corrections, keep every other word."
        );
        assert_eq!(
            instruction(&["Ivy".into(), "Tauri".into()]),
            "Write what the speaker means, ready to paste: apply their own corrections, keep every other word. Words that may appear: Ivy, Tauri."
        );
        let prompt = format_prompt(&[], "<__media__>");
        assert_eq!(
            prompt,
            "<|im_start|>system\nWrite what the speaker means, ready to paste: apply their own corrections, keep every other word.<|im_end|>\n<|im_start|>user\n<__media__><|im_end|>\n<|im_start|>assistant\nlanguage English<asr_text>"
        );
    }

    fn read_wav(path: &Path) -> Vec<f32> {
        let mut reader = hound::WavReader::open(path).unwrap_or_else(|e| panic!("failed to open {}: {e}", path.display()));
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16000, "expected 16kHz audio: {}", path.display());
        assert_eq!(spec.channels, 1, "expected mono audio: {}", path.display());
        reader.samples::<i16>().map(|s| s.unwrap() as f32 / i16::MAX as f32).collect()
    }

    #[derive(Deserialize)]
    struct GoldenItem {
        audio: String,
        expected: String,
        #[allow(dead_code)]
        kind: Option<String>,
        #[allow(dead_code)]
        human_gold: Option<Vec<String>>,
    }

    #[test]
    fn test_lite_golden_set_gpu() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let lite_dir = models_dir.join(LITE_DIR);
        let golden_path = lite_dir.join("golden.jsonl");
        if !golden_path.exists() {
            eprintln!("skipping: golden.jsonl not found at {}", golden_path.display());
            return;
        }

        let eng = engine(&models_dir, false).expect("lite engine loads on GPU");
        let file = File::open(&golden_path).expect("open golden.jsonl");
        let reader = BufReader::new(file);

        let mut total = 0;
        let mut matches = 0;
        let mut diffs = Vec::new();

        println!("\n=== RUNNING LITE GOLDEN SET ON GPU ===");
        let t_start = Instant::now();

        for (idx, line) in reader.lines().enumerate() {
            let line = line.expect("read line");
            if line.trim().is_empty() {
                continue;
            }
            let item: GoldenItem = serde_json::from_str(&line).expect("parse json");
            let wav_path = lite_dir.join(&item.audio);
            if !wav_path.exists() {
                eprintln!("skipping clip {}: not found at {}", idx, wav_path.display());
                continue;
            }
            let samples = read_wav(&wav_path);
            let t_clip = Instant::now();
            let result = eng.transcribe(&samples, &[], Duration::from_secs(30)).expect("transcribe");
            let clip_ms = t_clip.elapsed().as_millis();

            total += 1;
            let result_clean = result.trim();
            let expected_clean = item.expected.trim();
            if result_clean == expected_clean {
                matches += 1;
                println!("[{:02}/60] MATCH ({clip_ms}ms): {}", idx + 1, result_clean);
            } else {
                println!("[{:02}/60] DIFF ({clip_ms}ms):\n  GOT:      {}\n  EXPECTED: {}", idx + 1, result_clean, expected_clean);
                diffs.push((idx + 1, result_clean.to_string(), expected_clean.to_string()));
            }
        }

        let total_ms = t_start.elapsed().as_millis();
        println!("============================================================");
        println!("LITE GOLDEN RESULTS: {matches}/{total} matches in {total_ms}ms ({:.1}s)", total_ms as f64 / 1000.0);
        if !diffs.is_empty() {
            println!("Differences ({}):", diffs.len());
            for (idx, got, exp) in &diffs {
                println!("  Clip {idx}:\n    Got:      {got}\n    Expected: {exp}");
            }
        }
        println!("============================================================\n");

        assert!(matches >= 57, "Expected at least 57/60 matches, got {matches}/{total}");
    }

    #[test]
    fn test_task7_quiet_microphones() {
        let quiet_dir = Path::new(r"C:\Users\YASH\Downloads\IVY_decision_lab\para_test\quiet_repro");
        if !quiet_dir.exists() {
            eprintln!("skipping: quiet_repro not found");
            return;
        }
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let eng = engine(&models_dir, false).expect("lite engine loads on GPU");

        println!("\n=== TASK 7: QUIET MICROPHONES TEST (ENGINE = LITE) ===");
        println!("{:<8} {:<12} {:<12} {:<15} {}", "Clip", "Peak Before", "Peak After", "Voiced Secs", "Transcribed Text");
        println!("{:-<80}", "");

        for clip_name in ["q1.wav", "q2.wav", "q3.wav", "q4.wav", "q5.wav"] {
            let wav_path = quiet_dir.join(clip_name);
            if !wav_path.exists() {
                continue;
            }
            let raw_samples = read_wav(&wav_path);
            let peak_before = raw_samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));

            // Live pipeline AGC
            let normalized = crate::audio::normalize_audio(&raw_samples);
            let stt_samples = if normalized.is_empty() { &raw_samples } else { &normalized };
            let peak_after = stt_samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));

            // Speech presence check (Stage A)
            let (_voiced, voice) = crate::rulebooks::hallucinations::prepare_audio(stt_samples, 16000);
            
            let raw = eng.transcribe(stt_samples, &[], Duration::from_secs(30)).unwrap_or_default();
            let cleaned = crate::rulebooks::hallucinations::clean_asr_text(&raw, voice);
            let final_text = crate::rulebooks::after_voxtral(&cleaned, crate::rulebooks::Tone::Standard);
            let _ = final_text;

            // Also test peak-normalisation as described in Task 7
            let peak_norm_gain = (0.7 / peak_before).min(20.0);
            let peak_norm_samples: Vec<f32> = raw_samples.iter().map(|&s| s * peak_norm_gain).collect();
            let raw_peak_norm = eng.transcribe(&peak_norm_samples, &[], Duration::from_secs(30)).unwrap_or_default();

            // Test DAGC with lower TARGET_RMS (0.10) to avoid hard-clipping
            let mut agc_low = dagc::MonoAgc::new(0.10, 0.001).unwrap();
            let mut low_samples = raw_samples.clone();
            let frame_len = 320;
            if low_samples.len() >= frame_len * 4 {
                let mut f_rms: Vec<f32> = low_samples.chunks(frame_len).map(|f| (f.iter().map(|&s| s * s).sum::<f32>() / f.len() as f32).sqrt()).collect();
                f_rms.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let fl = f_rms[f_rms.len() / 10];
                for chunk in low_samples.chunks_mut(frame_len) {
                    let c_rms = (chunk.iter().map(|&s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
                    agc_low.freeze_gain(c_rms <= fl * 1.5);
                    agc_low.process(chunk);
                }
            }
            for s in low_samples.iter_mut() { *s = s.clamp(-1.0, 1.0); }
            let raw_low_agc = eng.transcribe(&low_samples, &[], Duration::from_secs(30)).unwrap_or_default();

            println!("{:<8} {:<12.4} {:<12.4} {:<15.2}\n  DAGC(0.25): \"{}\"\n  Peak-norm:  \"{}\"\n  DAGC(0.10): \"{}\"",
                clip_name, peak_before, peak_after, voice.voiced_secs, raw, raw_peak_norm, raw_low_agc);
        }
        println!("======================================================\n");
    }

    #[test]
    fn test_lite_benchmark_measurements() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let lite_dir = models_dir.join(LITE_DIR);
        if !lite_dir.join(MODEL_NAME).exists() || !lite_dir.join(MMPROJ_NAME).exists() {
            eprintln!("skipping: Lite models not present");
            return;
        }

        let audio_path = lite_dir.join("golden/session-1790795120901.wav");
        let base_samples = read_wav(&audio_path);

        let make_clip = |secs: usize| -> Vec<f32> {
            let target = secs * 16_000;
            let mut out = Vec::with_capacity(target);
            while out.len() < target {
                let take = (target - out.len()).min(base_samples.len());
                out.extend_from_slice(&base_samples[..take]);
            }
            out
        };

        let clip_5s = make_clip(5);
        let clip_15s = make_clip(15);
        let clip_30s = make_clip(30);

        fn get_mem_mb() -> (u64, u64) {
            #[cfg(windows)]
            {
                use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
                use windows::Win32::System::Threading::GetCurrentProcess;
                unsafe {
                    let mut pmc = PROCESS_MEMORY_COUNTERS::default();
                    pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                    if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb).is_ok() {
                        let cur_mb = (pmc.WorkingSetSize as u64) / (1024 * 1024);
                        let peak_mb = (pmc.PeakWorkingSetSize as u64) / (1024 * 1024);
                        return (cur_mb, peak_mb);
                    }
                }
            }
            (0, 0)
        }

        fn median_ms(mut vals: Vec<u128>) -> u128 {
            vals.sort();
            vals[vals.len() / 2]
        }

        // 1. Lite GPU
        println!("\n>>> Benchmarking Lite GPU (Vulkan)...");
        unload_engine();
        let (_, ram_baseline) = get_mem_mb();
        let vram_baseline = crate::gpu_monitor::query_gpu_telemetry().used_vram_mb;

        let t0 = Instant::now();
        let lite_gpu = engine(&models_dir, false).expect("load Lite GPU");
        let cold_load_gpu = t0.elapsed().as_millis();
        let vram_post_load = crate::gpu_monitor::query_gpu_telemetry().used_vram_mb;
        let (_, ram_post_load) = get_mem_mb();
        let lite_gpu_vram = vram_post_load.saturating_sub(vram_baseline);
        let lite_gpu_ram = ram_post_load.saturating_sub(ram_baseline);

        // Warm up
        let _ = lite_gpu.transcribe(&clip_5s, &[], Duration::from_secs(30));

        let mut times_5s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let _ = lite_gpu.transcribe(&clip_5s, &[], Duration::from_secs(30)).unwrap();
            times_5s.push(t.elapsed().as_millis());
        }
        let med_5s_gpu = median_ms(times_5s);

        let mut times_15s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let _ = lite_gpu.transcribe(&clip_15s, &[], Duration::from_secs(30)).unwrap();
            times_15s.push(t.elapsed().as_millis());
        }
        let med_15s_gpu = median_ms(times_15s);

        let mut times_30s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let _ = lite_gpu.transcribe(&clip_30s, &[], Duration::from_secs(30)).unwrap();
            times_30s.push(t.elapsed().as_millis());
        }
        let med_30s_gpu = median_ms(times_30s);

        drop(lite_gpu);
        unload_engine();

        // 2. Lite CPU
        println!("\n>>> Benchmarking Lite CPU...");
        let (_, ram_baseline_cpu) = get_mem_mb();
        let t0 = Instant::now();
        let lite_cpu = engine(&models_dir, true).expect("load Lite CPU");
        let cold_load_cpu = t0.elapsed().as_millis();
        let (_, ram_post_load_cpu) = get_mem_mb();
        let lite_cpu_ram = ram_post_load_cpu.saturating_sub(ram_baseline_cpu);

        // Warm up
        let _ = lite_cpu.transcribe(&clip_5s, &[], Duration::from_secs(30));

        let mut times_5s_cpu = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let _ = lite_cpu.transcribe(&clip_5s, &[], Duration::from_secs(30)).unwrap();
            times_5s_cpu.push(t.elapsed().as_millis());
        }
        let med_5s_cpu = median_ms(times_5s_cpu);

        let mut times_15s_cpu = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let _ = lite_cpu.transcribe(&clip_15s, &[], Duration::from_secs(60)).unwrap();
            times_15s_cpu.push(t.elapsed().as_millis());
        }
        let med_15s_cpu = median_ms(times_15s_cpu);

        let mut times_30s_cpu = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let _ = lite_cpu.transcribe(&clip_30s, &[], Duration::from_secs(90)).unwrap();
            times_30s_cpu.push(t.elapsed().as_millis());
        }
        let med_30s_cpu = median_ms(times_30s_cpu);

        drop(lite_cpu);
        unload_engine();

        println!("\n==================================== LITE BENCHMARK RESULTS ====================================");
        println!("{:<16} {:<14} {:<12} {:<12} {:<12} {:<10} {:<10} {:<10}",
            "Engine", "Backend", "Cold Load", "Peak VRAM", "Peak RAM", "5s med", "15s med", "30s med");
        println!("{:-<96}", "");
        println!("{:<16} {:<14} {:<12} {:<12} {:<12} {:<10.2} {:<10.2} {:<10.2}",
            "Lite (Qwen3)", "GPU (Vulkan)", format!("{} ms", cold_load_gpu), format!("{} MB", lite_gpu_vram), format!("{} MB", lite_gpu_ram),
            med_5s_gpu as f64 / 1000.0, med_15s_gpu as f64 / 1000.0, med_30s_gpu as f64 / 1000.0);
        println!("{:<16} {:<14} {:<12} {:<12} {:<12} {:<10.2} {:<10.2} {:<10.2}",
            "Lite (Qwen3)", "CPU", format!("{} ms", cold_load_cpu), "0 MB", format!("{} MB", lite_cpu_ram),
            med_5s_cpu as f64 / 1000.0, med_15s_cpu as f64 / 1000.0, med_30s_cpu as f64 / 1000.0);
        println!("================================================================================================\n");
    }

    #[test]
    fn test_lite_touch_up_summarize() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let lite_dir = models_dir.join(LITE_DIR);
        if !lite_dir.join(MODEL_NAME).exists() {
            eprintln!("skipping: Lite models not present");
            return;
        }

        let eng = engine(&models_dir, false).expect("load Lite GPU");

        let transcripts = [
            ("Short", "the meeting is at 5pm we need to finish the report before then can you send it over"),
            ("Medium", "Marketing finished the landing page yesterday. Email campaign goes out Tuesday morning at 9 AM. Payment integration bug is the main blocker; David thinks it will be fixed by Monday. Pricing page copy needs review before Tuesday; Priya should do this."),
            ("Long", "My neighbor asked me to water her plants while she's in Kochi for 2 weeks. So, writing this down before I forget. The money plant and the snake plant only need water once a week on Sundays. The tulsi needs water every morning before the sun gets too strong. The fern in the bathroom likes humidity, so just spray it with water every 3 days. Don't overwater the cactus. Once in 10 days is plenty. She already said the curry leaf plant has some white bugs on it and I should spray the neem oil mix that's on the shelf near the door. The spare key is with the watchman, Raju, and her flight lands back on the 18th."),
        ];

        let touch_up_sys = "You are an expert editor. Clean up the following dictation: fix punctuation, grammar, and formatting while preserving the speaker's exact meaning. Output ONLY the cleaned text.";
        let summarize_sys = "You are an executive assistant. Summarize the following transcript into concise, clear bullet points. Output ONLY the bullet points.";

        println!("\n=== EVALUATING TEXT GENERATION ON LITE (QWEN3-ASR) ===");
        for (name, text) in &transcripts {
            println!("\n--- Transcript: {name} ---");
            println!("Input: \"{}\"", text);
            let t0 = Instant::now();
            let touched = eng.generate_text(touch_up_sys, text, 256, Duration::from_secs(15));
            println!("Touch Up ({:?}): {:?}", t0.elapsed(), touched);

            let t1 = Instant::now();
            let summ = eng.generate_text(summarize_sys, text, 256, Duration::from_secs(15));
            println!("Summarize ({:?}): {:?}", t1.elapsed(), summ);
        }
        println!("======================================================\n");
    }
}
