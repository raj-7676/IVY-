//! Book 3 - Spoken commands. Turns things the speaker says TO Ivy into the symbol they asked for.
//!
//! C1 Line breaks: "new paragraph" -> blank line, "new line" / "newline" -> line break.
//! C2 Punctuation: period / full stop, comma, colon, semicolon, question mark, exclamation mark/point,
//!    em dash, en dash. Gated so the WORD survives when it is used as a word:
//!    - "period" / "full stop" only when it ends the line, or the next word starts with a capital
//!      letter, and never after a word that makes it a noun ("trial period", "a period").
//!    - every other mark: never right after a determiner ("a comma", "the question mark").
//! C3 Quotes and brackets: open/close quote, open/close paren(thesis), open/close bracket,
//!    open/close (curly) brace.
//! C4 Lists: "bullet point X" -> "• X" on its own line, but only at the start of a line or sentence;
//!    "number 1:" .. "number 9:" (after C2 turned "colon" into ":") -> "1." on its own line.
//! C5 Emoji only when the word "emoji" is said: "thumbs up emoji" -> 👍.
//! C6 "hashtag launch" -> "#launch". "in backticks config" -> "`config`".
//! Never: guesses a mark nobody said, or touches "period"/"comma"/"colon" used as nouns.

use super::{core_lower, per_line, split3};

const DETERMINERS: &[&str] = &["a", "an", "the", "this", "that", "each", "every", "no", "any", "my", "your"];
/// Words after "colon" that make it the body part ("colon surgery").
const COLON_NOUN_NEXT: &[&str] = &["surgery", "cancer", "cleanse", "health", "polyp", "polyps", "screening", "exam", "problems"];
/// Words that make "period" a noun ("trial period", "grace period").
const PERIOD_NOUN: &[&str] = &[
    "trial", "grace", "waiting", "free", "probation", "probationary", "notice", "time", "same", "whole", "short",
    "long", "cooling", "billing", "reporting", "rest", "test", "holding", "lock-in", "lock", "transition",
    "school", "class", "first", "second", "third", "last", "next", "this", "that", "a", "the", "my", "your",
    "her", "his", "their", "our", "menstrual",
];

pub fn apply(text: &str) -> String {
    let t = line_breaks(text);
    let t = per_line(&t, punctuation_line);
    let t = lists(&t);
    let t = per_line(&t, emoji_hashtag_backticks);
    let t = t.split('\n').map(|l| l.trim()).collect::<Vec<_>>().join("\n");
    t.trim_matches('\n').to_string()
}

/// C1. Works on tokens so "new line" inside "renew lines" never matches.
fn line_breaks(text: &str) -> String {
    let toks: Vec<&str> = text.split(' ').filter(|t| !t.is_empty()).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < toks.len() {
        let w = core_lower(toks[i]);
        let next = toks.get(i + 1).map(|t| core_lower(t)).unwrap_or_default();
        let (brk, used) = if w == "new" && next == "paragraph" {
            ("\n\n", 2)
        } else if w == "new" && next == "line" {
            ("\n", 2)
        } else if w == "newline" {
            ("\n", 1)
        } else {
            ("", 0)
        };
        let prev = if i > 0 { core_lower(toks[i - 1]) } else { String::new() };
        if used > 0 && (DETERMINERS.contains(&prev.as_str()) || toks[i + used - 1].ends_with(':')) {
            // "a new line manager", "new line:" - the words, not the command; keep as spoken
        } else if used > 0 {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push_str(brk);
            i += used;
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push_str(toks[i]);
        i += 1;
    }
    out
}

fn mark_for(words: &[String]) -> Option<(&'static str, usize)> {
    let w0 = words.first().map(|s| s.as_str()).unwrap_or("");
    let w1 = words.get(1).map(|s| s.as_str()).unwrap_or("");
    Some(match (w0, w1) {
        ("question", "mark") => ("?", 2),
        ("exclamation", "mark") | ("exclamation", "point") => ("!", 2),
        ("full", "stop") => (".", 2),
        ("em", "dash") => (" —", 2),
        ("en", "dash") => (" –", 2),
        ("period", _) => (".", 1),
        ("comma", _) => (",", 1),
        ("colon", _) => (":", 1),
        ("semicolon", _) => (";", 1),
        _ => return None,
    })
}

/// C2 + C3 on one line.
fn punctuation_line(line: &str) -> String {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let lower: Vec<String> = toks.iter().map(|t| core_lower(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let prev = out.last().map(|t| core_lower(t)).unwrap_or_default();
        let prev_ends_clause = out.last().map_or(true, |t| t.ends_with(|c: char| matches!(c, '.' | '?' | '!' | ':' | ';')));
        // C3 quotes and brackets (two- and three-word commands)
        let pair = (lower[i].as_str(), lower.get(i + 1).map(|s| s.as_str()).unwrap_or(""), lower.get(i + 2).map(|s| s.as_str()).unwrap_or(""));
        let bracket = match pair {
            ("open", "quote", _) => Some(("\"", 2, true)),
            ("close", "quote", _) | ("end", "quote", _) | ("unquote", _, _) => Some(("\"", if lower[i] == "unquote" { 1 } else { 2 }, false)),
            ("open", "paren", _) | ("open", "parenthesis", _) => Some(("(", 2, true)),
            ("close", "paren", _) | ("close", "parenthesis", _) => Some((")", 2, false)),
            ("open", "bracket", _) => Some(("[", 2, true)),
            ("close", "bracket", _) => Some(("]", 2, false)),
            ("open", "curly", "brace") => Some(("{", 3, true)),
            ("close", "curly", "brace") => Some(("}", 3, false)),
            ("open", "brace", _) => Some(("{", 2, true)),
            ("close", "brace", _) => Some(("}", 2, false)),
            _ => None,
        };
        if let Some((sym, used, opens)) = bracket {
            // the command words must be bare (no punctuation inside the phrase)
            if toks[i..i + used - 1].iter().all(|t| !t.ends_with(|c: char| !c.is_alphanumeric())) {
                if opens {
                    out.push(sym.to_string()); // glued to the next word below
                } else if let Some(last) = out.last_mut() {
                    last.push_str(sym);
                    // keep punctuation the speaker's tokens carried ("close quote." -> ".")
                    last.push_str(split3(toks[i + used - 1]).2);
                } else {
                    out.push(sym.to_string());
                }
                i += used;
                continue;
            }
        }
        if let Some((mark, used)) = mark_for(&lower[i..]) {
            let after = toks.get(i + used);
            let bare = toks[i..i + used].iter().all(|t| split3(t).0.is_empty());
            let allowed = !out.is_empty() && bare && !DETERMINERS.contains(&prev.as_str()) && !prev_ends_clause && {
                if mark == "." {
                    let next_cap = after.map_or(true, |t| t.chars().next().map_or(false, |c| c.is_uppercase()));
                    !PERIOD_NOUN.contains(&prev.as_str()) && next_cap
                } else if mark == ":" {
                    !after.map_or(false, |t| COLON_NOUN_NEXT.contains(&core_lower(t).as_str()))
                } else {
                    true
                }
            };
            if allowed {
                let last = out.last_mut().unwrap();
                let last_trimmed = last.trim_end_matches(|c: char| matches!(c, ',' | '.' | ';' | ':')).to_string();
                *last = format!("{last_trimmed}{mark}");
                // a command token may carry punctuation the ASR added ("period." / "comma,")
                i += used;
                continue;
            }
        }
        // open bracket/quote glue: attach this word to a pending opener
        if let Some(last) = out.last_mut() {
            if matches!(last.as_str(), "\"" | "(" | "[" | "{") {
                last.push_str(toks[i]);
                i += 1;
                continue;
            }
        }
        out.push(toks[i].to_string());
        i += 1;
    }
    out.join(" ")
}

/// C4. Bullets and "number N:" items, each on its own line.
fn lists(text: &str) -> String {
    per_line(text, |line| {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            let w = core_lower(toks[i]);
            let starts = out.is_empty() || out.last().map_or(false, |t| t.ends_with(|c: char| matches!(c, '.' | '?' | '!' | ':')));
            if starts && w == "bullet" && toks.get(i + 1).map(|t| core_lower(t)) == Some("point".into()) && i + 2 < toks.len() {
                out.push("\n•".into());
                i += 2;
                continue;
            }
            if w == "number" {
                if let Some(next) = toks.get(i + 1) {
                    let (_, core, trail) = split3(next);
                    let n = match core.to_lowercase().as_str() {
                        "one" | "1" => Some(1), "two" | "2" => Some(2), "three" | "3" => Some(3), "four" | "4" => Some(4),
                        "five" | "5" => Some(5), "six" | "6" => Some(6), "seven" | "7" => Some(7), "eight" | "8" => Some(8),
                        "nine" | "9" => Some(9), _ => None,
                    };
                    if let (Some(n), true) = (n, trail == ":") {
                        out.push(format!("\n{n}."));
                        i += 2;
                        continue;
                    }
                }
            }
            out.push(toks[i].to_string());
            i += 1;
        }
        out.join(" ").replace(" \n", "\n")
    })
}

const EMOJI: &[(&str, &str)] = &[
    ("thumbs up", "👍"), ("thumbs down", "👎"), ("smiley", "😊"), ("smile", "😊"), ("smiling", "😊"),
    ("laughing", "😂"), ("crying laughing", "😂"), ("fire", "🔥"), ("rocket", "🚀"), ("heart", "❤️"),
    ("red heart", "❤️"), ("check mark", "✅"), ("checkmark", "✅"), ("party", "🎉"), ("eyes", "👀"),
    ("mind blown", "🤯"), ("thinking", "🤔"), ("clap", "👏"), ("clapping", "👏"), ("pray", "🙏"),
    ("folded hands", "🙏"), ("wink", "😉"), ("sad", "😢"), ("hundred", "💯"), ("100", "💯"), ("ok hand", "👌"),
];

/// C5 + C6.
fn emoji_hashtag_backticks(line: &str) -> String {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let lower: Vec<String> = toks.iter().map(|t| core_lower(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        // C5: "<name> emoji" (name of 1 or 2 words)
        let mut done = false;
        for len in [2usize, 1] {
            if i + len < toks.len() && lower[i + len] == "emoji" {
                let name = lower[i..i + len].join(" ");
                let bare = toks[i..i + len].iter().all(|t| split3(t).2.is_empty());
                if let (Some(&(_, g)), true) = (EMOJI.iter().find(|(n, _)| *n == name), bare) {
                    out.push(format!("{g}{}", split3(toks[i + len]).2));
                    i += len + 1;
                    done = true;
                    break;
                }
            }
        }
        if done {
            continue;
        }
        // C6
        if lower[i] == "hashtag" && i + 1 < toks.len() && !split3(toks[i + 1]).1.is_empty() {
            let (_, core, trail) = split3(toks[i + 1]);
            out.push(format!("#{core}{trail}"));
            i += 2;
            continue;
        }
        if lower[i] == "in" && lower.get(i + 1).map(|s| s.as_str()) == Some("backticks") && i + 2 < toks.len() {
            let (_, core, trail) = split3(toks[i + 2]);
            out.push(format!("`{core}`{trail}"));
            i += 3;
            continue;
        }
        out.push(toks[i].to_string());
        i += 1;
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn does() {
        assert_eq!(apply("hello world period How are you question mark new line I am good exclamation mark"),
                   "hello world. How are you?\nI am good!");
        assert_eq!(apply("wait comma go"), "wait, go");
        assert_eq!(apply("note colon done"), "note: done");
        assert_eq!(apply("one semicolon two"), "one; two");
        assert_eq!(apply("first part new paragraph second part"), "first part\n\nsecond part");
        assert_eq!(apply("I'm done period"), "I'm done.");
        assert_eq!(apply("she said open quote hello close quote"), "she said \"hello\"");
        assert_eq!(apply("type open paren x close paren"), "type (x)");
        assert_eq!(apply("bullet point milk"), "• milk");
        assert_eq!(apply("my list colon number one: milk number two: eggs"), "my list:\n1. milk\n2. eggs");
        assert_eq!(apply("great work thumbs up emoji"), "great work 👍");
        assert_eq!(apply("post it with hashtag launch"), "post it with #launch");
        assert_eq!(apply("rename in backticks config"), "rename `config`");
        assert_eq!(apply("wait em dash actually never mind"), "wait — actually never mind");
    }

    #[test]
    fn must_not_touch() {
        for s in [
            "The trial period ended.",
            "The grace period is two weeks.",
            "Add a comma here.",
            "What does the question mark mean?",
            "He had colon surgery.",
            "Draw a smiley face.",
            "The bullet point is clear.",
            "We need a new line manager.",
            "Open the bracket factory.",
            "A period of calm followed.",
        ] {
            assert_eq!(apply(s), s, "changed {s:?}");
        }
    }

    #[test]
    fn idempotent() {
        let s = "hello period New line here comma ok";
        let once = apply(s);
        assert_eq!(apply(&once), once);
    }
}
