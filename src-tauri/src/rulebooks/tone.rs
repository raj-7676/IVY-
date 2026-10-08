//! Book 7 - Tone. The only book allowed to swap words, each tone a step up from the one before
//! (Yash, 2026-10-06; style-guide research: rule-based formality keeps meaning best, GYAFC 2018).
//!
//! Casual (texting style, Yash 2026-10-06): the speaker's own wording ("gonna", "like", "yeah") stays,
//!   and sentence-ending full stops go, each sentence on its own line (`casual_endings`, run last).
//!   "?" and "!" stay; "...", abbreviations ("Dr.", "e.g.") and numbers are untouched.
//! Standard (casual speech written cleanly):
//!   S1 Slang -> full words: gonna -> going to, wanna -> want to, gotta -> have to, kinda -> kind of,
//!      sorta -> sort of, dunno -> don't know, cuz / 'cause -> because, imma -> I'm going to.
//!   S2 Fillers-as-words between commas: ", like," and ", you know," are dropped.
//!   S3 "off of" -> "off".
//! Professional (business-email register), Standard plus:
//!   P1 Contractions written out: don't -> do not, can't -> cannot, I'm -> I am, it's -> it is
//!      ("it's been" -> "it has been"), I'd -> I would ("I'd been/better" -> "I had"), ...
//!   P2 Chat words -> formal: yeah/yep/yup -> yes, nope -> no, btw -> by the way, fyi -> for your
//!      information, asap -> as soon as possible, thanks -> thank you, thx -> thank you, pls -> please.
//!   P3 Opening crutch words dropped: "Honestly, ...", "Basically, ...", "Literally, ...", "Actually, ..."
//!      at the start of a sentence (followed by a comma) go; the sentence starts with the next word.
//!   P4 No exclamation marks: "!" -> "." ("?!" -> "?").
//! Never (any tone): grammar "fixes" (they was -> they were), homophone guesses (their -> there),
//! sound-alike guesses (affect/effect), or phrase rewrites ("in order to" -> "to"). Those change what
//! was said and were removed on 2026-10-02.

use super::{cap_first, core_lower, per_line, split3, Tone};

const SLANG: &[(&str, &str)] = &[
    ("gonna", "going to"), ("wanna", "want to"), ("gotta", "have to"), ("kinda", "kind of"), ("sorta", "sort of"),
    ("dunno", "don't know"), ("cuz", "because"), ("'cause", "because"), ("imma", "I'm going to"),
    ("i'ma", "I'm going to"), ("lemme", "let me"), ("gimme", "give me"),
];

/// Contractions whose meaning is fixed. ('s and 'd are decided by the next word, see `expand`.)
const CONTRACTIONS: &[(&str, &str)] = &[
    ("don't", "do not"), ("doesn't", "does not"), ("didn't", "did not"), ("can't", "cannot"), ("won't", "will not"),
    ("isn't", "is not"), ("aren't", "are not"), ("wasn't", "was not"), ("weren't", "were not"),
    ("haven't", "have not"), ("hasn't", "has not"), ("hadn't", "had not"), ("couldn't", "could not"),
    ("shouldn't", "should not"), ("wouldn't", "would not"), ("mustn't", "must not"), ("needn't", "need not"),
    ("i'm", "I am"), ("you're", "you are"), ("we're", "we are"), ("they're", "they are"),
    ("i've", "I have"), ("you've", "you have"), ("we've", "we have"), ("they've", "they have"),
    ("i'll", "I will"), ("you'll", "you will"), ("we'll", "we will"), ("they'll", "they will"),
    ("he'll", "he will"), ("she'll", "she will"), ("it'll", "it will"), ("that'll", "that will"),
    ("you'd", "you would"), ("we'd", "we would"), ("they'd", "they would"), ("he'd", "he would"), ("she'd", "she would"),
    ("y'all", "you all"),
];
/// x's -> "x is", or "x has" before one of `HAS_NEXT`.
const IS_HAS: &[(&str, &str)] = &[
    ("it's", "it"), ("that's", "that"), ("there's", "there"), ("here's", "here"), ("what's", "what"),
    ("who's", "who"), ("where's", "where"), ("how's", "how"), ("he's", "he"), ("she's", "she"),
];
const HAS_NEXT: &[&str] = &["been", "got", "gotten", "had", "done", "gone"];
const HAD_NEXT: &[&str] = &["been", "better", "had", "done", "gone", "seen", "already", "never"];
const CHAT: &[(&str, &str)] = &[
    ("yeah", "yes"), ("yep", "yes"), ("yup", "yes"), ("nope", "no"), ("btw", "by the way"),
    ("fyi", "for your information"), ("asap", "as soon as possible"), ("thx", "thank you"), ("pls", "please"),
    ("plz", "please"), ("thanks", "thank you"),
];
const CRUTCH_OPENERS: &[&str] = &["honestly", "basically", "literally", "actually"];

pub fn apply(text: &str, tone: Tone) -> String {
    if tone == Tone::Casual {
        return text.to_string();
    }
    per_line(text, |line| {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let mut out: Vec<String> = Vec::new();
        let mut cap_next = false; // P3 dropped the sentence's first word: capitalise the new first word
        let mut i = 0;
        while i < toks.len() {
            let (l, core, r) = split3(toks[i]);
            let lower = core.to_lowercase().replace('’', "'");
            let next = toks.get(i + 1).map(|n| core_lower(n)).unwrap_or_default();
            let sentence_start = out.last().map_or(true, |p| p.ends_with(['.', '?', '!', ':']));
            let mut word: Option<String> = None;

            // S1 ('cause keeps its apostrophe in the lead part)
            let key = if l.ends_with('\'') && lower == "cause" { "'cause".to_string() } else { lower.clone() };
            if let Some(&(_, full)) = SLANG.iter().find(|(k, _)| *k == key) {
                let lead = if key == "'cause" { &l[..l.len() - 1] } else { l };
                let full = if tone == Tone::Professional { full.replace("don't", "do not").replace("I'm", "I am") } else { full.to_string() };
                out.push(format!("{lead}{}{r}", match_case(core, &full)));
                i += 1;
                continue;
            }
            // S2: ", like," / ", you know," (the comma on the previous word is dropped too)
            let prev_comma = out.last().map_or(false, |p| p.ends_with(','));
            if prev_comma && lower == "like" && r == "," && l.is_empty() {
                out.last_mut().unwrap().pop();
                i += 1;
                continue;
            }
            if prev_comma && lower == "you" && next == "know" && toks[i + 1].ends_with(',') && l.is_empty() {
                out.last_mut().unwrap().pop();
                i += 2;
                continue;
            }
            // S3
            if lower == "off" && r.is_empty() && next == "of" && split3(toks[i + 1]).0.is_empty() {
                out.push(format!("{l}{core}{}", split3(toks[i + 1]).2));
                i += 2;
                continue;
            }

            if tone == Tone::Professional {
                // P3: "Honestly, ..." at a sentence start
                if sentence_start && l.is_empty() && r == "," && CRUTCH_OPENERS.contains(&lower.as_str()) {
                    cap_next = true;
                    i += 1;
                    continue;
                }
                // P1
                if let Some(&(_, full)) = CONTRACTIONS.iter().find(|(k, _)| *k == lower) {
                    word = Some(full.to_string());
                } else if let Some(&(_, subj)) = IS_HAS.iter().find(|(k, _)| *k == lower) {
                    let verb = if HAS_NEXT.contains(&next.as_str()) { "has" } else { "is" };
                    word = Some(format!("{subj} {verb}"));
                } else if lower == "i'd" {
                    word = Some(if HAD_NEXT.contains(&next.as_str()) { "I had" } else { "I would" }.to_string());
                } else if let Some(&(_, formal)) = CHAT.iter().find(|(k, _)| *k == lower) {
                    // P2 ("thanks" only as a thank-you, not "thanks to the rain")
                    if !(lower == "thanks" && next == "to") {
                        word = Some(formal.to_string());
                    }
                }
            }
            let mut w = match word {
                Some(full) => match_case(core, &full),
                None => core.to_string(),
            };
            if cap_next {
                w = cap_first(&w);
                cap_next = false;
            }
            let mut r = r.to_string();
            if tone == Tone::Professional {
                r = r.replace("?!", "?").replace('!', ".");
                while r.contains("..") && !toks[i].ends_with("...") {
                    r = r.replace("..", ".");
                }
            }
            out.push(format!("{l}{w}{r}"));
            i += 1;
        }
        out.join(" ")
    })
}

const ABBREVIATIONS: &[&str] = &["mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "inc", "ltd", "no", "approx"];

/// Casual: drop sentence-ending full stops; a sentence that follows on the same line starts a new line.
pub fn casual_endings(text: &str) -> String {
    per_line(text, |line| {
        let toks: Vec<&str> = line.split(' ').collect();
        let mut out = String::new();
        for (i, tok) in toks.iter().enumerate() {
            let (_, core, r) = split3(tok);
            let lower = core.to_lowercase();
            let ends_sentence = r.ends_with('.')
                && !r.ends_with("..")
                // e.g. / a.m. / U.S. / 3.5: a dotted token of short parts keeps its dot ("file.py." still loses it)
                && !(core.contains('.') && core.split('.').all(|p| p.len() <= 2))
                && !ABBREVIATIONS.contains(&lower.as_str())
                && !(core.len() == 1 && core.chars().all(|c| c.is_ascii_uppercase())); // initials: "J. K."
            let word = if ends_sentence { &tok[..tok.len() - 1] } else { tok };
            out.push_str(word);
            if i + 1 < toks.len() {
                out.push(if ends_sentence { '\n' } else { ' ' });
            }
        }
        out
    })
}

/// "Don't" -> "Do not", "don't" -> "do not"; "I am" keeps its capital I either way.
fn match_case(original: &str, replacement: &str) -> String {
    if original.chars().next().map_or(false, |c| c.is_uppercase()) {
        cap_first(replacement)
    } else {
        replacement.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{apply, Tone};

    #[test]
    fn casual_never_changes_words() {
        for s in ["I'm gonna go cuz I wanna see it, like, right now!", "they was talking and he don't know nothing", "Yeah, honestly, it's fine."] {
            assert_eq!(apply(s, Tone::Casual), s);
        }
    }

    #[test]
    fn casual_endings_text_style() {
        use super::casual_endings as c;
        assert_eq!(c("Hey, there's this crazy thing going on. Do you know Josh? Yeah, he got arrested."),
                   "Hey, there's this crazy thing going on\nDo you know Josh? Yeah, he got arrested");
        assert_eq!(c("Wow! Meet Dr. Rao at 5 p.m. tomorrow."), "Wow! Meet Dr. Rao at 5 p.m. tomorrow");
        assert_eq!(c("It costs 3.5 lakhs... maybe. Open auth_service.py."), "It costs 3.5 lakhs... maybe\nOpen auth_service.py");
        assert_eq!(c("Line one.\nLine two."), "Line one\nLine two");
        let once = c("One. Two? Three.");
        assert_eq!(c(&once), once);
    }

    #[test]
    fn standard_does() {
        let p = |s| apply(s, Tone::Standard);
        assert_eq!(p("I'm gonna leave cuz I gotta meet them"), "I'm going to leave because I have to meet them");
        assert_eq!(p("Gonna be late"), "Going to be late");
        assert_eq!(p("it was, like, really big"), "it was really big");
        assert_eq!(p("it was, you know, big"), "it was big");
        assert_eq!(p("take it off of the table"), "take it off the table");
        assert_eq!(p("dunno yet"), "don't know yet");
        // Standard keeps contractions, chat words, openers and exclamation marks
        assert_eq!(p("Yeah, honestly, I don't know!"), "Yeah, honestly, I don't know!");
    }

    #[test]
    fn professional_does() {
        let p = |s| apply(s, Tone::Professional);
        assert_eq!(p("I'm gonna send it, I don't think it's late."), "I am going to send it, I do not think it is late.");
        assert_eq!(p("It's been a week and she's got the files."), "It has been a week and she has got the files.");
        assert_eq!(p("Yeah, we can't make it! Thanks for waiting."), "Yes, we cannot make it. Thank you for waiting.");
        assert_eq!(p("Honestly, the plan works. Basically, we ship Friday."), "The plan works. We ship Friday.");
        assert_eq!(p("I'd like that, I'd been waiting."), "I would like that, I had been waiting.");
        assert_eq!(p("btw send it asap"), "by the way send it as soon as possible");
        assert_eq!(p("Don’t worry"), "Do not worry");
        assert_eq!(p("Really?!"), "Really?");
    }

    #[test]
    fn tone_screen_samples_are_real() {
        // the exact samples shown on the Tone screen (src/components/ToneView.tsx), through the full pipeline
        let said = "Yeah, I'm gonna push that fix before standup. Thanks!";
        let full = |t| crate::rulebooks::after_model(said, t);
        assert_eq!(full(Tone::Casual), "Yeah, I'm gonna push that fix before standup\nThanks!");
        assert_eq!(full(Tone::Standard), "Yeah, I'm going to push that fix before standup. Thanks!");
        assert_eq!(full(Tone::Professional), "Yes, I am going to push that fix before standup. Thank you.");
    }

    #[test]
    fn professional_must_not_touch() {
        for s in [
            "I like it.", "You know the answer.", "Turn it off.", "We need a kind of map.",
            "Thanks to the rain, we stayed in.", "It actually works.", "The rate is 18 lakhs...",
            "Email ana@outlook.com about auth_service.py.",
        ] {
            assert_eq!(apply(s, Tone::Professional), s, "changed {s:?}");
        }
    }

    #[test]
    fn idempotent() {
        for t in [Tone::Casual, Tone::Standard, Tone::Professional] {
            let once = apply("Yeah, honestly, I'm gonna say it's, like, done! Dunno, imma check.", t);
            assert_eq!(apply(&once, t), once);
        }
    }
}
