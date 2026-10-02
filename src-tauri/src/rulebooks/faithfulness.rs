//! Book 1, stage C - Faithfulness (part of the Hallucinations book, see hallucinations.rs).
//! The gate every AI / model output must pass before Ivy pastes it.
//! `check_ai_faithful(cleaned, raw)` compares the model's text with what the speech recognizer heard.
//! On any failure Ivy pastes the rules-only version instead (`rulebooks::deterministic`).
//!
//! F1 No new words. Every output word must be a spoken word, the same word in another form
//!    (5/five, budget/budgets, don't/do not), or a small function word (a, the, to, is ...).
//!    Pronouns and negations are NOT free: "give me" -> "give you" or an added "not" is rejected.
//! F2 No new numbers. Every number in the output must have been said (as digits or words).
//! F3 No silent drops. A content word may not vanish while both its neighbours stay side by side.
//! F4 No dropped sentences. A spoken sentence with 3+ content words cannot disappear entirely.
//! F5 No answering. Output that starts like an assistant ("Sure", "Here is", "I can't") is rejected
//!    unless the speaker said those words.
//! F6 No loops. A 3-word phrase repeated 3+ times more often than it was spoken is rejected.
//! F7 No growth. Cleanup removes words; output longer than the speech (+3 words, +10%) is rejected.
//! F8 No collapse. Output that keeps under 30% of the spoken content words is rejected unless the
//!    speaker used a correction cue ("scratch that", "no wait", "actually" ...).
//! Corrections still pass: they remove a span together with its cue.
//!
//! `is_word_subsequence` (Touch Up) and `contained_ratio` (Summarize) are stricter/looser siblings.

use std::collections::HashSet;

use super::disfluency::FILLERS;

/// Words the AI may add or reshape: articles, prepositions, auxiliaries.
/// Pronouns and negations are deliberately absent.
pub const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "so", "to", "of", "in", "on", "at", "for", "with", "from", "by", "as",
    "is", "are", "was", "were", "be", "been", "am", "it", "its", "this", "that", "do", "does", "did", "have",
    "has", "had", "will", "would", "can", "could", "should", "let", "also", "then", "there", "here", "just",
    "too", "very", "well", "okay", "ok", "oh", "yes", "yeah",
];
const CRUTCHES: &[&str] = &["like", "basically", "actually", "literally", "really", "know", "mean", "right"];
const CUES: &[&str] = &[
    "scratch that", "no wait", "wait no", "actually", "i mean", "i meant", "sorry", "no no", "correction",
    "let me rephrase", "never mind", "nevermind", "make that", "make it", "instead", "rather", "not that",
    "hold on", "oops", "my bad", "change that", "delete that", "forget that",
];
const ASSISTANT_OPENERS: &[&str] = &[
    "sure", "certainly", "of course", "here is", "here's", "here are", "i'm sorry", "i am sorry", "sorry, i",
    "as an ai", "i can't", "i cannot", "i'm not able", "i am not able", "could you", "can you please provide",
    "the answer is", "it seems", "note:",
];

fn digit_to_word(d: &str) -> Option<&'static str> {
    Some(match d {
        "0" => "zero", "1" => "one", "2" => "two", "3" => "three", "4" => "four", "5" => "five", "6" => "six",
        "7" => "seven", "8" => "eight", "9" => "nine", "10" => "ten", "11" => "eleven", "12" => "twelve",
        "13" => "thirteen", "14" => "fourteen", "15" => "fifteen", "16" => "sixteen", "17" => "seventeen",
        "18" => "eighteen", "19" => "nineteen", "20" => "twenty", "30" => "thirty", "40" => "forty",
        "50" => "fifty", "60" => "sixty", "70" => "seventy", "80" => "eighty", "90" => "ninety",
        "100" => "hundred",
        _ => return None,
    })
}

fn word_to_digit(w: &str) -> Option<&'static str> {
    Some(match w {
        "zero" => "0", "one" => "1", "two" => "2", "three" => "3", "four" => "4", "five" => "5", "six" => "6",
        "seven" => "7", "eight" => "8", "nine" => "9", "ten" => "10", "eleven" => "11", "twelve" => "12",
        "thirteen" => "13", "fourteen" => "14", "fifteen" => "15", "sixteen" => "16", "seventeen" => "17",
        "eighteen" => "18", "nineteen" => "19", "twenty" => "20", "thirty" => "30", "forty" => "40",
        "fifty" => "50", "sixty" => "60", "seventy" => "70", "eighty" => "80", "ninety" => "90",
        "hundred" => "100",
        _ => return None,
    })
}

fn levenshtein_small(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (cur[j] + 1).min(prev[j + 1] + 1).min(prev[j] + usize::from(ca != cb));
        }
        prev.clone_from_slice(&cur);
    }
    prev[b.len()]
}

fn words(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Lowercased words with contractions split, so "I'm"/"I am" and "don't"/"do not" match.
pub fn word_parts(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for w in s.split_whitespace() {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase().replace('’', "'");
        if w.is_empty() {
            continue;
        }
        if let Some(base) = w.strip_suffix("n't") {
            parts.push(match base { "ca" => "can", "wo" => "will", "sha" => "shall", b => b }.to_string());
            parts.push("not".into());
        } else if let Some((base, suffix)) = w.split_once('\'') {
            parts.push(base.to_string());
            let expanded: &[&str] = match suffix {
                "ll" => &["will"],
                "re" => &["are"],
                "ve" => &["have"],
                "m" => &["am"],
                "d" => &["would", "had"],
                "s" => &["is", "has", "us"],
                _ => &[],
            };
            parts.extend(expanded.iter().map(|s| s.to_string()));
        } else {
            parts.push(w);
        }
    }
    parts
}

/// Same word, a number written the other way (5/five), or an inflection (budget/budgets).
fn same_word(w: &str, set: &HashSet<String>) -> bool {
    set.contains(w)
        || digit_to_word(w).map_or(false, |x| set.contains(x))
        || word_to_digit(w).map_or(false, |x| set.contains(x))
        || set.iter().any(|k| {
            (k.starts_with(w) || w.starts_with(k.as_str())) && k.len().min(w.len()) >= 4 && k.len().abs_diff(w.len()) <= 3
        })
}

/// Every number the speaker said, as digit strings: digits they said plus number words read by book 4.
fn spoken_numbers(raw: &str) -> HashSet<String> {
    let as_digits = super::numbers::apply(raw);
    let mut out = HashSet::new();
    for src in [raw, as_digits.as_str()] {
        for tok in src.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == ':')) {
            for part in tok.split(|c| c == ':' || c == '.') {
                let d: String = part.chars().filter(|c| c.is_ascii_digit()).collect();
                if !d.is_empty() {
                    out.insert(d.trim_start_matches('0').to_string());
                }
            }
            let whole: String = tok.chars().filter(|c| c.is_ascii_digit()).collect();
            if !whole.is_empty() {
                out.insert(whole.trim_start_matches('0').to_string());
            }
        }
    }
    out
}

fn has_cue(raw: &str) -> bool {
    let l = format!(" {} ", raw.to_lowercase().replace(|c: char| !c.is_alphanumeric() && c != '\'', " "));
    let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
    let l = format!(" {l} ");
    CUES.iter().any(|c| l.contains(&format!(" {c} ")))
}

/// Book 1. `Ok` means the output may be pasted; `Err` says why it was refused.
pub fn check_ai_faithful(cleaned: &str, raw: &str) -> Result<(), String> {
    let said = word_parts(raw);
    let out = word_parts(cleaned);
    let spoken: HashSet<String> = said.iter().cloned().collect();
    // F5
    let lc = cleaned.trim().to_lowercase();
    let lr = raw.trim().to_lowercase();
    for opener in ASSISTANT_OPENERS {
        if lc.starts_with(opener) && !lr.contains(opener) {
            return Err(format!("answered like an assistant: starts with {opener:?}"));
        }
    }
    // F1 + F2
    let numbers = spoken_numbers(raw);
    for w in &out {
        if w.chars().any(|c| c.is_ascii_digit()) {
            let d: String = w.chars().filter(|c| c.is_ascii_digit()).collect();
            if !numbers.contains(d.trim_start_matches('0')) && !d.chars().all(|c| c == '0') {
                return Err(format!("added a number never spoken: {w:?}"));
            }
            continue;
        }
        if !(FUNCTION_WORDS.contains(&w.as_str()) || same_word(w, &spoken)) {
            return Err(format!("added a word never spoken: {w:?}"));
        }
    }
    // F7
    let (n_said, n_out) = (said.len(), out.len());
    if n_out > n_said + 3 + n_said / 10 {
        return Err(format!("output grew from {n_said} to {n_out} words"));
    }
    // F6
    let mut counts: std::collections::HashMap<(&str, &str, &str), (usize, usize)> = std::collections::HashMap::new();
    for t in out.windows(3) {
        counts.entry((&t[0], &t[1], &t[2])).or_default().0 += 1;
    }
    for t in said.windows(3) {
        if let Some(c) = counts.get_mut(&(t[0].as_str(), t[1].as_str(), t[2].as_str())) {
            c.1 += 1;
        }
    }
    if let Some(((a, b, c), _)) = counts.iter().find(|(_, &(o, s))| o >= 3 && o > s) {
        return Err(format!("repeated {:?} in a loop", format!("{a} {b} {c}")));
    }
    let kept: HashSet<String> = out.iter().cloned().collect();
    // F3
    let out_pairs: HashSet<(&str, &str)> = out.windows(2).map(|p| (p[0].as_str(), p[1].as_str())).collect();
    for i in 1..said.len().saturating_sub(1) {
        let w = said[i].as_str();
        let is_content = w.len() >= 3 && !FUNCTION_WORDS.contains(&w) && !FILLERS.contains(&w) && !CRUTCHES.contains(&w);
        if is_content && !same_word(w, &kept) && out_pairs.contains(&(said[i - 1].as_str(), said[i + 1].as_str())) {
            return Err(format!("silently dropped {w:?}"));
        }
    }
    // F4
    for sentence in raw.split(['.', '?', '!']) {
        let content: Vec<String> = word_parts(sentence)
            .into_iter()
            .filter(|w| w.len() >= 3 && !FUNCTION_WORDS.contains(&w.as_str()) && !FILLERS.contains(&w.as_str()))
            .collect();
        if content.len() >= 3 && !content.iter().any(|w| same_word(w, &kept)) {
            return Err(format!("dropped a whole sentence: {:?}", sentence.trim()));
        }
    }
    // F8
    let content_said: Vec<&String> = said.iter().filter(|w| w.len() >= 3 && !FUNCTION_WORDS.contains(&w.as_str()) && !FILLERS.contains(&w.as_str()) && !CRUTCHES.contains(&w.as_str())).collect();
    if content_said.len() >= 6 && !has_cue(raw) {
        let kept_n = content_said.iter().filter(|w| same_word(w, &kept)).count();
        if (kept_n as f32) < 0.3 * content_said.len() as f32 {
            return Err(format!("kept only {kept_n} of {} spoken content words", content_said.len()));
        }
    }
    Ok(())
}

/// Touch Up's gate: every output word appears in the input in the same order (dropping a
/// duplicate is fine, adding punctuation is fine, substituting or inventing a word is not).
pub fn is_word_subsequence(cleaned: &str, raw: &str) -> bool {
    let norm = |s: &str| -> Vec<String> {
        s.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|w| !w.is_empty())
            .collect()
    };
    let raw_words = norm(raw);
    let mut i = 0;
    for cw in norm(cleaned) {
        while i < raw_words.len() && raw_words[i] != cw {
            i += 1;
        }
        if i >= raw_words.len() {
            return false;
        }
        i += 1;
    }
    true
}

/// Summarize's gate: the fraction of the summary's words that were spoken (or close inflections).
pub fn contained_ratio(cleaned: &str, raw: &str) -> f32 {
    let (wc, wr) = (words(cleaned), words(raw));
    if wc.is_empty() {
        return 0.0;
    }
    let grounded = |w: &str| {
        wr.contains(w)
            || digit_to_word(w).map_or(false, |x| wr.contains(x))
            || word_to_digit(w).map_or(false, |x| wr.contains(x))
            || wr.iter().any(|r| {
                (w.starts_with(r.as_str()) && w.len() <= r.len() + 4)
                    || (r.starts_with(w) && r.len() <= w.len() + 4)
                    || (w.len() >= 4 && r.len() >= 4 && levenshtein_small(w, r) <= 2)
            })
    };
    wc.iter().filter(|w| grounded(w)).count() as f32 / wc.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects() {
        let france = "What is the capital of France? It also describes the item of the grocery list.";
        assert!(check_ai_faithful("The capital of France is Paris.", france).is_err());
        assert!(check_ai_faithful("I'm not sure I understand what you're asking. Could you provide more details?", "do that and give me the new exe file").is_err());
        assert!(check_ai_faithful("I want watermelon juice.", "I went to the office about the budget. I want biryani and I want lemonade. No, I want watermelon juice.").is_err());
        assert!(check_ai_faithful("I met Priya about the budget.", "I met Priya yesterday about the budget.").is_err());
        assert!(check_ai_faithful("I will also give you the questions.", "I will also give me the questions").is_err());
        // F2: a number nobody said
        assert!(check_ai_faithful("Transfer 150 bucks to Sarah.", "Transfer 50 bucks to Sarah.").is_err());
        // F5
        assert!(check_ai_faithful("Sure, send it to Ben.", "send it to Ben").is_err());
        // F6
        assert!(check_ai_faithful("call the bank call the bank call the bank call the bank", "call the bank").is_err());
        // F8: the model kept a fragment of a long dictation with no correction cue
        assert!(check_ai_faithful("Book a table.", "Book a table for six people at the Italian restaurant downtown tonight around eight").is_err());
    }

    #[test]
    fn accepts() {
        assert!(check_ai_faithful("Send it to sales.", "Send it to marketing, scratch that, send it to sales.").is_ok());
        assert!(check_ai_faithful("Book the room for Wednesday.", "Book the room for Tuesday. No, no, for Wednesday.").is_ok());
        assert!(check_ai_faithful("I do not want to stop.", "I don't want to stop.").is_ok());
        assert!(check_ai_faithful("I'm ready.", "I am ready.").is_ok());
        assert!(check_ai_faithful("I need eggs and I need rice.", "I need eggs and I need bread. No, no, I need rice.").is_ok());
        assert!(check_ai_faithful("I want it.", "I, like, want it.").is_ok());
        assert!(check_ai_faithful("Book a table for 6 at 7:30 PM.", "book a table for six at seven thirty pm").is_ok());
        assert!(check_ai_faithful("Send 1500 rupees.", "send fifteen hundred rupees").is_ok());
        assert!(check_ai_faithful("Transfer 2500 to Sam.", "transfer 2000, no wait, 2500 to Sam").is_ok());
    }

    #[test]
    fn subsequence_and_ratio() {
        assert!(is_word_subsequence("I want a burger.", "I I want a burger."));
        assert!(!is_word_subsequence("I want a pizza.", "I want a burger."));
        assert!(contained_ratio("walked to the park", "walk to the park") > 0.9);
    }
}
