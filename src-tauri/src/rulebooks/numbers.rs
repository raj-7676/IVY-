//! Book 4 - Numbers. Numbers are always written as digits (Yash's rule, 2026-10-02).
//!
//! N1 Dates first: "March twenty first" / "the twenty first of March" -> "March 21st". Lowercase
//!    "may" / "march" count as months only after in/on/by/until/since/from/of/next/last/this/early/late/mid
//!    or at the start of a sentence ("you may first check" stays).
//! N2 Years: "twenty twenty six" -> 2026, "nineteen ninety nine" -> 1999 (1950-2099 only).
//! N3 Times: "seven thirty pm" -> "7:30 PM", "five pm" / "5 p.m." / "5pm" -> "5 PM", "7.30 pm" -> "7:30 PM".
//! N4 Number words -> digits: "two" -> 2, "twenty five" -> 25, "fifteen hundred" -> 1500,
//!    "two thousand five hundred" -> 2500, "a hundred and fifty" -> 150, "one point five" -> 1.5,
//!    3+ single digits in a row -> one digit string ("six three seven nine" -> 6379).
//!    Exceptions that stay words: "one" / "zero" alone (they are pronouns and idioms: "no one",
//!    "the one", "one of them") unless a unit, currency, percent or time word follows; "the two of us",
//!    "a day or two"; ordinals ("first", "second") outside dates.
//! N5 Percent, money, units after a number: "20 percent" -> "20%", "5 dollars" -> "$5",
//!    "500 rupees" -> "₹500", "10 euros" -> "€10", "5 kilometers" -> "5 km", "128 gig" -> "128 GB",
//!    "60 miles per hour" -> "60 mph", "20 degrees celsius" -> "20°C". "pounds" stays (weight or money?).
//! N6 Simple math only between numbers: "5 plus 10 equals 15" -> "5 + 10 = 15" ("2 times a day" stays).
//! N7 Phone country codes: "plus 91 98765 43210" -> "+91 98765 43210".
//! N8 Big round amounts get the scale word: "18,00,000" -> "18 lakhs", "₹1,15,000" -> "₹1.15 lakhs",
//!    "2,50,00,000" -> "2.5 crores", "$2,000,000" -> "$2 million" ("1,15,437" and "68,990" stay).

use super::{core_lower, per_line, split3};

const SMALL: &[(&str, u64)] = &[
    ("zero", 0), ("one", 1), ("two", 2), ("three", 3), ("four", 4), ("five", 5), ("six", 6), ("seven", 7),
    ("eight", 8), ("nine", 9), ("ten", 10), ("eleven", 11), ("twelve", 12), ("thirteen", 13), ("fourteen", 14),
    ("fifteen", 15), ("sixteen", 16), ("seventeen", 17), ("eighteen", 18), ("nineteen", 19),
];
const TENS: &[(&str, u64)] = &[
    ("twenty", 20), ("thirty", 30), ("forty", 40), ("fifty", 50), ("sixty", 60), ("seventy", 70), ("eighty", 80),
    ("ninety", 90),
];
const SCALES: &[(&str, u64)] = &[("thousand", 1_000), ("million", 1_000_000), ("billion", 1_000_000_000)];
const ORDINALS: &[(&str, u64)] = &[
    ("first", 1), ("second", 2), ("third", 3), ("fourth", 4), ("fifth", 5), ("sixth", 6), ("seventh", 7),
    ("eighth", 8), ("ninth", 9), ("tenth", 10), ("eleventh", 11), ("twelfth", 12), ("thirteenth", 13),
    ("fourteenth", 14), ("fifteenth", 15), ("sixteenth", 16), ("seventeenth", 17), ("eighteenth", 18),
    ("nineteenth", 19), ("twentieth", 20), ("thirtieth", 30),
];
const MONTHS: &[&str] = &[
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
    "december",
];
const MONTH_LEAD: &[&str] = &[
    "in", "on", "by", "until", "till", "since", "from", "of", "next", "last", "this", "early", "late", "mid",
    "before", "after", "through", "every", "during",
];
/// Words after "one"/"zero" that make it a quantity, so it becomes a digit.
const UNIT_WORDS: &[&str] = &[
    "percent", "per", "dollar", "dollars", "rupee", "rupees", "euro", "euros", "yen", "hour", "hours", "minute",
    "minutes", "second", "seconds", "week", "weeks", "month", "months", "year", "years", "am", "pm", "a.m", "p.m",
    "o'clock", "kilometer", "kilometers", "kilometre", "kilometres", "km", "kilogram", "kilograms", "kg", "gram",
    "grams", "meter", "meters", "metre", "metres", "mile", "miles", "liter", "liters", "litre", "litres",
    "gb", "mb", "tb", "kb", "gig", "gigs", "gigabyte", "gigabytes", "megabyte", "megabytes", "terabyte",
    "terabytes", "degree", "degrees", "point", "inch", "inches", "foot", "feet", "lakh", "crore", "k",
];

fn small(w: &str) -> Option<u64> {
    SMALL.iter().find(|(k, _)| *k == w).map(|&(_, v)| v)
}
fn tens(w: &str) -> Option<u64> {
    TENS.iter().find(|(k, _)| *k == w).map(|&(_, v)| v)
}
fn scale(w: &str) -> Option<u64> {
    SCALES.iter().find(|(k, _)| *k == w).map(|&(_, v)| v)
}
fn is_number_word(w: &str) -> bool {
    small(w).is_some() || tens(w).is_some() || scale(w).is_some() || w == "hundred"
}
fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == ',' || c == '.') && s.chars().any(|c| c.is_ascii_digit())
}
fn bare(tok: &str) -> bool {
    let (l, _, r) = split3(tok);
    l.is_empty() && r.is_empty()
}

pub fn apply(text: &str) -> String {
    per_line(text, |line| {
        let toks: Vec<String> = line.split_whitespace().map(String::from).collect();
        let toks = dates(toks);
        let toks = years(toks);
        let toks = times(toks);
        let toks = number_words(toks);
        let toks = after_number(toks);
        let toks = math(toks);
        let toks = phone(toks);
        let toks = big_amounts(toks);
        toks.join(" ")
    })
}

// ---------------------------------------------------------------- N8 big round amounts

/// Big round amounts read better with the scale word than with five or six zeros (Yash, 2026-10-06):
/// Indian grouping or ₹ -> lakhs/crores ("18,00,000" -> "18 lakhs", "₹1,15,000" -> "₹1.15 lakhs",
/// "2,50,00,000" -> "2.5 crores"); Western grouping or $ € £ -> million/billion ("$2,000,000" -> "$2 million").
/// Only from 1 lakh / 1 million up, only when at most 2 decimals say it exactly ("1,15,437" stays),
/// and only for numbers written with commas or a currency sign (phone numbers and IDs never are).
fn big_amounts(toks: Vec<String>) -> Vec<String> {
    toks.into_iter()
        .map(|tok| {
            let (l, core, r) = split3(&tok);
            let groups: Vec<&str> = core.split(',').collect();
            if core.is_empty() || !groups.iter().all(|g| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit())) {
                return tok.clone();
            }
            let Ok(v) = groups.concat().parse::<u64>() else { return tok.clone() };
            let indian_commas = groups.len() >= 3 && groups.last().map_or(false, |g| g.len() == 3)
                && groups[1..groups.len() - 1].iter().all(|g| g.len() == 2);
            let western_commas = groups.len() >= 3 && groups[1..].iter().all(|g| g.len() == 3);
            let indian = l.ends_with('₹') || (indian_commas && !l.ends_with(['$', '€', '£']));
            let western = !indian && (l.ends_with(['$', '€', '£']) || western_commas);
            let scales: &[(u64, &str, &str)] = if indian {
                &[(10_000_000, "crore", "crores"), (100_000, "lakh", "lakhs")]
            } else if western {
                &[(1_000_000_000, "billion", "billion"), (1_000_000, "million", "million")]
            } else {
                return tok.clone();
            };
            for &(unit, one, many) in scales {
                if v >= unit && v % (unit / 100) == 0 {
                    let x = format!("{:.2}", v as f64 / unit as f64);
                    let x = x.trim_end_matches('0').trim_end_matches('.');
                    let word = if x == "1" { one } else { many };
                    return format!("{l}{x} {word}{r}");
                }
            }
            tok.clone()
        })
        .collect()
}

// ---------------------------------------------------------------- N1 dates

/// Parses an ordinal day at toks[i]: "first", "twenty first", "21st", "21". Returns (day, tokens used).
fn ordinal_day(toks: &[String], i: usize) -> Option<(u64, usize)> {
    let w = core_lower(toks.get(i)?);
    if let Some(&(_, d)) = ORDINALS.iter().find(|(k, _)| *k == w) {
        return Some((d, 1));
    }
    if (w == "twenty" || w == "thirty") && bare(&toks[i]) {
        let base = if w == "twenty" { 20 } else { 30 };
        let n = core_lower(toks.get(i + 1)?);
        if let Some(&(_, d)) = ORDINALS.iter().find(|(k, _)| *k == n) {
            if d < 10 && base + d <= 31 {
                return Some((base + d, 2));
            }
        }
    }
    let digits: String = w.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &w[digits.len()..];
    if !digits.is_empty() && matches!(rest, "" | "st" | "nd" | "rd" | "th") {
        let d: u64 = digits.parse().ok()?;
        if (1..=31).contains(&d) && !rest.is_empty() {
            return Some((d, 1));
        }
    }
    None
}

fn suffix(d: u64) -> &'static str {
    match (d % 10, d % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

fn month_at(toks: &[String], i: usize) -> Option<String> {
    let w = core_lower(&toks[i]);
    if !MONTHS.contains(&w.as_str()) {
        return None;
    }
    let capitalized = toks[i].chars().find(|c| c.is_alphabetic()).map_or(false, |c| c.is_uppercase());
    if (w == "may" || w == "march") && !capitalized {
        let prev = if i > 0 { core_lower(&toks[i - 1]) } else { String::new() };
        if !MONTH_LEAD.contains(&prev.as_str()) {
            return None;
        }
    }
    Some(super::cap_first(&w))
}

fn dates(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        // "March twenty first" / "March 21st"
        if let Some(month) = month_at(&toks, i) {
            if bare(&toks[i]) {
                if let Some((d, used)) = ordinal_day(&toks, i + 1) {
                    let trail = split3(&toks[i + used]).2;
                    out.push(format!("{}{month}", split3(&toks[i]).0));
                    out.push(format!("{d}{}{trail}", suffix(d)));
                    i += 1 + used;
                    continue;
                }
            }
        }
        // "the twenty first of March" -> "March 21st"
        if let Some((d, used)) = ordinal_day(&toks, i) {
            if i + used + 1 < toks.len() && core_lower(&toks[i + used]) == "of" && bare(&toks[i + used - 1]) {
                if let Some(month) = month_at(&toks, i + used + 1) {
                    if out.last().map(|t| core_lower(t)) == Some("the".into()) && bare(out.last().unwrap()) {
                        out.pop();
                    }
                    let trail = split3(&toks[i + used + 1]).2;
                    out.push(month);
                    out.push(format!("{d}{}{trail}", suffix(d)));
                    i += used + 2;
                    continue;
                }
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- N2 years

fn years(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let w = core_lower(&toks[i]);
        let century = match w.as_str() {
            "twenty" => Some(20),
            "nineteen" => Some(19),
            _ => None,
        };
        if let (Some(c), true) = (century, bare(&toks[i])) {
            // the rest must be a plain 10..99 number of 1-2 words, not followed by a scale word
            let a = toks.get(i + 1).map(|t| core_lower(t)).unwrap_or_default();
            let b = toks.get(i + 2).map(|t| core_lower(t)).unwrap_or_default();
            let (val, used) = if let Some(t) = tens(&a) {
                match small(&b) {
                    Some(u) if (1..=9).contains(&u) && bare(&toks[i + 1]) => (t + u, 2),
                    _ => (t, 1),
                }
            } else if let Some(s) = small(&a) {
                (s, 1)
            } else {
                (0, 0)
            };
            let next_after = toks.get(i + 1 + used).map(|t| core_lower(t)).unwrap_or_default();
            let year = c * 100 + val;
            if used > 0 && val >= 10 && (1950..=2099).contains(&year) && !is_number_word(&next_after) && next_after != "hundred" {
                let lead = split3(&toks[i]).0;
                let trail = split3(&toks[i + used]).2;
                out.push(format!("{lead}{year}{trail}"));
                i += 1 + used;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- N3 times

/// am/pm at toks[i]: "pm", "p.m.", "PM", or the two tokens "p m". Returns ("PM", used).
fn meridiem(toks: &[String], i: usize) -> Option<(&'static str, usize, String)> {
    let t = toks.get(i)?;
    let compact: String = t.to_lowercase().chars().filter(|c| c.is_alphabetic()).collect();
    let trail_of = |tok: &str| -> String {
        let r = split3(tok).2.to_string();
        r.trim_start_matches('.').to_string()
    };
    match compact.as_str() {
        "am" => return Some(("AM", 1, trail_of(t))),
        "pm" => return Some(("PM", 1, trail_of(t))),
        "a" | "p" => {
            let n = toks.get(i + 1)?;
            let nc: String = n.to_lowercase().chars().filter(|c| c.is_alphabetic()).collect();
            if nc == "m" && bare(t.trim_end_matches('.')) {
                return Some((if compact == "a" { "AM" } else { "PM" }, 2, trail_of(n)));
            }
        }
        _ => {}
    }
    None
}

fn hour_word(w: &str) -> Option<u64> {
    small(w).filter(|&h| (1..=12).contains(&h))
}

/// Minutes said after an hour: "fifteen", "thirty", "forty five", "oh five" -> (minutes, used).
fn minutes(toks: &[String], i: usize) -> Option<(u64, usize)> {
    let a = core_lower(toks.get(i)?);
    if a == "oh" || a == "o" {
        let b = core_lower(toks.get(i + 1)?);
        return small(&b).filter(|&m| (1..=9).contains(&m)).map(|m| (m, 2));
    }
    if let Some(t) = tens(&a).filter(|&t| t <= 50) {
        if let Some(b) = toks.get(i + 1) {
            if bare(&toks[i]) {
                if let Some(u) = small(&core_lower(b)).filter(|&u| (1..=9).contains(&u)) {
                    return Some((t + u, 2));
                }
            }
        }
        return Some((t, 1));
    }
    small(&a).filter(|&m| (10..=19).contains(&m)).map(|m| (m, 1))
}

fn times(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let w = core_lower(&toks[i]);
        let lead = split3(&toks[i]).0.to_string();
        // spoken hour (+ minutes) + am/pm
        if let Some(h) = hour_word(&w) {
            if bare(&toks[i]) || meridiem(&toks, i + 1).is_some() {
                if let Some((mer, used, trail)) = meridiem(&toks, i + 1) {
                    out.push(format!("{lead}{h}"));
                    out.push(format!("{mer}{trail}"));
                    i += 1 + used;
                    continue;
                }
                if let Some((m, mused)) = minutes(&toks, i + 1) {
                    if let Some((mer, used, trail)) = meridiem(&toks, i + 1 + mused) {
                        out.push(format!("{lead}{h}:{m:02}"));
                        out.push(format!("{mer}{trail}"));
                        i += 1 + mused + used;
                        continue;
                    }
                }
            }
        }
        // written time: "5pm", "5:30pm", "7.30" + am/pm, "5" + "p.m."
        let core = split3(&toks[i]).1.to_lowercase();
        let digits: String = core.chars().take_while(|c| c.is_ascii_digit() || *c == ':' || *c == '.').collect();
        let rest: String = core[digits.len()..].chars().filter(|c| c.is_alphabetic()).collect();
        if !digits.is_empty() && digits.chars().next().unwrap().is_ascii_digit() {
            let clock = normalize_clock(&digits);
            if let (Some(clock), true) = (clock.clone(), rest == "am" || rest == "pm") {
                let trail = split3(&toks[i]).2.trim_start_matches('.').to_string();
                out.push(format!("{lead}{clock}"));
                out.push(format!("{}{trail}", rest.to_uppercase()));
                i += 1;
                continue;
            }
            if let (Some(clock), true, Some((mer, used, trail))) = (clock, rest.is_empty() && split3(&toks[i]).2.is_empty(), meridiem(&toks, i + 1)) {
                out.push(format!("{lead}{clock}"));
                out.push(format!("{mer}{trail}"));
                i += 1 + used;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// "7" -> "7", "7:30" -> "7:30", "7.30" -> "7:30" (a clock time, not a decimal, because am/pm follows).
fn normalize_clock(d: &str) -> Option<String> {
    let parts: Vec<&str> = d.split(|c| c == ':' || c == '.').collect();
    let h: u64 = parts.first()?.parse().ok()?;
    if !(1..=12).contains(&h) {
        return None;
    }
    match parts.len() {
        1 => Some(h.to_string()),
        2 => {
            let m: u64 = parts[1].parse().ok()?;
            if m < 60 && parts[1].len() == 2 {
                Some(format!("{h}:{m:02}"))
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- N4 number words

#[derive(PartialEq, Clone, Copy)]
enum Last {
    Start,
    A,
    Small,
    Tens,
    TensUnit,
    Hundred,
    Scale,
    And,
}

/// Parses a cardinal at toks[i..]. Returns (value, tokens used). Punctuation inside stops it.
fn parse_cardinal(words: &[String], toks: &[String], i: usize) -> Option<(u64, usize)> {
    let (mut total, mut cur, mut last, mut used) = (0u64, 0u64, Last::Start, 0usize);
    let mut j = i;
    while j < words.len() {
        let w = words[j].as_str();
        let next = words.get(j + 1).map(|s| s.as_str()).unwrap_or("");
        let ok = if let Some(n) = small(w) {
            let fits = matches!(last, Last::Start | Last::Hundred | Last::Scale | Last::And)
                || (last == Last::Tens && (1..=9).contains(&n));
            if fits {
                cur += n;
                last = if last == Last::Tens { Last::TensUnit } else { Last::Small };
            }
            fits
        } else if let Some(t) = tens(w) {
            let fits = matches!(last, Last::Start | Last::Hundred | Last::Scale | Last::And);
            if fits {
                cur += t;
                last = Last::Tens;
            }
            fits
        } else if w == "hundred" {
            let fits = matches!(last, Last::A | Last::Small | Last::Tens | Last::TensUnit) && cur % 100 == cur;
            if fits {
                cur = cur.max(1) * 100;
                last = Last::Hundred;
            }
            fits
        } else if let Some(s) = scale(w) {
            let fits = matches!(last, Last::A | Last::Small | Last::Tens | Last::TensUnit | Last::Hundred);
            if fits {
                total += cur.max(1) * s;
                cur = 0;
                last = Last::Scale;
            }
            fits
        } else if w == "and" {
            matches!(last, Last::Hundred | Last::Scale) && (small(next).is_some() || tens(next).is_some()) && {
                last = Last::And;
                true
            }
        } else if w == "a" {
            last == Last::Start && (next == "hundred" || scale(next).is_some()) && {
                last = Last::A;
                true
            }
        } else {
            false
        };
        if !ok {
            break;
        }
        j += 1;
        used = j - i;
        // punctuation after this word ends the number ("two, three")
        if !split3(&toks[j - 1]).2.is_empty() {
            break;
        }
    }
    // never end on "and" / "a"
    while used > 0 && matches!(words[i + used - 1].as_str(), "and" | "a") {
        used -= 1;
    }
    if used == 0 {
        return None;
    }
    if matches!(last, Last::A) {
        return None;
    }
    Some((total + cur, used))
}

fn number_words(toks: Vec<String>) -> Vec<String> {
    // words of hyphenated numbers ("twenty-five") are split first
    let mut flat: Vec<String> = Vec::new();
    for t in toks {
        let (l, c, r) = split3(&t);
        let lc = c.to_lowercase();
        if lc.contains('-') && lc.split('-').all(|p| is_number_word(p)) {
            let parts: Vec<&str> = c.split('-').collect();
            for (k, p) in parts.iter().enumerate() {
                let lead = if k == 0 { l } else { "" };
                let trail = if k + 1 == parts.len() { r } else { "" };
                flat.push(format!("{lead}{p}{trail}"));
            }
        } else {
            flat.push(t);
        }
    }
    let words: Vec<String> = flat.iter().map(|t| core_lower(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < flat.len() {
        // 3+ single digits said one by one: a code, PIN, port or phone number
        let mut k = i;
        while k < flat.len() && small(&words[k]).map_or(false, |d| d <= 9) && (k == i || split3(&flat[k - 1]).2.is_empty()) {
            k += 1;
        }
        if k - i >= 3 {
            let digits: String = words[i..k].iter().map(|w| small(w).unwrap().to_string()).collect();
            out.push(format!("{}{digits}{}", split3(&flat[i]).0, split3(&flat[k - 1]).2));
            i = k;
            continue;
        }
        if let Some((value, used)) = parse_cardinal(&words, &flat, i) {
            let first = words[i].as_str();
            let next = words.get(i + used).map(|s| s.as_str()).unwrap_or("");
            let next2 = words.get(i + used + 1).map(|s| s.as_str()).unwrap_or("");
            let prev = if i > 0 { words[i - 1].as_str() } else { "" };
            let prev_is_num = i > 0 && (is_digits(prev) || is_number_word(prev));
            let next_is_num = is_digits(next) || is_number_word(next);
            // "one" / "zero" alone are words unless a quantity follows ("one hour") or they sit in a list of numbers
            let lone_pronoun = used == 1 && (first == "one" || first == "zero")
                && !UNIT_WORDS.contains(&next) && !prev_is_num && !next_is_num;
            // "the two of us", "a day or two"
            let idiom = (next == "of" && matches!(next2, "us" | "them" | "you" | "these" | "those"))
                || (prev == "or" && (next.is_empty() || !split3(&flat[i + used - 1]).2.is_empty()));
            if lone_pronoun || idiom {
                out.push(flat[i].clone());
                i += 1;
                continue;
            }
            // decimals: "one point five", "2 point 0"
            let mut text = value.to_string();
            let mut extra = 0;
            if next == "point" && split3(&flat[i + used - 1]).2.is_empty() {
                let mut dec = String::new();
                let mut k = i + used + 1;
                while k < flat.len() && (small(&words[k]).map_or(false, |d| d <= 9) || words[k].len() == 1 && is_digits(&words[k])) {
                    dec.push_str(&small(&words[k]).map_or(words[k].clone(), |d| d.to_string()));
                    k += 1;
                    if !split3(&flat[k - 1]).2.is_empty() {
                        break;
                    }
                }
                if !dec.is_empty() {
                    text = format!("{value}.{dec}");
                    extra = k - (i + used);
                }
            }
            // digits stay ungrouped (10000, not 10,000), like the training targets
            let lead = split3(&flat[i]).0;
            let trail = split3(&flat[i + used + extra - 1]).2;
            out.push(format!("{lead}{text}{trail}"));
            i += used + extra;
            continue;
        }
        out.push(flat[i].clone());
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- N5 percent, money, units

fn after_number(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let (lead, core, trail) = split3(&toks[i]);
        if is_digits(core) && trail.is_empty() && i + 1 < toks.len() {
            let n1 = core_lower(&toks[i + 1]);
            let n2 = toks.get(i + 2).map(|t| core_lower(t)).unwrap_or_default();
            let n3 = toks.get(i + 3).map(|t| core_lower(t)).unwrap_or_default();
            let t1 = split3(&toks[i + 1]).2;
            let bare1 = split3(&toks[i + 1]).0.is_empty();
            let emit = |s: String, used: usize, out: &mut Vec<String>| {
                let t = split3(&toks[i + used]).2;
                out.push(format!("{lead}{s}{t}"));
                used
            };
            let used = if !bare1 {
                0
            } else if n1 == "percent" || (n1 == "per" && n2 == "cent") {
                emit(format!("{core}%"), if n1 == "per" { 2 } else { 1 }, &mut out)
            } else if matches!(n1.as_str(), "dollars" | "dollar") {
                emit(format!("${core}"), 1, &mut out)
            } else if matches!(n1.as_str(), "rupees" | "rupee") {
                emit(format!("₹{core}"), 1, &mut out)
            } else if matches!(n1.as_str(), "euros" | "euro") {
                emit(format!("€{core}"), 1, &mut out)
            } else if n1 == "yen" {
                emit(format!("¥{core}"), 1, &mut out)
            } else if n1 == "miles" && n2 == "per" && n3 == "hour" {
                emit(format!("{core} mph"), 3, &mut out)
            } else if matches!(n1.as_str(), "kilometers" | "kilometres") && n2 == "per" && n3 == "hour" {
                emit(format!("{core} km/h"), 3, &mut out)
            } else if matches!(n1.as_str(), "degrees" | "degree") && matches!(n2.as_str(), "celsius" | "centigrade") && t1.is_empty() {
                emit(format!("{core}°C"), 2, &mut out)
            } else if matches!(n1.as_str(), "degrees" | "degree") && n2 == "fahrenheit" && t1.is_empty() {
                emit(format!("{core}°F"), 2, &mut out)
            } else if matches!(n1.as_str(), "degrees" | "degree") {
                emit(format!("{core}°"), 1, &mut out)
            } else if let Some(abbr) = unit_abbrev(&n1) {
                emit(format!("{core} {abbr}"), 1, &mut out)
            } else {
                0
            };
            if used > 0 {
                i += used + 1;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

fn unit_abbrev(w: &str) -> Option<&'static str> {
    Some(match w {
        "kilometer" | "kilometers" | "kilometre" | "kilometres" | "km" => "km",
        "centimeter" | "centimeters" | "centimetre" | "centimetres" | "cm" => "cm",
        "millimeter" | "millimeters" | "millimetre" | "millimetres" | "mm" => "mm",
        "meter" | "meters" | "metre" | "metres" => "m",
        "kilogram" | "kilograms" | "kilo" | "kilos" | "kg" => "kg",
        "milligram" | "milligrams" | "mg" => "mg",
        "gram" | "grams" => "g",
        "milliliter" | "milliliters" | "millilitre" | "millilitres" | "ml" => "mL",
        "liter" | "liters" | "litre" | "litres" => "L",
        "kilobyte" | "kilobytes" | "kb" => "KB",
        "megabyte" | "megabytes" | "mb" => "MB",
        "gigabyte" | "gigabytes" | "gig" | "gigs" | "gb" => "GB",
        "terabyte" | "terabytes" | "tb" => "TB",
        _ => return None,
    })
}

// ---------------------------------------------------------------- N6 math

fn operand(tok: &str) -> bool {
    let c = split3(tok).1;
    is_digits(c) || matches!(c, "x" | "y" | "z" | "n")
}

fn math(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if !out.is_empty() && operand(out.last().unwrap()) && split3(out.last().unwrap()).2.is_empty() {
            let w: Vec<String> = (0..5).map(|k| toks.get(i + k).map(|t| core_lower(t)).unwrap_or_default()).collect();
            let (sym, used) = match (w[0].as_str(), w[1].as_str(), w[2].as_str(), w[3].as_str()) {
                ("greater", "than", "or", "equal") if w[4] == "to" => (">=", 5),
                ("less", "than", "or", "equal") if w[4] == "to" => ("<=", 5),
                ("is", "equal", "to", _) => ("=", 3),
                ("not", "equal", "to", _) => ("!=", 3),
                ("divided", "by", _, _) | ("multiplied", "by", _, _) => (if w[0] == "divided" { "/" } else { "×" }, 2),
                ("greater", "than", _, _) => (">", 2),
                ("less", "than", _, _) => ("<", 2),
                ("plus", _, _, _) => ("+", 1),
                ("minus", _, _, _) => ("-", 1),
                ("times", _, _, _) => ("×", 1),
                ("equals", _, _, _) => ("=", 1),
                _ => ("", 0),
            };
            if used > 0 && i + used < toks.len() && operand(&toks[i + used])
                && toks[i..i + used].iter().all(|t| split3(t).2.is_empty() && split3(t).0.is_empty())
            {
                out.push(sym.to_string());
                out.push(toks[i + used].clone());
                i += used + 1;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- N7 phone country codes

fn phone(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let w = core_lower(&toks[i]);
        let prev_num = out.last().map_or(false, |t| is_digits(split3(t).1));
        if w == "plus" && bare(&toks[i]) && !prev_num && i + 2 < toks.len() {
            let (l1, c1, r1) = split3(&toks[i + 1]);
            let c2 = split3(&toks[i + 2]).1;
            if l1.is_empty() && r1.is_empty() && c1.len() <= 3 && c1.chars().all(|c| c.is_ascii_digit()) && c2.len() >= 3
                && c2.chars().all(|c| c.is_ascii_digit())
            {
                out.push(format!("+{c1}"));
                i += 2;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn does() {
        let cases = [
            ("book a table for two", "book a table for 2"),
            ("we need twenty five chairs", "we need 25 chairs"),
            ("it costs fifteen hundred rupees", "it costs ₹1500"),
            ("send two thousand five hundred dollars", "send $2500"),
            ("about a hundred and fifty people", "about 150 people"),
            ("version one point five", "version 1.5"),
            ("port six three seven nine", "port 6379"),
            ("meet at seven thirty pm", "meet at 7:30 PM"),
            ("meet at five pm tomorrow", "meet at 5 PM tomorrow"),
            ("at 5 p.m. sharp", "at 5 PM sharp"),
            ("at 5pm", "at 5 PM"),
            ("at 7.30 pm", "at 7:30 PM"),
            ("call at nine oh five am", "call at 9:05 AM"),
            ("on march twenty first twenty twenty six", "on March 21st 2026"),
            ("the twenty first of March", "March 21st"),
            ("May first is a holiday", "May 1st is a holiday"),
            ("in nineteen ninety nine", "in 1999"),
            ("a discount of twenty percent", "a discount of 20%"),
            ("that's 500 rupees and 300 euros.", "that's ₹500 and €300."),
            ("run five kilometers then drink two liters", "run 5 km then drink 2 L"),
            ("the 128 gig phone", "the 128 GB phone"),
            ("driving sixty miles per hour", "driving 60 mph"),
            ("it is 20 degrees celsius", "it is 20°C"),
            ("five plus ten equals fifteen", "5 + 10 = 15"),
            ("x is greater than or equal to 0", "x is greater than or equal to 0"),
            ("if x greater than or equal to 0", "if x >= 0"),
            ("ten divided by two", "10 / 2"),
            ("start a timer for one hour", "start a timer for 1 hour"),
            ("one, two, three, go", "1, 2, 3, go"),
            ("call plus 91 98765 43210", "call +91 98765 43210"),
            ("twenty-five people", "25 people"),
        ];
        for (input, want) in cases {
            assert_eq!(apply(input), want, "input {input:?}");
        }
    }

    #[test]
    fn must_not_touch() {
        for s in [
            "No one told me.",
            "The one on the left.",
            "One of them is mine.",
            "The two of us went.",
            "Give it a day or two.",
            "For the first time.",
            "You may first check the map.",
            "We march on.",
            "I lost 20 pounds.",
            "Take it 2 times a day.",
            "Someone said hello.",
            "Bachelor's degrees matter.",
            "That's a plus for us.",
            "Version 2.0 shipped.",
        ] {
            assert_eq!(apply(s), s, "changed {s:?}");
        }
    }

    #[test]
    fn big_round_amounts_get_the_scale_word() {
        let cases = [
            ("salary is ₹18,00,000 a year", "salary is ₹18 lakhs a year"),
            ("revenue 18,00,000 this month", "revenue 18 lakhs this month"),
            ("just 1,00,000.", "just 1 lakh."),
            ("rent ₹1,15,000, paid", "rent ₹1.15 lakhs, paid"),
            ("budget 2,50,00,000", "budget 2.5 crores"),
            ("raised $2,000,000 today", "raised $2 million today"),
            ("a 2,000,000 user base", "a 2 million user base"),
            ("worth 1,500,000,000", "worth 1.5 billion"),
        ];
        for (input, want) in cases {
            assert_eq!(apply(input), want, "input {input:?}");
        }
        for s in ["It costs 68,990.", "exactly 1,15,437 votes", "call 9845012367", "UTR 456789", "18 lakhs", "$150,000 deal"] {
            assert_eq!(apply(s), s, "changed {s:?}");
        }
    }

    #[test]
    fn idempotent() {
        for s in ["meet at seven thirty pm on march twenty first", "twenty five percent of 500 rupees"] {
            let once = apply(s);
            assert_eq!(apply(&once), once);
        }
    }
}
