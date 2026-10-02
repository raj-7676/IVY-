//! Book 6 - Names and capitals. Only fixes the CASE of a word; the word itself never changes.
//!
//! P1 Brands with fixed spelling: github -> GitHub, javascript -> JavaScript, node js -> Node.js ...
//!    Only names that are never ordinary words (no "react", "slack", "notion", "rust", "excel").
//! P2 Acronyms: api -> API, pdf -> PDF ... (no "ram" - it is a name; no "ml" - it is millilitres).
//! P3 Days always; months always, except "may" / "march", which need a date word around them
//!    (book 4 handles "on may first"; here: "in may", "next march").
//! P4 Holidays, nationalities and languages, time zones ("indian standard time" -> "IST").
//! P5 Sentence starts and the word "I" are capitalized.
//! Never: lowercases a word, or touches a word not in these lists.

use super::{cap_first, core_lower, per_line, split3};

const BRANDS: &[(&str, &str)] = &[
    ("github", "GitHub"), ("gitlab", "GitLab"), ("youtube", "YouTube"), ("chatgpt", "ChatGPT"), ("openai", "OpenAI"),
    ("javascript", "JavaScript"), ("typescript", "TypeScript"), ("macos", "macOS"), ("ios", "iOS"),
    ("iphone", "iPhone"), ("ipad", "iPad"), ("icloud", "iCloud"), ("tauri", "Tauri"), ("postgresql", "PostgreSQL"),
    ("mongodb", "MongoDB"), ("mysql", "MySQL"), ("sqlite", "SQLite"), ("nodejs", "Node.js"), ("nextjs", "Next.js"),
    ("vscode", "VS Code"), ("kubernetes", "Kubernetes"), ("linkedin", "LinkedIn"), ("whatsapp", "WhatsApp"),
    ("powerpoint", "PowerPoint"), ("wifi", "Wi-Fi"), ("bluetooth", "Bluetooth"), ("gmail", "Gmail"),
    ("instagram", "Instagram"), ("tiktok", "TikTok"), ("figma", "Figma"), ("jira", "Jira"), ("netflix", "Netflix"),
    ("spotify", "Spotify"), ("dropbox", "Dropbox"), ("onedrive", "OneDrive"), ("sharepoint", "SharePoint"),
    ("powershell", "PowerShell"), ("devops", "DevOps"), ("graphql", "GraphQL"), ("fastapi", "FastAPI"),
    ("numpy", "NumPy"), ("pytorch", "PyTorch"), ("tensorflow", "TensorFlow"), ("huggingface", "Hugging Face"),
    ("youtuber", "YouTuber"), ("paypal", "PayPal"), ("airbnb", "Airbnb"),
];
const BRAND_PAIRS: &[(&str, &str, &str)] = &[
    ("node", "js", "Node.js"), ("next", "js", "Next.js"), ("vs", "code", "VS Code"), ("visual", "studio", "Visual Studio"),
    ("google", "docs", "Google Docs"), ("google", "drive", "Google Drive"), ("google", "meet", "Google Meet"),
];
const ACRONYMS: &[&str] = &[
    "api", "apis", "sdk", "ui", "ux", "gpu", "cpu", "url", "urls", "html", "css", "http", "https", "json", "sql", "cli",
    "gui", "pr", "prs", "faq", "ceo", "cto", "cfo", "coo", "asap", "ai", "usb", "pdf", "pdfs", "csv", "vpn", "ssd",
    "hdmi", "qa", "eta", "os", "llm", "llms", "ide", "aws", "gcp", "npm", "sms", "otp", "upi", "emi", "gst", "kyc",
];
const DAYS: &[&str] = &["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
const MONTHS_SAFE: &[&str] = &[
    "january", "february", "april", "june", "july", "august", "september", "october", "november", "december",
];
const MONTH_LEAD: &[&str] = &[
    "in", "on", "by", "until", "till", "since", "from", "of", "next", "last", "this", "early", "late", "mid",
    "before", "after", "through", "every", "during",
];
const PROPER: &[(&str, &str)] = &[
    ("christmas", "Christmas"), ("thanksgiving", "Thanksgiving"), ("halloween", "Halloween"), ("diwali", "Diwali"),
    ("ramadan", "Ramadan"), ("eid", "Eid"), ("easter", "Easter"), ("hanukkah", "Hanukkah"), ("holi", "Holi"),
    ("navratri", "Navratri"), ("pongal", "Pongal"), ("onam", "Onam"),
    ("american", "American"), ("british", "British"), ("english", "English"), ("french", "French"),
    ("german", "German"), ("spanish", "Spanish"), ("italian", "Italian"), ("chinese", "Chinese"),
    ("japanese", "Japanese"), ("korean", "Korean"), ("indian", "Indian"), ("russian", "Russian"),
    ("canadian", "Canadian"), ("australian", "Australian"), ("mexican", "Mexican"), ("brazilian", "Brazilian"),
    ("dutch", "Dutch"), ("swedish", "Swedish"), ("irish", "Irish"), ("scottish", "Scottish"), ("welsh", "Welsh"),
    ("arabic", "Arabic"), ("hindi", "Hindi"), ("portuguese", "Portuguese"), ("vietnamese", "Vietnamese"),
    ("thai", "Thai"), ("greek", "Greek"), ("turkish", "Turkish"), ("tamil", "Tamil"), ("telugu", "Telugu"),
    ("kannada", "Kannada"), ("malayalam", "Malayalam"), ("bengali", "Bengali"), ("marathi", "Marathi"),
    ("gujarati", "Gujarati"), ("punjabi", "Punjabi"), ("urdu", "Urdu"), ("nigerian", "Nigerian"),
    ("african", "African"), ("european", "European"), ("asian", "Asian"),
];
const TIME_ZONES: &[(&str, &str)] = &[
    ("eastern standard time", "EST"), ("pacific standard time", "PST"), ("central standard time", "CST"),
    ("mountain standard time", "MST"), ("greenwich mean time", "GMT"), ("coordinated universal time", "UTC"),
    ("indian standard time", "IST"), ("british summer time", "BST"), ("central european time", "CET"),
    ("eastern time", "ET"), ("pacific time", "PT"),
];

pub fn apply(text: &str) -> String {
    let t = per_line(text, |line| {
        let toks: Vec<String> = line.split_whitespace().map(String::from).collect();
        let toks = time_zones(toks);
        let toks = words(toks);
        toks.join(" ")
    });
    sentence_caps(&t)
}

fn time_zones(toks: Vec<String>) -> Vec<String> {
    let lower: Vec<String> = toks.iter().map(|t| core_lower(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    'scan: while i < toks.len() {
        for &(phrase, abbr) in TIME_ZONES {
            let n = phrase.split(' ').count();
            if i + n <= toks.len() && lower[i..i + n].join(" ") == phrase
                && toks[i..i + n - 1].iter().all(|t| split3(t).2.is_empty())
            {
                out.push(format!("{}{abbr}{}", split3(&toks[i]).0, split3(&toks[i + n - 1]).2));
                i += n;
                continue 'scan;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

fn words(toks: Vec<String>) -> Vec<String> {
    let lower: Vec<String> = toks.iter().map(|t| core_lower(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let w = lower[i].as_str();
        // two-word brands
        if let Some(&(_, _, proper)) = BRAND_PAIRS.iter().find(|(a, b, _)| *a == w && lower.get(i + 1).map(|s| s.as_str()) == Some(*b)) {
            if split3(&toks[i]).2.is_empty() {
                out.push(format!("{}{proper}{}", split3(&toks[i]).0, split3(&toks[i + 1]).2));
                i += 2;
                continue;
            }
        }
        let fixed: Option<String> = if let Some(&(_, b)) = BRANDS.iter().find(|(k, _)| *k == w) {
            Some(b.to_string())
        } else if ACRONYMS.contains(&w) {
            // keep a plural "s" lowercase ("APIs", "PDFs")
            Some(if w.ends_with('s') && w.len() > 3 && ACRONYMS.contains(&&w[..w.len() - 1]) {
                format!("{}s", w[..w.len() - 1].to_uppercase())
            } else {
                w.to_uppercase()
            })
        } else if DAYS.contains(&w) || MONTHS_SAFE.contains(&w) {
            Some(cap_first(w))
        } else if w == "may" || w == "march" {
            let prev = if i > 0 { lower[i - 1].as_str() } else { "" };
            if MONTH_LEAD.contains(&prev) { Some(cap_first(w)) } else { None }
        } else if let Some(&(_, p)) = PROPER.iter().find(|(k, _)| *k == w) {
            Some(p.to_string())
        } else {
            None
        };
        match fixed {
            // never touch a word already written with inner capitals ("iOS" stays, "GitHub" stays)
            Some(f) if split3(&toks[i]).1 != f => {
                let (l, _, r) = split3(&toks[i]);
                out.push(format!("{l}{f}{r}"));
            }
            _ => out.push(toks[i].clone()),
        }
        i += 1;
    }
    out
}

/// P5. First letter of each sentence and line, and the pronoun "I" (also I'm, I've, I'll, I'd).
pub fn sentence_caps(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let mut cap_next = true;
    for i in 0..chars.len() {
        let c = chars[i];
        if cap_next && c.is_alphabetic() {
            // a token that is a link, email or file name keeps its case ("github.com", "config.ts")
            let token_end = chars[i..].iter().position(|ch| ch.is_whitespace()).map_or(chars.len(), |p| i + p);
            let token: String = chars[i..token_end].iter().collect();
            let looks_like_code = token.contains('@') || token.contains("://") || token.starts_with("www.")
                || token.trim_end_matches(|ch: char| matches!(ch, '.' | ',' | '?' | '!')).contains('.')
                || token.contains('_') || token.starts_with('`');
            if !looks_like_code {
                chars[i] = c.to_uppercase().next().unwrap_or(c);
            }
            cap_next = false;
        } else if matches!(c, '?' | '!' | '\n')
            || (c == '.' && (i + 1 == chars.len() || chars[i + 1].is_whitespace()) && !is_abbrev_dot(&chars, i))
        {
            cap_next = true;
        } else if !c.is_whitespace() && !matches!(c, '"' | '\'' | '(' | '•') && !c.is_ascii_digit() {
            cap_next = false;
        } else if c.is_ascii_digit() {
            cap_next = false;
        }
        // the pronoun I
        let boundary_before = i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '"');
        if boundary_before && chars[i] == 'i' {
            let next = chars.get(i + 1).copied();
            let standalone = match next {
                None => true,
                Some(n) if n.is_whitespace() || matches!(n, ',' | '.' | '?' | '!' | ')' | ';' | ':') => true,
                Some('\'') | Some('’') => chars.get(i + 2).map_or(false, |a| matches!(a, 'm' | 'v' | 'l' | 'd')),
                _ => false,
            };
            if standalone {
                chars[i] = 'I';
            }
        }
    }
    chars.into_iter().collect()
}

/// "e.g." / "i.e." / "etc." / "vs." do not end a sentence.
fn is_abbrev_dot(chars: &[char], i: usize) -> bool {
    let start = chars[..i].iter().rposition(|c| c.is_whitespace()).map_or(0, |p| p + 1);
    let word: String = chars[start..=i].iter().collect::<String>().to_lowercase();
    matches!(word.as_str(), "e.g." | "i.e." | "vs." | "mr." | "mrs." | "ms." | "dr." | "approx.")
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn does() {
        let cases = [
            ("push to github and ask chatgpt using the api on the gpu", "Push to GitHub and ask ChatGPT using the API on the GPU"),
            ("see you monday. back in september", "See you Monday. Back in September"),
            ("we launch in may", "We launch in May"),
            ("she speaks french and hindi", "She speaks French and Hindi"),
            ("call me at 5 PM indian standard time", "Call me at 5 PM IST"),
            ("i think i'm ready, i'll go", "I think I'm ready, I'll go"),
            ("hello world. this is a test! how are you? i am fine.\nnew line here.", "Hello world. This is a test! How are you? I am fine.\nNew line here."),
            ("open the pdfs on node js", "Open the PDFs on Node.js"),
            ("see you on christmas and diwali", "See you on Christmas and Diwali"),
        ];
        for (input, want) in cases {
            assert_eq!(apply(input), want, "input {input:?}");
        }
    }

    #[test]
    fn must_not_touch() {
        for s in [
            "Ram called about the deadline.",
            "It may work.",
            "We march on.",
            "React fast and slack off.",
            "Pour 5 ml of milk.",
            "E.g. this one.",
            "github.com is up.",
        ] {
            assert_eq!(apply(s), s, "changed {s:?}");
        }
    }

    #[test]
    fn idempotent() {
        let s = "i use github on monday in march";
        let once = apply(s);
        assert_eq!(once, "I use GitHub on Monday in March");
        assert_eq!(apply(&once), once);
    }
}
