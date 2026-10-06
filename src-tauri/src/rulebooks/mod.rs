//! Ivy's rulebooks: every deterministic text rule Ivy runs, in eight books.
//!
//! The speech model (Ivy's lite model, Qwen3-ASR-1.7B fine-tuned to hear and clean in one pass) decides
//! what was MEANT. The rulebooks only (1) refuse model output that is not faithful to what was said and
//! (2) format what was said. They never guess meaning.
//!
//! Laws (every rule in every book obeys them; the tests check them):
//! 1. **Whole words only.** A rule matches whole tokens, never letters inside a word.
//! 2. **Never trade a spoken word for a different word.** The only allowed changes are: removing
//!    explicit spoken commands -> symbols (books 3 and 5), number words
//!    -> digits and big round amounts -> "18 lakhs" (book 4), casing (book 6), the tone's word swaps
//!    (book 7: Standard and Professional only, from fixed lists), and missing apostrophes (book 8).
//! 3. **When in doubt, leave it as spoken.** A missed format is cheap; a changed word is a hallucination.
//! 4. **Idempotent.** Running a book twice gives the same text as running it once.
//! 5. **Fixed order** (see `deterministic` and `after_model`). A book only sees what earlier books produced.
//! 6. **Fast and dependency-free** (std only): all books together stay far under 1 ms per dictation.
//! 7. **Every rule has a "does" example and a "must not touch" example** in its book's tests.
//!
//! Books:
//! 1. HALLUCINATIONS - text that was never spoken is never pasted. Outranks every other book.
//!    `hallucinations` stage A (audio: silence gate) and stage B (model text: bag of hallucinations,
//!    loops, impossible speaking rate).
//! 2. (fillers and stutters: the lite model removes them itself, so there is no book for them now.)
//! 3. `commands`     - spoken punctuation, line breaks, lists, quotes/brackets, emoji, hashtags.
//! 4. `numbers`      - numbers as digits; big round amounts as lakhs/crores/millions; money, percent, time, dates, units.
//! 5. `tech`         - links, emails, file names, code casing, operators, shortcuts, ports.
//! 6. `names`        - brands, acronyms, days, months, holidays, languages, time zones, sentence caps.
//! 7. `tone`         - Casual keeps the speaker's wording; Standard expands slang; Professional is formal.
//! 8. `typography`   - spacing, punctuation hygiene, apostrophes, a dangling final "and".
//!
//! Standalone test (no app build needed): `rustc --edition 2021 --test src/rulebooks/mod.rs -o rb.exe && ./rb.exe`

pub mod commands;
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

/// Formatting after the lite model. The model already removed fillers and applied the speaker's
/// self-corrections from the audio, so book 2 does not run; the tone (book 7) and formatting do.
pub fn after_model(cleaned: &str, tone: Tone) -> String {
    let mut t = commands::apply(cleaned);
    t = tone::apply(&t, tone);
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
    }

    #[test]
    fn whole_pipeline_on_realistic_dictation() {
        let raw = "so book a table for two at seven thirty pm at the thai place period new line \
                   also email john dot doe at gmail dot com the pdf";
        let out = after_model(raw, Tone::Standard);
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
            assert_eq!(after_model(s, Tone::Standard), s, "rulebooks changed a spoken word in {s:?}");
        }
        // numbers become digits (Yash's rule), but "times" is not turned into a multiplication sign
        assert_eq!(after_model("Take it two times a day.", Tone::Standard), "Take it 2 times a day.");
    }

    #[test]
    fn every_book_is_idempotent() {
        let samples = [
            "book two tickets for nine pm comma then email ana at outlook dot com new line done period",
            "the meeting is on march twenty first twenty twenty six at 5 pm, i think so",
            "she said open quote hello close quote and twenty percent of five hundred rupees",
        ];
        for s in samples {
            for t in [Tone::Casual, Tone::Standard, Tone::Professional] {
                let once = after_model(s, t);
                assert_eq!(after_model(&once, t), once, "after_model not idempotent for {s:?}");
            }
        }
    }
}
