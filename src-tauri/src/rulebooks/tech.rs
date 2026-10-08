//! Book 5 - Tech. Links, emails, file names and code, only when the speaker spells them out.
//!
//! T1 Links: "https colon slash slash", "www dot", "<name> dot <com|org|io|...>", then "slash <part>".
//! T2 Emails: "<name> at <domain>.<tld>" -> "name@domain.tld", but ONLY when the domain part was
//!    actually spoken with "dot" + a known ending. "Farhan at accounts" and "Ravi at the office" stay.
//! T3 File extensions: "config dot ts" -> "config.ts", "the dot ts file" -> "the .ts file".
//! T4 Code casing commands: "camel case user profile" -> "userProfile" (also snake, kebab, pascal,
//!    screaming snake). Only when the command words are said.
//! T5 Code operators said by name: plus equals, minus equals, double/triple equals, double ampersand,
//!    double pipe, fat arrow.
//! T6 Keyboard shortcuts: "control c" -> "Ctrl+C", "command s" -> "Cmd+S", "alt tab" -> "Alt+Tab".
//! T7 Ports: "localhost colon 3000" -> "localhost:3000".
//! T8 Powers: "x squared" -> "x²", "5 cubed" -> "5³" (only after a 1-2 character term).
//! Never: invents an @, a dot or a slash the speaker did not say.

use super::{core_lower, per_line, split3};

const TLDS: &[&str] = &[
    "com", "org", "net", "io", "ai", "dev", "app", "edu", "gov", "co", "in", "uk", "de", "me", "us", "ca", "au",
    "info", "xyz", "tech", "so", "gg", "tv", "ly", "sh", "fr", "jp", "nz", "sg",
];
const EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "rs", "py", "ipynb", "json", "toml", "yaml", "yml", "md", "html",
    "css", "scss", "sql", "sh", "bat", "ps1", "env", "lock", "txt", "csv", "xml", "pdf", "docx", "xlsx", "pptx",
    "png", "jpg", "jpeg", "svg", "gif", "mp3", "mp4", "wav", "zip", "exe", "dll", "go", "java", "kt", "swift",
    "c", "cpp", "h", "hpp", "cs", "rb", "php", "vue", "svelte", "dart", "lua", "ini", "cfg", "log", "gitignore",
    "dockerfile",
];
/// A file extension is not glued onto these words ("in the dot ts file" -> "in the .ts file").
const NOT_A_FILENAME: &[&str] = &[
    "the", "a", "an", "this", "that", "these", "those", "my", "your", "our", "their", "his", "her", "its", "in",
    "into", "to", "from", "of", "on", "at", "with", "for", "and", "or", "open", "a", "any", "each", "every",
];

pub fn apply(text: &str) -> String {
    per_line(text, |line| {
        let toks: Vec<String> = line.split_whitespace().map(String::from).collect();
        let toks = links(toks);
        let toks = emails(toks);
        let toks = extensions(toks);
        let toks = code_casing(toks);
        let toks = phrases(toks);
        let toks = ports_and_powers(toks);
        toks.join(" ")
    })
}

fn is_bare_word(t: &str) -> bool {
    let (l, c, r) = split3(t);
    l.is_empty() && r.is_empty() && !c.is_empty()
}

/// T1.
fn links(toks: Vec<String>) -> Vec<String> {
    let lower: Vec<String> = toks.iter().map(|t| core_lower(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        // scheme: "https colon slash slash" (Whisper may also write "https://")
        if matches!(lower[i].as_str(), "http" | "https") && lower.get(i + 1).map(|s| s.as_str()) == Some("colon")
            && lower.get(i + 2).map(|s| s.as_str()) == Some("slash") && lower.get(i + 3).map(|s| s.as_str()) == Some("slash")
        {
            out.push(format!("{}://", lower[i]));
            i += 4;
            continue;
        }
        // "w w w dot" / "www dot"
        if lower[i] == "w" && lower.get(i + 1).map(|s| s.as_str()) == Some("w") && lower.get(i + 2).map(|s| s.as_str()) == Some("w")
            && lower.get(i + 3).map(|s| s.as_str()) == Some("dot")
        {
            glue(&mut out, "www.");
            i += 4;
            continue;
        }
        if lower[i] == "www" && lower.get(i + 1).map(|s| s.as_str()) == Some("dot") && is_bare_word(&toks[i]) {
            glue(&mut out, "www.");
            i += 2;
            continue;
        }
        // "<name> dot <tld>": glue onto the previous word
        if lower[i] == "dot" && is_bare_word(&toks[i]) && i + 1 < toks.len() && !out.is_empty() {
            let (_, next_core, next_trail) = split3(&toks[i + 1]);
            let prev_ok = out.last().map_or(false, |p| is_bare_word(p) || p.ends_with('.') && !p.ends_with("..") || p.ends_with("://"));
            if prev_ok && TLDS.contains(&next_core.to_lowercase().as_str()) && !NOT_A_FILENAME.contains(&core_lower(out.last().unwrap()).as_str()) {
                let last = out.last_mut().unwrap();
                last.push('.');
                last.push_str(&next_core.to_lowercase());
                last.push_str(next_trail);
                i += 2;
                // path parts: "slash docs"
                while i + 1 < toks.len() && lower[i] == "slash" && is_bare_word(&toks[i]) && !split3(&toks[i + 1]).1.is_empty() {
                    let (_, c, t) = split3(&toks[i + 1]);
                    let last = out.last_mut().unwrap();
                    let trimmed = last.trim_end_matches(|ch: char| matches!(ch, ',' | '.' | '?' | '!')).to_string();
                    *last = format!("{trimmed}/{}{t}", c.to_lowercase());
                    i += 2;
                }
                continue;
            }
        }
        if out.last().map_or(false, |p| p.ends_with("://") || p.ends_with("www.")) {
            let last = out.last_mut().unwrap();
            last.push_str(&toks[i].to_lowercase());
            i += 1;
            continue;
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

fn glue(out: &mut Vec<String>, piece: &str) {
    match out.last_mut() {
        Some(last) if last.ends_with("://") => last.push_str(piece),
        _ => out.push(piece.to_string()),
    }
}

/// T2. "<name> at <domain.tld>" where the domain token already holds a dot + known ending.
fn emails(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if core_lower(&toks[i]) == "at" && is_bare_word(&toks[i]) && !out.is_empty() && i + 1 < toks.len() {
            let (_, dom, trail) = split3(&toks[i + 1]);
            let tld = dom.rsplit('.').next().unwrap_or("").to_lowercase();
            let is_domain = dom.contains('.') && TLDS.contains(&tld.as_str()) && !dom.contains("://") && !dom.starts_with('.');
            // the user part: the previous word, plus "<w> dot <w>" chains before it ("john dot doe")
            if is_domain && is_bare_word(out.last().unwrap()) {
                let mut user = out.pop().unwrap().to_lowercase();
                while out.len() >= 2 && core_lower(out.last().unwrap()) == "dot" && is_bare_word(&out[out.len() - 2]) {
                    out.pop();
                    let part = out.pop().unwrap().to_lowercase();
                    user = format!("{part}.{user}");
                }
                out.push(format!("{user}@{}{trail}", dom.to_lowercase()));
                i += 2;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// T3.
fn extensions(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let (ext_tok, used) = if core_lower(&toks[i]) == "dot" && is_bare_word(&toks[i]) && i + 1 < toks.len() {
            (toks[i + 1].as_str(), 2)
        } else if let Some(rest) = toks[i].strip_prefix('.') {
            (rest, 1)
        } else {
            ("", 0)
        };
        let (l, ext, r) = split3(ext_tok);
        if used > 0 && l.is_empty() && EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
            let piece = format!(".{}{r}", ext.to_lowercase());
            let glue = out.last().map_or(false, |p| {
                p.ends_with(|c: char| c.is_alphanumeric()) && !NOT_A_FILENAME.contains(&core_lower(p).as_str())
            });
            match out.last_mut() {
                Some(prev) if glue => prev.push_str(&piece),
                _ => out.push(piece),
            }
            i += used;
            continue;
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// T4.
fn code_casing(toks: Vec<String>) -> Vec<String> {
    const STOP: &[&str] = &[
        "and", "the", "in", "for", "to", "at", "on", "with", "from", "by", "now", "then", "please", "here", "there",
        "is", "are", "was", "were", "a", "an", "of", "or", "but", "so", "as", "into",
    ];
    let lower: Vec<String> = toks.iter().map(|t| core_lower(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let mode = match (lower[i].as_str(), lower.get(i + 1).map(|s| s.as_str()).unwrap_or("")) {
            ("camel", "case") => Some("camel"),
            ("snake", "case") => Some("snake"),
            ("kebab", "case") => Some("kebab"),
            ("pascal", "case") => Some("pascal"),
            ("screaming", "snake") => Some("screaming"),
            _ => None,
        };
        if let (Some(m), true) = (mode, mode.is_some() && is_bare_word(&toks[i]) && is_bare_word(&toks[i + 1])) {
            let mut j = i + 2;
            if m == "screaming" && lower.get(j).map(|s| s.as_str()) == Some("case") {
                j += 1;
            }
            let mut words: Vec<String> = Vec::new();
            let mut trail = String::new();
            while j < toks.len() && words.len() < 5 {
                let (_, c, r) = split3(&toks[j]);
                if c.is_empty() || (!words.is_empty() && STOP.contains(&c.to_lowercase().as_str())) {
                    break;
                }
                words.push(c.to_lowercase());
                j += 1;
                if !r.is_empty() {
                    trail = r.to_string();
                    break;
                }
            }
            if !words.is_empty() {
                let ident = match m {
                    "camel" => words.iter().enumerate().map(|(k, w)| if k == 0 { w.clone() } else { super::cap_first(w) }).collect(),
                    "pascal" => words.iter().map(|w| super::cap_first(w)).collect(),
                    "snake" => words.join("_"),
                    "kebab" => words.join("-"),
                    _ => words.join("_").to_uppercase(),
                };
                out.push(format!("{ident}{trail}"));
                i = j;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// T5 + T6: multi-word names for symbols and key combos.
fn phrases(toks: Vec<String>) -> Vec<String> {
    const PAIRS: &[(&str, &str)] = &[
        ("plus equals", "+="), ("minus equals", "-="), ("double equals", "=="), ("triple equals", "==="),
        ("double ampersand", "&&"), ("double pipe", "||"), ("fat arrow", "=>"),
        ("control c", "Ctrl+C"), ("control v", "Ctrl+V"), ("control x", "Ctrl+X"), ("control z", "Ctrl+Z"),
        ("control s", "Ctrl+S"), ("control a", "Ctrl+A"), ("control f", "Ctrl+F"), ("control shift t", "Ctrl+Shift+T"),
        ("ctrl c", "Ctrl+C"), ("ctrl v", "Ctrl+V"), ("ctrl x", "Ctrl+X"), ("ctrl z", "Ctrl+Z"), ("ctrl s", "Ctrl+S"),
        ("ctrl a", "Ctrl+A"), ("ctrl f", "Ctrl+F"), ("command c", "Cmd+C"), ("command v", "Cmd+V"),
        ("command s", "Cmd+S"), ("command z", "Cmd+Z"), ("alt tab", "Alt+Tab"), ("alt f4", "Alt+F4"),
        ("shift enter", "Shift+Enter"),
    ];
    let lower: Vec<String> = toks.iter().map(|t| core_lower(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    'scan: while i < toks.len() {
        for &(phrase, sym) in PAIRS {
            let n = phrase.split(' ').count();
            if i + n <= toks.len() && lower[i..i + n].join(" ") == phrase
                && toks[i..i + n - 1].iter().all(|t| is_bare_word(t)) && split3(&toks[i]).0.is_empty()
            {
                out.push(format!("{sym}{}", split3(&toks[i + n - 1]).2));
                i += n;
                continue 'scan;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// T7 + T8.
fn ports_and_powers(toks: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let w = core_lower(&toks[i]);
        if w == "localhost" && i + 2 < toks.len() && core_lower(&toks[i + 1]) == "colon" {
            let (_, port, trail) = split3(&toks[i + 2]);
            if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
                out.push(format!("{}localhost:{port}{trail}", split3(&toks[i]).0));
                i += 3;
                continue;
            }
        }
        if (w == "squared" || w == "cubed") && !out.is_empty() {
            let prev = out.last().unwrap();
            let (_, pc, pr) = split3(prev);
            let short_term = !pc.is_empty() && pc.chars().count() <= 2 && pr.is_empty() && !matches!(pc.to_lowercase().as_str(), "a" | "i" | "is" | "it" | "be" | "so" | "to" | "of");
            if short_term {
                let sup = if w == "squared" { "²" } else { "³" };
                let trail = split3(&toks[i]).2;
                let last = out.last_mut().unwrap();
                last.push_str(sup);
                last.push_str(trail);
                i += 1;
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
            ("send to john dot doe at gmail dot com", "send to john.doe@gmail.com"),
            ("mail vivek at company dot io.", "mail vivek@company.io."),
            ("check github dot com slash repo", "check github.com/repo"),
            ("open https colon slash slash example dot com", "open https://example.com"),
            ("go to www dot stackoverflow dot com", "go to www.stackoverflow.com"),
            ("rename config dot ts", "rename config.ts"),
            ("in the dot ts file", "in the .ts file"),
            ("open README dot md", "open README.md"),
            ("define camel case user profile and snake case get user id now", "define userProfile and get_user_id now"),
            ("write i plus equals 1", "write i += 1"),
            ("just press control c to copy", "just press Ctrl+C to copy"),
            ("run it on localhost colon 3000", "run it on localhost:3000"),
            ("x squared plus y cubed", "x² plus y³"),
        ];
        for (input, want) in cases {
            assert_eq!(apply(input), want, "input {input:?}");
        }
    }

    #[test]
    fn must_not_touch() {
        for s in [
            "Email the invoice to Farhan in accounts.",
            "Send it to Farhan at accounts.",
            "Meet Ravi at the office.",
            "We were at Google yesterday.",
            "Connect the dots.",
            "Take control of the project.",
            "It's a dot on the map.",
            "The room felt squared off.",
            "Version 2.0 shipped.",
        ] {
            assert_eq!(apply(s), s, "changed {s:?}");
        }
    }

    #[test]
    fn idempotent() {
        let s = "mail ana at outlook dot com about config dot ts";
        let once = apply(s);
        assert_eq!(once, "mail ana@outlook.com about config.ts");
        assert_eq!(apply(&once), once);
    }
}
