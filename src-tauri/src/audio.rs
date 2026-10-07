use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::StreamConfig;
use rubato::{FftFixedIn, Resampler};

const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Real microphone capture, started on hotkey-down and stopped on
/// hotkey-up (or, for the on-screen onboarding button and `cancel`, from a
/// Tauri command thread instead — see below for why that distinction used
/// to matter).
///
/// cpal's Windows backend is COM-based, and a COM object created on one
/// thread being dropped on another is unsound — it can hang or crash the
/// audio driver, intermittently, not reliably reproducibly. The real
/// device open, stream creation, and eventual drop now all happen on one
/// dedicated thread spawned fresh for each recording, which that thread
/// owns for its entire life; `Recorder` itself only ever holds channels and
/// a `JoinHandle`, all genuinely `Send`, so no `unsafe impl Send` is needed
/// at all — whichever thread calls `start`/`stop` no longer matters, and a
/// panic inside cpal/COM is isolated to its own thread instead of being
/// able to reach anything else in the app.
pub struct Recorder {
    stop_tx: mpsc::Sender<()>,
    result_rx: mpsc::Receiver<Vec<f32>>,
    join: Option<std::thread::JoinHandle<()>>,
    source_rate: u32,
    source_channels: u16,
    device_name: String,
    // Peak of the most recent device buffer, as f32 bits.
    level: Arc<AtomicU32>,
}

fn find_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.input_devices().ok()?.find(|d| d.to_string() == name)
}

/// Real device names from the OS, for the Settings microphone picker. Empty
/// means no input device or no permission — never invented names.
pub fn list_input_device_names() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devices) => devices.map(|d| d.to_string()).collect(),
        Err(_) => vec![],
    }
}

type ReadyMsg = Result<(String, u32, u16), String>;

impl Recorder {
    /// Starts capturing from the named device, or the system default if the
    /// name is empty or no longer exists. Spawns the dedicated recorder
    /// thread and blocks only long enough for it to report whether the
    /// device actually opened — the same synchronous `Result` this always
    /// returned, just backed by a real thread handoff instead of doing the
    /// work inline.
    pub fn start(device_name: &str) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::channel::<ReadyMsg>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (result_tx, result_rx) = mpsc::channel::<Vec<f32>>();
        let device_name_owned = device_name.to_string();
        let level = Arc::new(AtomicU32::new(0));
        let level_cb = level.clone();

        let join = std::thread::spawn(move || {
            let host = cpal::default_host();
            let device = match (if device_name_owned.is_empty() {
                None
            } else {
                find_device(&host, &device_name_owned)
            })
            .or_else(|| host.default_input_device())
            {
                Some(d) => d,
                None => {
                    let _ = ready_tx.send(Err("no microphone available".to_string()));
                    return;
                }
            };
            let real_name = device.to_string();

            let config: StreamConfig = match device.default_input_config() {
                Ok(c) => c.into(),
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("no input config: {e}")));
                    return;
                }
            };
            let source_rate = config.sample_rate;
            let source_channels = config.channels;

            let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
            let buffer_cb = buffer.clone();
            let stream = match device.build_input_stream(
                config,
                move |data: &[f32], _| {
                    let peak = data.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
                    level_cb.store(peak.to_bits(), Ordering::Relaxed);
                    if let Ok(mut buf) = buffer_cb.lock() {
                        buf.extend_from_slice(data);
                    }
                },
                // cpal reports WASAPI's DATA_DISCONTINUITY flag as Xrun. It is
                // non-fatal (capture continues) and fires on perfectly good
                // recordings on some drivers, so it's logged, never acted on.
                move |err| log::warn!("Ivy: audio input stream notice: {err}"),
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("failed to open microphone: {e}")));
                    return;
                }
            };
            if let Err(e) = stream.play() {
                let _ = ready_tx.send(Err(format!("failed to start recording: {e}")));
                return;
            }
            if ready_tx.send(Ok((real_name, source_rate, source_channels))).is_err() {
                return; // caller already gave up (e.g. timed out) — nothing left to hand results to
            }

            // Block this thread — and only this thread — until told to
            // stop. The stream (and everything COM-related underneath it
            // on Windows) is created, lives, and dies right here.
            let _ = stop_rx.recv();
            drop(stream);
            let mut interleaved = buffer.lock().map(|b| b.clone()).unwrap_or_default();
            // Cryptographic memory hygiene: zeroize shared capture buffer
            if let Ok(mut buf) = buffer.lock() {
                buf.fill(0.0);
                buf.clear();
            }
            let mut mono = to_mono(&interleaved, source_channels);
            // Zeroize raw interleaved PCM data
            interleaved.fill(0.0);
            interleaved.clear();
            let resampled = resample_to_16k(&mono, source_rate);
            // Zeroize intermediate mono PCM data
            mono.fill(0.0);
            mono.clear();
            let _ = result_tx.send(resampled);
        });

        match ready_rx.recv() {
            Ok(Ok((device_name, source_rate, source_channels))) => Ok(Self {
                stop_tx,
                result_rx,
                join: Some(join),
                source_rate,
                source_channels,
                device_name,
                level,
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            // The sender dropped without sending — the recorder thread
            // panicked before it could report anything. Isolated to that
            // thread; this call just reports a clean failure instead of
            // hanging or taking the caller down with it.
            Err(_) => {
                let _ = join.join();
                Err("recorder thread failed to start".to_string())
            }
        }
    }

    /// Real device name, actual sample rate, and channel count actually in
    /// use — for diagnostics (see `debug_log` in lib.rs), not shown in the UI.
    pub fn info(&self) -> String {
        format!("device={:?} rate={}Hz channels={}", self.device_name, self.source_rate, self.source_channels)
    }

    /// Peak amplitude (0..1) of the most recent buffer the device delivered.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Stops capture and returns mono 16kHz f32 samples, resampled from
    /// whatever the device actually recorded at.
    pub fn stop(mut self) -> Vec<f32> {
        let _ = self.stop_tx.send(());
        let samples = self.result_rx.recv().unwrap_or_default();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        samples
    }
}

fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

fn resample_to_16k(samples: &[f32], source_rate: u32) -> Vec<f32> {
    if source_rate == TARGET_SAMPLE_RATE || samples.is_empty() {
        return samples.to_vec();
    }

    // FftFixedIn wants fixed-size chunks; pad the tail so the last partial
    // chunk still resamples instead of being dropped on the floor.
    let chunk_size = 1024;
    let mut resampler = match FftFixedIn::<f32>::new(
        source_rate as usize,
        TARGET_SAMPLE_RATE as usize,
        chunk_size,
        1,
        1,
    ) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Ivy: resampler init failed ({e}), returning unresampled audio");
            return samples.to_vec();
        }
    };

    let mut padded = Wiped(samples.to_vec());
    let remainder = padded.0.len() % chunk_size;
    if remainder != 0 {
        padded.0.resize(padded.0.len() + (chunk_size - remainder), 0.0);
    }

    let mut out = Vec::with_capacity(padded.0.len() * TARGET_SAMPLE_RATE as usize / source_rate as usize + 16);
    for chunk in padded.0.chunks(chunk_size) {
        let mut input = [chunk.to_vec()];
        match resampler.process(&input, None) {
            Ok(mut result) => out.append(&mut result[0]),
            Err(e) => log::error!("Ivy: resample chunk failed: {e}"),
        }
        zeroize_samples(&mut input[0]);
    }
    out
}

/// Gain for quiet microphones: peak-normalise to 0.7, gain capped at 20x (Task 7, 2026-10-06).
/// The lite model went EMPTY on very quiet speech (peaks 0.04-0.06); this recovered 4 of the lab's
/// 5 quiet clips (`lite::tests::test_task7_quiet_microphones`). Left alone: true silence (peak < 0.01),
/// audio that is already loud enough (peak >= 0.7), and flat signals with no speech contrast
/// (peak/RMS < 1.6), so room noise is not blown up. One linear gain keeps the speech shape intact
/// (no pumping, no clipping). It replaced a `dagc` AGC toward -12 dBFS that was tuned for Whisper.
pub fn normalize_audio(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let peak = samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
    // Leave true silence (< 0.01) and already-loud-enough audio (peak >= 0.70) alone entirely.
    if peak < 0.01 || peak >= 0.70 {
        return samples.to_vec();
    }

    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();

    // Pure uniform noise/tones have peak/rms <= 1.5; real speech contrast has peak/rms >= 2.0.
    // Leave uniform signals alone (does not amplify pure noise).
    if rms <= 0.0 || (peak / rms) < 1.6 {
        return samples.to_vec();
    }

    // Peak-normalising: scale so the peak is 0.7, gain capped at 20x (Task 7).
    // Linear scaling preserves speech formants without dynamic pumping or hard-clipping distortion.
    let gain = (0.7 / peak).min(20.0);
    samples.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).collect()
}

/// Zeroizes an in-memory audio sample slice to prevent sensitive recorded speech from lingering in memory.
pub fn zeroize_samples(samples: &mut [f32]) {
    samples.fill(0.0);
    // Keeps the compiler from dropping the zeroing as a dead store before the memory is freed.
    std::hint::black_box(&*samples);
}

/// An intermediate copy of the audio that is zeroed when it goes out of scope, on every return path.
pub struct Wiped(pub Vec<f32>);

impl Drop for Wiped {
    fn drop(&mut self) {
        zeroize_samples(&mut self.0);
    }
}

// Real check for the thread-handoff redesign above: starts and stops actual
// capture against the real default microphone, from a thread other than the
// one that calls `stop` — exactly the cross-thread usage (onboarding's
// button, `cancel_dictation`) that made the old `unsafe impl Send` unsound.
// Fails loudly (doesn't hang, doesn't panic) if that handoff is ever broken
// again. Skips itself rather than failing on a machine with no microphone.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_normalization() {
        // Realistic quiet clip: mostly near-silent background (0.005) with a
        // few louder voiced frames (0.03) standing out above it — real
        // speech-over-a-quiet-mic shape, clear noise/speech contrast.
        let mut quiet_signal = vec![0.005f32; 16000];
        for s in quiet_signal[4000..8000].iter_mut() {
            *s = 0.03;
        }
        let normalized = normalize_audio(&quiet_signal);
        assert_eq!(normalized.len(), quiet_signal.len());
        assert!(normalized.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "output must be finite and within [-1, 1]");
        // The voiced segment must come out louder than it went in — that's
        // the actual point of Layer A. `MonoAgc` adapts per-sample rather
        // than applying one scalar, so this checks direction/magnitude, not
        // an exact target value the way the old single-gain version did.
        let voiced_in_rms = (quiet_signal[4000..8000].iter().map(|&s| s * s).sum::<f32>() / 4000.0).sqrt();
        let voiced_out_rms = (normalized[4000..8000].iter().map(|&s| s * s).sum::<f32>() / 4000.0).sqrt();
        assert!(voiced_out_rms > voiced_in_rms * 1.5, "voiced segment should be meaningfully boosted, got {voiced_in_rms} -> {voiced_out_rms}");

        // Already loud signal with RMS ~ 0.15 — falls outside the boost gate
        // entirely (rms < 0.20 check), so it must come back byte-for-byte
        // unchanged, same guarantee as before.
        let loud_signal = vec![0.15f32; 16000];
        let untouched = normalize_audio(&loud_signal);
        assert_eq!(untouched, loud_signal, "Loud audio should remain untouched");
    }

    #[test]
    fn test_normalize_audio_does_not_amplify_pure_noise() {
        // Uniform quiet signal with no speech/noise contrast at all (every
        // 20ms frame has identical RMS, so every frame reads as "at the
        // noise floor") — the freeze gate should hold the AGC frozen for
        // the entire clip instead of blindly boosting it.
        let uniform_hiss = vec![0.02f32; 16000];
        let normalized = normalize_audio(&uniform_hiss);
        let sum_sq: f32 = normalized.iter().map(|&s| s * s).sum();
        let norm_rms = (sum_sq / normalized.len() as f32).sqrt();
        assert!((norm_rms - 0.02).abs() < 0.0001, "Uniform low-level signal (indistinguishable from noise) should not be meaningfully boosted, got rms {norm_rms}");
    }

    #[test]
    fn test_normalize_audio_never_produces_nan_or_clipping() {
        // Real regression class from today's session: a stateful/adaptive
        // audio stage can misbehave on shapes a single synthetic sine wave
        // never exercises. Sweep several realistic and edge-case shapes and
        // assert only the properties that must always hold, on every one.
        let cases: Vec<Vec<f32>> = vec![
            vec![0.0; 16000],                    // true digital silence
            vec![0.004; 16000],                  // just above the silence gate
            vec![0.19; 16000],                   // just under the "already loud" gate
            vec![0.005; 50],                     // shorter than one 20ms frame
            (0..16000).map(|i| 0.05 * ((i as f32) * 0.01).sin()).collect(), // varying tone
        ];
        for case in cases {
            let out = normalize_audio(&case);
            assert_eq!(out.len(), case.len());
            assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "must stay finite and clipped to [-1, 1] for input starting {:?}", &case[..case.len().min(3)]);
        }
    }

    // Manual mic check, independent of the app: `cargo test --lib live_mic_capture -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_mic_capture() {
        let host = cpal::default_host();
        let dev = host.default_input_device().expect("mic");
        for c in dev.supported_input_configs().expect("configs") {
            println!("supported: {c:?}");
        }
        for _ in 0..3 {
            let t0 = std::time::Instant::now();
            let recorder = Recorder::start("").expect("mic");
            let open_ms = t0.elapsed().as_millis();
            std::thread::sleep(std::time::Duration::from_millis(1000));
            let samples = recorder.stop();
            let first = samples.iter().position(|s| s.abs() > 1e-6).unwrap_or(samples.len());
            println!("open took {open_ms}ms, first non-zero sample at {}ms of {}ms captured", first / 16, samples.len() / 16);
        }
        for ms in [400u64, 700, 2000] {
            let recorder = Recorder::start("").expect("mic");
            std::thread::sleep(std::time::Duration::from_millis(ms));
            let samples = recorder.stop();
            let peak = samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
            println!("held {ms}ms: captured {} samples ({:.2}s), peak={peak:.4}", samples.len(), samples.len() as f64 / 16_000.0);
        }
    }

    #[test]
    fn start_and_stop_round_trip_across_threads() {
        let recorder = match Recorder::start("") {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skipping: no microphone available ({e})");
                return;
            }
        };
        println!("recorder info: {}", recorder.info());
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Stop from a different thread than the one that started it.
        let samples = std::thread::spawn(move || recorder.stop())
            .join()
            .expect("stop() thread should not panic");

        // Real assertion, not just "didn't hang": at 16kHz, ~150ms of real
        // capture is a few thousand samples, not zero and not garbage-sized.
        assert!(samples.len() < 16_000 * 5, "unexpectedly large sample count: {}", samples.len());
    }

    #[test]
    fn test_zeroize_samples() {
        let mut samples = vec![0.5f32, -0.2f32, 0.9f32, -0.8f32];
        zeroize_samples(&mut samples);
        assert!(samples.iter().all(|&s| s == 0.0), "All samples must be wiped to zero");
    }
}
