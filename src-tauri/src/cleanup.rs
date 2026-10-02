use std::path::Path;

use crate::rulebooks::{self, Tone};

pub fn unload_engine() {
    // No-op: Whisper fallback is purely deterministic; Voxtral manages its own lifecycle.
}

/// Entry point for transcript cleanup when running Whisper fallback (after the Hallucinations
/// book already cleaned the recognizer's text in `lib.rs::transcribe_and_clean`).
/// Runs the deterministic rulebooks.
pub fn clean_transcript(
    _models_dir: &Path,
    raw: &str,
    tone: &str,
    _is_cpu_mode: bool,
    dictation_mode: &str,
) -> String {
    let stripped = raw.replace(['♪', '♫', '🎵', '🎶'], "");
    let raw = stripped.trim();
    if raw.is_empty() {
        return String::new();
    }
    let is_speed_mode = dictation_mode.eq_ignore_ascii_case("speed");
    let tone = Tone::from_label(tone);
    rulebooks::deterministic(raw, tone, is_speed_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_on_the_rules_path() {
        let models_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let raw = "um I'm gonna go cuz I wanna see it, like, right now period";
        assert_eq!(clean_transcript(&models_dir, raw, "Casual", true, "accuracy"), "I'm gonna go cuz I wanna see it, like, right now.");
        assert_eq!(clean_transcript(&models_dir, raw, "Standard", true, "speed"), "I'm gonna go cuz I wanna see it, like, right now.");
        assert_eq!(clean_transcript(&models_dir, raw, "Professional", true, "accuracy"), "I'm going to go because I want to see it right now.");
        assert_eq!(clean_transcript(&models_dir, "they was talking on monday", "Standard", true, "speed"), "They was talking on Monday");
    }

    #[test]
    fn rules_path_formats_without_changing_words() {
        let models_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        let out = clean_transcript(&models_dir, "send twenty percent of five hundred rupees to ana at gmail dot com by seven pm", "Standard", true, "accuracy");
        assert_eq!(out, "Send 20% of ₹500 to ana@gmail.com by 7 PM");
        let keep = clean_transcript(&models_dir, "I need you to call me to check the trial period ended, I think so.", "Standard", true, "accuracy");
        assert_eq!(keep, "I need you to call me to check the trial period ended, I think so.");
    }
}
