//! Touch Up: fixes misspelled words in a dictation the user already pasted. Spell-check only: no
//! rephrasing, no grammar, no AI (Yash, 2026-10-06). The lite model writes real words almost always,
//! so this only catches true typos ("recieve" -> "receive"), and it is built to leave everything else alone:
//! - only plain lowercase words of 3+ letters are checked (names, acronyms, numbers, emails, file
//!   names and contractions are never touched);
//! - a word the dictionary knows, or one on the keep list (Indian English, Hinglish, chat words), stays;
//! - the fix must be a common word (`MIN_COUNT`), 1 edit away, or 2 edits away for words of 6+ letters;
//!   among candidates, the most frequent one wins.
//! Word list: SymSpell's English frequency dictionary (80,000 words, MIT, data/en-80k.LICENSE.txt).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const WORDS: &str = include_str!("../data/en-80k.txt");
/// A replacement must be at least this common in the frequency list (counts run 3,840 .. 2.6e10).
const MIN_COUNT: u64 = 100_000;
const KEEP: &str = "lakh lakhs crore crores rupees paise ji haan han nahi nahin achha acha accha theek thik hai hain kya \
    yaar yar bhai bhaiya didi dada dadi nani nana mausi mama mami chacha chachi bua beta beti amma appa anna akka \
    thatha paati ammi abba bhabhi jiju arre arey bas chalo matlab jugaad namaste namaskar chai masala paneer dosa \
    idli vada sambar biryani roti chapati naan paratha dal daal chutney samosa pakora lassi ghee jeera haldi dhaba \
    kirana mandi bazaar puja pooja diwali holi rakhi gmail whatsapp swiggy zomato paytm phonepe gpay upi emi gst \
    aadhaar ifsc neft rtgs imps otp kyc lol omg wfh fyi asap";

fn dict() -> &'static HashMap<&'static str, u64> {
    static D: OnceLock<HashMap<&'static str, u64>> = OnceLock::new();
    D.get_or_init(|| {
        WORDS
            .lines()
            .filter_map(|l| {
                let mut p = l.split_whitespace();
                Some((p.next()?, p.next()?.parse().ok()?))
            })
            .collect()
    })
}

fn keep() -> &'static HashSet<&'static str> {
    static K: OnceLock<HashSet<&'static str>> = OnceLock::new();
    K.get_or_init(|| KEEP.split_whitespace().collect())
}

/// Every string one edit (delete, swap of neighbours, replace, insert) away from `w`.
fn edits1(w: &str) -> Vec<String> {
    let b = w.as_bytes();
    let mut out = Vec::with_capacity(54 * b.len() + 25);
    for i in 0..=b.len() {
        let (l, r) = (&b[..i], &b[i..]);
        if !r.is_empty() {
            out.push([l, &r[1..]].concat());
        }
        if r.len() > 1 {
            out.push([l, &[r[1], r[0]], &r[2..]].concat());
        }
        for c in b'a'..=b'z' {
            if !r.is_empty() && r[0] != c {
                out.push([l, &[c], &r[1..]].concat());
            }
            out.push([l, &[c], r].concat());
        }
    }
    out.into_iter().map(|v| String::from_utf8(v).expect("ascii in, ascii out")).collect()
}

fn most_common<'a>(cands: impl Iterator<Item = &'a String>) -> Option<String> {
    let d = dict();
    cands
        .filter_map(|c| d.get(c.as_str()).map(|&n| (c, n)))
        .filter(|(_, n)| *n >= MIN_COUNT)
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| c.clone())
}

/// The fix for one word, or None to leave it exactly as it is.
pub fn fix_word(w: &str) -> Option<String> {
    if w.len() < 3 || !w.bytes().all(|b| b.is_ascii_lowercase()) || dict().contains_key(w) || keep().contains(w) {
        return None;
    }
    let e1 = edits1(w);
    if let Some(c) = most_common(e1.iter()) {
        return Some(c);
    }
    if w.len() < 6 {
        return None;
    }
    let e2: Vec<String> = e1.iter().flat_map(|e| edits1(e)).collect();
    most_common(e2.iter())
}

/// Spell-fixes `text`, keeping every space, line break and punctuation mark where it was.
pub fn touch_up(text: &str) -> String {
    crate::rulebooks::per_line(text, |line| {
        line.split(' ')
            .map(|tok| {
                let (l, core, r) = crate::rulebooks::split3(tok);
                match fix_word(core) {
                    Some(fixed) => format!("{l}{fixed}{r}"),
                    None => tok.to_string(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixes_real_typos() {
        assert_eq!(touch_up("I will recieve it tommorow."), "I will receive it tomorrow.");
        assert_eq!(touch_up("teh report is definately seperate"), "the report is definitely separate");
    }

    #[test]
    fn leaves_everything_else_alone() {
        let keep = [
            "Meet Ankit in Gachibowli at 7:30 PM.",
            "Send the PDF to kavitha.rao@gmail.com and open auth_service.py.",
            "I don't think we're gonna make it, yaar.",
            "Get chai and paneer, it's 18 lakhs, haan.",
            "Didi said the EMI is due.",
            "Line one\nline  two with two spaces",
        ];
        for s in keep {
            assert_eq!(touch_up(s), s, "touch up changed {s:?}");
        }
    }

    #[test]
    fn idempotent() {
        let once = touch_up("recieve teh seperate wierd things");
        assert_eq!(touch_up(&once), once);
    }
}
