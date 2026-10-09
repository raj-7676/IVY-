use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

mod audio;
mod model;
mod rulebooks;
pub(crate) mod gpu_monitor;
mod modifier_hotkey;
mod spellcheck;
mod update;
pub mod lite;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
#[cfg(windows)]
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
#[cfg(windows)]
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

// Foreground window captured the instant the hotkey goes down, before our
// own capsule window is ever touched — this is the paste target, and it is
// never re-read at release time because by then the user may have clicked
// something of ours.
static LAST_EXTERNAL_HWND: Mutex<isize> = Mutex::new(0);

static DICTATION_ID: AtomicU64 = AtomicU64::new(0);
// The dictation the capsule is currently showing/the user could cancel —
// set the instant a new one begins, independent of `PENDING` (which only
// covers the recording phase; cancel must still work once the dictation has
// moved on to transcribing in a spawned thread and PENDING is empty again).
static ACTIVE_ID: AtomicU64 = AtomicU64::new(0);
// Exact-match, not a watermark: rapid-fire dictation (start N while N-1 is
// still transcribing in the background) is this app's real core use case,
// so cancelling N must never also cancel N-1's still-in-flight work.
static CANCELLED_IDS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

fn next_dictation_id() -> u64 {
    DICTATION_ID.fetch_add(1, Ordering::SeqCst) + 1
}

fn mark_cancelled(id: u64) {
    let mut ids = CANCELLED_IDS.lock().unwrap_or_else(|e| e.into_inner());
    if !ids.contains(&id) {
        ids.push(id);
        if ids.len() > 32 {
            ids.remove(0); // cancels are a rare user action, never per-dictation — this never fills up in practice
        }
    }
}

fn is_dictation_cancelled(id: u64) -> bool {
    CANCELLED_IDS.lock().unwrap_or_else(|e| e.into_inner()).contains(&id)
}

// Rate limiting and concurrency guards across Tauri IPC endpoints
static LAST_SETTINGS_SAVE: AtomicU64 = AtomicU64::new(0);
static LAST_PASTE_TIMESTAMP: AtomicU64 = AtomicU64::new(0);
static TOUCHING_UP: AtomicBool = AtomicBool::new(false);
static RETRYING: AtomicBool = AtomicBool::new(false);

fn check_rate_limit(last_ts: &AtomicU64, min_interval_ms: u64) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let prev = last_ts.load(Ordering::Relaxed);
    if now.saturating_sub(prev) < min_interval_ms {
        return Err(format!("rate limit exceeded: please wait {min_interval_ms}ms between requests"));
    }
    last_ts.store(now, Ordering::Relaxed);
    Ok(())
}

/// The in-flight dictation, alive between hotkey-down and hotkey-up. The
/// mic stream and the app/tone context captured at press time both need to
/// survive until release, when the real transcribe+cleanup+paste pipeline
/// runs.
struct PendingDictation {
    id: u64,
    recorder: audio::Recorder,
    active_app: String,
    tone_preset: String,
    // Captured once, at this dictation's own hotkey-down — not read from
    // the shared `LAST_EXTERNAL_HWND` global at paste time, which a second,
    // overlapping dictation could have already overwritten by then.
    target_hwnd: isize,
    /// Started while the setup wizard was the window in front: a practice run. Its text goes back to the
    /// wizard only (no paste, no history, no Alt+V hold-back, no stats).
    for_wizard: bool,
}
static PENDING: Mutex<Option<PendingDictation>> = Mutex::new(None);
// See `handle_hotkey_down` — guards the real TOCTOU window between "check
// nothing's recording yet" and the mic actually finishing opening.
static STARTING_DICTATION: AtomicBool = AtomicBool::new(false);
// Set by the onboarding wizard (`FirstRunView.tsx`) while it's mounted.
// `bridge_capsule_show` hides the capsule only when this is set AND the main
// window has focus, so the capsule never overlaps the wizard but still works
// everywhere else (e.g. window closed to tray mid-wizard).
static WIZARD_ACTIVE: AtomicBool = AtomicBool::new(false);

// Why the dictation shortcut doesn't work, when registering it at startup failed because another app holds it (on a
// Mac, Raycast, Alfred and ChatGPT all default to Option + Space). The main window shows it until a shortcut is
// saved that registers.
static HOTKEY_PROBLEM: Mutex<Option<String>> = Mutex::new(None);

// The real global shortcut registered for "paste Ivy's held-back text" (see
// `MANUAL_PASTE_TEXT` below), kept as a parsed `Shortcut` so the handler can
// match an incoming press with a cheap `==` instead of re-parsing settings.
static MANUAL_PASTE_SHORTCUT: Mutex<Option<Shortcut>> = Mutex::new(None);

// The latest transcript, whether it auto-pasted or had no text field to go
// to. Alt+V (`MANUAL_PASTE_SHORTCUT`) pastes it on demand — see
// `paste_manual_clipboard`. The real clipboard is never touched when there's
// no text field (Yash: dictating with nothing focused silently destroyed a
// copied API key). Not single-use, so a paste into the wrong window doesn't
// burn the only copy.
static MANUAL_PASTE_TEXT: Mutex<Option<String>> = Mutex::new(None);

// Hold-to-record and double-press-to-toggle both live on the same hotkey
// at once — not a Settings choice between them (Yash: "both should be at
// same time, not an option... I'll double click and do some other work and
// talk, and then I again click Alt+Space, record and stop"). See
// `route_dictation_press`/`route_dictation_release`.
//
// True while a double-press-started dictation is still recording hands-free
// — the next press stops it outright, rather than being evaluated as a
// hold or the first half of another double-press.
static TOGGLE_RECORDING: AtomicBool = AtomicBool::new(false);
// True while the hotkey is physically held down. Confirmed live (Yash's own
// debug.log): Windows/`tauri_plugin_global_shortcut` delivers a genuine
// duplicate `Pressed` (and sometimes `Released`) event for a single
// physical key action — classic OS key-repeat leaking through a combo
// hotkey. A normal hold survives this by luck (`handle_hotkey_down`
// already no-ops if a recording is already `PENDING`), but a double-press
// doesn't: the legitimate second tap arms `TOGGLE_RECORDING`, and the verbatim
// duplicate `Pressed` event one instant later reads as "next press = stop",
// killing the hands-free recording before the user ever speaks (`captured 0
// samples` in the log, every time). This flag debounces at the earliest
// possible point — the raw OS event — so a repeat never reaches routing
// logic at all, fixing every downstream path uniformly instead of patching
// each branch that happens to lack its own guard.
static HOTKEY_PHYSICALLY_DOWN: AtomicBool = AtomicBool::new(false);
// Set on every ordinary (non-toggle) press, cleared on its matching
// release — lets the release handler tell a real hold apart from a quick
// tap without ever delaying the press itself (recording always starts the
// instant the key goes down; a tap's recording is provisionally discarded
// on release, not withheld up front).
static CURRENT_PRESS_TIME: Mutex<Option<Instant>> = Mutex::new(None);
// The timestamp of the most recent quick tap's *release* — a following
// press within `DOUBLE_PRESS_WINDOW_MS` confirms a double-press. Cleared
// once consumed; a stale, never-followed-up tap is simply overwritten by
// whatever press comes next, no separate expiry needed.
static LAST_TAP: Mutex<Option<Instant>> = Mutex::new(None);
// A release sooner than this was a tap, not a deliberate hold — real
// speech essentially never finishes in under 400ms, so nothing genuine is
// lost by treating anything shorter as a possible double-press half.
// Widened 2026-10-06 from 300/400 ms on Yash's real timings: a 312 ms tap counted as a hold, and
// a two-key chord (Alt+Space) is slower to repeat than a mouse double-click (Windows default 500 ms).
const TAP_THRESHOLD_MS: u64 = 400;
const DOUBLE_PRESS_WINDOW_MS: u64 = 600;

// The window Ivy's last real paste went into — set only when `paste_text`
// sent a genuine Ctrl+V into a real foreground window. Touch Up uses it to
// swap its corrected text into that same window, and consumes it.
struct LastPaste {
    target_hwnd: isize,
}
static LAST_PASTE: Mutex<Option<LastPaste>> = Mutex::new(None);

/// The user's clipboard from before Ivy swapped a transcript in: text, an image (a screenshot) or
/// copied files, so none of them is lost by dictating. Other formats (rich text, HTML) come back as
/// their plain text.
#[derive(Clone)]
enum SavedClipboard {
    Empty,
    Text(String),
    Image(arboard::ImageData<'static>),
    Files(Vec<std::path::PathBuf>),
}

impl SavedClipboard {
    fn capture() -> Self {
        let Ok(mut c) = arboard::Clipboard::new() else { return Self::Empty };
        if let Ok(files) = c.get().file_list() {
            if !files.is_empty() {
                return Self::Files(files);
            }
        }
        if let Ok(text) = c.get_text() {
            return Self::Text(text);
        }
        if let Ok(image) = c.get_image() {
            return Self::Image(image);
        }
        Self::Empty
    }

    fn restore(self) {
        let Ok(mut c) = arboard::Clipboard::new() else { return };
        let _ = match self {
            Self::Empty => c.clear(),
            Self::Text(text) => c.set_text(text),
            Self::Image(image) => c.set_image(image),
            Self::Files(files) => c.set().file_list(&files),
        };
    }
}

fn default_glass_opacity() -> f64 {
    90.0
}
fn default_glass_blur() -> f64 {
    40.0
}
fn default_hardware_mode() -> String {
    "gpu".to_string()
}
fn default_smart_vram_eviction() -> bool {
    true
}
fn default_vram_eviction_threshold() -> u32 {
    80
}
fn default_launch_at_startup() -> bool {
    true
}
// Alt+V: one-handed (both keys on the left side), verified via web search
// against every major overlay's actual default — Nvidia GeForce Experience
// is Alt+Z, AMD Adrenalin is Alt+R, Discord is Shift+`, Steam is Shift+Tab,
// Xbox Game Bar is Win+G — none of them Alt+V. Ctrl+Alt+V was considered
// and rejected: it's Excel's real, current default for Paste Special, a
// genuine collision, not a guess. "V" also reads naturally as "paste"
// (same finger memory as Ctrl+V). Any classic Win32 "Alt+V opens the View
// menu" mnemonic won't fire while Ivy holds the OS-level registration, the
// same trade-off as Alt+Space overriding the window system menu.
fn default_manual_paste_hotkey() -> String {
    "Alt + V".to_string()
}

fn default_onboarding_completed() -> bool {
    false
}

fn default_history_days() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SettingsConfig {
    hotkey: String,
    active_tone_preset: String,
    preset_apps: HashMap<String, Vec<String>>,
    personal_dictionary: Vec<String>,
    selected_mic: String,
    available_mics: Vec<String>,
    // `serde(default = ...)` so settings.json files saved before this
    // feature existed still load (rather than silently resetting every
    // other field to defaults too — see `load_settings`).
    #[serde(default = "default_glass_opacity")]
    glass_opacity: f64,
    #[serde(default = "default_glass_blur")]
    glass_blur: f64,
    #[serde(default = "default_hardware_mode")]
    hardware_mode: String,
    #[serde(default = "default_smart_vram_eviction")]
    smart_vram_eviction: bool,
    #[serde(default = "default_vram_eviction_threshold")]
    vram_eviction_threshold: u32,
    #[serde(default = "default_launch_at_startup")]
    launch_at_startup: bool,
    #[serde(default = "default_manual_paste_hotkey")]
    manual_paste_hotkey: String,
    #[serde(default = "default_onboarding_completed")]
    onboarding_completed: bool,
    /// Say a trigger on its own ("my email") and Ivy types the saved text instead.
    #[serde(default)]
    snippets: Vec<Snippet>,
    /// How long history (transcripts and recordings) is kept: 1 to 7 days, picked in History. 1 = 24 hours.
    #[serde(default = "default_history_days")]
    history_days: u32,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct Snippet {
    trigger: String,
    text: String,
}

/// Lowercase words only, so "My email." and "my email" match.
fn snippet_key(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Swaps each trigger for its saved text anywhere in the dictation (Yash, 2026-10-07: "send it to my
/// mail" must work). Whole words only, case and punctuation ignored; the UI asks for triggers nobody
/// says by accident. A dictation that is only the trigger becomes exactly the saved text.
fn apply_snippets(text: &str, snippets: &[Snippet]) -> String {
    let said = snippet_key(text);
    if let Some(s) = snippets.iter().find(|s| !said.is_empty() && snippet_key(&s.trigger) == said) {
        return s.text.clone();
    }
    // Longest trigger first, so "my email address" wins over "my email".
    let mut triggers: Vec<(Vec<String>, &str)> = snippets
        .iter()
        .map(|s| (snippet_key(&s.trigger).split_whitespace().map(String::from).collect::<Vec<_>>(), s.text.as_str()))
        .filter(|(words, _)| !words.is_empty())
        .collect();
    triggers.sort_by_key(|(words, _)| std::cmp::Reverse(words.len()));
    // Byte spans of the words, using snippet_key's idea of a word (a run of letters/digits).
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            words.push((s, i));
        }
    }
    if let Some(s) = start {
        words.push((s, text.len()));
    }
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut i) = (0, 0);
    while i < words.len() {
        let hit = triggers.iter().find(|(tw, _)| {
            i + tw.len() <= words.len() && tw.iter().zip(&words[i..]).all(|(t, &(a, b))| text[a..b].to_lowercase() == *t)
        });
        match hit {
            Some((tw, body)) => {
                out.push_str(&text[copied..words[i].0]);
                out.push_str(body);
                copied = words[i + tw.len() - 1].1;
                i += tw.len();
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

impl Default for SettingsConfig {
    fn default() -> Self {
        // Programs, picked by their .exe (Yash, 2026-10-06: names were confusing and often didn't match); on
        // macOS by their .app bundle.
        let list = |apps: &[&str]| apps.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        let mut preset_apps = HashMap::new();
        #[cfg(not(target_os = "macos"))]
        {
            preset_apps.insert("Casual".to_string(), list(&["WhatsApp.exe", "Discord.exe", "Telegram.exe"]));
            preset_apps.insert("Standard".to_string(), list(&["Code.exe", "Notion.exe", "WindowsTerminal.exe"]));
            preset_apps.insert("Professional".to_string(), list(&["OUTLOOK.EXE", "olk.exe", "slack.exe", "ms-teams.exe", "WINWORD.EXE"]));
        }
        #[cfg(target_os = "macos")]
        {
            preset_apps.insert("Casual".to_string(), list(&["WhatsApp.app", "Discord.app", "Telegram.app", "Messages.app"]));
            preset_apps.insert("Standard".to_string(), list(&["Visual Studio Code.app", "Notion.app", "Terminal.app"]));
            preset_apps.insert(
                "Professional".to_string(),
                list(&["Microsoft Outlook.app", "Mail.app", "Slack.app", "Microsoft Teams.app", "Microsoft Word.app"]),
            );
        }
        Self {
            hotkey: "Alt + Space".to_string(),
            active_tone_preset: "Standard".to_string(),
            preset_apps,
            personal_dictionary: vec![],
            snippets: vec![],
            // Empty means "system default input device" — resolved for
            // real by audio::Recorder::start, never a placeholder name.
            selected_mic: String::new(),
            available_mics: vec![],
            glass_opacity: default_glass_opacity(),
            glass_blur: default_glass_blur(),
            hardware_mode: default_hardware_mode(),
            smart_vram_eviction: default_smart_vram_eviction(),
            vram_eviction_threshold: default_vram_eviction_threshold(),
            launch_at_startup: default_launch_at_startup(),
            manual_paste_hotkey: default_manual_paste_hotkey(),
            onboarding_completed: default_onboarding_completed(),
            history_days: default_history_days(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DictationSession {
    id: String,
    preview: String,
    full_transcript: String,
    timestamp: String,
    // Epoch ms, stamped here on insert — `timestamp` above is a display
    // string ("Today, 2:42 PM") and can't be parsed back into a date, which
    // the Home screen's streak needs. Defaults to 0 for entries written
    // before this field existed.
    #[serde(default)]
    created_at: i64,
    duration: String,
    duration_sec: f64,
    tone_preset: String,
    app_target: String,
    words_count: u32,
    audio_duration: f64,
    // Absolute path to the saved recording, or "" if none was kept (never
    // saved for a session, or aged out — see `cleanup_old_audio`). Saved
    // for every real attempt, success or failure, so a failed transcription
    // can be retried from History without re-recording, and so the raw
    // audio itself is always inspectable — never just a claimed transcript.
    #[serde(default)]
    audio_path: String,
    // Short auto-generated title, filled in by a background thread after
    // the dictation has already pasted — see `spawn_title_generation`.
    // Absent until that thread finishes; the UI falls back to `preview`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ActiveContext {
    active_app: String,
    tone_preset: String,
}

// All of Ivy's data lives in Local AppData (%LOCALAPPDATA%\app.ivy.dictation), never Roaming: a company PC
// with roaming profiles copies Roaming AppData to a server at logoff, and recordings must stay on this PC.
fn settings_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_local_data_dir().expect("no app data dir");
    let _ = fs::create_dir_all(&dir);
    dir.join("settings.json")
}

fn history_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_local_data_dir().expect("no app data dir");
    let _ = fs::create_dir_all(&dir);
    dir.join("history.json")
}

fn stats_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_local_data_dir().expect("no app data dir");
    let _ = fs::create_dir_all(&dir);
    dir.join("stats.json")
}

/// Decoupled, zero-knowledge lifetime progress and productivity metrics.
/// Stores aggregate counters and activity history ONLY.
/// NEVER stores speech transcripts, audio files, or any raw spoken text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserStats {
    pub total_words: u64,
    pub words_per_minute: u32,
    pub day_streak: u32,
    pub last_active_date: String, // "YYYY-MM-DD"
    pub total_duration_sec: f64,
    pub session_count: u32,
    pub daily_words: HashMap<String, u32>, // "YYYY-MM-DD" -> word count
}

impl Default for UserStats {
    fn default() -> Self {
        // A fresh install has dictated nothing, so every counter starts at
        // zero. Never seed a starter baseline here: the numbers on Home are
        // a claim about what this user actually did, and an invented one is
        // a lie the user can't tell from a real total.
        Self {
            total_words: 0,
            words_per_minute: 0,
            day_streak: 0,
            last_active_date: String::new(),
            total_duration_sec: 0.0,
            session_count: 0,
            daily_words: HashMap::new(),
        }
    }
}

impl UserStats {
    pub fn record_dictation(&mut self, words: u32, duration_sec: f64) {
        if words == 0 {
            return;
        }
        self.total_words += words as u64;
        self.total_duration_sec += duration_sec.max(0.5);
        self.session_count += 1;

        if self.total_duration_sec > 0.0 {
            let computed = (self.total_words as f64 / (self.total_duration_sec / 60.0)).round() as u32;
            self.words_per_minute = computed.clamp(40, 350);
        }

        let today_date = chrono::Local::now().date_naive();
        let today_str = today_date.format("%Y-%m-%d").to_string();
        *self.daily_words.entry(today_str.clone()).or_insert(0) += words;

        if self.last_active_date.is_empty() {
            self.day_streak = 1;
        } else if let Ok(last_date) = chrono::NaiveDate::parse_from_str(&self.last_active_date, "%Y-%m-%d") {
            let diff = (today_date - last_date).num_days();
            if diff == 1 {
                self.day_streak += 1;
            } else if diff > 1 {
                self.day_streak = 1;
            }
            // diff == 0: already dictated today, keep current running streak
        } else {
            self.day_streak = 1;
        }
        self.last_active_date = today_str;
    }
}

static STATS_LOCK: Mutex<()> = Mutex::new(());

fn load_user_stats(app: &tauri::AppHandle) -> UserStats {
    let path = stats_path(app);
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(stats) = serde_json::from_str::<UserStats>(&data) {
            return stats;
        }
    }
    let default_stats = UserStats::default();
    let _ = save_user_stats_atomic(app, &default_stats);
    default_stats
}

fn save_user_stats_atomic(app: &tauri::AppHandle, stats: &UserStats) -> Result<(), String> {
    let path = stats_path(app);
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(stats).map_err(|e| e.to_string())?;
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

// Privacy retention: recordings and transcripts are deleted after 24 hours by default, or after up to
// 7 days if the user picks that in History (Yash, 2026-10-08). User progress/stats stay in `stats.json`
// and are NEVER reset to zero.
fn retention_secs(app: &tauri::AppHandle) -> i64 {
    load_settings(app).history_days.clamp(1, 7) as i64 * 24 * 60 * 60
}

fn audio_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_local_data_dir().expect("no app data dir").join("audio");
    let _ = fs::create_dir_all(&dir);
    dir
}

// `audio_path` in a `DictationSession` is always written by `save_audio_wav`
// under `audio_dir`, but it's read back from a plain-text JSON file on disk
// that a user could hand-edit — cheap hardening so a tampered/corrupt
// history.json can't point a delete/copy/read at an arbitrary file the
// process has access to.
// Both sides must be canonicalized, not just `path` — on Windows,
// `canonicalize()` prepends the `\\?\` verbatim-path prefix, so comparing a
// canonicalized path against a plain, uncanonicalized `dir` never matches
// even for a completely real, valid file. That exact bug made every real
// recording report "no audio" on retry/extract — caught live, not in
// review, and worth a real test (below) so it can't silently come back.
fn is_path_within(path: &str, dir: &std::path::Path) -> bool {
    if path.is_empty() {
        return false;
    }
    let Ok(dir) = dir.canonicalize() else { return false };
    std::path::Path::new(path)
        .canonicalize()
        .map(|p| p.starts_with(&dir))
        .unwrap_or(false)
}

fn path_within_audio_dir(app: &tauri::AppHandle, path: &str) -> bool {
    is_path_within(path, &audio_dir(app))
}

/// Security hardening: validates that an incoming session ID is strictly
/// alphanumeric with hyphens/underscores, preventing path traversal attacks.
fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod path_guard_tests {
    use super::is_path_within;

    #[test]
    fn accepts_a_real_file_actually_inside_the_dir() {
        let base = std::env::temp_dir().join(format!("ivy-test-{}", std::process::id()));
        let inside_dir = base.join("audio");
        std::fs::create_dir_all(&inside_dir).unwrap();
        let file = inside_dir.join("session-123.wav");
        std::fs::write(&file, b"fake wav").unwrap();

        assert!(is_path_within(file.to_str().unwrap(), &inside_dir));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn rejects_a_file_outside_the_dir_and_an_empty_path() {
        let base = std::env::temp_dir().join(format!("ivy-test-outside-{}", std::process::id()));
        let inside_dir = base.join("audio");
        let outside_file = base.join("not-audio.wav");
        std::fs::create_dir_all(&inside_dir).unwrap();
        std::fs::write(&outside_file, b"fake wav").unwrap();

        assert!(!is_path_within(outside_file.to_str().unwrap(), &inside_dir));
        assert!(!is_path_within("", &inside_dir));
        assert!(!is_path_within("C:\\nonexistent\\path.wav", &inside_dir));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_is_valid_session_id() {
        use super::is_valid_session_id;
        assert!(is_valid_session_id("session-1718000000000"));
        assert!(is_valid_session_id("session_42-abc"));
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("../../etc/passwd"));
        assert!(!is_valid_session_id("..\\windows\\system32"));
        assert!(!is_valid_session_id("session;rm -rf /"));
    }

    #[test]
    fn test_user_stats_start_empty_and_record() {
        use super::UserStats;
        let mut stats = UserStats::default();
        assert_eq!(stats.total_words, 0, "Fresh install has dictated nothing");
        assert_eq!(stats.words_per_minute, 0);
        assert_eq!(stats.day_streak, 0);
        assert_eq!(stats.session_count, 0);
        assert!(stats.daily_words.is_empty());

        stats.record_dictation(50, 15.0);
        assert_eq!(stats.total_words, 50);
        assert_eq!(stats.session_count, 1);
        assert_eq!(stats.day_streak, 1, "First dictation starts the streak");
        assert!(stats.words_per_minute > 0);
    }
}

/// Saves the real recorded audio as a 16kHz mono WAV — for every attempt,
/// not just successful ones, so a failed transcription can be retried from
/// the saved audio instead of asking the user to speak again, and so
/// History can offer real playback rather than just a claimed transcript.
/// Returns "" (not saved) rather than erroring — a missing audio file
/// should never block the rest of the pipeline.
fn save_audio_wav(app: &tauri::AppHandle, id: &str, samples: &[f32]) -> String {
    if samples.is_empty() {
        return String::new();
    }
    let path = audio_dir(app).join(format!("{id}.wav"));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let write = || -> Result<(), hound::Error> {
        let mut writer = hound::WavWriter::create(&path, spec)?;
        for &s in samples {
            let clamped = s.clamp(-1.0, 1.0);
            writer.write_sample((clamped * i16::MAX as f32) as i16)?;
        }
        writer.finalize()
    };
    match write() {
        Ok(()) => path.to_string_lossy().into_owned(),
        Err(e) => {
            log::error!("Ivy: failed to save audio: {e}");
            String::new()
        }
    }
}

/// Real disk cleanup, not a display filter: deletes both the audio file
/// AND the history entry (transcript, preview, everything) once it's older
/// than the chosen retention (`retention_secs`) — the app makes no lasting record of what was
/// said. Called at launch and again on an hourly timer (see `run()`) since
/// Ivy is designed to keep running for weeks without a restart (the main
/// window only ever hides on close, never actually quits).
fn purge_old_history(app: &tauri::AppHandle) {
    let retention = retention_secs(app);
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut sessions = load_history(app);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let before = sessions.len();
    for s in &sessions {
        if s.created_at != 0 && (now - s.created_at) / 1000 > retention && path_within_audio_dir(app, &s.audio_path) {
            let _ = fs::remove_file(&s.audio_path);
        }
    }
    sessions.retain(|s| s.created_at == 0 || (now - s.created_at) / 1000 <= retention);
    // Any recording older than that, even one history no longer lists, goes too.
    if let Ok(entries) = fs::read_dir(audio_dir(app)) {
        for entry in entries.flatten() {
            let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok())
                .map_or(false, |age| age.as_secs() as i64 > retention);
            if old {
                wipe_file(&entry.path());
            }
        }
    }
    if sessions.len() != before {
        write_history_atomic(app, &sessions);
        let _ = app.emit("ivy://history-updated", ());
    }
}

/// Release builds run with `tauri-plugin-log` disabled (see `run()`), so
/// `log::error!` from the dictation pipeline is otherwise invisible —
/// nothing to inspect after a real failure. This appends one line per
/// dictation attempt to a plain text file so a failure is diagnosable
/// (mic captured nothing vs. STT/cleanup actually erroring) instead of
/// just "couldn't transcribe" with no further information.
const DEBUG_LOG_CAP_BYTES: u64 = 1_000_000;

fn debug_log(app: &tauri::AppHandle, line: &str) {
    let Ok(dir) = app.path().app_local_data_dir() else { return };
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("debug.log");
    // A privacy-positioned voice app can't grow an unbounded, un-rotated
    // plaintext log forever — truncate rather than let it grow without
    // limit (this file is diagnostics only, never a second history).
    if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > DEBUG_LOG_CAP_BYTES {
        let _ = fs::remove_file(&path);
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(f, "[{ts}] {line}");
    }
}

/// Where Ivy's model files (models/ivy-lite) live. Bundled into the
/// installer as a resource directory (see tauri.conf.json) so the shipped
/// app makes zero network calls; falls back to the repo-relative `models/`
/// used by `npm run tauri dev` before a resource dir exists.
fn models_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join("models");
        if bundled.exists() {
            return bundled;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let beside = parent.join("models");
            if beside.exists() {
                return beside;
            }
            let in_sub = parent.join("IVY_Transcriber").join("src-tauri").join("models");
            if in_sub.exists() {
                return in_sub;
            }
        }
    }
    // The source tree is only a fallback for dev builds: a release exe must never load a model from a
    // build machine's path that might exist (and be writable) on someone else's PC.
    if cfg!(debug_assertions) {
        return std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
    }
    // On macOS the app is a signed, read-only bundle (and may run from the disk image), so the downloaded model
    // lives with Ivy's data in ~/Library/Application Support/app.ivy.dictation/models.
    #[cfg(target_os = "macos")]
    if let Ok(dir) = app.path().app_local_data_dir() {
        return dir.join("models");
    }
    app.path().resource_dir().unwrap_or_default().join("models")
}

fn format_timestamp(epoch_ms: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_millis_opt(epoch_ms).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    if dt.date_naive() == today {
        format!("Today, {}", dt.format("%-I:%M %p"))
    } else {
        format!("{}", dt.format("%b %-d, %-I:%M %p"))
    }
}

// `text.len()` is bytes, not chars — slicing at a fixed byte offset panics
// the moment it lands inside a multi-byte character (accented letters,
// curly quotes from the cleanup pass), which would kill the dictation
// worker thread and leave `ivy://dictation-complete` never fired.
fn char_safe_preview(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn format_duration(seconds: f64) -> String {
    let total = seconds.round().max(0.0) as u64;
    if total < 60 {
        format!("{total}s")
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}

fn load_settings(app: &tauri::AppHandle) -> SettingsConfig {
    // GPU or CPU is chosen in the setup wizard; the installer no longer asks (Yash, 2026-10-07).
    fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn load_history(app: &tauri::AppHandle) -> Vec<DictationSession> {
    fs::read_to_string(history_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// Serializes every history read-modify-write so two overlapping dictations
// (or a dictation racing a delete/retry from the UI) can't lose one side's
// change to the other ("last writer wins" on a plain concurrent `fs::write`).
static HISTORY_LOCK: Mutex<()> = Mutex::new(());

// Writes via a temp file + rename so a crash or power loss mid-write can
// never leave `history.json` truncated/corrupt — `fs::write` alone
// overwrites in place, and `load_history` treats any parse failure as "no
// history at all" (silently losing everything the user ever dictated).
fn write_history_atomic(app: &tauri::AppHandle, sessions: &[DictationSession]) {
    let Ok(json) = serde_json::to_string_pretty(sessions) else { return };
    let path = history_path(app);
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, &json).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

/// Up to 0.1.5 Ivy kept its data in Roaming AppData. Moves whatever is still there into Local AppData
/// (once; anything already in Local wins) and points saved recordings at their new folder. Runs at startup
/// before anything else reads settings or history.
fn migrate_roaming_data(app: &tauri::AppHandle) {
    let (Ok(old), Ok(new)) = (app.path().app_data_dir(), app.path().app_local_data_dir()) else { return };
    if old == new || !old.exists() {
        return;
    }
    let _ = fs::create_dir_all(&new);
    if let Ok(entries) = fs::read_dir(&old) {
        for entry in entries.flatten() {
            let dest = new.join(entry.file_name());
            if !dest.exists() {
                let _ = fs::rename(entry.path(), &dest);
            }
        }
    }
    let _ = fs::remove_dir(&old); // only succeeds once it's empty
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut sessions = load_history(app);
    let mut changed = false;
    for s in &mut sessions {
        if let Some(p) = repoint_audio(&s.audio_path, &old.join("audio"), &new.join("audio")) {
            s.audio_path = p;
            changed = true;
        }
    }
    if changed {
        write_history_atomic(app, &sessions);
    }
}

/// The recording's path inside `new_audio` if it was saved under `old_audio`, else None.
fn repoint_audio(path: &str, old_audio: &std::path::Path, new_audio: &std::path::Path) -> Option<String> {
    let rest = std::path::Path::new(path).strip_prefix(old_audio).ok()?;
    Some(new_audio.join(rest).to_string_lossy().into_owned())
}

#[cfg(test)]
mod migrate_tests {
    use super::repoint_audio;
    #[cfg(windows)]
    use std::path::Path;

    #[test]
    #[cfg(windows)]
    fn moves_recordings_saved_in_roaming_and_leaves_others() {
        let old = Path::new(r"C:\Users\a\AppData\Roaming\app.ivy.dictation\audio");
        let new = Path::new(r"C:\Users\a\AppData\Local\app.ivy.dictation\audio");
        assert_eq!(
            repoint_audio(r"C:\Users\a\AppData\Roaming\app.ivy.dictation\audio\s-1.wav", old, new).as_deref(),
            Some(r"C:\Users\a\AppData\Local\app.ivy.dictation\audio\s-1.wav")
        );
        assert_eq!(repoint_audio(r"C:\Users\a\AppData\Local\app.ivy.dictation\audio\s-2.wav", old, new), None);
        assert_eq!(repoint_audio("", old, new), None);
    }

    // The same with the platform's own separators (Roaming and Local are one folder on macOS, so the move never
    // runs there, but the path logic must still hold).
    #[test]
    fn repoints_with_native_paths() {
        let base = std::env::temp_dir();
        let (old, new) = (base.join("Roaming").join("audio"), base.join("Local").join("audio"));
        let moved = repoint_audio(old.join("s-1.wav").to_str().unwrap(), &old, &new);
        assert_eq!(moved.as_deref(), new.join("s-1.wav").to_str());
        assert_eq!(repoint_audio(new.join("s-2.wav").to_str().unwrap(), &old, &new), None);
    }
}

// Windows process/window helpers — best-effort app identification for tone
// presets. Falls back to "Desktop" if anything fails; never panics the
// hotkey path over an identification miss.
//
// Two labels on purpose. Tone matching needs the window title: several default tone-preset apps
// (Instagram, Gmail, Notion, Figma, Linear) are only ever browser tabs. But a title can carry a document
// name or page content, so it is used in memory for that match only; what is shown and saved in history is
// just the program's name (`foreground_app_label`).
#[cfg(windows)]
fn exe_name_for_pid(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; MAX_PATH as usize];
        let len = GetModuleBaseNameW(handle, None, &mut buf);
        let _ = CloseHandle(handle);
        if len == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

/// What tone presets match against: the foreground window's title, else its program name.
#[cfg(windows)]
fn foreground_tone_label() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return "Desktop".to_string();
        }
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        let title = String::from_utf16_lossy(&title_buf[..len.max(0) as usize]);
        if title.trim().is_empty() { foreground_app_label() } else { title }
    }
}

/// The foreground program's name for the capsule and history ("Brave", "Notepad"), never a window title.
#[cfg(windows)]
fn foreground_app_label() -> String {
    app_name_from_exe(&foreground_exe_name())
}

#[cfg(any(windows, test))]
fn app_name_from_exe(exe: &str) -> String {
    let name = exe.trim();
    let name = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name);
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Desktop".to_string(),
    }
}

#[cfg(test)]
mod app_name_tests {
    use super::app_name_from_exe;

    #[test]
    fn shows_the_program_not_the_window_title() {
        assert_eq!(app_name_from_exe("brave.exe"), "Brave");
        assert_eq!(app_name_from_exe("WindowsTerminal.exe"), "WindowsTerminal");
        assert_eq!(app_name_from_exe("NOTEPAD.EXE"), "NOTEPAD");
        assert_eq!(app_name_from_exe(""), "Desktop");
    }
}

/// The foreground window's program, e.g. "brave.exe" ("" if unknown). Tone app lists match on this.
#[cfg(windows)]
fn foreground_exe_name() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        exe_name_for_pid(pid)
    }
}

#[cfg(windows)]
fn capture_foreground_hwnd() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

// The desktop background and a plain File Explorer folder window are both
// hosted by explorer.exe and have no real editable text field — but they
// pass the ordinary "is this a different, external window" check just like
// any real app, so Ivy would genuinely fire Ctrl+V into them and (honestly,
// from its own point of view) report a real "Pasted", which reads to the
// user as a wrong/broken paste rather than the "nothing to paste into"
// case it actually is. Caught live: "pasted to Program Manager" while
// dictating with everything minimized. Excluding explorer.exe entirely
// means a real Explorer text field (address bar, a file being renamed) also
// falls back to clipboard-only — a deliberate, narrow trade-off for an
// uncommon case, in exchange for never showing this specific confusing lie.
#[cfg(windows)]
fn is_explorer_shell(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void), Some(&mut pid));
        exe_name_for_pid(pid).eq_ignore_ascii_case("explorer.exe")
    }
}

// Whether `hwnd` has a text field in focus right now. A window that only takes focus (a dock such as
// Winstep Nexus, a widget, a game) would otherwise get a Ctrl+V that lands nowhere and a capsule saying
// "Pasted to Nexus" (Yash, 2026-10-07). A Win32 caret settles it for native apps; otherwise UI Automation
// asks for the focused element. Unknown (UI Automation fails, or focus sits in another process) counts as
// a text field, which is how every paste worked before this check.
#[cfg(windows)]
fn has_text_focus(hwnd: isize) -> bool {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_DocumentControlTypeId,
        UIA_EditControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
    unsafe {
        let mut pid: u32 = 0;
        let thread = GetWindowThreadProcessId(windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void), Some(&mut pid));
        let mut gui = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if GetGUIThreadInfo(thread, &mut gui).is_ok() && !gui.hwndCaret.0.is_null() {
            return true;
        }
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) else {
            return true;
        };
        let Ok(el) = uia.GetFocusedElement() else {
            return true;
        };
        if el.CurrentProcessId().map_or(true, |p| p as u32 != pid) {
            return true;
        }
        let kind = el.CurrentControlType().unwrap_or_default();
        kind == UIA_EditControlTypeId
            || kind == UIA_DocumentControlTypeId
            || el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).is_ok()
            || el
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .and_then(|v| v.CurrentIsReadOnly())
                .map_or(false, |read_only| !read_only.as_bool())
    }
}

// macOS: the foreground "window" is the app in front, identified by its process id (src/macos.rs). Same two
// labels as on Windows: the window title for tone matching only, the app's name for the capsule and history.
#[cfg(target_os = "macos")]
fn foreground_app_label() -> String {
    let name = macos::app_name(macos::frontmost_pid());
    if name.is_empty() { "Desktop".to_string() } else { name }
}
#[cfg(target_os = "macos")]
fn foreground_tone_label() -> String {
    let title = macos::window_title(macos::frontmost_pid());
    if title.trim().is_empty() { foreground_app_label() } else { title }
}
/// The app in front as its bundle is named on disk, e.g. "Slack.app" ("" if unknown). Tone app lists match on this.
#[cfg(target_os = "macos")]
fn foreground_exe_name() -> String {
    macos::app_bundle_name(macos::frontmost_pid())
}
#[cfg(target_os = "macos")]
fn capture_foreground_hwnd() -> isize {
    macos::frontmost_pid() as isize
}

// Finder (the desktop and its folder windows) has no text box except while renaming or searching, so it gets an
// honest "couldn't paste" instead of a Cmd+V into nothing, like explorer.exe on Windows (`is_explorer_shell`). Any
// other app counts as a text box, which is how every paste on Windows worked before UI Automation could tell.
// ponytail: only Finder is asked; extend the role check to other apps if "Pasted to X" shows up where nothing was typed.
#[cfg(target_os = "macos")]
fn has_text_focus(pid: isize) -> bool {
    let pid = pid as i32;
    if macos::app_bundle_id(pid) != "com.apple.finder" {
        return true;
    }
    matches!(macos::focused_role(pid).as_deref(), Some("AXTextField" | "AXTextArea" | "AXComboBox"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn foreground_app_label() -> String {
    "Desktop".to_string()
}
#[cfg(not(any(windows, target_os = "macos")))]
fn foreground_tone_label() -> String {
    "Desktop".to_string()
}
#[cfg(not(any(windows, target_os = "macos")))]
fn foreground_exe_name() -> String {
    String::new()
}
#[cfg(not(any(windows, target_os = "macos")))]
fn capture_foreground_hwnd() -> isize {
    0
}
#[cfg(not(windows))]
fn is_explorer_shell(_hwnd: isize) -> bool {
    false
}

/// Ivy's own windows as foreground ids (`capture_foreground_hwnd`): the capsule's and the main window's handles
/// on Windows. On macOS the id is the app's process id, so both are Ivy's own.
fn own_window_ids(app: &tauri::AppHandle) -> (isize, isize) {
    #[cfg(windows)]
    {
        let hwnd = |label: &str| app.get_webview_window(label).and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0);
        (hwnd("capsule"), hwnd("main"))
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        let own = std::process::id() as isize;
        (own, own)
    }
}

fn code_for_letter(c: char) -> Option<Code> {
    Some(match c.to_ascii_uppercase() {
        'A' => Code::KeyA, 'B' => Code::KeyB, 'C' => Code::KeyC, 'D' => Code::KeyD,
        'E' => Code::KeyE, 'F' => Code::KeyF, 'G' => Code::KeyG, 'H' => Code::KeyH,
        'I' => Code::KeyI, 'J' => Code::KeyJ, 'K' => Code::KeyK, 'L' => Code::KeyL,
        'M' => Code::KeyM, 'N' => Code::KeyN, 'O' => Code::KeyO, 'P' => Code::KeyP,
        'Q' => Code::KeyQ, 'R' => Code::KeyR, 'S' => Code::KeyS, 'T' => Code::KeyT,
        'U' => Code::KeyU, 'V' => Code::KeyV, 'W' => Code::KeyW, 'X' => Code::KeyX,
        'Y' => Code::KeyY, 'Z' => Code::KeyZ,
        _ => return None,
    })
}

fn code_for_digit(c: char) -> Option<Code> {
    Some(match c {
        '0' => Code::Digit0, '1' => Code::Digit1, '2' => Code::Digit2, '3' => Code::Digit3,
        '4' => Code::Digit4, '5' => Code::Digit5, '6' => Code::Digit6, '7' => Code::Digit7,
        '8' => Code::Digit8, '9' => Code::Digit9,
        _ => return None,
    })
}

/// Turns a `tauri-plugin-global-shortcut` registration failure into an
/// honest, actionable message. The plugin collapses every underlying
/// `global-hotkey` error into one `GlobalHotkey(String)` variant (see its
/// `error.rs`), so the only way to tell "already bound to something else on
/// this PC" apart from "this key can't be a hotkey at all" is the wording
/// global-hotkey itself puts in that string ("HotKey already registered").
/// Every prior version of this message called both cases "already used by
/// something else" — technically true of neither for an unsupported key,
/// and unhelpfully vague for the real conflict case Yash asked this to
/// name plainly: which key it is, and that the fix is picking a different
/// one or freeing it up in whatever app/OS feature is holding it.
fn describe_register_failure(spec: &str, e: impl std::fmt::Display) -> String {
    let msg = e.to_string();
    #[cfg(not(target_os = "macos"))]
    let (taken, spec) = (msg.to_lowercase().contains("already registered"), spec.to_string());
    // macOS's RegisterEventHotKey fails when another app holds the combination (Alfred and ChatGPT use Option + Space).
    #[cfg(target_os = "macos")]
    let (taken, spec) = (msg.contains("RegisterEventHotKey failed"), spec.replace("Alt", "Option").replace("Ctrl", "Control"));
    if taken {
        #[cfg(not(target_os = "macos"))]
        let owner = "on this PC (another app or a Windows shortcut)";
        #[cfg(target_os = "macos")]
        let owner = "on this Mac (another app or a macOS shortcut)";
        format!(
            "\"{spec}\" is already bound to something else {owner}. Pick a different key, or free this one up in that app's settings first."
        )
    } else {
        format!("Ivy couldn't register \"{spec}\" as a hotkey ({msg}). Try a different key.")
    }
}

/// Turns a hotkey string like the Settings screen's free-form capture
/// produces ("Alt + Space", "Ctrl + Shift + K", "Right Alt") into a real
/// `Shortcut` the OS can register — this is what makes the Settings hotkey
/// field actually change what Alt+Space does, instead of only updating what
/// the UI displays while the backend keeps listening for the old one.
fn parse_hotkey(spec: &str) -> Option<Shortcut> {
    let mut mods = Modifiers::empty();
    let mut code: Option<Code> = None;
    for part in spec.split('+').map(|s| s.trim()) {
        match part {
            "" => {}
            "Alt" => mods |= Modifiers::ALT,
            "Ctrl" | "Control" => mods |= Modifiers::CONTROL,
            "Shift" => mods |= Modifiers::SHIFT,
            "Cmd" | "Meta" | "Super" | "Win" => mods |= Modifiers::SUPER,
            "Space" => code = Some(Code::Space),
            "Right Alt" | "AltRight" => code = Some(Code::AltRight),
            "Left Alt" | "AltLeft" => code = Some(Code::AltLeft),
            "Caps Lock" | "CapsLock" => code = Some(Code::CapsLock),
            "Enter" | "Return" => code = Some(Code::Enter),
            "Escape" | "Esc" => code = Some(Code::Escape),
            "Tab" => code = Some(Code::Tab),
            "Backspace" => code = Some(Code::Backspace),
            "ArrowUp" | "Up" => code = Some(Code::ArrowUp),
            "ArrowDown" | "Down" => code = Some(Code::ArrowDown),
            "ArrowLeft" | "Left" => code = Some(Code::ArrowLeft),
            "ArrowRight" | "Right" => code = Some(Code::ArrowRight),
            s if s.len() == 1 && s.chars().next().unwrap().is_ascii_alphabetic() => {
                code = code_for_letter(s.chars().next().unwrap());
            }
            s if s.len() == 1 && s.chars().next().unwrap().is_ascii_digit() => {
                code = code_for_digit(s.chars().next().unwrap());
            }
            s if (s.starts_with('F') || s.starts_with('f')) && s[1..].parse::<u8>().map(|n| (1..=12).contains(&n)).unwrap_or(false) => {
                code = match s[1..].parse::<u8>().unwrap() {
                    1 => Some(Code::F1), 2 => Some(Code::F2), 3 => Some(Code::F3), 4 => Some(Code::F4),
                    5 => Some(Code::F5), 6 => Some(Code::F6), 7 => Some(Code::F7), 8 => Some(Code::F8),
                    9 => Some(Code::F9), 10 => Some(Code::F10), 11 => Some(Code::F11), 12 => Some(Code::F12),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    code.map(|c| Shortcut::new(if mods.is_empty() { None } else { Some(mods) }, c))
}

/// Unregisters whatever binding was previously active and registers the new
/// one — called at startup (unregistering nothing) and again whenever
/// Settings saves a changed value, so the real global shortcut always
/// matches what the UI displays instead of silently staying on the old one.
/// `slot` is the cached-comparison static (`MANUAL_PASTE_SHORTCUT`) the
/// shared handler matches an incoming press against — `None` for the main
/// dictation hotkey (anything that isn't the manual-paste shortcut is it).
// Register the NEW shortcut before unregistering the old one — not the
// reverse. Unregister-then-register left a real gap: if the new combo is
// already taken by something else on this PC (the exact case the error
// message below warns about), the old binding was already gone by the
// time registration failed, so `save_settings` returns `Err` (never
// writes settings.json) but dictation is now silently dead until the user
// successfully saves a different combo — with the UI still showing the
// old hotkey as if it were live.
fn apply_hotkey_binding(
    app: &tauri::AppHandle,
    old_spec: &str,
    new_spec: &str,
    slot: Option<&Mutex<Option<Shortcut>>>,
) -> Result<(), String> {
    if slot.is_none() && new_spec == modifier_hotkey::SPEC {
        set_ctrl_shift(app, true);
        if let Some(old) = parse_hotkey(old_spec) {
            let _ = app.global_shortcut().unregister(old);
        }
        return Ok(());
    }
    let new_shortcut = parse_hotkey(new_spec).ok_or_else(|| format!("Couldn't understand the shortcut \"{new_spec}\""))?;
    app.global_shortcut()
        .register(new_shortcut)
        .map_err(|e| describe_register_failure(new_spec, e))?;
    if let Some(old) = parse_hotkey(old_spec) {
        let _ = app.global_shortcut().unregister(old);
    }
    if let Some(cell) = slot {
        *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(new_shortcut);
    } else {
        set_ctrl_shift(app, false);
    }
    Ok(())
}

/// The tone for a dictation (Yash, 2026-10-06): three modes. A program the user added to a mode's list
/// always gets that mode; every other app gets the mode clicked on the Tone screen, at once. List entries
/// ending in ".exe" (Windows) or ".app" (macOS) match the foreground program exactly; older name entries match
/// the window title.
fn tone_for_label(settings: &SettingsConfig, title: &str, exe: &str) -> String {
    let title = title.to_lowercase();
    for tone in ["Casual", "Professional", "Standard"] {
        if let Some(apps) = settings.preset_apps.get(tone) {
            let hit = apps.iter().any(|a| {
                let a = a.trim().to_lowercase();
                if a.ends_with(".exe") || a.ends_with(".app") { a.eq_ignore_ascii_case(exe) } else { !a.is_empty() && title.contains(&a) }
            });
            if hit {
                return tone.to_string();
            }
        }
    }
    settings.active_tone_preset.clone()
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> SettingsConfig {
    load_settings(&app)
}

// Mirrors `settings.launch_at_startup` into the real Windows registry Run
// key via the autostart plugin — best-effort, since a user without write
// access to that key (locked-down corporate machine) must not crash a
// settings save over it.
fn sync_autostart(app: &tauri::AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let result = if enabled { manager.enable() } else { manager.disable() };
    if let Err(e) = result {
        log::warn!("Ivy: couldn't sync launch-at-startup registration: {e}");
        return;
    }
    #[cfg(windows)]
    if enabled {
        quote_autostart_entry(app);
    }
}

/// The autostart plugin writes Ivy's Run entry with the exe path unquoted. With a space in the path (a
/// Windows user name like "Jane Doe"), Windows would first try to run C:\Users\Jane.exe. Rewrites it quoted.
#[cfg(windows)]
fn quote_autostart_entry(app: &tauri::AppHandle) {
    use winreg::{enums::*, RegKey};
    let Ok(exe) = std::env::current_exe() else { return };
    let run = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_SET_VALUE);
    if let Ok(run) = run {
        let _ = run.set_value(&app.package_info().name, &format!("\"{}\" --autostart", exe.display()));
    }
}

/// The only dictation keys Ivy offers (Yash, 2026-10-06). Held-back paste (Alt + V) is fixed. There is no undo
/// key: Ctrl+Z in the app itself does the same (Yash removed Alt + B, 2026-10-07).
const DICTATION_KEYS: [&str; 2] = ["Alt + Space", modifier_hotkey::SPEC];

fn validate_settings(s: &SettingsConfig) -> Result<(), String> {
    if !DICTATION_KEYS.contains(&s.hotkey.as_str()) {
        return Err("The dictation shortcut can be Alt + Space or Ctrl + Shift.".into());
    }
    if s.hotkey.len() > 64 || s.manual_paste_hotkey.len() > 64 {
        return Err("hotkey string exceeds maximum allowable length (64 chars)".into());
    }
    let valid_tones = ["Casual", "Standard", "Professional"];
    if !valid_tones.contains(&s.active_tone_preset.as_str()) {
        return Err("invalid tone preset: must be Casual, Standard, or Professional".into());
    }
    // Only "gpu"/"cpu" are real — `HardwareMode` on the frontend only ever
    // offers those two, and `should_use_gpu`/`get_hardware_status` only
    // ever check for "gpu" (anything else, "auto" included, already behaved
    // exactly like "cpu"). Accepting "auto" here without any actual
    // auto-detection logic behind it was a mode nothing could ever
    // knowingly choose and nothing ever implemented.
    let valid_modes = ["gpu", "cpu"];
    if !valid_modes.contains(&s.hardware_mode.as_str()) {
        return Err("invalid hardware mode: must be gpu or cpu".into());
    }
    if !(0.0..=100.0).contains(&s.glass_opacity) {
        return Err("glass opacity must be between 0.0 and 100.0".into());
    }
    if !(0.0..=100.0).contains(&s.glass_blur) {
        return Err("glass blur must be between 0.0 and 100.0".into());
    }
    if !(50..=100).contains(&s.vram_eviction_threshold) {
        return Err("vram eviction threshold must be between 50 and 100".into());
    }
    if s.selected_mic.len() > 256 {
        return Err("selected microphone identifier too long".into());
    }
    if s.snippets.len() > 200 || s.snippets.iter().any(|x| x.trigger.len() > 80 || x.text.len() > 5000 || snippet_key(&x.trigger).is_empty()) {
        return Err("A snippet needs a trigger (up to 80 characters) and text up to 5,000 characters; 200 at most.".into());
    }
    if s.personal_dictionary.len() > 5000 {
        return Err("personal dictionary exceeds maximum allowable entries (5000)".into());
    }
    for word in &s.personal_dictionary {
        if word.len() > 100 {
            return Err("personal dictionary word exceeds maximum length (100 chars)".into());
        }
    }
    if s.preset_apps.len() > 3
        || s.preset_apps.iter().any(|(tone, apps)| {
            !valid_tones.contains(&tone.as_str()) || apps.len() > 200 || apps.iter().any(|a| a.len() > 256)
        })
    {
        return Err("Per-app tones: up to 200 apps per tone, 256 characters each.".into());
    }
    if s.available_mics.len() > 64 || s.available_mics.iter().any(|m| m.len() > 256) {
        return Err("microphone list too long".into());
    }
    Ok(())
}

fn write_settings_atomic(app: &tauri::AppHandle, settings: &SettingsConfig) -> Result<(), String> {
    let path = settings_path(app);
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: SettingsConfig) -> Result<(), String> {
    validate_settings(&settings)?;
    check_rate_limit(&LAST_SETTINGS_SAVE, 100)?;
    let old_settings = load_settings(&app);
    // Apply the real OS registration BEFORE persisting — if the new combo
    // is already taken by something else, settings.json must keep the
    // hotkey that's actually still active, not the one the UI merely asked
    // for and never really got. Each field is persisted right after its own
    // OS-level change succeeds, not batched into one write at the very
    // end — a later field failing (e.g. saving all three hotkeys at once,
    // and the second one is already taken) must not leave an earlier
    // field's already-live OS change unpersisted, the exact desync
    // `apply_hotkey_binding`'s register-before-unregister ordering exists
    // to prevent for a single field.
    let mut to_persist = old_settings.clone();
    if settings.hotkey != old_settings.hotkey {
        apply_hotkey_binding(&app, &old_settings.hotkey, &settings.hotkey, None)?;
        *HOTKEY_PROBLEM.lock().unwrap_or_else(|e| e.into_inner()) = None;
        to_persist.hotkey = settings.hotkey.clone();
        write_settings_atomic(&app, &to_persist)?;
    }
    if settings.manual_paste_hotkey != old_settings.manual_paste_hotkey {
        apply_hotkey_binding(&app, &old_settings.manual_paste_hotkey, &settings.manual_paste_hotkey, Some(&MANUAL_PASTE_SHORTCUT))?;
        to_persist.manual_paste_hotkey = settings.manual_paste_hotkey.clone();
        write_settings_atomic(&app, &to_persist)?;
    }
    sync_autostart(&app, settings.launch_at_startup);

    write_settings_atomic(&app, &settings)?;
    debug_log(&app, &format!("settings saved: hardware={}", settings.hardware_mode));
    // A shorter history time applies at once, not at the next hourly clean-up.
    if settings.history_days != old_settings.history_days {
        purge_old_history(&app);
    }
    // GPU <-> CPU: load the model in the new place now, so the next dictation isn't the slow first one.
    if settings.hardware_mode != old_settings.hardware_mode {
        lite::unload_engine();
        prewarm(&app);
    }
    // Long-lived windows (the capsule) are created once at startup and
    // never poll for settings changes on their own — without this, e.g.
    // changing Dictation Mode here left the capsule's Touch Up gate
    // (`CapsuleWindow.tsx`'s `dictationModeRef`) stuck on whatever mode was
    // active when it first mounted, for the rest of the session.
    let _ = app.emit("ivy://settings-updated", ());
    Ok(())
}

#[tauri::command]
fn get_history(app: tauri::AppHandle) -> Vec<DictationSession> {
    load_history(&app)
}

#[tauri::command]
fn delete_history_entry(app: tauri::AppHandle, id: String) -> Result<(), String> {
    if !is_valid_session_id(&id) {
        return Err("invalid session id format".into());
    }
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut sessions = load_history(&app);
    if let Some(s) = sessions.iter().find(|s| s.id == id) {
        if path_within_audio_dir(&app, &s.audio_path) {
            let _ = fs::remove_file(&s.audio_path);
        }
    }
    sessions.retain(|s| s.id != id);
    write_history_atomic(&app, &sessions);
    let _ = app.emit("ivy://history-updated", ());
    Ok(())
}

// Manual, explicit, user-triggered — sensitive voice recordings and transcripts
// are deleted immediately. Decoupled lifetime progress and stats are PRESERVED in stats.json.
#[tauri::command]
fn clear_all_history(app: tauri::AppHandle) -> Result<(), String> {
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Every file in the recordings folder, listed in history or not, overwritten with zeros, then deleted.
    if let Ok(entries) = fs::read_dir(audio_dir(&app)) {
        for entry in entries.flatten() {
            wipe_file(&entry.path());
        }
    }
    // The transcripts: the old history file is overwritten in place before it's replaced.
    wipe_file(&history_path(&app));
    let _ = fs::remove_file(history_path(&app).with_extension("json.tmp"));
    write_history_atomic(&app, &[]);
    // The copy held in memory for Alt + V, and Touch Up's record of the last paste.
    *MANUAL_PASTE_TEXT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    debug_log(&app, "clear all: recordings and transcripts wiped");
    let _ = app.emit("ivy://history-updated", ());
    Ok(())
}

/// Overwrites a file with zeros, then deletes it. (On an SSD the drive itself may still keep old blocks
/// until it reuses them; no app can force that.)
fn wipe_file(path: &std::path::Path) {
    if let Ok(meta) = fs::metadata(path) {
        if meta.is_file() {
            let _ = fs::write(path, vec![0u8; meta.len() as usize]);
            let _ = fs::remove_file(path);
        }
    }
}

/// Returns the user's lifetime progress, streak, and daily activity.
/// Stored separately from ephemeral history so sensitive voice/transcripts
/// can be wiped without resetting productivity progress to zero.
#[tauri::command]
fn get_user_stats(app: tauri::AppHandle) -> UserStats {
    let _guard = STATS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    load_user_stats(&app)
}

/// Persists a finished session and performs the real paste into whatever
/// app was focused when the hotkey first went down. The whole
/// record→transcribe→cleanup→paste pipeline (see `run_dictation_pipeline`)
/// converges here. Returns whether a real paste happened (vs. clipboard-only
/// fallback) so the caller can report the truth, not an assumed success.
fn persist_and_paste(app: &tauri::AppHandle, session: DictationSession, target_hwnd: isize) -> bool {
    {
        let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut sessions = load_history(app);
        sessions.insert(0, session.clone());
        write_history_atomic(app, &sessions);
    }
    // Update decoupled lifetime user stats (zero-text, pure metrics)
    {
        let _guard = STATS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut stats = load_user_stats(app);
        stats.record_dictation(session.words_count, session.duration_sec);
        let _ = save_user_stats_atomic(app, &stats);
    }
    let _ = app.emit("ivy://history-updated", ());
    let _ = app.emit("ivy://stats-updated", ());
    paste_text(session.full_transcript, Some(target_hwnd))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DictationComplete {
    success: bool,
    // "" for the ordinary "no speech heard" case; "mic_error" specifically
    // when the microphone itself never opened, so the UI can tell the user
    // the real cause instead of a generic "didn't hear anything" message.
    #[serde(default)]
    reason: String,
    active_app: String,
    // "" if no audio was kept (e.g. truly nothing captured) — otherwise the
    // capsule can offer an inline retry against this exact session without
    // asking the user to speak again.
    session_id: String,
    // Real Ctrl+V happened vs. clipboard-only fallback (no matching text
    // box found) — the capsule must say "Copied", never "Pasted", when this
    // is false. Meaningless when `success` is false.
    pasted: bool,
    text: String,
}

/// How the GPU is reached, for debug.log: llama.cpp's Vulkan backend on Windows, Metal on macOS.
const GPU_BACKEND: &str = if cfg!(target_os = "macos") { "GPU (Metal)" } else { "GPU (Vulkan)" };

/// Capsule note when macOS hasn't given Ivy the Accessibility permission that pasting needs.
#[cfg(target_os = "macos")]
const ACCESSIBILITY_NOTICE: &str = "Allow Ivy in System Settings › Accessibility to paste";

/// macOS: set when the model gave garbage on the Mac's GPU in the once-per-run check (`metal_check`); Ivy then
/// dictates on the chip's CPU cores until it restarts.
#[cfg(target_os = "macos")]
static METAL_BROKEN: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static METAL_CHECKED: AtomicBool = AtomicBool::new(false);

/// macOS: files for a crash during the GPU check. `metal-check.running` exists only while the check runs; found at
/// the next start, Ivy died inside Metal, and `metal-disabled` (holding this build's id) keeps this build on the CPU
/// instead of crashing again at every start. A newer build tries the GPU again.
#[cfg(target_os = "macos")]
fn metal_marker(app: &tauri::AppHandle, name: &str) -> Option<std::path::PathBuf> {
    app.path().app_local_data_dir().ok().map(|dir| dir.join(name))
}

/// Which build this is: the version plus, for a build made on GitHub, its commit.
#[cfg(target_os = "macos")]
fn build_id() -> String {
    format!("{}-{}", env!("CARGO_PKG_VERSION"), option_env!("GITHUB_SHA").unwrap_or("local"))
}

/// macOS, at start-up: a check that never finished means a crash inside Metal last time.
#[cfg(target_os = "macos")]
fn remember_metal_crash(app: &tauri::AppHandle) {
    let (Some(running), Some(disabled)) = (metal_marker(app, "metal-check.running"), metal_marker(app, "metal-disabled")) else {
        return;
    };
    if running.exists() {
        let _ = fs::write(&disabled, build_id());
        let _ = fs::remove_file(&running);
    }
    if fs::read_to_string(&disabled).is_ok_and(|id| id == build_id()) {
        METAL_BROKEN.store(true, Ordering::SeqCst);
        METAL_CHECKED.store(true, Ordering::SeqCst);
        debug_log(app, "Ivy stopped during its last GPU check: this build dictates on the CPU");
    }
}

/// macOS: proof that the model computes correctly on this Mac's GPU, from a known clip (tests/fixtures/sample.wav,
/// "The quick brown fox jumps over the lazy dog."). GitHub's virtual Macs write "!!!!" there; a real Apple GPU
/// writes the sentence.
#[cfg(target_os = "macos")]
fn metal_check(engine: &lite::LiteEngine) -> Result<(), String> {
    const CLIP: &[u8] = include_bytes!("../tests/fixtures/sample.wav");
    let mut reader = hound::WavReader::new(std::io::Cursor::new(CLIP)).map_err(|e| e.to_string())?;
    let samples: Vec<f32> = reader.samples::<i16>().map(|s| s.unwrap_or(0) as f32 / i16::MAX as f32).collect();
    let text = engine.transcribe(&samples, &[], std::time::Duration::from_secs(20))?;
    let heard = text.to_lowercase();
    if heard.contains("fox") && heard.contains("dog") { Ok(()) } else { Err(format!("heard {text:?}")) }
}

/// A Mac has one chip: the model runs on its GPU through Metal, unless that GPU failed `metal_check`. No CPU mode,
/// battery rule or GPU sharing there (Settings and the setup wizard show neither).
#[cfg(target_os = "macos")]
fn should_use_gpu(_app: &tauri::AppHandle) -> bool {
    !METAL_BROKEN.load(Ordering::SeqCst)
}

#[cfg(not(target_os = "macos"))]
fn should_use_gpu(app: &tauri::AppHandle) -> bool {
    let settings = load_settings(app);

    // Real, unconditional override: a laptop running on battery (AC
    // unplugged) always dictates on CPU, regardless of the configured
    // GPU/CPU mode — GPU inference is the single biggest battery drain in
    // the app, and there's no UI toggle for this because it isn't a mode
    // choice, it's the same automatic-fallback pattern as VRAM eviction
    // below (see `is_on_battery`).
    if gpu_monitor::is_on_battery() {
        return false;
    }

    if !settings.smart_vram_eviction {
        return settings.hardware_mode == "gpu";
    }

    if gpu_monitor::is_vram_evicted() {
        log::info!("Ivy: VRAM is currently evicted (high GPU load). Falling back to CPU for dictation.");
        return false;
    }

    settings.hardware_mode == "gpu"
}

/// Turns samples into final text: the lite engine (Qwen3-ASR-1.7B, fine-tuned by the lab; it resolves
/// self-corrections itself), then the rulebooks for the chosen tone, then the personal dictionary. Shared by the
/// fresh-dictation pipeline and `retry_transcription` so there's one real implementation, not two that could drift.
fn transcribe_and_clean(
    app: &tauri::AppHandle,
    models: &std::path::Path,
    samples: &[f32],
    tone_preset: &str,
) -> String {
    let prefer_gpu = should_use_gpu(app);
    // Quiet microphones: peak-normalise so soft voices reach the model at a usable level (audio::normalize_audio).
    // Both audio copies here are zeroed when this function returns (audio::Wiped).
    let normalized = audio::Wiped(audio::normalize_audio(samples));
    let stt_samples = if normalized.0.is_empty() { samples } else { &normalized.0 };

    // Hallucinations book, stage A (src/rulebooks/hallucinations.rs): no voice -> no text. The model gets the
    // full audio (not the pause-squeezed copy): squeezing pauses dropped the end of long dictations (Task 6 Bug B).
    let (voiced_audio, voice) = rulebooks::hallucinations::prepare_audio(stt_samples, 16000);
    drop(audio::Wiped(voiced_audio));
    if voice.voiced_secs < 0.25 {
        debug_log(app, &format!("no speech detected ({:.2}s voiced of {:.2}s) — nothing transcribed", voice.voiced_secs, voice.total_secs));
        return String::new();
    }

    let settings = load_settings(app);
    let dictionary = settings.personal_dictionary;
    let t_lite = std::time::Instant::now();
    let is_cpu_mode = !prefer_gpu;
    let raw = match lite::engine(models, is_cpu_mode) {
        Ok(engine) => {
            let audio_secs = stt_samples.len() as u64 / 16_000;
            let timeout = if is_cpu_mode {
                std::time::Duration::from_secs(60 + audio_secs * 2)
            } else {
                std::time::Duration::from_secs(30 + audio_secs / 2)
            };
            match engine.transcribe(stt_samples, &dictionary, timeout) {
                Ok(text) => {
                    let backend = if engine.is_cpu_mode { "CPU" } else { GPU_BACKEND };
                    debug_log(app, &format!("lite ok via {backend} in {}ms, {} chars", t_lite.elapsed().as_millis(), text.chars().count()));
                    text
                }
                Err(e) => {
                    debug_log(app, &format!("lite transcribe() failed in {}ms: {e}", t_lite.elapsed().as_millis()));
                    log::error!("Ivy: Lite transcription failed: {e}");
                    String::new()
                }
            }
        }
        Err(e) => {
            debug_log(app, &format!("lite engine load failed: {e}"));
            log::error!("Ivy: Lite engine unavailable: {e}");
            String::new()
        }
    };

    // Hallucinations book, stage B: safety net against loops, impossible speaking rate
    let raw = rulebooks::hallucinations::clean_asr_text(&raw, voice);
    if raw.is_empty() {
        return raw;
    }
    let formatted = rulebooks::after_model(&raw, rulebooks::Tone::from_label(tone_preset));
    // Snippets go last, so tone, formatting and the dictionary never touch the saved text.
    apply_snippets(&apply_personal_dictionary(&formatted, &dictionary), &settings.snippets)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![0usize; m + 1];
    for (j, cell) in dp.iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        let mut prev_diag = dp[0];
        dp[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            let cur = (dp[j] + 1).min(dp[j - 1] + 1).min(prev_diag + cost);
            prev_diag = dp[j];
            dp[j] = cur;
        }
    }
    dp[m]
}

/// Metaphone-inspired phonetic encoder tuned for ASR error patterns:
/// - Preserves the first vowel (so "lattice" and "lettuce" remain distinct).
/// - Drops unstressed/subsequent vowels.
/// - Collapses ASR confusion pairs (e.g. 'v' and 'w', soft 'c' and 's', 'ph' and 'f').
/// - Collapses adjacent duplicate consonants.
fn phonetic_code(word: &str) -> String {
    let lower: Vec<char> = word
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect();
    if lower.is_empty() {
        return String::new();
    }

    let is_vowel = |c: char| matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y');

    let mut start_idx = 0;
    // Silent initial pairs (kn, gn, pn, wr, ps)
    if lower.len() >= 2 {
        let first_two = (lower[0], lower[1]);
        if matches!(first_two, ('k', 'n') | ('g', 'n') | ('p', 'n') | ('w', 'r') | ('p', 's')) {
            start_idx = 1;
        }
    }

    let mut code = String::with_capacity(lower.len());
    let mut i = start_idx;
    let mut first_vowel_captured = false;

    while i < lower.len() {
        let c = lower[i];
        let next = lower.get(i + 1).copied();

        if is_vowel(c) {
            if !first_vowel_captured {
                code.push(c.to_ascii_uppercase());
                first_vowel_captured = true;
            }
            while i + 1 < lower.len() && is_vowel(lower[i + 1]) {
                i += 1;
            }
            i += 1;
            continue;
        }

        match c {
            'b' => code.push('B'),
            'c' => {
                if next == Some('h') {
                    // "ch" in French loanwords (crochet, machine) and sibilants -> 'S'
                    code.push('S');
                    i += 1;
                } else if next == Some('k') {
                    code.push('K');
                    i += 1;
                } else if matches!(next, Some('e') | Some('i') | Some('y')) {
                    // Soft C (lattice, city, circle) -> 'S'
                    code.push('S');
                } else {
                    code.push('K');
                }
            }
            'd' => {
                if next == Some('g') {
                    code.push('J');
                    i += 1;
                } else {
                    code.push('D');
                }
            }
            'f' => code.push('F'),
            'g' => {
                if next == Some('h') {
                    if i == start_idx {
                        code.push('G');
                    }
                    i += 1;
                } else if matches!(next, Some('e') | Some('i') | Some('y')) {
                    code.push('J');
                } else {
                    code.push('G');
                }
            }
            'h' => {
                if i == start_idx {
                    code.push('H');
                }
            }
            'j' => code.push('J'),
            'k' => code.push('K'),
            'l' => code.push('L'),
            'm' => code.push('M'),
            'n' => code.push('N'),
            'p' => {
                if next == Some('h') {
                    code.push('F');
                    i += 1;
                } else {
                    code.push('P');
                }
            }
            'q' => code.push('K'),
            'r' => code.push('R'),
            's' => {
                if next == Some('h') {
                    code.push('S');
                    i += 1;
                } else {
                    code.push('S');
                }
            }
            't' => {
                if next == Some('h') {
                    code.push('T');
                    i += 1;
                } else {
                    code.push('T');
                }
            }
            // Collapse V and W: the most frequent ASR acoustic confusion pair (vendor/wender)
            'v' | 'w' => code.push('V'),
            'x' => {
                code.push('K');
                code.push('S');
            }
            'z' => code.push('S'),
            _ => {}
        }
        i += 1;
    }

    let mut collapsed = String::with_capacity(code.len());
    let mut prev_char = None;
    for ch in code.chars() {
        if Some(ch) != prev_char {
            collapsed.push(ch);
            prev_char = Some(ch);
        }
    }
    collapsed
}

/// Matches the casing of the original word when applying a dictionary replacement:
/// Preserves uppercase sentence starts, ALL-CAPS acronyms, and internal casing (e.g. "iPhone", "PostgreSQL").
fn match_casing(original: &str, replacement: &str) -> String {
    if original.is_empty() || replacement.is_empty() {
        return replacement.to_string();
    }
    if replacement.chars().skip(1).any(|c| c.is_uppercase()) {
        return replacement.to_string();
    }
    let is_all_upper = original.chars().all(|c| !c.is_alphabetic() || c.is_uppercase());
    if is_all_upper && original.chars().any(|c| c.is_alphabetic()) {
        return replacement.to_uppercase();
    }
    let first_orig_upper = original.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
    if first_orig_upper {
        let mut chars = replacement.chars();
        if let Some(first) = chars.next() {
            return format!("{}{}", first.to_uppercase(), chars.as_str());
        }
    }
    replacement.to_string()
}

/// Safely replaces occurrences of `needle` in `haystack` case-insensitively,
/// guarding against Unicode length expansion shifts on character boundaries.
fn replace_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    let needle_lower = needle.to_lowercase();
    let mut result = haystack.to_string();
    let mut search_from = 0;
    while search_from < result.len() {
        let Some(rel_pos) = result[search_from..].to_lowercase().find(&needle_lower) else { break };
        let pos = search_from + rel_pos;
        let end = pos + needle.len();
        if result.is_char_boundary(pos)
            && result.is_char_boundary(end)
            && result[pos..end].to_lowercase() == needle_lower
        {
            result.replace_range(pos..end, replacement);
            search_from = pos + replacement.len();
        } else {
            let mut next = pos + 1;
            while next < result.len() && !result.is_char_boundary(next) {
                next += 1;
            }
            search_from = next;
        }
    }
    result
}

/// Handles multi-word phrases and domain names in personal dictionary:
/// - Replaces exact occurrences case-insensitively.
/// - Corrects glued or phonetically misheard domain names (e.g. "thewender.com" -> "the vendor.com" or "thevendor.com").
/// - Corrects multi-token phonetic variants (e.g. "the wender.com" -> "the vendor.com").
fn apply_domain_and_phrase_corrections(text: &str, dictionary: &[String]) -> String {
    let mut result = text.to_string();

    for entry in dictionary {
        let entry_trimmed = entry.trim();
        if entry_trimmed.is_empty() {
            continue;
        }

        let is_domain = entry_trimmed.contains('.');
        let is_phrase = entry_trimmed.contains(' ');

        // 1. Exact phrase / domain replace (case-insensitive)
        if is_phrase || is_domain {
            result = replace_insensitive(&result, entry_trimmed, entry_trimmed);
        }

        if !is_domain && !is_phrase {
            continue;
        }

        let entry_compact: String = entry_trimmed
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase();
        let p_entry = phonetic_code(&entry_compact.replace('.', " "));

        // 2. Fuzzy single-token domain match (e.g. "thewender.com" matching "the vendor.com" or "thevendor.com")
        let tokens: Vec<String> = result
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        for tok in tokens {
            let tok_clean: String = tok
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
                .collect();
            let tok_compact = tok_clean.to_lowercase();
            if tok_compact.is_empty() || tok_compact == entry_trimmed.to_lowercase() {
                continue;
            }

            let is_match = if tok_compact == entry_compact {
                true
            } else if is_domain && tok_compact.contains('.') && entry_compact.contains('.') {
                let dist = levenshtein(&tok_compact, &entry_compact);
                let p_tok = phonetic_code(&tok_compact.replace('.', " "));
                let tok_ext = tok_compact.rsplit('.').next().unwrap_or("");
                let entry_ext = entry_compact.rsplit('.').next().unwrap_or("");
                let same_ext = !tok_ext.is_empty() && tok_ext == entry_ext;

                (dist <= 2 && same_ext) || (p_tok == p_entry && !p_entry.is_empty() && same_ext)
            } else {
                false
            };

            if is_match {
                result = replace_insensitive(&result, &tok_clean, entry_trimmed);
            }
        }

        // 3. Fuzzy multi-token phrase match (e.g. "the wender.com" matching "the vendor.com")
        let entry_word_count = entry_trimmed.split_whitespace().count();
        if entry_word_count >= 2 {
            let words: Vec<String> = result.split_whitespace().map(|s| s.to_string()).collect();
            if words.len() >= entry_word_count {
                for i in 0..=(words.len() - entry_word_count) {
                    let span_slice = &words[i..i + entry_word_count];
                    let span_text = span_slice.join(" ");
                    let span_compact: String = span_text
                        .chars()
                        .filter(|c| !c.is_whitespace() && (c.is_alphanumeric() || *c == '.'))
                        .collect::<String>()
                        .to_lowercase();

                    let dist = levenshtein(&span_compact, &entry_compact);
                    let p_span = phonetic_code(&span_compact.replace('.', " "));

                    let is_match = if span_compact == entry_compact {
                        true
                    } else if dist <= 2 && !p_entry.is_empty() && p_span == p_entry {
                        true
                    } else {
                        false
                    };

                    if is_match {
                        result = replace_insensitive(&result, &span_text, entry_trimmed);
                        break;
                    }
                }
            }
        }
    }

    result
}

fn is_common_english_word(lower: &str) -> bool {
    const COMMON_WORDS: &[&str] = &[
        "about", "above", "across", "after", "again", "against", "almost", "along", "already",
        "also", "although", "always", "among", "another", "answer", "around", "because",
        "before", "behind", "being", "below", "between", "both", "call", "came", "come",
        "could", "different", "does", "done", "down", "during", "each", "early", "even",
        "every", "find", "first", "found", "from", "gave", "give", "good", "great", "have",
        "having", "head", "hear", "help", "here", "high", "home", "house", "into", "just",
        "keep", "kind", "know", "large", "last", "leave", "left", "life", "light", "like",
        "line", "little", "live", "long", "look", "made", "make", "many", "might", "more",
        "most", "move", "much", "must", "name", "near", "need", "never", "next", "night",
        "number", "often", "once", "only", "open", "order", "other", "over", "part", "people",
        "place", "point", "read", "real", "right", "said", "same", "seem", "should", "show",
        "small", "some", "something", "sound", "stand", "start", "state", "still", "such",
        "take", "tell", "than", "that", "their", "them", "then", "there", "these", "they",
        "thing", "think", "this", "those", "thought", "three", "through", "time", "together",
        "under", "until", "upon", "very", "want", "water", "well", "went", "were", "what",
        "when", "where", "which", "while", "white", "will", "with", "without", "word", "work",
        "world", "would", "write", "year", "your",
    ];
    COMMON_WORDS.binary_search(&lower).is_ok()
}

fn correct_word(word: &str, dictionary: &[&String]) -> String {
    if word.is_empty() {
        return String::new();
    }
    let lower = word.to_lowercase();
    for term in dictionary {
        if term.to_lowercase() == lower {
            return match_casing(word, term);
        }
    }

    // Task 6 Bug A: never fuzzy- or phonetic-replace contractions, very short words,
    // or common English words. Exact case-insensitive matches above already handled intentional terms.
    let is_contraction = word.contains('\'') || word.contains('’');
    let is_short = word.chars().count() < 4;
    let is_common = is_common_english_word(&lower);
    if is_contraction || is_short || is_common {
        return word.to_string();
    }

    let p_word = phonetic_code(&lower);

    // Phonetic matching pass
    let mut best_phonetic: Option<&str> = None;
    let mut best_phonetic_pdist = usize::MAX;
    let mut best_phonetic_cdist = usize::MAX;

    if !p_word.is_empty() {
        for term in dictionary {
            let term_lower = term.to_lowercase();
            let p_term = phonetic_code(&term_lower);
            if p_term.is_empty() {
                continue;
            }

            let p_dist = levenshtein(&p_word, &p_term);
            let c_dist = levenshtein(&lower, &term_lower);
            let len_diff = (word.chars().count() as isize - term.chars().count() as isize).abs() as usize;

            // Condition 1: Exact phonetic sound-alike match
            // e.g. "lattice" -> "lattes" (LATS == LATS), "wender" -> "vendor" (VENDR == VENDR)
            if p_dist == 0 && len_diff <= 3 {
                let max_c_dist = (term.chars().count() / 2).max(2);
                if c_dist <= max_c_dist && c_dist < best_phonetic_cdist {
                    best_phonetic = Some(term.as_str());
                    best_phonetic_pdist = 0;
                    best_phonetic_cdist = c_dist;
                }
            }
            // Condition 2: Near-phonetic match (phonetic edit distance 1) for longer words (>= 6 chars)
            // e.g. "crochetant" (KROSTNT) -> "croissant" (KROSNT)
            else if p_dist == 1 && p_word.len() >= 5 && p_term.len() >= 5 && len_diff <= 2 {
                let max_c_dist = (term.chars().count() * 55 / 100).max(2);
                if c_dist <= max_c_dist && (p_dist < best_phonetic_pdist || (p_dist == best_phonetic_pdist && c_dist < best_phonetic_cdist)) {
                    best_phonetic = Some(term.as_str());
                    best_phonetic_pdist = p_dist;
                    best_phonetic_cdist = c_dist;
                }
            }
        }
    }

    if let Some(matched) = best_phonetic {
        return match_casing(word, matched);
    }

    // Levenshtein character edit distance fallback:
    // Scaled to word length: short words (<4 chars) require exact match,
    // medium words allow 1-2 edits, long words allow up to len/3 edits.
    let mut best: Option<&str> = None;
    let mut best_dist = usize::MAX;
    for term in dictionary {
        let term_len = term.chars().count();
        let max_dist = match term_len {
            0..=3 => 0,
            4..=5 => 1,
            6..=8 => 2,
            _ => (term_len / 3).max(2),
        };
        if max_dist == 0 {
            continue;
        }
        let dist = levenshtein(&lower, &term.to_lowercase());
        if dist <= max_dist && dist < best_dist {
            best = Some(term.as_str());
            best_dist = dist;
        }
    }
    match_casing(word, best.unwrap_or(word))
}

/// Real personal-dictionary correction, run on every dictation: swaps in
/// the user's taught spelling for a near-miss the STT model produced.
/// Multi-word phrases and domain names get exact and fuzzy glued/phonetic replacement;
/// single words get phonetic sound-alike and scaled edit-distance matching.
fn apply_personal_dictionary(text: &str, dictionary: &[String]) -> String {
    if dictionary.is_empty() || text.trim().is_empty() {
        return text.to_string();
    }

    // Step 1: Domain and multi-word phrase corrections
    let result = apply_domain_and_phrase_corrections(text, dictionary);

    // Step 2: Single word corrections
    let single_words: Vec<&String> = dictionary
        .iter()
        .filter(|d| !d.trim().contains(' ') && !d.trim().contains('.'))
        .collect();

    if single_words.is_empty() {
        return result;
    }

    let mut out = String::with_capacity(result.len());
    let mut word = String::new();
    for c in result.chars() {
        if c.is_alphanumeric() || c == '\'' || c == '’' {
            word.push(c);
        } else {
            out.push_str(&correct_word(&word, &single_words));
            word.clear();
            out.push(c);
        }
    }
    out.push_str(&correct_word(&word, &single_words));
    out
}

/// Runs on a spawned worker thread (never the shortcut-handler thread, and
/// never touches the cpal `Recorder` — that's already been stopped and
/// reduced to plain samples by the caller): saves the real audio, transcribes
/// it for real (the lite model, then the tone's rulebooks), then pastes and persists — for every attempt, success or
/// failure, so a failure is retryable from the saved audio and never just
/// silently drops what was recorded. The capsule only ever hears the result
/// via `ivy://dictation-complete`.
// Below this peak amplitude, treat the recording as real silence (wrong/
// muted mic, or Windows' "Let desktop apps access your microphone" toggle
// off) rather than real speech the model merely failed on. Silence never
// gets a History entry at all (Yash: "it shouldn't go to history... just
// keep that in the overlay" — real speech that still fails to transcribe
// is the one case that still deserves a saved recording + retry option).
const SILENCE_PEAK_THRESHOLD: f32 = 0.01;

// Real repetition loops ("you can do it" x40) are now caught by the
// decoder's own `repeating_cycle` guard (`stt.rs`) the instant they start,
// so this cap only needs to be a sane outer bound on a single dictation,
// not a workaround for that failure mode — a real ~2 minute recording was
// cut off at the old 60s value (Yash: "it only translated half, the other
// half is gone"). 5 minutes (Yash, 2026-10-06). At the limit the recording is finished and pasted
// (see the level thread in `handle_hotkey_down`), never silently cut.
const MAX_DICTATION_SECS: f64 = 300.0;

fn run_dictation_pipeline(
    app: tauri::AppHandle,
    mut samples: Vec<f32>,
    active_app: String,
    tone_preset: String,
    dictation_id: u64,
    target_hwnd: isize,
    for_wizard: bool,
) {
    if is_dictation_cancelled(dictation_id) {
        debug_log(&app, "pipeline aborted: dictation cancelled by user before processing");
        bridge_capsule_show(&app, false);
        audio::zeroize_samples(&mut samples);
        return;
    }
    let models = models_dir(&app);
    let max_samples = (MAX_DICTATION_SECS * 16_000.0) as usize;
    if samples.len() > max_samples {
        debug_log(
            &app,
            &format!("held for {:.1}s, longer than the {MAX_DICTATION_SECS}s cap — truncating", samples.len() as f64 / 16_000.0),
        );
        samples.truncate(max_samples);
    }
    let duration_sec = samples.len() as f64 / 16_000.0;
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let id = format!("session-{created_at}");

    let peak = samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
    let rms = if samples.is_empty() {
        0.0
    } else {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    };
    debug_log(
        &app,
        &format!(
            "captured {} samples ({:.2}s @16kHz), peak={:.4} rms={:.5}, models_dir={}",
            samples.len(),
            duration_sec,
            peak,
            rms,
            models.display(),
        ),
    );

    let heard_something = peak >= SILENCE_PEAK_THRESHOLD;
    if !heard_something {
        debug_log(&app, "peak amplitude near-zero — treating as silence, not persisting to history (hold too short, wrong/muted device, or mic privacy off)");
        audio::zeroize_samples(&mut samples);
        let _ = app.emit(
            "ivy://dictation-complete",
            DictationComplete { success: false, reason: String::new(), active_app, session_id: String::new(), pasted: false, text: String::new() },
        );
        return;
    }

    if is_dictation_cancelled(dictation_id) {
        debug_log(&app, "pipeline aborted: dictation cancelled by user before STT");
        bridge_capsule_show(&app, false);
        audio::zeroize_samples(&mut samples);
        return;
    }

    let text = transcribe_and_clean(&app, &models, &samples, &tone_preset);

    if is_dictation_cancelled(dictation_id) {
        debug_log(&app, "pipeline aborted: dictation cancelled by user after transcription — discarding recording and history");
        bridge_capsule_show(&app, false);
        audio::zeroize_samples(&mut samples);
        return;
    }

    if for_wizard {
        audio::zeroize_samples(&mut samples);
        debug_log(&app, "wizard practice dictation: result sent to the wizard only (not pasted, not saved)");
        let _ = app.emit(
            "ivy://dictation-complete",
            DictationComplete { success: !text.is_empty(), reason: String::new(), active_app, session_id: String::new(), pasted: false, text },
        );
        return;
    }

    if text.is_empty() {
        // Real audio, real energy, but the model still came back empty —
        // rare now with the decoder's premature-EOS guard, but when it
        // happens the recording is real and worth keeping with a retry
        // option, unlike plain silence above.
        let audio_path = save_audio_wav(&app, &id, &samples);
        audio::zeroize_samples(&mut samples);
        let session = DictationSession {
            id: id.clone(),
            preview: "Couldn't transcribe — tap to retry".to_string(),
            full_transcript: String::new(),
            timestamp: format_timestamp(created_at),
            created_at,
            duration: format_duration(duration_sec),
            duration_sec,
            tone_preset,
            app_target: active_app.clone(),
            words_count: 0,
            audio_duration: duration_sec,
            audio_path: audio_path.clone(),
            title: None,
        };
        {
            let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut sessions = load_history(&app);
            sessions.insert(0, session);
            write_history_atomic(&app, &sessions);
        }
        let _ = app.emit("ivy://history-updated", ());
        let _ = app.emit(
            "ivy://dictation-complete",
            DictationComplete {
                success: false,
                reason: String::new(),
                active_app,
                session_id: if audio_path.is_empty() { String::new() } else { id },
                pasted: false,
                text: String::new(),
            },
        );
        return;
    }

    let audio_path = save_audio_wav(&app, &id, &samples);
    // Cryptographic zeroization: overwrite raw voice samples in RAM
    audio::zeroize_samples(&mut samples);
    let session = DictationSession {
        id: id.clone(),
        preview: char_safe_preview(&text),
        full_transcript: text.clone(),
        timestamp: format_timestamp(created_at),
        created_at,
        duration: format_duration(duration_sec),
        duration_sec,
        tone_preset,
        app_target: active_app.clone(),
        words_count: text.split_whitespace().count() as u32,
        audio_duration: duration_sec,
        audio_path,
        title: None,
    };

    let pasted = persist_and_paste(&app, session, target_hwnd);
    // Paste has already happened — nothing below this can add latency to
    // what the user is actually waiting on. Runs on its own thread so a
    // slow/cold model load never delays `ivy://dictation-complete` either.
    spawn_title_generation(&app, id.clone(), text.clone());
    let _ = app.emit(
        "ivy://dictation-complete",
        DictationComplete { success: true, reason: String::new(), active_app, session_id: id, pasted, text },
    );
    // After the result, so the capsule ends on the reason rather than on "press Option + V" (which can't paste
    // without the permission either).
    #[cfg(target_os = "macos")]
    if !pasted && !macos::is_trusted() {
        let _ = app.emit("ivy://notice", ACCESSIBILITY_NOTICE);
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RetryResult {
    success: bool,
    pasted: bool,
    transcript: String,
    words_count: u32,
}

/// Re-runs transcription against a previously saved recording — no
/// re-recording needed. Used both by the capsule's own inline retry button
/// (right after a failure) and History's retry action (any time after).
/// Updates the existing entry in place on success, copies the result to clipboard,
/// and pastes the result; leaves it untouched on a second failure.
/// `from_capsule`: the capsule's own Retry button (not History's); on macOS that click made Ivy the active app, and
/// the app the text is for comes back to the front before the paste.
#[tauri::command]
fn retry_transcription(app: tauri::AppHandle, id: String, from_capsule: Option<bool>) -> Result<RetryResult, String> {
    if !is_valid_session_id(&id) {
        return Err("invalid session id format".into());
    }
    if RETRYING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("a transcription retry is already running".into());
    }
    struct RetryGuard;
    impl Drop for RetryGuard {
        fn drop(&mut self) {
            RETRYING.store(false, Ordering::SeqCst);
        }
    }
    let _retry_guard = RetryGuard;

    let sessions = load_history(&app);
    let idx = sessions.iter().position(|s| s.id == id).ok_or("no such session")?;
    let audio_path = sessions[idx].audio_path.clone();
    let tone_preset = sessions[idx].tone_preset.clone();
    if !path_within_audio_dir(&app, &audio_path) {
        return Err("no saved audio for this session".into());
    }

    // Audio file safety checks: size and format limits
    let metadata = fs::metadata(&audio_path).map_err(|e| e.to_string())?;
    if metadata.len() > 100 * 1024 * 1024 {
        return Err("audio file exceeds maximum safe limit (100MB)".into());
    }

    let mut reader = hound::WavReader::open(&audio_path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.channels > 2 || spec.sample_rate < 8000 || spec.sample_rate > 96000 {
        return Err("unsupported audio format specification".into());
    }
    if reader.duration() > 16000 * 60 * 30 {
        return Err("audio recording exceeds maximum allowable duration (30 minutes)".into());
    }

    let mut samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap_or(0) as f32 / i16::MAX as f32)
        .collect();

    let models = models_dir(&app);
    let text = transcribe_and_clean(&app, &models, &samples, &tone_preset);
    
    // In-memory audio sample zeroization for sensitive speech data
    audio::zeroize_samples(&mut samples);

    if text.is_empty() {
        return Err("still couldn't transcribe".into());
    }

    let words_count = text.split_whitespace().count() as u32;
    {
        let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut sessions = load_history(&app);
        if let Some(idx) = sessions.iter().position(|s| s.id == id) {
            sessions[idx].full_transcript = text.clone();
            sessions[idx].preview = char_safe_preview(&text);
            sessions[idx].words_count = words_count;
        }
        write_history_atomic(&app, &sessions);
    }
    let _ = app.emit("ivy://history-updated", ());

    #[cfg(target_os = "macos")]
    if from_capsule == Some(true) {
        let target = *LAST_EXTERNAL_HWND.lock().unwrap_or_else(|e| e.into_inner()) as i32;
        macos::bring_to_front(target, std::time::Duration::from_millis(600), true);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = from_capsule;
    // `paste_text` itself always ends up putting `text` on the clipboard,
    // in both its real-paste and clipboard-only-fallback paths — writing it
    // here first was redundant, and worse, clobbered the user's real
    // clipboard before `paste_text` got a chance to capture it for restoring.
    let pasted = paste_text(text.clone(), None);
    Ok(RetryResult {
        success: true,
        pasted,
        transcript: text,
        words_count,
    })
}

const MIN_WORDS_FOR_TITLE: usize = 5;

/// Heuristic, instant title generator — takes the first 4 to 6 words, cleans trailing
/// punctuation, and formats with clean capitalization. Zero latency, zero model load.
fn heuristic_title(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < MIN_WORDS_FOR_TITLE {
        return None;
    }
    let title_words = &words[..words.len().min(6)];
    let joined = title_words.join(" ");
    let cleaned = joined.trim_end_matches(|c: char| !c.is_alphanumeric()).trim();
    if cleaned.is_empty() {
        return None;
    }
    let title = cleaned
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(title)
}

/// Instant title generation for History entries — runs immediately without loading
/// or running any neural models.
fn spawn_title_generation(app: &tauri::AppHandle, id: String, text: String) {
    let Some(title) = heuristic_title(&text) else { return };
    let app = app.clone();
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut sessions = load_history(&app);
    if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
        s.title = Some(title);
    }
    write_history_atomic(&app, &sessions);
    let _ = app.emit("ivy://history-updated", ());
}

/// "Touch Up" — the user clicked the optional capsule button that appears
/// for a few seconds right after a real paste (see `FloatingCapsule.tsx`).
/// Never runs automatically and never adds latency to the live pipeline;
/// it swaps the just-pasted text for a spell-fixed copy of itself
/// (`spellcheck.rs`: typos only, never rephrasing). Nothing to fix -> nothing
/// is swapped. Returns whether the text was swapped.
#[tauri::command]
fn touch_up_transcript(app: tauri::AppHandle, id: String) -> Result<bool, String> {
    if !is_valid_session_id(&id) {
        return Err("invalid session id format".into());
    }
    if TOUCHING_UP.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("a touch up is already in progress".into());
    }
    struct TouchUpGuard;
    impl Drop for TouchUpGuard {
        fn drop(&mut self) {
            TOUCHING_UP.store(false, Ordering::SeqCst);
        }
    }
    let _guard = TouchUpGuard;

    let raw = {
        let sessions = load_history(&app);
        let session = sessions.iter().find(|s| s.id == id).ok_or("no such session")?;
        session.full_transcript.clone()
    };
    if raw.trim().is_empty() {
        return Err("no transcript to touch up".into());
    }
    let corrected = spellcheck::touch_up(&raw);
    if corrected == raw {
        return Ok(false); // no typos: leave the pasted text alone
    }

    // Peek (not consume) — if the window has already changed since the
    // original paste, `LAST_PASTE` is left alone; only take it once we're
    // actually committing to the swap below.
    let target_hwnd = LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|lp| lp.target_hwnd);
    let Some(target_hwnd) = target_hwnd else {
        return Err("nothing was pasted for this dictation, nothing to touch up".into());
    };
    #[cfg(target_os = "macos")]
    if !macos::is_trusted() {
        return Err("Ivy needs Accessibility permission to swap the text (System Settings › Privacy & Security › Accessibility)".into());
    }
    #[cfg(windows)]
    let same_window = unsafe { GetForegroundWindow().0 as isize == target_hwnd };
    // The Touch Up click made Ivy the active app: the app the text went to is brought back to the front first.
    #[cfg(target_os = "macos")]
    let same_window = macos::bring_to_front(target_hwnd as i32, std::time::Duration::from_millis(600), true);
    #[cfg(not(any(windows, target_os = "macos")))]
    let same_window = false;
    if !same_window {
        return Err("switched to a different window since the paste — can't safely swap the text there".into());
    }

    // Re-check focus right before sending keys, the same race `paste_text`'s own
    // "re-check right before sending keys" guards against for a fresh paste.
    #[cfg(windows)]
    let still_same_window = unsafe { GetForegroundWindow().0 as isize == target_hwnd };
    #[cfg(target_os = "macos")]
    let still_same_window = macos::frontmost_pid() as isize == target_hwnd;
    #[cfg(not(any(windows, target_os = "macos")))]
    let still_same_window = false;
    if !still_same_window {
        return Err("switched to a different window while touching up — can't safely swap the text there".into());
    }

    // Now committing: consume the original paste record for the Ctrl+Z below.
    let Some(last) = LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return Err("nothing was pasted for this dictation, nothing to touch up".into());
    };
    release_held_modifiers();
    let undone = send_ctrl_key(0x5A); // VK_Z
    if !undone {
        // Put the original record back — the undo never happened, so a later Touch Up can still try.
        *LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(last);
        return Err("couldn't undo the original paste".into());
    }

    let pasted = paste_text(corrected.clone(), Some(target_hwnd));
    if !pasted {
        debug_log(&app, "touch-up: undo succeeded but re-paste failed — original text is gone from the target window");
        return Err("undid the original text but couldn't paste the touched-up version — check the target window".into());
    }

    let _guard_h = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut sessions = load_history(&app);
    if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
        s.full_transcript = corrected.clone();
        s.preview = char_safe_preview(&corrected);
        s.words_count = corrected.split_whitespace().count() as u32;
    }
    write_history_atomic(&app, &sessions);
    drop(_guard_h);
    let _ = app.emit("ivy://history-updated", ());
    debug_log(&app, "touch-up: swapped in the corrected text");
    Ok(true)
}

/// Copies a session's saved recording out to the user's own Downloads
/// folder as a plain, permanent .wav — independent of the daily retention
/// on Ivy's internal copy. Returns the destination path for a confirmation
/// toast.
#[tauri::command]
fn extract_audio(app: tauri::AppHandle, id: String) -> Result<String, String> {
    if !is_valid_session_id(&id) {
        return Err("invalid session id format".into());
    }
    let sessions = load_history(&app);
    let session = sessions.iter().find(|s| s.id == id).ok_or("no such session")?;
    if !path_within_audio_dir(&app, &session.audio_path) {
        return Err("no saved audio for this session".into());
    }
    let downloads = app.path().download_dir().map_err(|e| e.to_string())?;
    // Strictly sanitize filename characters to prevent directory traversal
    let safe_ts: String = session.timestamp.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dest = downloads.join(format!("Ivy Recording - {safe_ts} ({}).wav", session.id));
    fs::copy(&session.audio_path, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().into_owned())
}

#[tauri::command]
fn list_audio_input_devices() -> Vec<String> {
    audio::list_input_device_names()
}

// Re-paste an existing transcript from History. Copies to the clipboard and,
// when the previously focused window is still the foreground one, sends the
// paste — same guarded path a fresh dictation takes.
#[tauri::command]
fn repaste_transcript(text: String) -> Result<bool, String> {
    check_rate_limit(&LAST_PASTE_TIMESTAMP, 100)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("nothing to paste".into());
    }
    if trimmed.len() > 100_000 {
        return Err("transcript exceeds maximum allowable paste length (100,000 characters)".into());
    }
    let sanitized = trimmed.replace('\0', "");
    Ok(paste_text(sanitized, None))
}

#[tauri::command]
fn get_active_context(app: tauri::AppHandle) -> ActiveContext {
    let settings = load_settings(&app);
    let tone = tone_for_label(&settings, &foreground_tone_label(), &foreground_exe_name());
    ActiveContext {
        active_app: foreground_app_label(),
        tone_preset: tone,
    }
}

/// Puts a transcript on the clipboard just long enough to paste it, marked so Windows keeps it out of
/// clipboard history (Win+V), cloud clipboard sync and clipboard monitors: no copy outlives Ivy's history.
/// On macOS it's marked concealed (nspasteboard.org), which clipboard managers such as Maccy and Raycast skip.
fn set_clipboard_private(text: &str) -> Result<(), arboard::Error> {
    let mut clipboard = arboard::Clipboard::new()?;
    #[cfg(windows)]
    {
        use arboard::SetExtWindows;
        clipboard.set().exclude_from_history().exclude_from_cloud().exclude_from_monitoring().text(text.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        use arboard::SetExtApple;
        clipboard.set().exclude_from_history().text(text.to_string())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    clipboard.set_text(text.to_string())
}

#[cfg(windows)]
fn send_ctrl_key(vk: u16) -> bool {
    #[derive(Clone, Copy)]
    #[allow(non_snake_case)]
    #[repr(C)]
    struct KEYBDINPUT {
        wVk: u16,
        wScan: u16,
        dwFlags: u32,
        time: u32,
        dwExtraInfo: usize,
    }
    #[derive(Clone, Copy)]
    #[repr(C)]
    union INPUT_UNION {
        ki: KEYBDINPUT,
        padding: [u8; 32],
    }
    #[derive(Clone, Copy)]
    #[repr(C)]
    struct INPUT {
        r#type: u32,
        u: INPUT_UNION,
    }

    #[link(name = "user32")]
    extern "system" {
        fn SendInput(cInputs: u32, pInputs: *const INPUT, cbSize: i32) -> u32;
    }

    const INPUT_KEYBOARD: u32 = 1;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const VK_CONTROL: u16 = 0x11;

    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            u: INPUT_UNION {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            u: INPUT_UNION {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            u: INPUT_UNION {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            u: INPUT_UNION {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    sent == inputs.len() as u32
}

/// macOS: Cmd, not Ctrl, is the paste and undo modifier.
#[cfg(target_os = "macos")]
fn send_ctrl_key(vk: u16) -> bool {
    macos::press_cmd(if vk == 0x5A { macos::KEY_Z } else { macos::KEY_V })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn send_ctrl_key(vk: u16) -> bool {
    let key = if vk == 0x5A { enigo::Key::Unicode('z') } else { enigo::Key::Unicode('v') };
    match enigo::Enigo::new(&enigo::Settings::default()) {
        Ok(mut enigo) => {
            use enigo::{Direction::Click, Direction::Press, Direction::Release, Key, Keyboard};
            let ok = enigo.key(Key::Control, Press).is_ok() && enigo.key(key, Click).is_ok();
            let _ = enigo.key(Key::Control, Release);
            ok
        }
        Err(_) => false,
    }
}

/// Waits for physical modifier keys to be released after a global shortcut press,
/// and synthesizes explicit key-ups for all modifiers so Windows' keyboard state
/// is clean before injecting synthetic Ctrl keystrokes.
#[cfg(windows)]
fn release_held_modifiers() {
    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(vKey: i32) -> i16;
        fn keybd_event(bVk: u8, bScan: u8, dwFlags: u32, dwExtraInfo: usize);
    }

    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const MODIFIERS: &[u8] = &[
        0x12, // VK_MENU (Alt)
        0xA4, // VK_LMENU
        0xA5, // VK_RMENU
        0x11, // VK_CONTROL
        0xA2, // VK_LCONTROL
        0xA3, // VK_RCONTROL
        0x10, // VK_SHIFT
        0xA0, // VK_LSHIFT
        0xA1, // VK_RSHIFT
        0x5B, // VK_LWIN
        0x5C, // VK_RWIN
        0x56, // VK_V
    ];

    let start = std::time::Instant::now();
    let max_wait = std::time::Duration::from_millis(350);
    while start.elapsed() < max_wait {
        let any_down = MODIFIERS.iter().any(|&k| unsafe { (GetAsyncKeyState(k as i32) as u16 & 0x8000) != 0 });
        if !any_down {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }

    for &k in MODIFIERS {
        unsafe {
            keybd_event(k, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

/// macOS: waits for the user to let go of Option and the other modifiers; the Cmd+V events themselves carry
/// only Command (`macos::press_cmd`), so nothing needs a synthetic key-up.
#[cfg(target_os = "macos")]
fn release_held_modifiers() {
    macos::wait_modifiers_released(std::time::Duration::from_millis(350));
}

#[cfg(not(any(windows, target_os = "macos")))]
fn release_held_modifiers() {}

// RegisterHotKey swallows our hotkey's Space/V/B, so the focused app sees Alt
// pressed and released alone — which opens its menu (Win11 Notepad/Office show
// KeyTips) and eats the Ctrl+V that follows. Tapping an unassigned VK while
// Alt/Win is held prevents that (same trick as AutoHotkey's #MenuMaskKey).
#[cfg(windows)]
fn suppress_alt_menu() {
    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(vKey: i32) -> i16;
        fn keybd_event(bVk: u8, bScan: u8, dwFlags: u32, dwExtraInfo: usize);
    }
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const VK_UNASSIGNED: u8 = 0xE8;
    let held = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
    // VK_MENU, VK_LWIN, VK_RWIN
    if held(0x12) || held(0x5B) || held(0x5C) {
        unsafe {
            keybd_event(VK_UNASSIGNED, 0, 0, 0);
            keybd_event(VK_UNASSIGNED, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

#[cfg(not(windows))]
fn suppress_alt_menu() {}

/// Returns whether a real Ctrl+V was actually sent — `false` means it only
/// landed on the clipboard (no matching foreground window to paste into),
/// which callers must not report as "Pasted" (Yash: claiming a paste that
/// didn't happen is exactly the kind of thing this app promised never to
/// fake — show "Copied" honestly instead).
// `target`: the specific foreground window to paste into, captured at this
// dictation's own hotkey-down. `None` (retry/repaste from History, which
// have no "hotkey-down moment" of their own) falls back to whatever the
// last real dictation captured.
fn paste_text(text: String, target: Option<isize>) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let target = target.unwrap_or_else(|| *LAST_EXTERNAL_HWND.lock().unwrap_or_else(|e| e.into_inner()));
    #[cfg(windows)]
    unsafe {
        // Only paste if the app that had focus when the hotkey went down
        // still has it — otherwise leave the text on the clipboard rather
        // than blind-firing Ctrl+V into whatever the user clicked into
        // meanwhile. A single instant check here used to fail on a brief
        // focus flicker (e.g. the capsule overlay itself briefly grabbing
        // foreground during show/hide, or a compositor hiccup) even though
        // the user never actually switched apps — Yash caught this live as
        // "it's not detecting the text box" while dictating into a window
        // that plainly still had focus. Poll for up to 300ms before giving
        // up; a real app-switch away is still caught well within that.
        let mut regained = target != 0 && GetForegroundWindow().0 as isize == target;
        if target != 0 && !regained {
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(30));
                if GetForegroundWindow().0 as isize == target {
                    regained = true;
                    break;
                }
            }
        }
        if !regained || !has_text_focus(target) {
            // Never touch the real clipboard here — Ivy has no idea what's
            // already on it (could be an API key, a password, anything the
            // user actually meant to keep), and overwriting it just because
            // no text field was found is exactly the real bug Yash caught
            // live. The transcript waits in `MANUAL_PASTE_TEXT` instead,
            // retrievable on demand via `MANUAL_PASTE_SHORTCUT` (Alt+V) —
            // see `paste_manual_clipboard`.
            *MANUAL_PASTE_TEXT.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
            return false;
        }
    }
    // macOS, same rules: the app from hotkey-down must still be in front (polled 300 ms, like above) and have a
    // text box. Capsule clicks bring it back first (`touch_up_transcript`, `retry_transcription`). Without
    // Accessibility permission macOS would silently drop the Cmd+V, so it isn't sent at all.
    #[cfg(target_os = "macos")]
    if !macos::is_trusted() || !macos::bring_to_front(target as i32, std::time::Duration::from_millis(300), false) || !has_text_focus(target) {
        *MANUAL_PASTE_TEXT.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
        return false;
    }
    // Read before overwriting — this is the one and only chance to know
    // what the clipboard held before Ivy clobbered it, needed to restore it
    // afterwards.
    let previous_clipboard = SavedClipboard::capture();
    let expected_text = text.clone();
    // A target app can still swallow a Ctrl+V that SendInput reports as sent,
    // so Alt+V must always be able to re-paste the latest transcript.
    *MANUAL_PASTE_TEXT.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
    let _ = set_clipboard_private(&text);
    std::thread::sleep(std::time::Duration::from_millis(60));
    #[cfg(windows)]
    unsafe {
        // Re-check right before sending keys — the sleep above is enough
        // time for focus to have moved again since the check above. Put the
        // user's clipboard back now: the transcript must not be left on it
        // (Alt+V still has it in MANUAL_PASTE_TEXT).
        if GetForegroundWindow().0 as isize != target {
            previous_clipboard.restore();
            return false;
        }
    }
    #[cfg(target_os = "macos")]
    {
        // A quick dictation can finish while Option (from Option + Space) is still held.
        release_held_modifiers();
        if macos::frontmost_pid() as isize != target {
            previous_clipboard.restore();
            return false;
        }
    }
    let ok = send_ctrl_key(0x56);
    if ok {
        // A real keystroke paste just happened into `target` — record it for
        // Touch Up. Never set for the clipboard-only fallback above (early
        // return): no real text-field edit happened there.
        *LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(LastPaste { target_hwnd: target });

        // Real bug caught live: this path (the normal automatic paste, run
        // on every single successful dictation) set the real clipboard to
        // the transcript but never restored it — unlike
        // `paste_manual_clipboard`'s Alt+V fallback, which already does
        // this correctly. Left unrestored, ANY later real Ctrl+V (to paste
        // something completely unrelated) would silently paste the last
        // dictation instead, until the user happened to copy something new
        // over it. Same restore pattern as the manual path: only restore if
        // the clipboard still holds what Ivy put there — if the user or
        // another app copied something new in the meantime, leave it alone.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                if let Ok(curr) = clipboard.get_text() {
                    if curr == expected_text {
                        drop(clipboard);
                        previous_clipboard.restore();
                        log::debug!("Ivy: restored the real clipboard after auto-paste");
                    }
                }
            }
        });
    }
    ok
}

#[tauri::command]
fn minimize_main(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
}

#[tauri::command]
fn toggle_maximize_main(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_maximized().unwrap_or(false) {
            let _ = w.unmaximize();
        } else {
            let _ = w.maximize();
        }
    }
}

#[tauri::command]
fn close_main(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

// Click-through from the capsule to the full app (Yash: Wispr Flow's own
// indicator does this too) — restores it if minimized/hidden and brings it
// to the front, rather than requiring a taskbar hunt.
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg(windows)]
fn make_window_non_activating(window: &tauri::WebviewWindow) {
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_NOACTIVATE: isize = 0x0800_0000;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;

    #[link(name = "user32")]
    extern "system" {
        fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: isize, index: i32, new_long: isize) -> isize;
    }

    match window.hwnd() {
        Ok(hwnd) => {
            let hwnd = hwnd.0 as isize;
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW);
            }
        }
        Err(e) => log::error!("Capsule: couldn't reach window handle ({e})"),
    }
}

#[cfg(target_os = "macos")]
fn make_window_non_activating(window: &tauri::WebviewWindow) {
    match window.ns_window() {
        // Called from `setup`, which runs on the main thread AppKit needs.
        Ok(ns_window) => unsafe { macos::capsule_setup(ns_window) },
        Err(e) => log::error!("Capsule: couldn't reach the window ({e})"),
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn make_window_non_activating(_window: &tauri::WebviewWindow) {}

#[cfg(windows)]
fn get_cursor_monitor(app: &tauri::AppHandle) -> Option<tauri::Monitor> {
    #[repr(C)]
    struct POINT {
        x: i32,
        y: i32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(lpPoint: *mut POINT) -> i32;
    }
    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut pt) != 0 {
            if let Ok(monitors) = app.available_monitors() {
                for m in monitors {
                    let pos = m.position();
                    let size = m.size();
                    if pt.x >= pos.x
                        && pt.x < pos.x + size.width as i32
                        && pt.y >= pos.y
                        && pt.y < pos.y + size.height as i32
                    {
                        return Some(m);
                    }
                }
            }
        }
    }
    app.primary_monitor().ok().flatten()
}

#[cfg(not(windows))]
fn get_cursor_monitor(app: &tauri::AppHandle) -> Option<tauri::Monitor> {
    app.cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
}

fn position_capsule_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("capsule") {
        let width = 440.0;
        let top_margin = 8.0;
        let mut x = 0.0;
        let mut y = top_margin;

        if let Some(monitor) = get_cursor_monitor(app) {
            let scale = monitor.scale_factor();
            #[cfg(not(target_os = "macos"))]
            let (mon_pos, mon_size) = (monitor.position(), monitor.size());
            // macOS: the work area starts below the menu bar (and a MacBook's camera notch), where the capsule
            // must sit to be seen.
            #[cfg(target_os = "macos")]
            let (mon_pos, mon_size) = (&monitor.work_area().position, &monitor.work_area().size);

            let mx = mon_pos.x as f64 / scale;
            let my = mon_pos.y as f64 / scale;
            let mw = mon_size.width as f64 / scale;

            x = mx + (mw - width) / 2.0;
            y = my + top_margin;
        }

        let _ = w.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(x, y)));
    }
}

fn spawn_capsule_window(app: &tauri::AppHandle) {
    let builder = WebviewWindowBuilder::new(
        app,
        "capsule",
        WebviewUrl::App("capsule.html".into()),
    )
    .title("Ivy Capsule")
    .inner_size(440.0, 100.0)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .resizable(false)
    .focused(false)
    .visible(false);
    // macOS: never the key window (typing stays in the app in front), and a click on a button reaches it even
    // while another app is active.
    #[cfg(target_os = "macos")]
    let builder = builder.focusable(false).accept_first_mouse(true);
    let built = builder.build();

    match built {
        Ok(window) => {
            make_window_non_activating(&window);
            position_capsule_window(app);
        }
        Err(e) => log::error!("Capsule: couldn't create the capsule window: {e}"),
    }
}

fn bridge_capsule_show(app: &tauri::AppHandle, visible: bool) {
    // Only while the wizard is actually in front: closing the window to the
    // tray keeps it mounted, and the capsule must work normally then.
    let wizard_in_front = WIZARD_ACTIVE.load(Ordering::SeqCst)
        && app
            .get_webview_window("main")
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
    let visible = visible && !wizard_in_front;
    if let Some(window) = app.get_webview_window("capsule") {
        #[cfg(windows)]
        {
            if let Ok(hwnd) = window.hwnd() {
                unsafe {
                    #[link(name = "user32")]
                    extern "system" {
                        fn ShowWindow(hwnd: isize, nCmdShow: i32) -> i32;
                    }
                    if visible {
                        ShowWindow(hwnd.0 as isize, 8); // SW_SHOWNA: Show without activating
                    } else {
                        ShowWindow(hwnd.0 as isize, 0); // SW_HIDE
                    }
                }
                return;
            }
        }
        // macOS: shown without becoming the key window or activating Ivy, on the main thread AppKit requires.
        #[cfg(target_os = "macos")]
        {
            let capsule = window.clone();
            let _ = window.run_on_main_thread(move || {
                if let Ok(ns_window) = capsule.ns_window() {
                    unsafe { macos::capsule_show(ns_window, visible) };
                }
            });
        }
        #[cfg(not(target_os = "macos"))]
        if visible {
            let _ = window.show();
        } else {
            let _ = window.hide();
        }
    }
}

#[tauri::command]
fn set_wizard_active(app: tauri::AppHandle, active: bool) {
    WIZARD_ACTIVE.store(active, Ordering::SeqCst);
    if active {
        bridge_capsule_show(&app, false);
    }
}

#[tauri::command]
fn model_status() -> model::Status {
    model::status()
}

#[tauri::command]
fn retry_model_download(app: tauri::AppHandle) {
    let dir = models_dir(&app).join("ivy-lite");
    model::ensure(&app, dir, model_ready);
}

/// The speech model is usable: pre-load it. Right after the first-start download (setup wizard not done yet),
/// also bring the window back so the wizard opens by itself, even if it was closed meanwhile. Once only:
/// a model that was already here, or any later download, never opens the window (Yash, 2026-10-08).
fn model_ready(app: &tauri::AppHandle, just_downloaded: bool) {
    prewarm(app);
    if just_downloaded && !load_settings(app).onboarding_completed {
        show_main_window(app.clone());
    }
}

#[tauri::command]
fn hide_capsule_window(app: tauri::AppHandle) {
    bridge_capsule_show(&app, false);
}

/// Loads the model in the background, with one silent practice pass, so the next dictation starts at once
/// (Yash: "the first reply takes a lot of time"). At startup and when a full-screen app goes away.
fn prewarm(app: &tauri::AppHandle) {
    if !model::is_ready() {
        return; // model::ensure pre-warms once the download is done
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let models = models_dir(&app);
        let prefer_gpu = should_use_gpu(&app);
        // macOS: the first GPU load of this run is the check (`metal_check`), marked on disk until it's done.
        #[cfg(target_os = "macos")]
        let checking = prefer_gpu && !METAL_CHECKED.swap(true, Ordering::SeqCst);
        #[cfg(target_os = "macos")]
        let marker = metal_marker(&app, "metal-check.running").filter(|_| checking);
        #[cfg(target_os = "macos")]
        if let Some(m) = &marker {
            let _ = fs::write(m, build_id());
        }
        let t_warm = std::time::Instant::now();
        match lite::engine(&models, !prefer_gpu) {
            Ok(eng) => {
                #[cfg(target_os = "macos")]
                if checking {
                    let result = metal_check(&eng);
                    if let Some(m) = &marker {
                        let _ = fs::remove_file(m);
                    }
                    match result {
                        Ok(()) => debug_log(&app, "Metal check passed: dictating on the Mac's GPU"),
                        Err(why) => {
                            METAL_BROKEN.store(true, Ordering::SeqCst);
                            debug_log(&app, &format!("Metal check failed ({why}): dictating on the CPU until Ivy restarts"));
                            drop(eng);
                            lite::unload_engine();
                            prewarm(&app);
                            return;
                        }
                    }
                }
                let backend = if eng.is_cpu_mode { "CPU" } else { GPU_BACKEND };
                let _ = eng.transcribe(&vec![0.0; 16_000], &[], std::time::Duration::from_secs(60));
                debug_log(&app, &format!("lite engine pre-warmed via {backend} in {}ms (incl. a silent warm-up pass)", t_warm.elapsed().as_millis()));
            }
            Err(e) => {
                #[cfg(target_os = "macos")]
                if let Some(m) = &marker {
                    let _ = fs::remove_file(m);
                }
                debug_log(&app, &format!("lite engine pre-warm failed: {e}"));
            }
        }
    });
}

fn handle_hotkey_down(app: &tauri::AppHandle) {
    // No model yet (first start, still downloading): say so instead of recording audio nobody can transcribe.
    if !model::is_ready() {
        let s = model::status();
        let notice = match s.state {
            "failed" => "Speech model didn't download · open Ivy to retry".to_string(),
            "downloading" | "retrying" if s.total > 0 => {
                format!("Speech model still downloading · {}%", s.done * 100 / s.total)
            }
            "missing" => "Speech model still downloading".to_string(),
            _ => "Speech model almost ready · try again in a moment".to_string(),
        };
        debug_log(app, &format!("hotkey-down ignored: speech model not ready ({})", s.state));
        if !WIZARD_ACTIVE.load(Ordering::SeqCst) {
            position_capsule_window(app);
            bridge_capsule_show(app, true);
            let _ = app.emit("ivy://notice", notice);
        }
        return;
    }
    // Real TOCTOU otherwise: this function is reachable concurrently from
    // three independent threads (the global-shortcut handler, the tray
    // "Dictate Now" menu item, and the `start_manual_dictation` command),
    // and `audio::Recorder::start` below is real blocking I/O (opens the
    // device, spawns a thread, waits for it to be ready) — two callers can
    // both see `PENDING` empty, both start a real mic stream, and the
    // second write to `PENDING` silently drops the first recording with no
    // error, no log line, and no `dictation-complete` event. This
    // compare-exchange collapses "check nothing's pending, then claim the
    // slot" into one atomic step; every exit path below must clear it.
    //
    // A separate 150ms time-based rate limit used to sit in front of this
    // (the same `check_rate_limit` used elsewhere for IPC endpoints) — real
    // bug, caught by Yash's own live testing: a deliberate fast double-tap
    // (the double-press-to-toggle gesture this app explicitly supports up
    // to `DOUBLE_PRESS_WINDOW_MS` = 600ms apart) can easily land its second
    // press within 150ms of the first, silently hitting this now-removed
    // gate with zero `debug_log` trace — "Alt+Space sometimes just does
    // nothing," completely undiagnosable from the log. The compare-exchange
    // below already fully and correctly solves the actual concurrency race
    // the time-based gate was guarding against — it doesn't need a time
    // window at all, so removing the redundant, weaker guard costs nothing.
    if STARTING_DICTATION
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        debug_log(app, "hotkey-down ignored — a dictation is already starting");
        return;
    }
    if PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        debug_log(app, "hotkey-down ignored — a dictation is already recording");
        STARTING_DICTATION.store(false, Ordering::SeqCst);
        return;
    }
    let id = next_dictation_id();
    ACTIVE_ID.store(id, Ordering::SeqCst);
    let fg = capture_foreground_hwnd();
    let (capsule_hwnd, main_hwnd) = own_window_ids(app);
    let target_hwnd = if fg != 0 && fg != capsule_hwnd && fg != main_hwnd && !is_explorer_shell(fg) { fg } else { 0 };
    let for_wizard = WIZARD_ACTIVE.load(Ordering::SeqCst) && fg != 0 && fg == main_hwnd;
    if target_hwnd != 0 {
        *LAST_EXTERNAL_HWND.lock().unwrap_or_else(|e| e.into_inner()) = target_hwnd;
    }
    let ctx = get_active_context(app.clone());
    let settings = load_settings(app);
    match audio::Recorder::start(&settings.selected_mic) {
        Ok(recorder) => {
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(PendingDictation {
                id,
                recorder,
                active_app: ctx.active_app.clone(),
                tone_preset: ctx.tone_preset.clone(),
                target_hwnd,
                for_wizard,
            });
            STARTING_DICTATION.store(false, Ordering::SeqCst);
        }
        Err(e) => {
            debug_log(app, &format!("recorder start failed (mic={:?}): {e}", settings.selected_mic));
            log::error!("Ivy: couldn't start recording: {e}");
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = None;
            STARTING_DICTATION.store(false, Ordering::SeqCst);
            // Tell the UI right away instead of leaving it to guess for 15s —
            // there is no recorder to stop and no hotkey-up handler will ever
            // fire this dictation's completion, so this is the only signal.
            let _ = app.emit(
                "ivy://dictation-complete",
                DictationComplete {
                    success: false,
                    reason: "mic_error".to_string(),
                    active_app: ctx.active_app.clone(),
                    session_id: String::new(),
                    pasted: false,
                    text: String::new(),
                },
            );
            // Nothing is actually recording — showing the capsule and
            // emitting hotkey-down here would leave the overlay stuck on
            // "recording"/"transcribing" forever, since hotkey-up finds
            // PENDING empty and has nothing to complete.
            return;
        }
    }
    position_capsule_window(app);
    bridge_capsule_show(app, true);
    let _ = app.emit("ivy://hotkey-down", ctx);

    // Real input level from the one stream Ivy already has open, so UI
    // meters never need to open the mic a second time (a concurrent WebView2
    // getUserMedia on the same device delayed and zeroed this capture).
    let app_level = app.clone();
    let started = Instant::now();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let level = match PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(p) if p.id == id => p.recorder.level(),
            _ => break,
        };
        if started.elapsed().as_secs_f64() >= MAX_DICTATION_SECS {
            // Time's up: finish it like a release (hands-free or held), so nothing said is lost.
            debug_log(&app_level, "reached the 5-minute limit — finishing the dictation");
            TOGGLE_RECORDING.store(false, Ordering::SeqCst);
            CURRENT_PRESS_TIME.lock().unwrap_or_else(|e| e.into_inner()).take();
            handle_hotkey_up(&app_level);
            break;
        }
        let _ = app_level.emit("ivy://mic-level", level);
    });
}

fn handle_hotkey_up(app: &tauri::AppHandle) {
    // Taken out of the mutex (and the guard dropped) before any real work —
    // `recorder.stop()` can resample up to 300s of audio and `debug_log`
    // does file I/O; holding the lock across either would block every other
    // `PENDING`/`LAST_EXTERNAL_HWND` access (including a panic anywhere in
    // that span poisoning the lock) for as long as it takes.
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(pending) = pending else {
        // Nothing was ever recording (e.g. the mic-error path above already
        // reported completion) — emitting hotkey-up here would still move
        // the capsule into "transcribing" with nothing to ever clear it.
        return;
    };
    let _ = app.emit("ivy://hotkey-up", ());
    if is_dictation_cancelled(pending.id) {
        let _ = pending.recorder.stop();
        return;
    }
    let recorder_info = pending.recorder.info();
    let samples = pending.recorder.stop();
    debug_log(app, &format!("recorder stopped: {recorder_info}"));
    let app_handle = app.clone();
    let dictation_id = pending.id;
    let target_hwnd = pending.target_hwnd;
    let for_wizard = pending.for_wizard;
    std::thread::spawn(move || {
        run_dictation_pipeline(
            app_handle,
            samples,
            pending.active_app,
            pending.tone_preset,
            dictation_id,
            target_hwnd,
            for_wizard,
        );
    });
}

/// One press or release of the dictation key, from the OS hotkey (Alt + Space) or the Ctrl + Shift watcher.
fn dictation_key(app: &tauri::AppHandle, pressed: bool) {
    // Raw event, logged before any routing: shows whether a Released ever arrived during a stuck recording.
    debug_log(app, &format!("raw hotkey event: {}", if pressed { "Pressed" } else { "Released" }));
    if pressed && is_paused() {
        return; // e.g. the shortcut was changed in Settings during a pause
    }
    if pressed {
        // A duplicate Pressed while already down (OS key-repeat) is swallowed here.
        if HOTKEY_PHYSICALLY_DOWN.swap(true, Ordering::SeqCst) {
            debug_log(app, "raw hotkey event: Pressed while already down — duplicate/auto-repeat, ignored");
            return;
        }
        route_dictation_press(app);
    } else {
        if !HOTKEY_PHYSICALLY_DOWN.swap(false, Ordering::SeqCst) {
            debug_log(app, "raw hotkey event: Released while already up — duplicate, ignored");
            return;
        }
        route_dictation_release(app);
    }
}

/// Turns the Ctrl + Shift dictation key on or off (modifier_hotkey.rs).
fn set_ctrl_shift(app: &tauri::AppHandle, on: bool) {
    let app = app.clone();
    modifier_hotkey::set_enabled(on, move |key| {
        debug_log(&app, &format!("ctrl+shift watcher: {key:?}"));
        match key {
        modifier_hotkey::Key::Press if !gpu_monitor::is_sleeping() => dictation_key(&app, true),
        modifier_hotkey::Key::Press => {}
        modifier_hotkey::Key::Release => dictation_key(&app, false),
        // Ctrl+Shift+T and friends: not a dictation. Drop the recording it started, if one is running.
        modifier_hotkey::Key::Cancel => {
            HOTKEY_PHYSICALLY_DOWN.store(false, Ordering::SeqCst);
            CURRENT_PRESS_TIME.lock().unwrap_or_else(|e| e.into_inner()).take();
            let recording = PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some();
            if recording && !TOGGLE_RECORDING.load(Ordering::SeqCst) {
                cancel_dictation_internal(&app, "Ctrl + Shift was part of another shortcut — recording dropped");
            }
        }
        }
    });
}

/// Turns the dictation key on or off (pause, full-screen sleep), whichever key the user picked.
fn set_dictation_key_active(app: &tauri::AppHandle, on: bool) {
    let spec = load_settings(app).hotkey;
    if spec == modifier_hotkey::SPEC {
        set_ctrl_shift(app, on);
    } else if let Some(h) = parse_hotkey(&spec) {
        let _ = if on { app.global_shortcut().register(h) } else { app.global_shortcut().unregister(h) };
    }
}

/// "Pause Ivy" (Yash, 2026-10-06): 1 hour by default, or any length from 1 minute up to 24 hours. While
/// paused, the dictation key goes back to other apps and the model is unloaded; Ivy resumes by itself when
/// the time is up. Kept in memory only, so restarting Ivy or the PC also resumes it.
const MAX_PAUSE_MINUTES: u32 = 24 * 60;
static PAUSED_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static PAUSE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn is_paused() -> bool {
    PAUSED_UNTIL_MS.load(Ordering::SeqCst) > now_ms()
}

fn pause_changed(app: &tauri::AppHandle) {
    let until = PAUSED_UNTIL_MS.load(Ordering::SeqCst);
    if let Some(item) = app.try_state::<PauseMenuItem>() {
        let _ = item.0.set_text(if until > 0 { "Resume Ivy" } else { "Pause Ivy for 1 hour" });
    }
    let _ = app.emit("ivy://pause-changed", until);
}

struct PauseMenuItem(tauri::menu::MenuItem<tauri::Wry>);

/// Returns when the pause ends (ms since 1970).
#[tauri::command]
fn pause_ivy(app: tauri::AppHandle, minutes: u32) -> Result<u64, String> {
    if minutes == 0 || minutes > MAX_PAUSE_MINUTES {
        return Err("Pause for 1 minute up to 24 hours.".into());
    }
    let until = now_ms() + minutes as u64 * 60_000;
    let was_paused = PAUSED_UNTIL_MS.swap(until, Ordering::SeqCst) > 0;
    if !was_paused {
        if PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            cancel_dictation_internal(&app, "Ivy paused — recording dropped");
        }
        set_dictation_key_active(&app, false);
        lite::unload_engine();
    }
    debug_log(&app, &format!("paused for {minutes} min"));
    let generation = PAUSE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let timer_app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if PAUSE_GENERATION.load(Ordering::SeqCst) != generation {
            break; // resumed, or the pause was changed
        }
        if now_ms() >= PAUSED_UNTIL_MS.load(Ordering::SeqCst) {
            resume_ivy(timer_app);
            break;
        }
    });
    pause_changed(&app);
    Ok(until)
}

#[tauri::command]
fn resume_ivy(app: tauri::AppHandle) {
    PAUSE_GENERATION.fetch_add(1, Ordering::SeqCst);
    if PAUSED_UNTIL_MS.swap(0, Ordering::SeqCst) == 0 {
        return;
    }
    // While a full-screen app is in front the key stays off; waking from that turns it back on.
    if !gpu_monitor::is_sleeping() {
        set_dictation_key_active(&app, true);
        prewarm(&app);
    }
    debug_log(&app, "resumed");
    pause_changed(&app);
}

/// When the current pause ends (ms since 1970), 0 when not paused.
#[tauri::command]
fn get_pause() -> u64 {
    if is_paused() { PAUSED_UNTIL_MS.load(Ordering::SeqCst) } else { 0 }
}

/// Routes every press of the dictation hotkey — hold and double-press live
/// on the same key at once, always, not a Settings choice. Recording starts
/// immediately on every ordinary press (zero added latency for the common
/// hold gesture); `route_dictation_release` decides afterward, from how
/// long it was actually held, whether that was a real hold (finish
/// normally) or a quick tap (provisionally discard it, arm for a possible
/// second tap). A press that arrives while already toggle-armed always
/// means "stop", regardless of how long that press itself is held.
fn route_dictation_press(app: &tauri::AppHandle) {
    if TOGGLE_RECORDING.load(Ordering::SeqCst) {
        debug_log(app, "hotkey press while toggle-armed -> stopping hands-free recording");
        handle_hotkey_up(app);
        TOGGLE_RECORDING.store(false, Ordering::SeqCst);
        *LAST_TAP.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return;
    }

    let now = Instant::now();
    let mut last_tap = LAST_TAP.lock().unwrap_or_else(|e| e.into_inner());
    let gap_ms = last_tap.map(|t| now.duration_since(t).as_millis());
    let is_second_tap = last_tap
        .map(|t| now.duration_since(t) < std::time::Duration::from_millis(DOUBLE_PRESS_WINDOW_MS))
        .unwrap_or(false);
    *last_tap = None;
    drop(last_tap);
    debug_log(
        app,
        &format!(
            "hotkey press (gap since last tap release: {}, is_second_tap={is_second_tap})",
            gap_ms.map_or("none".to_string(), |g| format!("{g}ms"))
        ),
    );

    if is_second_tap {
        // Confirmed double-press: this press's own recording (started
        // below like any other) stays live hands-free until the next press,
        // not just for as long as this key happens to be held.
        handle_hotkey_down(app);
        // Only arm toggle mode if a dictation actually started — mic-open
        // failure (device busy/unplugged) makes `handle_hotkey_down` bail
        // out with nothing in `PENDING`. Arming unconditionally would leave
        // TOGGLE_RECORDING stuck true with nothing recording, so the next
        // real press gets swallowed as a no-op "stop" instead of starting.
        let started = PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        TOGGLE_RECORDING.store(started, Ordering::SeqCst);
    } else {
        handle_hotkey_down(app);
        *CURRENT_PRESS_TIME.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
    }
}

/// The release half of `route_dictation_press`. Ignored entirely while
/// toggle-armed (that state only ever ends on a press). Otherwise: held
/// past `TAP_THRESHOLD_MS` finishes the dictation exactly as a plain hold
/// always has; held less than that is a tap, not a deliberate recording —
/// its audio is discarded (the same real cancel path the capsule's own X
/// button uses) and a timestamp is armed so the very next press, if it
/// comes quickly, is recognized as the second half of a double-press.
fn route_dictation_release(app: &tauri::AppHandle) {
    if TOGGLE_RECORDING.load(Ordering::SeqCst) {
        debug_log(app, "hotkey release ignored — toggle-armed, only a press ends this");
        return;
    }
    let Some(press_time) = CURRENT_PRESS_TIME.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        debug_log(app, "hotkey release with no matching press-time — ignored");
        return;
    };
    let held = Instant::now().duration_since(press_time);
    debug_log(app, &format!("hotkey release after {}ms held (tap threshold {TAP_THRESHOLD_MS}ms)", held.as_millis()));
    if held >= std::time::Duration::from_millis(TAP_THRESHOLD_MS) {
        let t = Instant::now();
        handle_hotkey_up(app);
        debug_log(app, &format!("handle_hotkey_up returned after {:?}", t.elapsed()));
    } else {
        cancel_dictation_internal(app, "quick tap released — held for a possible double-press");
        *LAST_TAP.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
}


#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManualPastePayload {
    pasted: bool,
    active_app: String,
}

/// Pastes the latest transcript (`MANUAL_PASTE_TEXT`) into whatever has
/// focus right now — for a dictation that had no text field, or whose
/// auto-paste the target app swallowed. Never leaves the user's own clipboard permanently
/// overwritten: swaps it out only for the instant the keystroke needs, then
/// restores it safely once the paste is consumed. Not single-use: a failed attempt
/// doesn't burn the only copy, since nothing here clears `MANUAL_PASTE_TEXT`.
fn paste_manual_clipboard(app: &tauri::AppHandle) {
    let Some(text) = MANUAL_PASTE_TEXT.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        debug_log(app, "manual-paste pressed but there's no transcript yet this session");
        let _ = app.emit("ivy://manual-paste", false);
        return;
    };

    #[cfg(windows)]
    let target = unsafe { GetForegroundWindow().0 as isize };
    #[cfg(not(windows))]
    let target = capture_foreground_hwnd();

    let (capsule_hwnd, main_hwnd) = own_window_ids(app);

    // macOS: also refused without Accessibility permission (macOS would drop the Cmd+V) and in Finder outside a
    // text box, the same no-text-box case as Windows' Explorer below.
    #[cfg(target_os = "macos")]
    if target == 0 || target == capsule_hwnd || target == main_hwnd || !macos::is_trusted() || !has_text_focus(target) {
        debug_log(app, &format!("manual-paste: no text box or no Accessibility permission (app {target})"));
        position_capsule_window(app);
        bridge_capsule_show(app, true);
        let _ = app.emit(
            "ivy://manual-paste",
            ManualPastePayload {
                pasted: false,
                active_app: foreground_app_label(),
            },
        );
        if !macos::is_trusted() {
            let _ = app.emit("ivy://notice", ACCESSIBILITY_NOTICE);
        }
        return;
    }

    #[cfg(windows)]
    if target == 0 || target == capsule_hwnd || target == main_hwnd || is_explorer_shell(target) {
        debug_log(app, &format!("manual-paste: invalid or shell target window ({target})"));
        position_capsule_window(app);
        bridge_capsule_show(app, true);
        let _ = app.emit(
            "ivy://manual-paste",
            ManualPastePayload {
                pasted: false,
                active_app: foreground_app_label(),
            },
        );
        return;
    }

    let active_app = foreground_app_label();

    // Read before overwriting — this is the one and only chance to know
    // what the clipboard held before Ivy temporarily swaps it out.
    let previous_clipboard = SavedClipboard::capture();
    if arboard::Clipboard::new().is_ok() {
        if set_clipboard_private(&text).is_err() {
            debug_log(app, "manual-paste: couldn't access the clipboard");
            position_capsule_window(app);
            bridge_capsule_show(app, true);
            let _ = app.emit(
                "ivy://manual-paste",
                ManualPastePayload {
                    pasted: false,
                    active_app,
                },
            );
            return;
        }
    } else {
        debug_log(app, "manual-paste: couldn't open clipboard");
        position_capsule_window(app);
        bridge_capsule_show(app, true);
        let _ = app.emit(
            "ivy://manual-paste",
            ManualPastePayload {
                pasted: false,
                active_app,
            },
        );
        return;
    }

    // Crucial: Wait for the user to physically release the hotkey modifier keys
    // (e.g. Alt in Alt+V) and ensure clean keyboard state before sending Ctrl+V.
    // Injecting Ctrl+V while physical Alt is still held causes Windows to interpret
    // the keystrokes as Ctrl+Alt+V (WM_SYSKEYDOWN), which standard text fields ignore.
    release_held_modifiers();

    std::thread::sleep(std::time::Duration::from_millis(60));

    #[cfg(windows)]
    unsafe {
        // Re-check right before sending keys that focus didn't move away
        if GetForegroundWindow().0 as isize != target {
            debug_log(app, "manual-paste: foreground window changed before keystroke");
            position_capsule_window(app);
            bridge_capsule_show(app, true);
            let _ = app.emit(
                "ivy://manual-paste",
                ManualPastePayload {
                    pasted: false,
                    active_app,
                },
            );
            return;
        }
    }
    #[cfg(target_os = "macos")]
    if macos::frontmost_pid() as isize != target {
        debug_log(app, "manual-paste: the app in front changed before the keystroke");
        position_capsule_window(app);
        bridge_capsule_show(app, true);
        let _ = app.emit(
            "ivy://manual-paste",
            ManualPastePayload {
                pasted: false,
                active_app,
            },
        );
        return;
    }

    let sent = send_ctrl_key(0x56); // VK_V

    if sent {
        *LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(LastPaste { target_hwnd: target });

        // Restore the user's real clipboard on a background thread after a generous margin,
        // but ONLY if the clipboard still contains Ivy's transcript. If the user or another
        // app copied something new in the meantime, do not overwrite it.
        let app_handle = app.clone();
        let expected_text = text;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                if let Ok(curr) = clipboard.get_text() {
                    if curr == expected_text {
                        drop(clipboard);
                        previous_clipboard.restore();
                        debug_log(&app_handle, "manual-paste: restored the real clipboard");
                        return;
                    }
                }
            }
            debug_log(&app_handle, "manual-paste: clipboard was modified or unavailable, skipped restore");
        });
    }

    debug_log(app, &format!("manual-paste: sent Ctrl+V to target {target} ({sent})"));

    // Show capsule confirmation AFTER sending keystrokes into target
    position_capsule_window(app);
    bridge_capsule_show(app, true);
    let _ = app.emit(
        "ivy://manual-paste",
        ManualPastePayload {
            pasted: sent,
            active_app,
        },
    );
}

// ACTIVE_ID, not "whatever's in PENDING" — the X button is shown (and
// wired to this command) during both the recording AND transcribing
// capsule states, and PENDING is already empty again by the time a
// dictation is transcribing. Exact-match cancellation (see
// `mark_cancelled`) means this can never reach back and cancel an
// unrelated earlier dictation still finishing up in the background.
fn cancel_dictation_internal(app: &tauri::AppHandle, reason: &str) {
    let id = ACTIVE_ID.load(Ordering::SeqCst);
    if id != 0 {
        mark_cancelled(id);
    }
    // A plain `let` statement, not `if let` — Rust 2021 extends a
    // temporary created in an `if let` scrutinee (the `MutexGuard` from
    // `.lock()`) to the end of the whole block, so `if let Some(pending) =
    // PENDING.lock()...take() { pending.recorder.stop(); }` would hold
    // `PENDING` locked for the entire `stop()` call — exactly the hazard
    // `handle_hotkey_up` already documents and avoids the same way.
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(pending) = pending {
        let _ = pending.recorder.stop();
    }
    bridge_capsule_show(app, false);
    let _ = app.emit("ivy://dictation-cancelled", ());
    debug_log(app, reason);
}

#[tauri::command]
fn cancel_dictation(app: tauri::AppHandle) {
    cancel_dictation_internal(&app, "dictation cancelled by user — discarded audio and state");
    // macOS: the click on the capsule's X made Ivy the active app; give the front back to the app the user was
    // typing in, unless Ivy's own window is open.
    #[cfg(target_os = "macos")]
    {
        let main_visible = app.get_webview_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
        let target = *LAST_EXTERNAL_HWND.lock().unwrap_or_else(|e| e.into_inner()) as i32;
        if !main_visible && target > 0 && macos::frontmost_pid() == macos::own_pid() {
            macos::activate(target);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Permissions {
    /// macOS Accessibility, which pasting into other apps needs.
    accessibility: bool,
    /// "granted", "denied", or "ask" (macOS hasn't asked yet).
    microphone: &'static str,
}

/// The permissions macOS asks the user for, for Settings and the setup wizard. Windows has none: always granted.
#[tauri::command]
fn get_permissions() -> Permissions {
    #[cfg(target_os = "macos")]
    let permissions = Permissions { accessibility: macos::is_trusted(), microphone: macos::mic_permission() };
    #[cfg(not(target_os = "macos"))]
    let permissions = Permissions { accessibility: true, microphone: "granted" };
    permissions
}

/// macOS: asks for a permission. "accessibility": macOS's own prompt (which adds Ivy to the list) and the list in
/// System Settings, where the user switches Ivy on. "microphone": a moment of recording brings up macOS's prompt
/// the first time; after "Don't Allow" only System Settings can change it, so that opens instead.
#[tauri::command]
async fn request_permission(kind: String) {
    #[cfg(target_os = "macos")]
    match kind.as_str() {
        "accessibility" => {
            macos::prompt_trust();
            macos::open_privacy_settings("Privacy_Accessibility");
        }
        "microphone" if macos::mic_permission() == "ask" => {
            if let Ok(recorder) = audio::Recorder::start("") {
                std::thread::sleep(std::time::Duration::from_millis(300));
                audio::zeroize_samples(&mut recorder.stop());
            }
        }
        "microphone" => macos::open_privacy_settings("Privacy_Microphone"),
        _ => {}
    }
    #[cfg(not(target_os = "macos"))]
    let _ = kind;
}

/// Why the dictation shortcut isn't working (see `HOTKEY_PROBLEM`), or nothing.
#[tauri::command]
fn get_hotkey_problem() -> Option<String> {
    HOTKEY_PROBLEM.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// macOS: the apps running now ("Notes.app"), for the Tone screen's "Add app". Windows picks an .exe instead.
#[tauri::command]
fn list_running_apps() -> Vec<String> {
    #[cfg(target_os = "macos")]
    let apps = macos::running_apps();
    #[cfg(not(target_os = "macos"))]
    let apps = Vec::new();
    apps
}

#[tauri::command]
fn start_manual_dictation(app: tauri::AppHandle) {
    handle_hotkey_down(&app);
}

#[tauri::command]
fn stop_manual_dictation(app: tauri::AppHandle) {
    handle_hotkey_up(&app);
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HardwareStatusDto {
    active_engine: String,
    configured_mode: String,
    gpu_name: String,
    gpu_usage_percent: u32,
    vram_used_mb: u64,
    vram_total_mb: u64,
    is_evicted: bool,
    on_battery: bool,
}

#[tauri::command]
fn get_hardware_status(app: tauri::AppHandle) -> HardwareStatusDto {
    let settings = load_settings(&app);
    let telemetry = gpu_monitor::query_gpu_telemetry();
    let is_evicted = gpu_monitor::is_vram_evicted();
    let on_battery = gpu_monitor::is_on_battery();

    #[cfg(target_os = "macos")]
    let mac_engine = Some(if METAL_BROKEN.load(Ordering::SeqCst) { "cpu" } else { "gpu" });
    #[cfg(not(target_os = "macos"))]
    let mac_engine: Option<&str> = None;
    let active_engine = if let Some(engine) = mac_engine {
        engine.to_string()
    } else if on_battery {
        "cpu".to_string()
    } else if is_evicted {
        "evicted".to_string()
    } else if settings.hardware_mode == "cpu" {
        "cpu".to_string()
    } else {
        "gpu".to_string()
    };

    HardwareStatusDto {
        active_engine,
        configured_mode: settings.hardware_mode,
        gpu_name: telemetry.adapter_name,
        gpu_usage_percent: telemetry.usage_percent,
        vram_used_mb: telemetry.used_vram_mb,
        vram_total_mb: telemetry.total_vram_mb,
        is_evicted,
        on_battery,
    }
}

/// Frees the loaded model so a hardware-mode switch (GPU/CPU)
/// takes effect on the next dictation instead of persisting the old engine
/// for the rest of the session. No process restart: `lite::engine()` already
/// checks the requested mode against what's loaded and reloads itself when
/// they differ, so a full `app.restart()` was never doing anything a plain
/// unload didn't already accomplish — and it raced the single-instance
/// plugin's OS mutex on Windows badly enough that the "restart" sometimes
/// silently kept the old (stale) process alive instead.
#[tauri::command]
fn apply_hardware_mode() {
    lite::unload_engine();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin registered. A second launch (double-
        // clicking the exe/shortcut again, or a stray installer relaunch)
        // is handed here instead of starting a whole second process — this
        // is the exact class of bug ("multiple instances fighting over the
        // same hotkey/tray icon") the project has hit before.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // Real Windows Run-key registration so Ivy is actually running (tray
        // + hotkey live) the moment the machine boots, with no window ever
        // shown — `--flag` hidden launch args aren't needed since the main
        // window already starts with `visible: false` in tauri.conf.json.
        // `--autostart` is only ever present when Windows itself launched
        // this process from the real Run-key entry — never on a manual
        // double-click/Start-Menu launch — so it's the real signal for
        // "should I show the delayed background-launch confirmation".
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                // Two real bindings can fire this one handler: the dictation
                // hotkey (`apply_hotkey` always keeps exactly one registered,
                // so any firing that isn't the manual-paste shortcut is the
                // user's current dictation hotkey by construction) and the
                // manual-paste shortcut (`MANUAL_PASTE_SHORTCUT`).
                .with_handler(move |app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        suppress_alt_menu();
                    }
                    let is_manual_paste = MANUAL_PASTE_SHORTCUT
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .map_or(false, |u| u == *shortcut);
                    if is_manual_paste {
                        if event.state() == ShortcutState::Pressed {
                            paste_manual_clipboard(app);
                        }
                        return;
                    }
                    dictation_key(app, event.state() == ShortcutState::Pressed);
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_history,
            delete_history_entry,
            repaste_transcript,
            get_active_context,
            list_audio_input_devices,
            minimize_main,
            toggle_maximize_main,
            close_main,
            show_main_window,
            hide_capsule_window,
            set_wizard_active,
            retry_transcription,
                touch_up_transcript,
            clear_all_history,
            get_user_stats,
            extract_audio,
            start_manual_dictation,
            stop_manual_dictation,
            pause_ivy,
            resume_ivy,
            get_pause,
            get_hardware_status,
            apply_hardware_mode,
            cancel_dictation,
            model_status,
            retry_model_download,
            update::check_for_update,
            update::install_update,
            get_permissions,
            request_permission,
            list_running_apps,
            get_hotkey_problem,
        ])
        .setup(move |app| {
            // Was debug-only — meant every `log::info!`/`log::warn!` call
            // (including the LLM cleanup timing/guard-rejection diagnostics
            // in cleanup.rs) silently went nowhere in a release build, the
            // only kind Yash ever actually runs. Always on now; default
            // targets are stdout + a file in the app's log dir (see
            // `tauri_plugin_log`'s own default()), so a release exe still
            // leaves a real trail to check after the fact.
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;
            // Checked BEFORE load_settings/save_settings ever touch the
            // filesystem — settings.json only exists once onboarding (or
            // any prior settings save) has actually happened, so this is a
            // real "has Ivy ever run on this machine before" signal, not a
            // guess. A first-ever launch shows the window (onboarding lives
            // in the frontend and needs to be seen); every later boot stays
            // hidden per tauri.conf.json's `visible: false`, reachable only
            // via the tray icon or the hotkey.
            migrate_roaming_data(&app.handle());
            #[cfg(target_os = "macos")]
            {
                macos::disable_app_nap();
                remember_metal_crash(&app.handle());
            }
            let first_launch = !settings_path(&app.handle()).exists();
            // Real Windows Run-key launch, not a manual one — see the
            // `--autostart` arg registered with the plugin above.
            let launched_via_autostart = std::env::args().any(|a| a == "--autostart");
            let mut startup_settings = load_settings(&app.handle());
            // Anything else saved by an older version (e.g. a bare "Space" that would eat the spacebar) goes back
            // to the default, and the fixed held-back paste key is restored.
            if !DICTATION_KEYS.contains(&startup_settings.hotkey.as_str())
                || startup_settings.manual_paste_hotkey != "Alt + V"
            {
                if !DICTATION_KEYS.contains(&startup_settings.hotkey.as_str()) {
                    startup_settings.hotkey = "Alt + Space".to_string();
                }
                startup_settings.manual_paste_hotkey = "Alt + V".to_string();
                let _ = write_settings_atomic(&app.handle(), &startup_settings);
            }
            sync_autostart(&app.handle(), startup_settings.launch_at_startup);
            // Registers whatever hotkey the user last saved, not a hardcoded
            // Alt+Space — `parse_hotkey` returning `None` for a corrupt
            // settings value falls back to the real default rather than
            // registering nothing.
            if startup_settings.hotkey == modifier_hotkey::SPEC {
                set_ctrl_shift(&app.handle().clone(), true);
            } else {
                let startup_hotkey = parse_hotkey(&startup_settings.hotkey)
                    .unwrap_or_else(|| Shortcut::new(Some(Modifiers::ALT), Code::Space));
                if let Err(e) = app.global_shortcut().register(startup_hotkey) {
                    log::error!("Ivy: {} didn't register ({e}) — something else on this PC already has it.", startup_settings.hotkey);
                    *HOTKEY_PROBLEM.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(describe_register_failure(&startup_settings.hotkey, e));
                }
            }
            // Same real registration as the dictation hotkey, for the manual-paste binding —
            // `MANUAL_PASTE_SHORTCUT` is what the shared handler compares an
            // incoming press against.
            let startup_manual_paste_hotkey = parse_hotkey(&startup_settings.manual_paste_hotkey)
                .unwrap_or_else(|| Shortcut::new(Some(Modifiers::ALT), Code::KeyV));
            if let Err(e) = app.global_shortcut().register(startup_manual_paste_hotkey) {
                log::error!("Ivy: {} (manual paste) didn't register ({e}) — something else on this PC already has it.", startup_settings.manual_paste_hotkey);
            } else {
                *MANUAL_PASTE_SHORTCUT.lock().unwrap_or_else(|e| e.into_inner()) = Some(startup_manual_paste_hotkey);
            }
            spawn_capsule_window(&app.handle());

            // Real, once-per-boot confirmation that Ivy is actually alive
            // in the background — deliberately delayed 90s past launch
            // (Yash: many other startup apps are also fighting for
            // attention right at boot; showing up after the herd has
            // settled reads as considered, not another thing piling on).
            // Never fires on a manual launch or the first-ever run (which
            // already shows the real window for onboarding).
            if launched_via_autostart && !first_launch {
                let app_handle_bg = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(90));
                    position_capsule_window(&app_handle_bg);
                    bridge_capsule_show(&app_handle_bg, true);
                    let _ = app_handle_bg.emit("ivy://background-ready", ());
                });
            }
            purge_old_history(&app.handle());

            // Ivy is designed to keep running for weeks (the main window
            // only ever hides on close, never quits) — a launch-only purge
            // would leave the advertised 2-day retention silently not
            // holding for a machine that sleeps rather than restarts.
            let app_handle_purge = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60 * 60));
                purge_old_history(&app_handle_purge);
            });

            // Real system tray: the only way to know Ivy is running while
            // the main window is hidden, plus real actions — not a
            // decorative icon.
            {
                use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
                use tauri::tray::TrayIconBuilder;

                let dictate_item = MenuItem::with_id(app, "dictate_now", "Dictate Now", true, None::<&str>)?;
                let pause_item = MenuItem::with_id(app, "pause", "Pause Ivy for 1 hour", true, None::<&str>)?;
                let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
                let separator = PredefinedMenuItem::separator(app)?;
                let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let tray_menu = Menu::with_items(app, &[&dictate_item, &pause_item, &settings_item, &separator, &quit_item])?;
                app.manage(PauseMenuItem(pause_item.clone()));

                // `TrayIconBuilder::build` returns a real `TrayIcon` handle
                // whose `Drop` impl removes the icon from the tray — ending
                // this in a bare `;` statement (the original bug: it built,
                // then was destroyed at the end of that same statement, so
                // nothing was ever actually visible) instead of keeping it
                // alive for the app's lifetime via managed state.
                // macOS: the logo as a black silhouette that the menu bar tints for light and dark mode, like every
                // menu bar icon there.
                #[cfg(target_os = "macos")]
                let tray_image = tauri::include_image!("icons/tray-mac.png");
                #[cfg(not(target_os = "macos"))]
                let tray_image = app.default_window_icon().cloned().ok_or("no default window icon")?;
                let tray_icon = TrayIconBuilder::new()
                    .icon(tray_image)
                    .icon_as_template(cfg!(target_os = "macos"))
                    .menu(&tray_menu)
                    .show_menu_on_left_click(true)
                    .tooltip("Ivy — offline dictation")
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "dictate_now" => {
                            // Real toggle through the exact same functions the
                            // hotkey itself uses — start if idle, stop (and run
                            // the real pipeline) if a dictation is already in
                            // flight. No separate simulated "tray dictation".
                            if PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                                handle_hotkey_up(app);
                            } else {
                                // Asking to dictate ends a pause: it recorded while the title bar still said Paused.
                                if is_paused() {
                                    resume_ivy(app.clone());
                                }
                                handle_hotkey_down(app);
                            }
                        }
                        "pause" => {
                            if is_paused() {
                                resume_ivy(app.clone());
                            } else {
                                let _ = pause_ivy(app.clone(), 60);
                            }
                        }
                        "settings" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                            let _ = app.emit("ivy://open-settings", ());
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .build(app)?;
                app.manage(tray_icon);
            }

            let app_handle = app.handle().clone();
            let monitor_app = app_handle.clone();
            let notice_app = app_handle.clone();
            gpu_monitor::start_gpu_monitor(
                move || {
                    let s = load_settings(&monitor_app);
                    (s.smart_vram_eviction, s.vram_eviction_threshold, s.hardware_mode == "gpu")
                },
                move |event| {
                    // never pull the model out from under a dictation that's recording or starting
                    let busy = PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() || STARTING_DICTATION.load(Ordering::SeqCst);
                    match event {
                        gpu_monitor::Event::Sleep if !busy => {
                            // Full screen (Yash, 2026-10-06): fully unloaded, Alt+Space goes to the game, no overlay.
                            lite::unload_engine();
                            set_dictation_key_active(&notice_app, false);
                            debug_log(&notice_app, "full-screen app in front: asleep (model unloaded, hotkey off)");
                        }
                        gpu_monitor::Event::Wake => {
                            if !is_paused() {
                                set_dictation_key_active(&notice_app, true);
                                debug_log(&notice_app, "full screen ended: awake, loading the model again");
                                prewarm(&notice_app);
                            }
                        }
                        gpu_monitor::Event::GpuBusy(load, threshold) if !busy => {
                            lite::unload_engine();
                            debug_log(&notice_app, &format!("GPU busy ({load}% >= {threshold}%): model unloaded, dictating on CPU"));
                            position_capsule_window(&notice_app);
                            bridge_capsule_show(&notice_app, true);
                            let _ = notice_app.emit("ivy://gpu-evicted", serde_json::json!({ "load": load, "threshold": threshold }));
                        }
                        _ => return false,
                    }
                    true
                },
            );

            // Downloads the speech model if it isn't here yet (first start after a normal install), then the
            // background pre-warm so the very first dictation starts instantly. The pre-warm waits out the launch
            // intro (about 3.5 s): loading the model onto the GPU during it froze the animation for up to 440 ms.
            // The file check runs here, before the window loads, so the window never shows a download by guess.
            let model_dir = models_dir(&app.handle()).join("ivy-lite");
            model::check(&model_dir);
            let warm_app = app.handle().clone();
            std::thread::spawn(move || {
                if model::is_ready() {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                }
                model::ensure(&warm_app, model_dir, model_ready);
            });

            if let Some(window) = app.get_webview_window("main") {
                // Windows rounds the window itself, so the app's box fills it edge to edge: the page's own
                // rounded corners left black triangles at the corners (Yash, 2026-10-06).
                #[cfg(windows)]
                if let Ok(hwnd) = window.hwnd() {
                    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
                    unsafe {
                        let _ = DwmSetWindowAttribute(
                            windows::Win32::Foundation::HWND(hwnd.0),
                            DWMWA_WINDOW_CORNER_PREFERENCE,
                            &DWMWCP_ROUND as *const _ as *const core::ffi::c_void,
                            std::mem::size_of_val(&DWMWCP_ROUND) as u32,
                        );
                    }
                }
                let close_window = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = close_window.hide();
                    }
                });
                // Only a real Windows Run-key launch stays hidden — any
                // manual double-click/Start-Menu launch (first-ever or not)
                // shows the window, matching Yash's actual ask ("background
                // on startup", not "background whenever I open the exe").
                if !launched_via_autostart {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // macOS: clicking Ivy in the Dock (or opening it again from Finder) while its window is closed brings
            // the window back, as every Mac app does.
            #[cfg(target_os = "macos")]
            match event {
                tauri::RunEvent::Reopen { .. } => show_main_window(app.clone()),
                // Quitting while the GPU check runs isn't a crash inside it (`remember_metal_crash`).
                tauri::RunEvent::Exit => {
                    if let Some(m) = metal_marker(app, "metal-check.running") {
                        let _ = fs::remove_file(m);
                    }
                }
                _ => {}
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_session_id() {
        assert!(is_valid_session_id("session-123456789"));
        assert!(is_valid_session_id("session_abc-DEF_01"));
        assert!(!is_valid_session_id("../etc/passwd"));
        assert!(!is_valid_session_id("session/123"));
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("session;rm -rf /"));
    }

    #[test]
    fn snippets_replace_the_trigger_anywhere_as_whole_words() {
        let snips = vec![
            Snippet { trigger: "my email".into(), text: "yash@example.com".into() },
            Snippet { trigger: "My email address".into(), text: "work@example.com".into() },
        ];
        // The whole dictation is the trigger: exactly the saved text, no full stop.
        assert_eq!(apply_snippets("My email.", &snips), "yash@example.com");
        assert_eq!(apply_snippets("  my EMAIL ", &snips), "yash@example.com");
        // Inside a sentence, the punctuation around it is kept.
        assert_eq!(apply_snippets("Can you send that to my email?", &snips), "Can you send that to yash@example.com?");
        assert_eq!(apply_snippets("My email, please.", &snips), "yash@example.com, please.");
        // Longest trigger wins.
        assert_eq!(apply_snippets("Use my email address here.", &snips), "Use work@example.com here.");
        // Whole words only; everything else is untouched.
        assert_eq!(apply_snippets("Check my emails.", &snips), "Check my emails.");
        assert_eq!(apply_snippets("", &snips), "");
        assert_eq!(apply_snippets("Héllo my email, ok", &snips), "Héllo yash@example.com, ok");
    }

    #[test]
    fn test_validate_settings_valid() {
        let mut settings = SettingsConfig::default();
        settings.hotkey = "Alt + Space".to_string();
        settings.active_tone_preset = "Standard".to_string();
        settings.hardware_mode = "gpu".to_string();
        assert!(validate_settings(&settings).is_ok());
        settings.hotkey = "Ctrl + Shift".to_string();
        assert!(validate_settings(&settings).is_ok());
        // only the two offered keys (a bare "Space" saved by an old picker would eat the spacebar)
        settings.hotkey = "Space".to_string();
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_settings_invalid_tone() {
        let mut settings = SettingsConfig::default();
        settings.active_tone_preset = "SuperAggressive".to_string();
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_settings_oversized_hotkey() {
        let mut settings = SettingsConfig::default();
        settings.hotkey = "A".repeat(65);
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_check_rate_limit() {
        let ts = AtomicU64::new(0);
        // First check succeeds
        assert!(check_rate_limit(&ts, 50).is_ok());
        // Immediate second check should fail because interval is 50ms
        assert!(check_rate_limit(&ts, 50).is_err());
    }

    #[test]
    fn added_apps_keep_their_tone_and_every_other_app_gets_the_clicked_one() {
        let mut settings = SettingsConfig::default();
        settings.preset_apps.clear();
        settings.preset_apps.insert("Professional".to_string(), vec!["brave.exe".to_string()]);
        settings.preset_apps.insert("Casual".to_string(), vec!["WhatsApp.exe".to_string(), "Gmail".to_string()]);
        settings.active_tone_preset = "Standard".to_string();
        // .exe entries match the program, whatever the window title says
        assert_eq!(tone_for_label(&settings, "New Tab - Brave", "brave.exe"), "Professional");
        assert_eq!(tone_for_label(&settings, "Anything at all", "Brave.EXE"), "Professional");
        assert_eq!(tone_for_label(&settings, "WhatsApp", "WhatsApp.exe"), "Casual");
        // a title that merely mentions "brave" isn't brave.exe
        assert_eq!(tone_for_label(&settings, "brave new world.txt - Notepad", "notepad.exe"), "Standard");
        // older name entries still match the title
        assert_eq!(tone_for_label(&settings, "Inbox - Gmail - Google Chrome", "chrome.exe"), "Casual");
        // clicking a mode changes every app that isn't on a list, at once
        settings.active_tone_preset = "Professional".to_string();
        assert_eq!(tone_for_label(&settings, "Untitled - Notepad", "notepad.exe"), "Professional");
        assert_eq!(tone_for_label(&settings, "WhatsApp", "WhatsApp.exe"), "Casual");
        // macOS app bundles match the program the same way
        settings.preset_apps.insert("Casual".to_string(), vec!["Messages.app".to_string()]);
        assert_eq!(tone_for_label(&settings, "Messages", "Messages.app"), "Casual");
        assert_eq!(tone_for_label(&settings, "Messages.app tips - Safari", "Safari.app"), "Professional");
    }

    #[test]
    fn personal_dictionary_corrects_every_occurrence_of_a_phrase() {
        let dictionary = vec!["New York".to_string()];
        let text = "flying to new york then back from new york tomorrow";
        let corrected = apply_personal_dictionary(text, &dictionary);
        assert_eq!(
            corrected.matches("New York").count(),
            2,
            "every spoken repeat of a taught phrase should be corrected, got {corrected:?}"
        );
    }

    // Regression test for a real bug: the phrase-replacement offset was
    // computed from a *separately lowercased* copy of the text and then
    // reused against the original string — safe for plain ASCII, but a
    // handful of Unicode characters (Turkish İ here) expand when lowercased,
    // which used to shift every later byte offset and could panic
    // `replace_range` for landing mid-character. The only claim this test
    // makes is that it returns instead of panicking either way; an offset
    // that no longer verifiably lines up is meant to be skipped, not forced.
    #[test]
    fn personal_dictionary_phrase_replace_does_not_panic_on_unicode_that_changes_length_when_lowercased() {
        let dictionary = vec!["new york".to_string()];
        let text = "İstanbul — we are flying to New York tomorrow";
        let _ = apply_personal_dictionary(text, &dictionary);
    }

    #[test]
    fn test_phonetic_code_sound_alikes() {
        // "lattice" vs "lattes": exact phonetic match
        assert_eq!(phonetic_code("lattice"), phonetic_code("lattes"));
        // "lettuce" vs "lattes": must NOT match because first vowel differs ('e' vs 'a')
        assert_ne!(phonetic_code("lettuce"), phonetic_code("lattes"));
        // "vendor" vs "wender": exact phonetic match (v/w collapsed)
        assert_eq!(phonetic_code("vendor"), phonetic_code("wender"));
        // "crochetant" vs "croissant": near phonetic match (phonetic edit distance <= 1)
        assert!(levenshtein(&phonetic_code("crochetant"), &phonetic_code("croissant")) <= 1);
    }

    #[test]
    fn test_personal_dictionary_phonetic_matching() {
        // 1. "crochetant" -> "croissant"
        let dict = vec!["croissant".to_string()];
        assert_eq!(
            apply_personal_dictionary("I would like a crochetant please.", &dict),
            "I would like a croissant please."
        );

        // 2. "lattice" -> "lattes"
        let dict = vec!["lattes".to_string()];
        assert_eq!(
            apply_personal_dictionary("two iced lattice to go", &dict),
            "two iced lattes to go"
        );

        // 3. Factual "lettuce" must NOT be overwritten with "lattes"
        assert_eq!(
            apply_personal_dictionary("extra lettuce on my burger", &dict),
            "extra lettuce on my burger"
        );
    }

    #[test]
    fn test_personal_dictionary_domain_and_glued_words() {
        // "thewender.com" -> "the vendor.com"
        let dict = vec!["the vendor.com".to_string()];
        assert_eq!(
            apply_personal_dictionary("please check out thewender.com for details", &dict),
            "please check out the vendor.com for details"
        );

        // "the wender.com" -> "the vendor.com"
        assert_eq!(
            apply_personal_dictionary("visit the wender.com today", &dict),
            "visit the vendor.com today"
        );

        // "thewender.com" with spaceless dictionary entry -> "thevendor.com"
        let dict_compact = vec!["thevendor.com".to_string()];
        assert_eq!(
            apply_personal_dictionary("please visit thewender.com", &dict_compact),
            "please visit thevendor.com"
        );
    }

    #[test]
    fn test_personal_dictionary_casing_preservation() {
        let dict = vec!["croissant".to_string(), "PostgreSQL".to_string()];
        // Capitalized sentence start
        assert_eq!(
            apply_personal_dictionary("Crochetant is delicious.", &dict),
            "Croissant is delicious."
        );
        // Preserves internal casing of technical terms
        assert_eq!(
            apply_personal_dictionary("connect to postgresql database", &dict),
            "connect to PostgreSQL database"
        );
    }

    #[test]
    fn test_common_words_sorted() {
        assert!(is_common_english_word("about"));
        assert!(is_common_english_word("have"));
        assert!(is_common_english_word("your"));
        assert!(!is_common_english_word("ivy"));
        assert!(!is_common_english_word("croissant"));
    }

    #[test]
    fn test_task6_bug_a_ive_not_replaced_by_ivy() {
        let dict = vec!["IVY".to_string(), "Notion".to_string()];
        // "I've" with dictionary ["IVY"] must stay "I've"
        assert_eq!(
            apply_personal_dictionary("I've contacted support twice", &dict),
            "I've contacted support twice"
        );
        assert_eq!(
            apply_personal_dictionary("I've booked 40 rooms", &dict),
            "I've booked 40 rooms"
        );
        // Curly apostrophe contraction also stays "I’ve"
        assert_eq!(
            apply_personal_dictionary("I’ve contacted support twice", &dict),
            "I’ve contacted support twice"
        );
        // Exact term "ivy" becomes "IVY"
        assert_eq!(
            apply_personal_dictionary("ivy is climbing the wall", &dict),
            "IVY is climbing the wall"
        );
        assert_eq!(
            apply_personal_dictionary("I love ivy plants", &dict),
            "I love IVY plants"
        );
    }
}

