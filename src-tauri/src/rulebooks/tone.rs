//! Book 7 - Tone. The only book allowed to swap words, and only in Professional tone.
//!
//! Casual and Standard: no change at all. The speaker's own wording ("gonna", "like", grammar) stays.
//! Professional:
//!   T1 Slang -> full words: gonna -> going to, wanna -> want to, gotta -> have to, kinda -> kind of,
//!      sorta -> sort of, dunno -> don't know, cuz / 'cause -> because, imma -> I'm going to.
//!   T2 Fillers-as-words between commas: ", like," and ", you know," are dropped.
//!   T3 "off of" -> "off".
//! Never (any tone): grammar "fixes" (they was -> they were, use to -> used to), homophone guesses
//! (to -> too, their -> there), sound-alike guesses (affect/effect), or phrase rewrites
//! ("in order to" -> "to"). Those change what was said and were removed on 2026-10-02.

use super::{core_lower, per_line, split3, Tone};

const SLANG: &[(&str, &str)] = &[
    ("gonna", "going to"), ("wanna", "want to"), ("gotta", "have to"), ("kinda", "kind of"), ("sorta", "sort of"),
    ("dunno", "don't know"), ("cuz", "because"), ("'cause", "because"), ("imma", "I'm going to"),
    ("i'ma", "I'm going to"), ("lemme", "let me"), ("gimme", "give me"),
];

pub fn apply(text: &str, tone: Tone) -> String {
    if tone != Tone::Professional {
        return text.to_string();
    }
    per_line(text, |line| {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            let (l, core, r) = split3(toks[i]);
            let lower = core.to_lowercase();
            // T1 ('cause keeps its apostrophe in the lead part)
            let key = if l.ends_with('\'') && lower == "cause" { "'cause".to_string() } else { lower.clone() };
            if let Some(&(_, full)) = SLANG.iter().find(|(k, _)| *k == key) {
                let lead = if key == "'cause" { &l[..l.len() - 1] } else { l };
                let full = if core.chars().next().map_or(false, |c| c.is_uppercase()) { super::cap_first(full) } else { full.to_string() };
                out.push(format!("{lead}{full}{r}"));
                i += 1;
                continue;
            }
            // T2: ", like," / ", you know," (the comma before stays on the previous word's end... and is dropped)
            let prev_comma = out.last().map_or(false, |p| p.ends_with(','));
            if prev_comma && lower == "like" && r == "," && l.is_empty() {
                let last = out.last_mut().unwrap();
                last.pop();
                i += 1;
                continue;
            }
            if prev_comma && lower == "you" && toks.get(i + 1).map_or(false, |n| core_lower(n) == "know" && n.ends_with(',')) && l.is_empty() {
                let last = out.last_mut().unwrap();
                last.pop();
                i += 2;
                continue;
            }
            // T3
            if lower == "off" && r.is_empty() && toks.get(i + 1).map_or(false, |n| split3(n).1.eq_ignore_ascii_case("of") && split3(n).0.is_empty()) {
                let of_trail = split3(toks[i + 1]).2;
                out.push(format!("{l}{core}{of_trail}"));
                i += 2;
                continue;
            }
            out.push(toks[i].to_string());
            i += 1;
        }
        out.join(" ")
    })
}

#[cfg(test)]
mod tests {
    use super::{apply, Tone};

    #[test]
    fn professional_does() {
        let p = |s| apply(s, Tone::Professional);
        assert_eq!(p("I'm gonna leave cuz I gotta meet them"), "I'm going to leave because I have to meet them");
        assert_eq!(p("Gonna be late"), "Going to be late");
        assert_eq!(p("it was, like, really big"), "it was really big");
        assert_eq!(p("it was, you know, big"), "it was big");
        assert_eq!(p("take it off of the table"), "take it off the table");
        assert_eq!(p("dunno yet"), "don't know yet");
    }

    #[test]
    fn casual_and_standard_never_change() {
        for s in ["I'm gonna go cuz I wanna see it, like, right now.", "they was talking and he don't know nothing"] {
            assert_eq!(apply(s, Tone::Casual), s);
            assert_eq!(apply(s, Tone::Standard), s);
        }
    }

    #[test]
    fn professional_must_not_touch() {
        for s in ["I like it.", "You know the answer.", "Turn it off.", "We need a kind of map."] {
            assert_eq!(apply(s, Tone::Professional), s);
        }
    }
}
