//! Book 2 - Disfluency. Removes sounds and repeats that carry no words the speaker meant to write.
//!
//! D1 Hesitation sounds: um, uh, er, ah, hmm, mhm ... are removed (with the comma they leave behind).
//! D2 Cut-off word starts: "w- we", "th- that" -> the cut-off piece is removed.
//! D3 A word said twice in a row is kept once ("the the" -> "the"), except words people double on
//!    purpose: "that that", "had had", "very very", "really really", "bye bye", "ha ha".
//! D4 A 2-4 word phrase said twice in a row is kept once ("we can we can" -> "we can",
//!    "I wanted to, I wanted to ask" -> "I wanted to ask").
//! Never: removes "like", "you know", "actually", "so" (those are words; book 7 decides in
//! Professional tone), touches digits ("5 5 5" stays), or changes a word.

use super::{core_lower, per_line};

/// Hesitation sounds. Single source of truth (book 1 also treats these as non-words).
pub const FILLERS: &[&str] = &[
    "um", "umm", "ummm", "uh", "uhh", "uhm", "er", "erm", "ah", "ahh", "hmm", "hmmm", "mhm", "mm-hmm", "mm",
];

/// Words people double on purpose.
const KEEP_DOUBLE: &[&str] = &["that", "had", "very", "really", "bye", "ha", "no"];

pub fn apply(text: &str) -> String {
    per_line(text, apply_line)
}

fn apply_line(line: &str) -> String {
    // D1 + D2 + D3
    let mut out: Vec<String> = Vec::new();
    for tok in line.split_whitespace() {
        // D2: "w-" / "th-" (a short cut-off start ending in a hyphen)
        if tok.ends_with('-') && tok.len() <= 4 && tok.chars().all(|c| c.is_alphabetic() || c == '-') {
            continue;
        }
        let core = core_lower(tok);
        // D1
        if FILLERS.contains(&core.as_str()) {
            if let Some(last) = out.last_mut() {
                // "Hello, um, world" -> "Hello, world": the filler's own trailing punctuation goes
                // unless it ends a sentence, in which case the previous word takes it.
                if tok.ends_with(|c: char| matches!(c, '.' | '?' | '!')) {
                    let end: String = tok.chars().rev().take_while(|c| matches!(c, '.' | '?' | '!')).collect();
                    let last_trim = last.trim_end_matches(',').to_string();
                    *last = format!("{last_trim}{}", end.chars().rev().collect::<String>());
                }
            }
            continue;
        }
        // D3
        if let Some(prev) = out.last() {
            let prev_core = core_lower(prev);
            let alphabetic = !core.is_empty() && core.chars().all(|c| c.is_alphabetic() || c == '\'');
            if alphabetic && prev_core == core && !KEEP_DOUBLE.contains(&core.as_str())
                && !prev.ends_with(|c: char| matches!(c, '.' | '?' | '!'))
            {
                // keep the later copy's trailing punctuation ("the, the." -> "the.")
                let last = out.last_mut().unwrap();
                *last = tok.to_string();
                continue;
            }
        }
        out.push(tok.to_string());
    }
    // D4: repeated 2-4 word phrases, longest first
    let mut changed = true;
    while changed {
        changed = false;
        'outer: for n in (2..=4).rev() {
            if out.len() < 2 * n {
                continue;
            }
            for i in 0..=out.len() - 2 * n {
                let a: Vec<String> = out[i..i + n].iter().map(|t| core_lower(t)).collect();
                let b: Vec<String> = out[i + n..i + 2 * n].iter().map(|t| core_lower(t)).collect();
                let clean = a.iter().all(|w| !w.is_empty() && w.chars().all(|c| c.is_alphabetic() || c == '\''));
                // the first copy must not end a sentence ("Go home. Go home." is two sentences)
                let sentence_break = out[i + n - 1].ends_with(|c: char| matches!(c, '.' | '?' | '!'));
                if clean && a == b && !sentence_break {
                    out.drain(i..i + n);
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn does() {
        assert_eq!(apply("Um, hello, I I think w- we should meet."), "hello, I think we should meet.");
        assert_eq!(apply("we can we can do this"), "we can do this");
        assert_eq!(apply("I wanted to, I wanted to ask you"), "I wanted to ask you");
        assert_eq!(apply("the report is uh ready"), "the report is ready");
        assert_eq!(apply("it's done, um."), "it's done.");
        assert_eq!(apply("no no no, Tuesday"), "no no no, Tuesday");
    }

    #[test]
    fn must_not_touch() {
        for s in [
            "I know that that is true.",
            "He had had enough.",
            "It was very very good.",
            "Dial 5 5 5 0.",
            "Go home. Go home.",
            "I like it, you know, actually.",
            "Bye bye.",
        ] {
            assert_eq!(apply(s), s);
        }
    }

    #[test]
    fn idempotent_and_keeps_lines() {
        let s = "um the the plan\nwe can we can go";
        let once = apply(s);
        assert_eq!(once, "the plan\nwe can go");
        assert_eq!(apply(&once), once);
    }
}
