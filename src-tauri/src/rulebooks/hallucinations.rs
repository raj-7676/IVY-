//! Book 1 - HALLUCINATIONS. Text that was never spoken must never be pasted.
//!
//! A hallucination is any word in the output with no matching speech in the audio. It is the worst
//! failure a dictation app can have: a mishearing looks like a typo, a hallucination looks like the
//! user wrote something they never said. Every other book is subordinate to this one.
//!
//! What the research says (sources in RULEBOOKS.md):
//! - Whisper invents text mostly on NON-SPEECH: silence, pauses, noise, music, breathing.
//!   On pure non-speech audio it hallucinated in 40% of clips; two-thirds of those were a small set of
//!   recurring phrases ("Thank you." 25%, "Thanks for watching" 10%) [Baranski et al., ICASSP 2025].
//! - Long pauses inside speech raise the hallucination rate [Careless Whisper, FAccT 2024]; very short
//!   (about 1 s) and very long (30 s) segments are the worst [ICASSP 2025].
//! - Whisper's own no-speech threshold barely helps on its own [Hallucination Space Projection, 2026].
//! - What works: voice-activity gating before recognition (Silero VAD: hallucinations 40% -> 0.2%),
//!   then removing loops and the "bag of hallucinations" after it (WER 104% -> 6.5% on noisy audio).
//! - LLM / audio-LLM cleanup adds a second kind: answering, swapping words, inventing numbers.
//!   The fix is verification against what was heard, never trusting the model [3-stage verify, 2025].
//!
//! Rules. Stage A runs on the AUDIO before recognition, stage B on the model's text:
//!   A1 Silence in, nothing out: under 0.25 s of voiced audio -> no transcription at all.
//!   A2 Trim leading and trailing non-speech (keep 0.3 s around the speech).
//!   A3 Shorten every pause longer than 1.5 s to 0.6 s (pauses are where hallucinations grow).
//!      Conservative on purpose: a quiet word must never be cut, so the voice threshold is only 6 dB
//!      over the room's own noise floor.
//!   B1 Bag of hallucinations, certain: subscription / outro / caption-credit phrases
//!      ("thanks for watching", "please subscribe", "subtitles by ...") are removed as whole sentences.
//!   B2 Bag of hallucinations, ambiguous ("Thank you.", "Bye.", "You", "Okay."): removed only when it
//!      is the entire output AND under 0.8 s of voice was heard - a real "Thank you." has real voice.
//!   B3 Loops: a phrase of 2-8 words repeated 3+ times in a row is kept once.
//!   B4 Speaking rate: more words than 7 per voiced second is impossible speech; trailing sentences
//!      are dropped until the rate is plausible (hallucinations attach at the end, in the pauses).
//! What to KNOW (do not "fix" these into hallucinations):
//!   - Never fill a gap: if audio is unclear, write what was heard or nothing, never a guess.
//!   - Never complete a sentence the speaker abandoned ("I want pizza and" stays short).
//!   - Never translate, summarize or answer during dictation.
//!   - A repeated word the speaker meant ("no, no, no") is not a loop: loops need 2+ word phrases x3.

/// Voiced-audio facts from stage A that stage B needs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VoiceStats {
    pub voiced_secs: f32,
    pub total_secs: f32,
}

const FRAME_SECS: f32 = 0.03;

/// Per-frame voice decision with a noise-adaptive energy threshold: a frame is voiced when it is
/// 6 dB above the recording's own noise floor (10th percentile) and above an absolute -50 dBFS.
/// Dependency-free stand-in for Silero VAD (the measured best choice; upgrade path in RULEBOOKS.md).
pub fn voiced_frames(samples: &[f32], sample_rate: u32) -> Vec<bool> {
    let n = ((sample_rate as f32 * FRAME_SECS) as usize).max(1);
    let rms: Vec<f32> = samples
        .chunks(n)
        .map(|f| (f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt())
        .collect();
    if rms.is_empty() {
        return Vec::new();
    }
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = sorted[sorted.len() / 10];
    let threshold = (floor * 2.0).max(0.003);
    let raw: Vec<bool> = rms.iter().map(|&r| r > threshold).collect();
    // hangover: keep 2 frames after speech so word endings (s, t, f) are not cut
    let mut out = raw.clone();
    for i in 0..raw.len() {
        if raw[i] {
            for k in 1..=2 {
                if i + k < out.len() {
                    out[i + k] = true;
                }
            }
        }
    }
    out
}

/// Stage A. Returns the audio to recognize (empty = silence, transcribe nothing) and its stats.
pub fn prepare_audio(samples: &[f32], sample_rate: u32) -> (Vec<f32>, VoiceStats) {
    let n = ((sample_rate as f32 * FRAME_SECS) as usize).max(1);
    let voiced = voiced_frames(samples, sample_rate);
    let voiced_secs = voiced.iter().filter(|&&v| v).count() as f32 * FRAME_SECS;
    let stats = VoiceStats { voiced_secs, total_secs: samples.len() as f32 / sample_rate as f32 };
    // A1
    if voiced_secs < 0.25 {
        return (Vec::new(), stats);
    }
    // A2
    let first = voiced.iter().position(|&v| v).unwrap_or(0);
    let last = voiced.iter().rposition(|&v| v).unwrap_or(voiced.len() - 1);
    let margin = (0.3 / FRAME_SECS) as usize;
    let (start_f, end_f) = (first.saturating_sub(margin), (last + margin + 1).min(voiced.len()));
    // A3
    let max_gap = (1.5 / FRAME_SECS) as usize;
    let keep_gap = (0.6 / FRAME_SECS) as usize;
    let mut out: Vec<f32> = Vec::with_capacity(samples.len());
    let mut f = start_f;
    while f < end_f {
        if !voiced[f] {
            let run_end = (f..end_f).find(|&k| voiced[k]).unwrap_or(end_f);
            let run = run_end - f;
            let keep = if run > max_gap { keep_gap } else { run };
            // keep the middle of a long pause: the edges hold breath and word tails
            let skip_front = (run - keep) / 2;
            for k in f + skip_front..f + skip_front + keep {
                let s = k * n;
                out.extend_from_slice(&samples[s.min(samples.len())..((k + 1) * n).min(samples.len())]);
            }
            f = run_end;
        } else {
            let s = f * n;
            out.extend_from_slice(&samples[s.min(samples.len())..((f + 1) * n).min(samples.len())]);
            f += 1;
        }
    }
    (out, stats)
}

/// B1: never dictated by a real user; Whisper's training data (YouTube captions) leaking through.
const BAG_CERTAIN: &[&str] = &[
    "thanks for watching", "thank you for watching", "thank you so much for watching", "thanks for watching and",
    "please subscribe", "please like and subscribe", "like and subscribe", "don't forget to subscribe",
    "subscribe to my channel", "see you in the next video", "see you next time", "subtitles by",
    "subtitles by the amara org community", "captions by", "transcribed by", "transcription by",
    "translated by", "amara org", "www mooji org", "thank you for listening", "thanks for listening",
    "i'll see you in the next one", "bye bye", "music", "applause", "laughter",
];
/// B2: real phrases too, so only removed when they are everything AND there was almost no voice.
const BAG_AMBIGUOUS: &[&str] = &["thank you", "thanks", "bye", "you", "okay", "ok", "oh", "so", "hmm", "yeah", "the end"];

fn norm_sentence(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits into sentences, keeping each sentence's own punctuation.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let end = matches!(c, '.' | '?' | '!') && chars.get(i + 1).map_or(true, |n| n.is_whitespace());
        if end || c == '\n' {
            if !cur.trim().is_empty() {
                out.push(cur.trim().to_string());
            }
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// B3: a 2-8 word phrase repeated 3+ times back to back is kept once.
fn deloop(text: &str) -> String {
    let mut toks: Vec<&str> = text.split_whitespace().collect();
    let key = |t: &str| t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let mut changed = true;
    while changed {
        changed = false;
        'outer: for n in 2..=8 {
            if toks.len() < 3 * n {
                continue;
            }
            for i in 0..=toks.len() - 3 * n {
                let a: Vec<String> = toks[i..i + n].iter().map(|t| key(t)).collect();
                if a.iter().any(|w| w.is_empty()) {
                    continue;
                }
                let mut reps = 1;
                while i + (reps + 1) * n <= toks.len()
                    && toks[i + reps * n..i + (reps + 1) * n].iter().map(|t| key(t)).collect::<Vec<_>>() == a
                {
                    reps += 1;
                }
                if reps >= 3 {
                    toks.drain(i + n..i + reps * n);
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
    toks.join(" ")
}

/// Stage B: the speech recognizer's text, given what stage A measured.
pub fn clean_asr_text(text: &str, stats: VoiceStats) -> String {
    if stats.voiced_secs < 0.25 {
        return String::new();
    }
    // B1
    let kept: Vec<String> = sentences(text)
        .into_iter()
        .filter(|s| {
            let n = norm_sentence(s);
            !n.is_empty() && !BAG_CERTAIN.contains(&n.as_str())
        })
        .collect();
    let mut t = kept.join(" ");
    // B2
    let whole = norm_sentence(&t);
    if BAG_AMBIGUOUS.contains(&whole.as_str()) && stats.voiced_secs < 0.8 {
        return String::new();
    }
    // B3
    t = deloop(&t);
    // B4
    let max_words = (stats.voiced_secs * 7.0).ceil() as usize + 2;
    let mut parts = sentences(&t);
    while parts.len() > 1 && parts.iter().map(|s| s.split_whitespace().count()).sum::<usize>() > max_words {
        parts.pop();
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(secs: f32) -> VoiceStats {
        VoiceStats { voiced_secs: secs, total_secs: secs + 1.0 }
    }

    #[test]
    fn removes_known_hallucinations() {
        assert_eq!(clean_asr_text("Send the report to Ana. Thanks for watching!", speech(2.5)), "Send the report to Ana.");
        assert_eq!(clean_asr_text("Subtitles by the Amara.org community", speech(1.0)), "");
        assert_eq!(clean_asr_text("Thank you.", speech(0.4)), "");
        assert_eq!(clean_asr_text("Please call me back.", speech(0.1)), "");
    }

    #[test]
    fn keeps_real_speech() {
        assert_eq!(clean_asr_text("Thank you.", speech(1.2)), "Thank you.");
        assert_eq!(clean_asr_text("No, no, no, Tuesday.", speech(1.5)), "No, no, no, Tuesday.");
        assert_eq!(clean_asr_text("Thank you for the update, I'll check it.", speech(2.5)), "Thank you for the update, I'll check it.");
        assert_eq!(clean_asr_text("We can do it. We can do it.", speech(3.0)), "We can do it. We can do it.");
    }

    #[test]
    fn loops_and_rate() {
        assert_eq!(clean_asr_text("call the bank call the bank call the bank call the bank today", speech(3.0)), "call the bank today");
        // 1 s of voice cannot hold two long sentences: the trailing one is dropped
        assert_eq!(
            clean_asr_text("Book it. And then we went to the market and bought a lot of things for the party.", speech(1.0)),
            "Book it."
        );
    }

    #[test]
    fn audio_stage() {
        let sr = 16000;
        let silence = vec![0.0001f32; sr as usize * 2];
        let (out, st) = prepare_audio(&silence, sr);
        assert!(out.is_empty() && st.voiced_secs < 0.25);
        // 0.5 s silence, 1 s tone, 3 s silence, 1 s tone, 0.5 s silence
        let tone = |secs: f32| (0..(sr as f32 * secs) as usize).map(|i| (i as f32 * 0.07).sin() * 0.2).collect::<Vec<f32>>();
        let gap = |secs: f32| vec![0.0005f32; (sr as f32 * secs) as usize];
        let audio: Vec<f32> = [gap(0.5), tone(1.0), gap(3.0), tone(1.0), gap(0.5)].concat();
        let (out, st) = prepare_audio(&audio, sr);
        let secs = out.len() as f32 / sr as f32;
        assert!(st.voiced_secs > 1.9 && st.voiced_secs < 2.3, "voiced {}", st.voiced_secs);
        assert!(secs > 2.7 && secs < 3.6, "kept {secs} s (2 s speech + 0.6 s pause + margins)");
    }
}
