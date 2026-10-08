//! Book 8 - Typography. Spacing and punctuation hygiene; the words stay.
//!
//! Y1 Missing apostrophes: dont -> don't, cant -> can't, im -> I'm, thats -> that's ... (not "lets",
//!    "id", "wont", "ill", "were", "its": those are real words).
//! Y2 "could of" / "should of" / "would of" / "must of" -> "... have" (a spelling of what was said).
//! Y3 Latin abbreviations said as letters: "e g" -> "e.g.", "i e" -> "i.e.", "et cetera" -> "etc.".
//! Y4 A final dangling "and" / "but" / "or" / "because" (the speaker trailed off) is dropped.
//!    Not "so" ("I think so") and not "which".
//! Y5 Spacing: no space before , . ? ! : ; no doubled punctuation, single spaces, trimmed lines.
//! Never: adds or removes a word other than Y4, or changes letters inside a word.

use super::{core_lower, per_line, split3};

const APOSTROPHES: &[(&str, &str)] = &[
    ("dont", "don't"), ("cant", "can't"), ("didnt", "didn't"), ("doesnt", "doesn't"), ("isnt", "isn't"),
    ("arent", "aren't"), ("wasnt", "wasn't"), ("werent", "weren't"), ("havent", "haven't"), ("hasnt", "hasn't"),
    ("hadnt", "hadn't"), ("couldnt", "couldn't"), ("shouldnt", "shouldn't"), ("wouldnt", "wouldn't"),
    ("thats", "that's"), ("whats", "what's"), ("theres", "there's"), ("im", "I'm"), ("ive", "I've"),
    ("youre", "you're"), ("theyre", "they're"), ("weve", "we've"), ("youve", "you've"), ("theyve", "they've"),
    ("youll", "you'll"), ("theyll", "they'll"), ("itll", "it'll"), ("couldve", "could've"),
    ("shouldve", "should've"), ("wouldve", "would've"), ("hows", "how's"), ("whos", "who's"),
];

pub fn apply(text: &str) -> String {
    let t = per_line(text, |line| {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            let (l, core, r) = split3(toks[i]);
            let lower = core.to_lowercase();
            // Y1
            if let Some(&(_, fixed)) = APOSTROPHES.iter().find(|(k, _)| *k == lower) {
                let fixed = if core.chars().next().map_or(false, |c| c.is_uppercase()) { super::cap_first(fixed) } else { fixed.to_string() };
                out.push(format!("{l}{fixed}{r}"));
                i += 1;
                continue;
            }
            // Y2
            if matches!(lower.as_str(), "could" | "should" | "would" | "must") && r.is_empty() {
                if let Some(next) = toks.get(i + 1) {
                    let (nl, nc, nr) = split3(next);
                    if nl.is_empty() && nc.eq_ignore_ascii_case("of") {
                        out.push(format!("{l}{core} have{nr}"));
                        i += 2;
                        continue;
                    }
                }
            }
            // Y3
            let next = toks.get(i + 1).map(|t| core_lower(t)).unwrap_or_default();
            if r.is_empty() && l.is_empty() && ((lower == "e" && next == "g") || (lower == "i" && next == "e")) {
                let abbr = if lower == "e" { "e.g." } else { "i.e." };
                let trail = split3(toks[i + 1]).2.trim_start_matches('.').to_string();
                out.push(format!("{abbr}{trail}"));
                i += 2;
                continue;
            }
            if lower == "et" && r.is_empty() && next == "cetera" {
                let trail = split3(toks[i + 1]).2.trim_start_matches('.').to_string();
                out.push(format!("{l}etc.{trail}"));
                i += 2;
                continue;
            }
            if lower == "etcetera" {
                out.push(format!("{l}etc.{}", r.trim_start_matches('.')));
                i += 1;
                continue;
            }
            out.push(toks[i].to_string());
            i += 1;
        }
        // Y4: only at the very end of the dictation line, and only if words remain
        if out.len() > 1 {
            let (_, c, r) = split3(out.last().unwrap());
            if matches!(c.to_lowercase().as_str(), "and" | "but" | "or" | "because") && !r.contains('?') {
                let end: String = r.chars().filter(|c| matches!(c, '.' | '!')).collect();
                out.pop();
                if let Some(last) = out.last_mut() {
                    let trimmed = last.trim_end_matches(',').to_string();
                    *last = format!("{trimmed}{end}");
                }
            }
        }
        out.join(" ")
    });
    spacing(&t)
}

/// Y5.
fn spacing(text: &str) -> String {
    let mut s = text.to_string();
    for (from, to) in [(" ,", ","), (" ?", "?"), (" !", "!"), (" :", ":"), (" ;", ";"), (",,", ","), (",.", "."), ("?.", "?"), ("!.", "!")] {
        while s.contains(from) {
            s = s.replace(from, to);
        }
    }
    // " ." -> "." unless it starts a file extension (" .ts file")
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        let before_extension = chars.get(i + 2).map_or(false, |n| n.is_alphanumeric());
        if c == ' ' && chars.get(i + 1) == Some(&'.') && !before_extension {
            continue;
        }
        out.push(c);
    }
    // collapse a doubled sentence period, but keep "..." (an ellipsis) and "etc." endings intact
    let mut cleaned = String::with_capacity(out.len());
    let oc: Vec<char> = out.chars().collect();
    for i in 0..oc.len() {
        if oc[i] == '.' && i > 0 && oc[i - 1] == '.' && oc.get(i + 1) != Some(&'.') && !(i >= 2 && oc[i - 2] == '.') {
            continue;
        }
        cleaned.push(oc[i]);
    }
    cleaned
        .split('\n')
        .map(|l| l.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn does() {
        let cases = [
            ("i cant make it, im busy", "i can't make it, I'm busy"),
            ("you should of told me", "you should have told me"),
            ("apples e g pears et cetera", "apples e.g. pears etc."),
            ("I want pizza and", "I want pizza"),
            ("I want pizza and.", "I want pizza."),
            ("done ,really ?", "done,really?"),
            ("Rename it in the .ts file .", "Rename it in the .ts file."),
            ("wait..", "wait."),
        ];
        for (input, want) in cases {
            assert_eq!(apply(input), want, "input {input:?}");
        }
    }

    #[test]
    fn must_not_touch() {
        for s in [
            "I think so.",
            "It lets you in.",
            "Show your ID.",
            "We were there.",
            "Its tail wagged.",
            "I'll be ill.",
            "Wait... what?",
            "Ready, set, go!",
            "Pizza and fries.",
            "Is it this or?",
        ] {
            assert_eq!(apply(s), s, "changed {s:?}");
        }
    }

    #[test]
    fn idempotent() {
        let s = "dont go , you should of known e g this and";
        let once = apply(s);
        assert_eq!(apply(&once), once);
    }
}
