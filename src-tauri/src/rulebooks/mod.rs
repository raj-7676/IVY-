//! Ivy's rulebooks: every deterministic text rule Ivy runs, in eight books.
//!
//! The speech model (Whisper, a cleanup AI, or a fine-tuned hear-and-clean model) decides what was
//! MEANT. The rulebooks only (1) refuse model output that is not faithful to what was said and
//! (2) format what was said. They never guess meaning.
//!
//! Laws (every rule in every book obeys them; the tests check them):
//! 1. **Whole words only.** A rule matches whole tokens, never letters inside a word.
//! 2. **Never trade a spoken word for a different word.** The only allowed changes are: removing
//!    fillers and stutters (book 2), explicit spoken commands -> symbols (books 3 and 5), number words
//!    -> digits (book 4), casing (book 6), Professional-tone slang expansion (book 7), and missing
//!    apostrophes (book 8).
//! 3. **When in doubt, leave it as spoken.** A missed format is cheap; a changed word is a hallucination.
//! 4. **Idempotent.** Running a book twice gives the same text as running it once.
//! 5. **Fixed order** (see `deterministic` and `after_ai`). A book only sees what earlier books produced.
//! 6. **Fast and dependency-free** (std only): all books together stay far under 1 ms per dictation.
//! 7. **Every rule has a "does" example and a "must not touch" example** in its book's tests.
//!
//! Books:
//! 1. HALLUCINATIONS - text that was never spoken is never pasted. Outranks every other book.
//!    `hallucinations` stage A (audio: silence gate, trim, shorten pauses) and stage B (recognizer text:
//!    bag of hallucinations, loops, impossible speaking rate); `faithfulness` stage C (any AI/model
//!    output: no new words or numbers, no answering, no silent drops, no loops, no growth or collapse).
//! 2. `disfluency`   - fillers, stutters, cut-off words, repeated phrases.
//! 3. `commands`     - spoken punctuation, line breaks, lists, quotes/brackets, emoji, hashtags.
//! 4. `numbers`      - numbers always as digits; money, percent, time, dates, units, simple math.
//! 5. `tech`         - links, emails, file names, code casing, operators, shortcuts, ports.
//! 6. `names`        - brands, acronyms, days, months, holidays, languages, time zones, sentence caps.
//! 7. `tone`         - Casual/Standard keep wording; Professional expands slang and drops crutches.
//! 8. `typography`   - spacing, punctuation hygiene, apostrophes, a dangling final "and".
//!
//! Standalone test (no app build needed): `rustc --edition 2021 --test src/rulebooks/mod.rs -o rb.exe && ./rb.exe`

pub mod commands;
pub mod disfluency;
pub mod faithfulness;
pub mod hallucinations;
pub mod names;
pub mod numbers;
pub mod tech;
pub mod tone;
pub mod typography;

/// Speaking style chosen per app (IVY.md section 8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Casual,
    Standard,
    Professional,
}

impl Tone {
    pub fn from_label(s: &str) -> Tone {
        if s.eq_ignore_ascii_case("casual") {
            Tone::Casual
        } else if s.eq_ignore_ascii_case("professional") {
            Tone::Professional
        } else {
            Tone::Standard
        }
    }
}

/// Rules-only pipeline (Speed mode, CPU Accuracy, or a rejected/failed AI pass).
/// Speed mode skips book 5 (links, files, code), which is the only formatting that needs the
/// "Accuracy" promise of reading the whole dictation carefully.
pub fn deterministic(raw: &str, tone: Tone, speed_mode: bool) -> String {
    let mut t = disfluency::apply(raw);
    t = commands::apply(&t);
    t = tone::apply(&t, tone);
    t = numbers::apply(&t);
    if !speed_mode {
        t = tech::apply(&t);
    }
    t = names::apply(&t);
    typography::apply(&t)
}

/// Formatting after an AI pass that book 1 accepted. The AI already removed fillers, applied
/// corrections and the tone, so books 2 and 7 do not run again.
pub fn after_ai(cleaned: &str) -> String {
    let mut t = commands::apply(cleaned);
    t = numbers::apply(&t);
    t = tech::apply(&t);
    t = names::apply(&t);
    typography::apply(&t)
}

// ---------------------------------------------------------------------------------------------
// Shared token helpers. A token is one whitespace-separated piece; its "core" is the token with
// leading and trailing punctuation removed (inner apostrophes, dots and hyphens stay).

/// Splits a token into (leading punctuation, core, trailing punctuation).
pub(crate) fn split3(tok: &str) -> (&str, &str, &str) {
    let start = tok.find(|c: char| c.is_alphanumeric()).unwrap_or(tok.len());
    let end = tok.rfind(|c: char| c.is_alphanumeric()).map_or(start, |i| i + tok[i..].chars().next().unwrap().len_utf8());
    if start >= end {
        return (tok, "", "");
    }
    (&tok[..start], &tok[start..end], &tok[end..])
}

/// Lowercased core of a token ("Hello," -> "hello").
pub(crate) fn core_lower(tok: &str) -> String {
    split3(tok).1.to_lowercase()
}

/// Replaces a token's core, keeping its punctuation ("five," + "5" -> "5,").
pub(crate) fn with_core(tok: &str, new_core: &str) -> String {
    let (l, _, r) = split3(tok);
    format!("{l}{new_core}{r}")
}

/// Uppercases the first letter of a word.
pub(crate) fn cap_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Applies `f` to every line separately, keeping line breaks exactly as they are.
pub(crate) fn per_line(text: &str, f: impl Fn(&str) -> String) -> String {
    text.split('\n').map(|l| f(l)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split3_keeps_inner_punctuation() {
        assert_eq!(split3("\"don't,"), ("\"", "don't", ","));
        assert_eq!(split3("e.g."), ("", "e.g", "."));
        assert_eq!(split3("..."), ("...", "", ""));
        assert_eq!(with_core("five,", "5"), "5,");
    }

    #[test]
    fn whole_pipeline_on_realistic_dictation() {
        let raw = "um so book a table for two at seven thirty pm at the thai place period new line \
                   also email john dot doe at gmail dot com the pdf";
        let out = deterministic(raw, Tone::Standard, false);
        assert_eq!(out, "So book a table for 2 at 7:30 PM at the Thai place.\nAlso email john.doe@gmail.com the PDF");
    }

    #[test]
    fn pipeline_never_invents_words_on_tricky_speech() {
        // Every one of these was mangled by the old 60-rule suite.
        let keep = [
            "I need you to call me to check the trial period ended.",
            "What should I use to cut it? I think so.",
            "You may first check the map.",
            "Ram called about the bachelor's degrees.",
            "For the first time, the two of us won.",
            "Email the invoice to Farhan in accounts.",
            "I know that that is true.",
        ];
        for s in keep {
            assert_eq!(deterministic(s, Tone::Standard, false), s, "rulebooks changed a spoken word in {s:?}");
        }
        // numbers become digits (Yash's rule), but "times" is not turned into a multiplication sign
        assert_eq!(deterministic("Take it two times a day.", Tone::Standard, false), "Take it 2 times a day.");
    }

    #[test]
    fn every_book_is_idempotent() {
        let samples = [
            "um book two tickets for nine pm comma then email ana at outlook dot com new line done period",
            "the the meeting is on march twenty first twenty twenty six at 5 pm, i think so",
            "she said open quote hello close quote and twenty percent of five hundred rupees",
        ];
        for s in samples {
            for t in [Tone::Casual, Tone::Standard, Tone::Professional] {
                let once = deterministic(s, t, false);
                assert_eq!(deterministic(&once, t, false), once, "not idempotent for {s:?}");
            }
            let once = after_ai(s);
            assert_eq!(after_ai(&once), once);
        }
    }
}
