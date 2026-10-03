use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaLoraAdapter, LlamaModel};

static GLOBAL_BACKEND: OnceLock<Arc<LlamaBackend>> = OnceLock::new();

pub fn get_or_init_backend() -> Result<Arc<LlamaBackend>, String> {
    if let Some(backend) = GLOBAL_BACKEND.get() {
        return Ok(backend.clone());
    }
    let backend = Arc::new(LlamaBackend::init().map_err(|e| format!("llama backend init failed: {e}"))?);
    let _ = GLOBAL_BACKEND.set(backend.clone());
    Ok(GLOBAL_BACKEND.get().unwrap().clone())
}

const VOXTRAL_DIR: &str = "voxtral-ivy";
const MODEL_NAME: &str = "Voxtral-Mini-3B-2507-Q4_K_M.gguf";
const MMPROJ_NAME: &str = "mmproj-Voxtral-Mini-3B-2507-Q8_0.gguf";
const LORA_NAME: &str = "ivy-lora.gguf";

pub struct VoxtralEngine {
    _backend: Arc<LlamaBackend>,
    model: LlamaModel,
    lora_adapter: LlamaLoraAdapter,
    mtmd_ctx: *mut llama_cpp_sys_2::mtmd_context,
    lctx: *mut llama_cpp_sys_2::llama_context,
    pub is_cpu_mode: bool,
    inference_lock: Mutex<()>,
}

unsafe impl Send for VoxtralEngine {}
unsafe impl Sync for VoxtralEngine {}

impl Drop for VoxtralEngine {
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

type CachedEngine = (bool, Arc<VoxtralEngine>);
static ENGINE: RwLock<Option<CachedEngine>> = RwLock::new(None);

pub fn unload_engine() {
    if let Ok(mut lock) = ENGINE.write() {
        if lock.is_some() {
            log::info!("Ivy: Unloading Voxtral engine from memory");
            *lock = None;
        }
    }
}

pub fn engine(models_dir: &Path, is_cpu_mode: bool) -> Result<Arc<VoxtralEngine>, String> {
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

    let loaded = Arc::new(VoxtralEngine::load(models_dir, is_cpu_mode)?);
    *lock = Some((is_cpu_mode, loaded.clone()));
    Ok(loaded)
}

fn clear_kv_cache(lctx: *mut llama_cpp_sys_2::llama_context) {
    unsafe {
        let mem = llama_cpp_sys_2::llama_get_memory(lctx);
        llama_cpp_sys_2::llama_memory_clear(mem, true);
    }
}

impl VoxtralEngine {
    fn load(models_dir: &Path, is_cpu_mode: bool) -> Result<Self, String> {
        let voxtral_path = models_dir.join(VOXTRAL_DIR);
        let model_path = voxtral_path.join(MODEL_NAME);
        let mmproj_path = voxtral_path.join(MMPROJ_NAME);
        let lora_path = voxtral_path.join(LORA_NAME);

        if !model_path.exists() {
            return Err(format!("Voxtral base model not found: {}", model_path.display()));
        }
        if !mmproj_path.exists() {
            return Err(format!("Voxtral mmproj not found: {}", mmproj_path.display()));
        }
        if !lora_path.exists() {
            return Err(format!("Voxtral LoRA adapter not found: {}", lora_path.display()));
        }

        let backend = get_or_init_backend()?;
        let mparams = LlamaModelParams::default().with_n_gpu_layers(if is_cpu_mode { 0 } else { 99 });
        let model = LlamaModel::load_from_file(&backend, &model_path, &mparams)
            .map_err(|e| format!("Voxtral model load failed: {e}"))?;

        let lora_adapter = model
            .lora_adapter_init(&lora_path)
            .map_err(|e| format!("Voxtral LoRA adapter load failed: {e}"))?;

        let raw_model: *mut llama_cpp_sys_2::llama_model = unsafe { std::mem::transmute_copy(&model) };

        // Initialize multimodal context
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
            return Err("failed to initialize mtmd context".into());
        }

        // Initialize llama context
        let mut cparams = unsafe { llama_cpp_sys_2::llama_context_default_params() };
        cparams.n_ctx = 4096;
        cparams.n_batch = 1024;
        cparams.n_ubatch = 512;
        cparams.n_threads = threads;
        cparams.n_threads_batch = threads;

        let lctx = unsafe { llama_cpp_sys_2::llama_new_context_with_model(raw_model, cparams) };
        if lctx.is_null() {
            unsafe { llama_cpp_sys_2::mtmd_free(mtmd_ctx); }
            return Err("failed to initialize llama context for Voxtral".into());
        }

        log::info!(
            "Ivy: Voxtral engine loaded (is_cpu_mode={}, threads={})",
            is_cpu_mode,
            threads
        );

        Ok(Self {
            _backend: backend,
            model,
            lora_adapter,
            mtmd_ctx,
            lctx,
            is_cpu_mode,
            inference_lock: Mutex::new(()),
        })
    }

    /// Transcribe 16 kHz mono f32 audio with self-corrections resolved via the fine-tuned LoRA.
    /// Audio longer than `CHUNK_SECS` is cut at the quietest moment near each boundary and the
    /// pieces are transcribed in order, so a long dictation never overflows the context window.
    /// Errors (never a silently truncated paste) if the deadline passes; the caller keeps the
    /// recording for retry.
    pub fn transcribe(
        &self,
        samples_16k_mono: &[f32],
        dictionary: &[String],
        timeout: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let _guard = self.inference_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut parts = Vec::new();
        for chunk in split_long_audio(samples_16k_mono) {
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
        let raw_adapter: *mut llama_cpp_sys_2::llama_adapter_lora = unsafe { std::mem::transmute_copy(&self.lora_adapter) };

        // 1. Enable LoRA with scale 1.0
        unsafe {
            let mut adapter_ptr = raw_adapter;
            let mut scale = 1.0f32;
            let err = llama_cpp_sys_2::llama_set_adapters_lora(self.lctx, &mut adapter_ptr, 1, &mut scale);
            if err != 0 {
                return Err(format!("failed to set LoRA adapter: {err}"));
            }
        }
        clear_kv_cache(self.lctx);

        // 2. Create audio bitmap from raw 16kHz f32 samples
        let bitmap = unsafe { llama_cpp_sys_2::mtmd_bitmap_init_from_audio(samples.len(), samples.as_ptr()) };
        if bitmap.is_null() {
            return Err("failed to create audio bitmap from samples".into());
        }

        // 3. The exact layout the LoRA was trained on (HF VoxtralProcessor):
        //    <s>[INST][BEGIN_AUDIO][AUDIO]...[AUDIO]Write what ...[/INST]
        //    mtmd expands the marker to [BEGIN_AUDIO] + the audio embeddings. No space, no newline.
        let marker = unsafe {
            let m = llama_cpp_sys_2::mtmd_default_marker();
            if m.is_null() {
                "<__media__>"
            } else {
                CStr::from_ptr(m).to_str().unwrap_or("<__media__>")
            }
        };
        let user_prompt = format!("<s>[INST]{marker}{}[/INST]", instruction(dictionary));
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

        // 4. Evaluate multimodal chunks
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

        // 5. Greedy decoding. Fast speech runs ~4-5 tokens/s, so leave generous headroom.
        let max_new_tokens = (samples.len() as f32 / 16000.0 * 6.0) as usize + 64;
        let text = self.greedy_decode(new_n_past, max_new_tokens, deadline);
        clear_kv_cache(self.lctx);
        Ok(text?.trim().to_string())
    }

    /// Greedy decoding from the current KV cache. Collects raw bytes and decodes UTF-8 once at
    /// the end, so characters split across tokens (₹, é, emoji) survive.
    fn greedy_decode(&self, mut n_past: llama_cpp_sys_2::llama_pos, max_tokens: usize, deadline: Instant) -> Result<String, String> {
        let raw_model: *mut llama_cpp_sys_2::llama_model = unsafe { std::mem::transmute_copy(&self.model) };
        let vocab = unsafe { llama_cpp_sys_2::llama_model_get_vocab(raw_model) };
        let n_vocab = unsafe { llama_cpp_sys_2::llama_n_vocab(vocab) };
        let mut bytes: Vec<u8> = Vec::new();
        let batch = unsafe { llama_cpp_sys_2::llama_batch_init(1, 0, 1) };
        let mut result = Ok(());

        for _ in 0..max_tokens {
            if Instant::now() > deadline {
                result = Err("Voxtral generation timed out".to_string());
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
            if unsafe { llama_cpp_sys_2::llama_vocab_is_eog(vocab, best_id) } {
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

    /// Generate text without LoRA for Touch Up, Summarization, and titles. `prompt` carries its
    /// own `<s>[INST]...[/INST]`, so the tokenizer must not add a second BOS.
    pub fn generate_text(
        &self,
        prompt: &str,
        max_tokens: usize,
        timeout: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let _guard = self.inference_lock.lock().unwrap_or_else(|e| e.into_inner());

        let raw_model: *mut llama_cpp_sys_2::llama_model = unsafe { std::mem::transmute_copy(&self.model) };

        // 1. Turn LoRA OFF for base text generation
        unsafe {
            llama_cpp_sys_2::llama_set_adapters_lora(self.lctx, std::ptr::null_mut(), 0, std::ptr::null_mut());
        }
        clear_kv_cache(self.lctx);

        // 2. Tokenize prompt (add_special = false: the prompt already starts with a literal <s>)
        let vocab = unsafe { llama_cpp_sys_2::llama_model_get_vocab(raw_model) };
        let c_prompt = CString::new(prompt).map_err(|e| e.to_string())?;
        let tokenize = |buf: &mut Vec<llama_cpp_sys_2::llama_token>| unsafe {
            llama_cpp_sys_2::llama_tokenize(vocab, c_prompt.as_ptr(), c_prompt.as_bytes().len() as i32, buf.as_mut_ptr(), buf.len() as i32, false, true)
        };
        let mut tokens: Vec<llama_cpp_sys_2::llama_token> = vec![0; prompt.len() + 32];
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

        // 3. Decode prompt in batch
        let prompt_batch = unsafe { llama_cpp_sys_2::llama_batch_init(tokens.len() as i32, 0, 1) };
        let last_idx = tokens.len() - 1;
        for (i, &tok) in tokens.iter().enumerate() {
            unsafe {
                *prompt_batch.token.add(i) = tok;
                *prompt_batch.pos.add(i) = i as llama_cpp_sys_2::llama_pos;
                *prompt_batch.n_seq_id.add(i) = 1;
                *(*prompt_batch.seq_id.add(i)) = 0;
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

        // 4. Greedy generation
        let text = self.greedy_decode(tokens.len() as llama_cpp_sys_2::llama_pos, max_tokens, deadline);
        clear_kv_cache(self.lctx);
        text
    }

    /// Summarize dictated recording into bullet points (with LoRA OFF).
    pub fn summarize(&self, raw: &str) -> Result<String, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("nothing to summarize".into());
        }
        let system = "Summarize this dictated recording into a few short, clear bullet points. \
            Preserve every key fact, name, number, and decision exactly as said — never invent \
            anything that wasn't actually said. Output ONLY the bullet points, one per line, each \
            starting with \"- \", nothing else.";
        let prompt = format!("[INST] {system}\n\n{raw} [/INST]");
        let timeout = if self.is_cpu_mode {
            Duration::from_secs(35)
        } else {
            Duration::from_secs(20)
        };
        let summary = self.generate_text(&prompt, 400, timeout)?;
        let summary = summary.trim().to_string();

        let cont_ratio = crate::rulebooks::faithfulness::contained_ratio(&summary, raw);
        if cont_ratio < 0.20 {
            return Err(format!(
                "summary diverged too far from what was actually said (contained={cont_ratio:.2})"
            ));
        }
        Ok(summary)
    }

    /// Touch Up proofreading on-demand (with LoRA OFF).
    pub fn touch_up(&self, raw: &str, tone: &str) -> Result<String, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("nothing to touch up".into());
        }
        if raw.chars().count() > 1000 {
            return Err("transcript too long for touch up".into());
        }

        let scope = if tone.eq_ignore_ascii_case("casual") {
            "Only remove a word or short phrase the speaker accidentally repeated back to back \
            (a stutter or restart) that survived cleanup. Do not add or change any punctuation. \
            Do not touch anything else."
        } else {
            "Add or fix punctuation only where a natural speaking pause clearly needed a period, \
            question mark, or comma that's missing, and remove a word or short phrase the speaker \
            accidentally repeated back to back (a stutter or restart) that survived cleanup. \
            Do not touch anything else."
        };
        let system = format!(
            "You are given an already-cleaned spoken transcript. {scope} \
            Every single word the speaker actually said must stay exactly as it is — same words, \
            same order, same meaning. Never reword, rephrase, summarize, shorten, or substitute \
            any word, even one that seems misheard or wrong; you may only add punctuation and \
            delete an exact repeated duplicate. Output ONLY the result with no quotes or notes."
        );
        let prompt = format!("[INST] {system}\n\n{raw} [/INST]");
        let max_gen_tokens = (raw.split_whitespace().count() + 40).min(200);
        let timeout = if self.is_cpu_mode {
            Duration::from_secs(15)
        } else {
            Duration::from_secs(10)
        };
        let result = self.generate_text(&prompt, max_gen_tokens, timeout)?;
        let result = result.trim().to_string();

        if result.is_empty() || result == raw {
            return Err("touch up made no changes".into());
        }
        if !crate::rulebooks::faithfulness::is_word_subsequence(&result, raw) {
            return Err("touch up would have changed a word the speaker said, discarded".into());
        }
        Ok(result)
    }
}

/// The dictation instruction, verbatim from training (lab `train_voxtral.py` PROMPT + context suffix).
fn instruction(dictionary: &[String]) -> String {
    let mut s = String::from("Write what the speaker means, ready to paste: apply their own corrections, keep every other word.");
    if !dictionary.is_empty() {
        s.push_str(" Words that may appear: ");
        s.push_str(&dictionary.join(", "));
        s.push('.');
    }
    s
}

/// Longest piece sent to Voxtral in one pass. 120 s of audio is ~1,500 audio tokens plus up to
/// ~780 output tokens, well inside the 4,096-token context. Ivy allows dictations up to 500 s.
const CHUNK_SECS: usize = 120;

/// Splits audio longer than `CHUNK_SECS` into pieces, cutting each at the quietest 100 ms window
/// in the last 20 s before the limit, so a cut lands in a pause rather than mid-word.
// ponytail: energy-based cut, not VAD; a correction spoken across a cut is not merged. Fine for
// dictations under 2 min (almost all); revisit if long-form dictation becomes common.
fn split_long_audio(samples: &[f32]) -> Vec<&[f32]> {
    const SR: usize = 16_000;
    const WIN: usize = SR / 10;
    let limit = CHUNK_SECS * SR;
    let mut out = Vec::new();
    let mut rest = samples;
    while rest.len() > limit {
        let search_start = limit - 20 * SR;
        let mut best = limit;
        let mut best_energy = f32::INFINITY;
        let mut i = search_start;
        while i + WIN <= limit {
            let e: f32 = rest[i..i + WIN].iter().map(|s| s * s).sum();
            if e < best_energy {
                best_energy = e;
                best = i + WIN / 2;
            }
            i += WIN / 2;
        }
        out.push(&rest[..best]);
        rest = &rest[best..];
    }
    out.push(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn instruction_matches_training_prompt_exactly() {
        assert_eq!(
            instruction(&[]),
            "Write what the speaker means, ready to paste: apply their own corrections, keep every other word."
        );
        assert!(instruction(&["Ivy".into(), "Tauri".into()]).ends_with("other word. Words that may appear: Ivy, Tauri."));
    }

    #[test]
    fn long_audio_is_split_at_the_quiet_spot_and_nothing_is_lost() {
        let sr = 16_000;
        // 300 s of "speech" with one silent 1 s gap at 110 s and another at 220 s.
        let mut a = vec![0.5f32; 300 * sr];
        for s in &mut a[110 * sr..111 * sr] { *s = 0.0; }
        for s in &mut a[220 * sr..221 * sr] { *s = 0.0; }
        let parts = split_long_audio(&a);
        assert_eq!(parts.iter().map(|p| p.len()).sum::<usize>(), a.len());
        assert!(parts.iter().all(|p| p.len() <= CHUNK_SECS * sr));
        let first_cut = parts[0].len() as f32 / sr as f32;
        assert!((110.0..111.0).contains(&first_cut), "cut at {first_cut}s, expected inside the 110-111 s pause");
        assert_eq!(split_long_audio(&a[..60 * sr]).len(), 1);
    }

    fn read_wav(path: &Path) -> Vec<f32> {
        let mut reader = hound::WavReader::open(path).expect("open wav");
        let spec = reader.spec();
        println!(
            "WAV spec for {:?}: sample_rate={}, channels={}, bits={}, format={:?}",
            path.file_name().unwrap(),
            spec.sample_rate,
            spec.channels,
            spec.bits_per_sample,
            spec.sample_format
        );
        match spec.sample_format {
            hound::SampleFormat::Int => {
                if spec.bits_per_sample == 16 {
                    reader
                        .samples::<i16>()
                        .map(|s| s.expect("read sample") as f32 / i16::MAX as f32)
                        .collect()
                } else {
                    panic!("unsupported bits per sample: {}", spec.bits_per_sample);
                }
            }
            hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.expect("sample")).collect(),
        }
    }

    #[test]
    fn test_voxtral_chat_template_and_transcribe() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let voxtral_dir = models_dir.join(VOXTRAL_DIR);
        if !voxtral_dir.join(MODEL_NAME).exists()
            || !voxtral_dir.join(MMPROJ_NAME).exists()
            || !voxtral_dir.join(LORA_NAME).exists()
        {
            eprintln!("skipping: Voxtral models not present");
            return;
        }

        let eng = engine(&models_dir, false).expect("engine loads");
        let raw_model: *mut llama_cpp_sys_2::llama_model = unsafe { std::mem::transmute_copy(&eng.model) };
        let tmpl = unsafe { llama_cpp_sys_2::llama_model_chat_template(raw_model, std::ptr::null()) };

        let marker = unsafe {
            let m = llama_cpp_sys_2::mtmd_default_marker();
            if m.is_null() {
                "<__media__>"
            } else {
                CStr::from_ptr(m).to_str().unwrap_or("<__media__>")
            }
        };

        let instruction = "Write what the speaker means, ready to paste: apply their own corrections, keep every other word.";
        let content_str = format!("{marker}\n{instruction}");
        let role = CString::new("user").unwrap();
        let content = CString::new(content_str.clone()).unwrap();
        let msg = llama_cpp_sys_2::llama_chat_message {
            role: role.as_ptr(),
            content: content.as_ptr(),
        };

        let mut buf = vec![0u8; 2048];
        let res_len = unsafe {
            llama_cpp_sys_2::llama_chat_apply_template(
                tmpl,
                &msg,
                1,
                true,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as i32,
            )
        };
        if res_len > 0 {
            let templated = String::from_utf8_lossy(&buf[..res_len as usize]);
            println!("llama_chat_apply_template output ({} bytes):\n{}", res_len, templated);
        } else {
            println!("llama_chat_apply_template returned error: {}", res_len);
        }

        let jsonl_path = voxtral_dir.join("golden.jsonl");
        if !jsonl_path.exists() {
            eprintln!("skipping: golden.jsonl not found");
            return;
        }

        let content = std::fs::read_to_string(&jsonl_path).expect("read jsonl");
        let mut count = 0;
        for line in content.lines().take(5) {
            if line.trim().is_empty() {
                continue;
            }
            count += 1;
            let val: serde_json::Value = serde_json::from_str(line).expect("parse json");
            let rel_audio = val["audio"].as_str().unwrap();
            let audio_path = voxtral_dir.join(rel_audio);
            let expected = val["expected"].as_str().unwrap_or("");
            let pytorch_v5 = val["pytorch_v5"].as_str().unwrap_or("");

            let samples = read_wav(&audio_path);
            let t0 = Instant::now();
            let res = eng.transcribe(&samples, &[], Duration::from_secs(30));
            let elapsed = t0.elapsed();
            println!("\n--- Clip {} ({}) in {:?} ---", count, rel_audio, elapsed);
            println!("Expected:   {:?}", expected);
            println!("PyTorch v5: {:?}", pytorch_v5);
            println!("Voxtral:    {:?}", res);
        }
    }

    #[test]
    fn test_voxtral_generate_text() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let voxtral_dir = models_dir.join(VOXTRAL_DIR);
        if !voxtral_dir.join(MODEL_NAME).exists()
            || !voxtral_dir.join(MMPROJ_NAME).exists()
            || !voxtral_dir.join(LORA_NAME).exists()
        {
            eprintln!("skipping: Voxtral models not present");
            return;
        }

        let eng = engine(&models_dir, false).expect("engine loads");
        let prompt = "[INST] Say hello in exactly two words. [/INST]";
        let text = eng.generate_text(prompt, 16, Duration::from_secs(10)).expect("generate succeeds");
        println!("Generated text: {:?}", text);
        assert!(!text.trim().is_empty());
    }

    #[test]
    fn test_voxtral_run_all_golden() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let voxtral_dir = models_dir.join(VOXTRAL_DIR);
        let jsonl_path = voxtral_dir.join("golden.jsonl");
        if !voxtral_dir.join(MODEL_NAME).exists()
            || !voxtral_dir.join(MMPROJ_NAME).exists()
            || !voxtral_dir.join(LORA_NAME).exists()
            || !jsonl_path.exists()
        {
            eprintln!("skipping: Voxtral models or golden.jsonl not present");
            return;
        }

        let eng = engine(&models_dir, false).expect("engine loads");
        let content = std::fs::read_to_string(&jsonl_path).expect("read jsonl");

        let mut total = 0;
        let mut match_expected = 0;
        let mut match_pytorch = 0;
        let mut match_human = 0;
        let mut total_time_ms = 0u128;

        println!("\n==================== VOXTRAL GOLDEN SET BENCHMARK (60 CLIPS) ====================");

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            total += 1;
            let val: serde_json::Value = serde_json::from_str(line).expect("parse json");
            let rel_audio = val["audio"].as_str().unwrap();
            let audio_path = voxtral_dir.join(rel_audio);
            let expected = val["expected"].as_str().unwrap_or("");
            let pytorch_v5 = val["pytorch_v5"].as_str().unwrap_or("");
            let human_gold: Vec<String> = val["human_gold"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let samples = read_wav(&audio_path);
            let t0 = Instant::now();
            let res = eng.transcribe(&samples, &[], Duration::from_secs(30));
            let elapsed_ms = t0.elapsed().as_millis();
            total_time_ms += elapsed_ms;

            let actual = match res {
                Ok(ref t) => t.as_str(),
                Err(ref e) => {
                    println!("[#{:02}] FAIL (error: {}) {}", total, e, rel_audio);
                    continue;
                }
            };

            let is_match_exp = actual == expected;
            let is_match_pyt = actual == pytorch_v5;
            let is_match_hum = human_gold.iter().any(|h| h == actual);

            if is_match_exp { match_expected += 1; }
            if is_match_pyt { match_pytorch += 1; }
            if is_match_hum { match_human += 1; }

            let mark = if is_match_exp { "MATCH" } else { "DIFF " };
            println!(
                "[#{:02}] {} ({:4}ms) | Voxtral: {:?}\n      Expected: {:?} | PyTorch v5: {:?}",
                total, mark, elapsed_ms, actual, expected, pytorch_v5
            );
        }

        let avg_ms = if total > 0 { total_time_ms / total as u128 } else { 0 };
        println!("================================================================================");
        println!("TOTAL CLIPS: {}", total);
        println!("MATCH EXPECTED:    {}/{} ({:.1}%)", match_expected, total, (match_expected as f64 / total as f64) * 100.0);
        println!("MATCH PYTORCH V5:  {}/{} ({:.1}%)", match_pytorch, total, (match_pytorch as f64 / total as f64) * 100.0);
        println!("EXACT HUMAN GOLD:  {}/{} ({:.1}%)  (strict; the lab's lenient score is in IVY.md)", match_human, total, (match_human as f64 / total as f64) * 100.0);
        println!("AVERAGE TIME:      {} ms/clip", avg_ms);
        println!("================================================================================");

        // The Rust engine must reproduce llama.cpp's own reference output (same weights, prompt, greedy).
        assert!(match_expected >= 57, "Rust engine reproduced only {match_expected}/60 llama-mtmd-cli outputs");
    }

    #[test]
    fn test_voxtral_summarize() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let voxtral_dir = models_dir.join(VOXTRAL_DIR);
        if !voxtral_dir.join(MODEL_NAME).exists()
            || !voxtral_dir.join(MMPROJ_NAME).exists()
            || !voxtral_dir.join(LORA_NAME).exists()
        {
            eprintln!("skipping: Voxtral models not present");
            return;
        }

        let raw = "Okay so for the product launch next week, here is where we stand. \
            Marketing finished the landing page yesterday, and Sarah confirmed the email \
            campaign goes out Tuesday morning at 9 AM. Engineering is still working on \
            the payment integration bug, that is the main blocker right now, David thinks \
            it will be fixed by Monday but he is not fully sure. We also need someone to review \
            the pricing page copy before Tuesday, I think Priya should do that since she \
            wrote the original draft.";
        let eng = engine(&models_dir, false).expect("engine loads");
        let summary = eng.summarize(raw).expect("summarize should succeed");
        println!("Voxtral summary:\n{}", summary);
        assert!(!summary.is_empty());
        let lower = summary.to_lowercase();
        assert!(lower.contains("tuesday") || lower.contains("monday") || lower.contains("launch"));
    }

    #[test]
    fn test_voxtral_touch_up() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let voxtral_dir = models_dir.join(VOXTRAL_DIR);
        if !voxtral_dir.join(MODEL_NAME).exists()
            || !voxtral_dir.join(MMPROJ_NAME).exists()
            || !voxtral_dir.join(LORA_NAME).exists()
        {
            eprintln!("skipping: Voxtral models not present");
            return;
        }

        let raw = "so the the meeting is at 5 PM we need to finish the report before then \
            can you send it over";
        let eng = engine(&models_dir, false).expect("engine loads");
        let touched = eng.touch_up(raw, "Standard").expect("touch up should succeed");
        println!("Voxtral touch up: {:?}", touched);
        assert!(!touched.is_empty());
        assert!(crate::rulebooks::faithfulness::is_word_subsequence(&touched, raw));
    }

    #[test]
    fn test_task2_benchmark_measurements() {
        let models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let voxtral_dir = models_dir.join(VOXTRAL_DIR);
        let whisper_dir = models_dir.join("whisper");
        if !voxtral_dir.join(MODEL_NAME).exists()
            || !voxtral_dir.join(MMPROJ_NAME).exists()
            || !voxtral_dir.join(LORA_NAME).exists()
            || !whisper_dir.join("encoder_model_int8.onnx").exists()
        {
            eprintln!("skipping: Voxtral or Whisper models not present");
            return;
        }

        let audio_path = voxtral_dir.join("golden/session-1790795120901.wav");
        let base_samples = if audio_path.exists() {
            read_wav(&audio_path)
        } else {
            read_wav(&models_dir.join("../tests/fixtures/real_speech_sample.wav"))
        };

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

        fn append_debug_log(line: &str) {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let p = std::path::PathBuf::from(appdata).join("app.ivy.dictation").join("debug.log");
                let _ = std::fs::create_dir_all(p.parent().unwrap());
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                    use std::io::Write;
                    let _ = writeln!(f, "[{ts}] {line}");
                }
            }
        }

        fn median_ms(mut vals: Vec<u128>) -> u128 {
            vals.sort();
            vals[vals.len() / 2]
        }

        struct BenchResult {
            engine: &'static str,
            backend: &'static str,
            cold_load_ms: u128,
            peak_vram_mb: u64,
            peak_ram_mb: u64,
            time_5s_ms: u128,
            time_15s_ms: u128,
            time_30s_ms: u128,
        }

        let mut results = Vec::new();

        // 1. Voxtral GPU (Vulkan)
        println!("\n>>> Benchmarking Voxtral GPU (Vulkan)...");
        unload_engine();
        let (_, ram_baseline) = get_mem_mb();
        let vram_baseline = crate::gpu_monitor::query_gpu_telemetry().used_vram_mb;

        let t0 = Instant::now();
        let vox_gpu = engine(&models_dir, false).expect("load Voxtral GPU");
        let cold_load_vox_gpu = t0.elapsed().as_millis();
        let vram_post_load = crate::gpu_monitor::query_gpu_telemetry().used_vram_mb;
        let (_, ram_post_load) = get_mem_mb();
        let vox_gpu_vram = vram_post_load.saturating_sub(vram_baseline);
        let vox_gpu_ram = ram_post_load.saturating_sub(ram_baseline);

        // Warm up
        let _ = vox_gpu.transcribe(&clip_5s, &[], Duration::from_secs(30));

        // 5s clip (3 runs)
        let mut times_5s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = vox_gpu.transcribe(&clip_5s, &[], Duration::from_secs(30)).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("voxtral ok via GPU (Vulkan) in {ms}ms, {} chars", res.chars().count()));
            times_5s.push(ms);
        }
        let med_5s = median_ms(times_5s);

        // 15s clip (3 runs)
        let mut times_15s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = vox_gpu.transcribe(&clip_15s, &[], Duration::from_secs(45)).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("voxtral ok via GPU (Vulkan) in {ms}ms, {} chars", res.chars().count()));
            times_15s.push(ms);
        }
        let med_15s = median_ms(times_15s);

        // 30s clip (3 runs)
        let mut times_30s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = vox_gpu.transcribe(&clip_30s, &[], Duration::from_secs(60)).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("voxtral ok via GPU (Vulkan) in {ms}ms, {} chars", res.chars().count()));
            times_30s.push(ms);
        }
        let med_30s = median_ms(times_30s);
        unload_engine();

        results.push(BenchResult {
            engine: "Voxtral Mini 3B",
            backend: "GPU (Vulkan)",
            cold_load_ms: cold_load_vox_gpu,
            peak_vram_mb: vox_gpu_vram,
            peak_ram_mb: vox_gpu_ram,
            time_5s_ms: med_5s,
            time_15s_ms: med_15s,
            time_30s_ms: med_30s,
        });

        // 2. Voxtral CPU
        println!("\n>>> Benchmarking Voxtral CPU...");
        unload_engine();
        let (_, ram_baseline) = get_mem_mb();
        let t0 = Instant::now();
        let vox_cpu = engine(&models_dir, true).expect("load Voxtral CPU");
        let cold_load_vox_cpu = t0.elapsed().as_millis();
        let (_, ram_post_load) = get_mem_mb();
        let vox_cpu_ram = ram_post_load.saturating_sub(ram_baseline);

        // Warm up
        let _ = vox_cpu.transcribe(&clip_5s, &[], Duration::from_secs(60));

        // 5s clip (3 runs)
        let mut times_5s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = vox_cpu.transcribe(&clip_5s, &[], Duration::from_secs(60)).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("voxtral ok via CPU in {ms}ms, {} chars", res.chars().count()));
            times_5s.push(ms);
        }
        let med_5s = median_ms(times_5s);

        // 15s clip (3 runs)
        let mut times_15s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = vox_cpu.transcribe(&clip_15s, &[], Duration::from_secs(90)).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("voxtral ok via CPU in {ms}ms, {} chars", res.chars().count()));
            times_15s.push(ms);
        }
        let med_15s = median_ms(times_15s);

        // 30s clip (3 runs)
        let mut times_30s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = vox_cpu.transcribe(&clip_30s, &[], Duration::from_secs(120)).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("voxtral ok via CPU in {ms}ms, {} chars", res.chars().count()));
            times_30s.push(ms);
        }
        let med_30s = median_ms(times_30s);
        unload_engine();

        results.push(BenchResult {
            engine: "Voxtral Mini 3B",
            backend: "CPU",
            cold_load_ms: cold_load_vox_cpu,
            peak_vram_mb: 0,
            peak_ram_mb: vox_cpu_ram,
            time_5s_ms: med_5s,
            time_15s_ms: med_15s,
            time_30s_ms: med_30s,
        });

        // 3. Whisper GPU (DirectML)
        println!("\n>>> Benchmarking Whisper GPU (DirectML)...");
        crate::stt::unload_engine();
        let (_, ram_baseline) = get_mem_mb();
        let vram_baseline = crate::gpu_monitor::query_gpu_telemetry().used_vram_mb;

        let t0 = Instant::now();
        let wh_gpu = crate::stt::engine(&models_dir, true).expect("load Whisper GPU");
        let cold_load_wh_gpu = t0.elapsed().as_millis();
        let vram_post_load = crate::gpu_monitor::query_gpu_telemetry().used_vram_mb;
        let (_, ram_post_load) = get_mem_mb();
        let wh_gpu_vram = vram_post_load.saturating_sub(vram_baseline);
        let wh_gpu_ram = ram_post_load.saturating_sub(ram_baseline);

        // Warm up
        let _ = wh_gpu.transcribe_with_vocabulary(&clip_5s, &[]);

        // 5s clip (3 runs)
        let mut times_5s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = wh_gpu.transcribe_with_vocabulary(&clip_5s, &[]).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("stt ok via GPU (DirectML) in {ms}ms, {} chars", res.chars().count()));
            times_5s.push(ms);
        }
        let med_5s = median_ms(times_5s);

        // 15s clip (3 runs)
        let mut times_15s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = wh_gpu.transcribe_with_vocabulary(&clip_15s, &[]).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("stt ok via GPU (DirectML) in {ms}ms, {} chars", res.chars().count()));
            times_15s.push(ms);
        }
        let med_15s = median_ms(times_15s);

        // 30s clip (3 runs)
        let mut times_30s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = wh_gpu.transcribe_with_vocabulary(&clip_30s, &[]).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("stt ok via GPU (DirectML) in {ms}ms, {} chars", res.chars().count()));
            times_30s.push(ms);
        }
        let med_30s = median_ms(times_30s);
        crate::stt::unload_engine();

        results.push(BenchResult {
            engine: "Whisper large-v3-turbo",
            backend: "GPU (DirectML)",
            cold_load_ms: cold_load_wh_gpu,
            peak_vram_mb: wh_gpu_vram,
            peak_ram_mb: wh_gpu_ram,
            time_5s_ms: med_5s,
            time_15s_ms: med_15s,
            time_30s_ms: med_30s,
        });

        // 4. Whisper CPU
        println!("\n>>> Benchmarking Whisper CPU...");
        crate::stt::unload_engine();
        let (_, ram_baseline) = get_mem_mb();
        let t0 = Instant::now();
        let wh_cpu = crate::stt::engine(&models_dir, false).expect("load Whisper CPU");
        let cold_load_wh_cpu = t0.elapsed().as_millis();
        let (_, ram_post_load) = get_mem_mb();
        let wh_cpu_ram = ram_post_load.saturating_sub(ram_baseline);

        // Warm up
        let _ = wh_cpu.transcribe_with_vocabulary(&clip_5s, &[]);

        // 5s clip (3 runs)
        let mut times_5s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = wh_cpu.transcribe_with_vocabulary(&clip_5s, &[]).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("stt ok via CPU in {ms}ms, {} chars", res.chars().count()));
            times_5s.push(ms);
        }
        let med_5s = median_ms(times_5s);

        // 15s clip (3 runs)
        let mut times_15s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = wh_cpu.transcribe_with_vocabulary(&clip_15s, &[]).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("stt ok via CPU in {ms}ms, {} chars", res.chars().count()));
            times_15s.push(ms);
        }
        let med_15s = median_ms(times_15s);

        // 30s clip (3 runs)
        let mut times_30s = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let res = wh_cpu.transcribe_with_vocabulary(&clip_30s, &[]).unwrap();
            let ms = t.elapsed().as_millis();
            append_debug_log(&format!("stt ok via CPU in {ms}ms, {} chars", res.chars().count()));
            times_30s.push(ms);
        }
        let med_30s = median_ms(times_30s);
        crate::stt::unload_engine();

        results.push(BenchResult {
            engine: "Whisper large-v3-turbo",
            backend: "CPU",
            cold_load_ms: cold_load_wh_cpu,
            peak_vram_mb: 0,
            peak_ram_mb: wh_cpu_ram,
            time_5s_ms: med_5s,
            time_15s_ms: med_15s,
            time_30s_ms: med_30s,
        });

        println!("\n================================== BENCHMARK MEASUREMENTS ==================================");
        println!("| Engine | Backend | Cold Load | Peak VRAM | Peak RAM | 5s Clip (med) | 15s Clip (med) | 30s Clip (med) |");
        println!("|---|---|---|---|---|---|---|---|");
        for r in &results {
            println!(
                "| {} | {} | {:.2} s | {} MB | {} MB | {:.2} s | {:.2} s | {:.2} s |",
                r.engine,
                r.backend,
                r.cold_load_ms as f64 / 1000.0,
                r.peak_vram_mb,
                r.peak_ram_mb,
                r.time_5s_ms as f64 / 1000.0,
                r.time_15s_ms as f64 / 1000.0,
                r.time_30s_ms as f64 / 1000.0,
            );
        }
        println!("============================================================================================\n");
    }
}
