use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

mod audio;
mod rulebooks;
pub(crate) mod gpu_monitor;
mod spellcheck;
pub mod lite;

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

// The one real global shortcut registered for "undo last paste", kept as a
// parsed `Shortcut` (not just the settings string) so the handler can match
// an incoming press against it with a cheap `==` instead of re-parsing
// settings.json on every single keystroke of every registered hotkey.
// Updated at startup and whenever Settings saves a changed binding.
static UNDO_SHORTCUT: Mutex<Option<Shortcut>> = Mutex::new(None);

// Same purpose as `UNDO_SHORTCUT`, for the separate "paste Ivy's held-back
// text" binding (see `MANUAL_PASTE_TEXT` below).
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

// What a real paste actually overwrote — captured only when `paste_text`
// sent a genuine Ctrl+V into a real foreground window (the clipboard-only
// fallback never touches the real clipboard at all now, see
// `MANUAL_PASTE_TEXT`, so there's nothing to undo in that case).
// Single-slot and single-use: undoing consumes it, so pressing undo twice
// in a row does nothing the second time rather than replaying stale state.
struct LastPaste {
    target_hwnd: isize,
    // What the clipboard held immediately before Ivy's paste overwrote it —
    // restored on undo so the user's own prior clipboard item isn't lost.
    // `None` if the clipboard was empty or held non-text content.
    previous_clipboard: Option<String>,
}
static LAST_PASTE: Mutex<Option<LastPaste>> = Mutex::new(None);

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
// Alt+B, not Alt+Z: Alt+Z is the Nvidia app's own overlay hotkey (AMD
// Adrenalin listens for it too on hybrid-GPU laptops). Yash's call: two
// keys, not three — Ctrl+Alt+Space was the safer three-key pick but he
// found it awkward to reach one-handed. Alt+B isn't a known global hotkey
// for Nvidia/AMD/Xbox Game Bar/Discord/Steam; any in-app "Alt+B" menu
// mnemonic (some apps use it for a Bookmarks/sidebar toggle) simply won't
// fire while Ivy holds the real OS-level registration — same trade-off
// already accepted for the main Alt+Space hotkey overriding the native
// window system-menu shortcut. If something on a given PC really does
// register Alt+B as a true global hotkey first, registration just fails
// loudly (see `apply_hotkey`/`apply_undo_hotkey`) rather than silently
// double-firing — never a fake "saved" state.
fn default_undo_paste_hotkey() -> String {
    "Alt + B".to_string()
}
// Alt+V: one-handed (both keys on the left side), verified via web search
// against every major overlay's actual default — Nvidia GeForce Experience
// is Alt+Z, AMD Adrenalin is Alt+R, Discord is Shift+`, Steam is Shift+Tab,
// Xbox Game Bar is Win+G — none of them Alt+V. Ctrl+Alt+V was considered
// and rejected: it's Excel's real, current default for Paste Special, a
// genuine collision, not a guess. "V" also reads naturally as "paste"
// (same finger memory as Ctrl+V). Same accepted trade-off class as Alt+B
// above for any classic Win32 "Alt+V opens the View menu" mnemonic.
fn default_manual_paste_hotkey() -> String {
    "Alt + V".to_string()
}

fn default_onboarding_completed() -> bool {
    false
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
    #[serde(default = "default_undo_paste_hotkey")]
    undo_paste_hotkey: String,
    #[serde(default = "default_manual_paste_hotkey")]
    manual_paste_hotkey: String,
    #[serde(default = "default_onboarding_completed")]
    onboarding_completed: bool,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        let mut preset_apps = HashMap::new();
        preset_apps.insert(
            "Casual".to_string(),
            vec!["Instagram", "Discord", "WhatsApp", "Messages"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        preset_apps.insert(
            "Standard".to_string(),
            vec!["VS Code", "Figma", "Terminal", "Notion", "Cursor"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        preset_apps.insert(
            "Professional".to_string(),
            vec!["Outlook", "Gmail", "Slack", "Linear"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        Self {
            hotkey: "Alt + Space".to_string(),
            active_tone_preset: "Standard".to_string(),
            preset_apps,
            personal_dictionary: vec![],
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
            undo_paste_hotkey: default_undo_paste_hotkey(),
            manual_paste_hotkey: default_manual_paste_hotkey(),
            onboarding_completed: default_onboarding_completed(),
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

fn settings_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_data_dir().expect("no app data dir");
    let _ = fs::create_dir_all(&dir);
    dir.join("settings.json")
}

fn history_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_data_dir().expect("no app data dir");
    let _ = fs::create_dir_all(&dir);
    dir.join("history.json")
}

fn stats_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_data_dir().expect("no app data dir");
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

// Daily privacy retention: sensitive recordings and transcripts are completely purged every 24 hours.
// User progress/stats remain permanently decoupled in `stats.json` and are NEVER reset to zero.
const RETENTION_SECS: i64 = 24 * 60 * 60;

fn audio_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_data_dir().expect("no app data dir").join("audio");
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
/// than `RETENTION_SECS` — the app makes no lasting record of what was
/// said. Called at launch and again on an hourly timer (see `run()`) since
/// Ivy is designed to keep running for weeks without a restart (the main
/// window only ever hides on close, never actually quits).
fn purge_old_history(app: &tauri::AppHandle) {
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut sessions = load_history(app);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let before = sessions.len();
    for s in &sessions {
        if s.created_at != 0 && (now - s.created_at) / 1000 > RETENTION_SECS && path_within_audio_dir(app, &s.audio_path) {
            let _ = fs::remove_file(&s.audio_path);
        }
    }
    sessions.retain(|s| s.created_at == 0 || (now - s.created_at) / 1000 <= RETENTION_SECS);
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
    let Ok(dir) = app.path().app_data_dir() else { return };
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
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models")
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
    let mut config: SettingsConfig = fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if let Ok(dir) = app.path().app_data_dir() {
        let pref_file = dir.join("hardware_preference.txt");
        if pref_file.exists() {
            if let Ok(pref) = fs::read_to_string(&pref_file) {
                let p = pref.trim().to_lowercase();
                if p == "gpu" || p == "cpu" {
                    log::info!("Ivy: Applying installer hardware preference: {p}");
                    config.hardware_mode = p;
                }
            }
            let _ = fs::remove_file(pref_file);
        }
    }
    config
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

// Windows process/window helpers — best-effort app identification for tone
// presets. Falls back to "Desktop" if anything fails; never panics the
// hotkey path over an identification miss.
//
// Prefers the window title over the exe name deliberately: several of the
// app's own default tone-preset apps (Instagram, Gmail, Notion, Figma,
// Linear) have no native Windows exe at all and are only ever reached
// through a browser tab, so exe-name-only matching would silently break
// tone routing for exactly the apps it ships configured for out of the
// box. The real cost — a window title (which can carry document/page
// content) ending up in `history.json`'s app_target forever — is a known,
// deliberately deferred trade-off, not an oversight; fixing it properly
// needs a second, storage-only value threaded alongside this one, not a
// one-line swap here.
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

#[cfg(windows)]
fn foreground_app_label() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return "Desktop".to_string();
        }
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        let title = String::from_utf16_lossy(&title_buf[..len.max(0) as usize]);

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let exe_name = exe_name_for_pid(pid).trim_end_matches(".exe").to_string();

        if !title.trim().is_empty() {
            title
        } else if !exe_name.is_empty() {
            exe_name
        } else {
            "Desktop".to_string()
        }
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

#[cfg(not(windows))]
fn foreground_app_label() -> String {
    "Desktop".to_string()
}
#[cfg(not(windows))]
fn capture_foreground_hwnd() -> isize {
    0
}
#[cfg(not(windows))]
fn is_explorer_shell(_hwnd: isize) -> bool {
    false
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
    if msg.to_lowercase().contains("already registered") {
        format!(
            "\"{spec}\" is already bound to something else on this PC (another app or a Windows shortcut). Pick a different key, or free this one up in that app's settings first."
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
/// `slot` is whichever cached-comparison static (`UNDO_SHORTCUT`,
/// `MANUAL_PASTE_SHORTCUT`) the shared handler matches an incoming press
/// against — `None` for the main dictation hotkey, which has no such static
/// (anything that isn't the undo/manual-paste shortcut is treated as it).
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
    let new_shortcut = parse_hotkey(new_spec).ok_or_else(|| format!("Couldn't understand the shortcut \"{new_spec}\""))?;
    app.global_shortcut()
        .register(new_shortcut)
        .map_err(|e| describe_register_failure(new_spec, e))?;
    if let Some(old) = parse_hotkey(old_spec) {
        let _ = app.global_shortcut().unregister(old);
    }
    if let Some(cell) = slot {
        *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(new_shortcut);
    }
    Ok(())
}

fn tone_for_label(settings: &SettingsConfig, label: &str) -> String {
    let lower = label.to_lowercase();
    for (tone, apps) in &settings.preset_apps {
        if tone == "Standard" {
            continue;
        }
        if apps.iter().any(|a| lower.contains(&a.to_lowercase())) {
            return tone.clone();
        }
    }
    // Not a hardcoded "Standard" — that silently ignored the Tone screen's
    // "Make X the default" button entirely (it only ever updated a badge
    // in the UI and settings.json, never what actually got applied to an
    // app with no explicit preset match).
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
    }
}

fn validate_settings(s: &SettingsConfig) -> Result<(), String> {
    if s.hotkey.len() > 64 || s.undo_paste_hotkey.len() > 64 || s.manual_paste_hotkey.len() > 64 {
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
    if s.personal_dictionary.len() > 5000 {
        return Err("personal dictionary exceeds maximum allowable entries (5000)".into());
    }
    for word in &s.personal_dictionary {
        if word.len() > 100 {
            return Err("personal dictionary word exceeds maximum length (100 chars)".into());
        }
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
        to_persist.hotkey = settings.hotkey.clone();
        write_settings_atomic(&app, &to_persist)?;
    }
    if settings.undo_paste_hotkey != old_settings.undo_paste_hotkey {
        apply_hotkey_binding(&app, &old_settings.undo_paste_hotkey, &settings.undo_paste_hotkey, Some(&UNDO_SHORTCUT))?;
        to_persist.undo_paste_hotkey = settings.undo_paste_hotkey.clone();
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
    let sessions = load_history(&app);
    for s in &sessions {
        if path_within_audio_dir(&app, &s.audio_path) {
            let _ = fs::remove_file(&s.audio_path);
        }
    }
    write_history_atomic(&app, &[]);
    let _ = app.emit("ivy://history-updated", ());
    Ok(())
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
    let normalized = audio::normalize_audio(samples);
    let stt_samples = if normalized.is_empty() { samples } else { &normalized };

    // Hallucinations book, stage A (src/rulebooks/hallucinations.rs): no voice -> no text. The model gets the
    // full audio (not the pause-squeezed copy): squeezing pauses dropped the end of long dictations (Task 6 Bug B).
    let (_voiced_audio, voice) = rulebooks::hallucinations::prepare_audio(stt_samples, 16000);
    if voice.voiced_secs < 0.25 {
        debug_log(app, &format!("no speech detected ({:.2}s voiced of {:.2}s) — nothing transcribed", voice.voiced_secs, voice.total_secs));
        return String::new();
    }

    let dictionary = load_settings(app).personal_dictionary;
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
                    let backend = if engine.is_cpu_mode { "CPU" } else { "GPU (Vulkan)" };
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
    apply_personal_dictionary(&formatted, &dictionary)
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
// half is gone"). Raised again to 500s (Yash: "people talk a lot, man").
const MAX_DICTATION_SECS: f64 = 500.0;

fn run_dictation_pipeline(
    app: tauri::AppHandle,
    mut samples: Vec<f32>,
    active_app: String,
    tone_preset: String,
    dictation_id: u64,
    target_hwnd: isize,
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
#[tauri::command]
fn retry_transcription(app: tauri::AppHandle, id: String) -> Result<RetryResult, String> {
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

    // `paste_text` itself always ends up putting `text` on the clipboard,
    // in both its real-paste and clipboard-only-fallback paths — writing it
    // here first was redundant, and worse, clobbered the user's real
    // clipboard before `paste_text` got a chance to capture it for undo.
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
    // original paste, `LAST_PASTE` should be left alone so a plain Alt+B
    // undo still works normally; only take it once we're actually
    // committing to the swap below.
    let target_hwnd = LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|lp| lp.target_hwnd);
    let Some(target_hwnd) = target_hwnd else {
        return Err("nothing was pasted for this dictation, nothing to touch up".into());
    };
    #[cfg(windows)]
    let same_window = unsafe { GetForegroundWindow().0 as isize == target_hwnd };
    #[cfg(not(windows))]
    let same_window = false;
    if !same_window {
        return Err("switched to a different window since the paste — can't safely swap the text there".into());
    }

    // Re-check focus right before sending keys, the same race `paste_text`'s own
    // "re-check right before sending keys" guards against for a fresh paste.
    #[cfg(windows)]
    let still_same_window = unsafe { GetForegroundWindow().0 as isize == target_hwnd };
    #[cfg(not(windows))]
    let still_same_window = false;
    if !still_same_window {
        return Err("switched to a different window while touching up — can't safely swap the text there".into());
    }

    // Now committing: consume the original paste record for the Ctrl+Z
    // below, exactly like `undo_last_paste`, but keep its
    // `previous_clipboard` — the clipboard state from *before Ivy's very
    // first paste* — so a follow-up Alt+B after this still restores the
    // user's real original clipboard, not some meaningless intermediate
    // value this function's own clipboard swap would otherwise leave.
    let Some(last) = LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return Err("nothing was pasted for this dictation, nothing to touch up".into());
    };
    #[cfg(windows)]
    {
        release_held_modifiers();
    }
    let undone = send_ctrl_key(0x5A); // VK_Z
    if !undone {
        // Put the original record back — the undo never happened, so
        // there's nothing new to swap in, and Alt+B should still work.
        *LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(last);
        return Err("couldn't undo the original paste".into());
    }

    let pasted = paste_text(corrected.clone(), Some(target_hwnd));
    if !pasted {
        debug_log(&app, "touch-up: undo succeeded but re-paste failed — original text is gone from the target window");
        return Err("undid the original text but couldn't paste the touched-up version — check the target window".into());
    }
    // `paste_text` just set a fresh `LAST_PASTE` with an intermediate
    // clipboard value that means nothing to the user; restore the true
    // original so a subsequent Alt+B reverts all the way back.
    if let Some(entry) = LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        entry.previous_clipboard = last.previous_clipboard;
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
    let label = foreground_app_label();
    let settings = load_settings(&app);
    let tone = tone_for_label(&settings, &label);
    ActiveContext {
        active_app: label,
        tone_preset: tone,
    }
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

#[cfg(not(windows))]
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
        0x42, // VK_B
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

#[cfg(not(windows))]
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
        if !regained {
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
    // Read before overwriting — this is the one and only chance to know
    // what the clipboard held before Ivy clobbered it, needed to restore it
    // on undo. Best-effort: `None` for an empty clipboard or non-text
    // content (an image, a file selection) — undo then just clears the
    // clipboard on restore rather than fabricating something to put back.
    let previous_clipboard = arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok();
    let expected_text = text.clone();
    // A target app can still swallow a Ctrl+V that SendInput reports as sent,
    // so Alt+V must always be able to re-paste the latest transcript.
    *MANUAL_PASTE_TEXT.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set_text(text);
    }
    std::thread::sleep(std::time::Duration::from_millis(60));
    #[cfg(windows)]
    unsafe {
        // Re-check right before sending keys — the sleep above is enough
        // time for focus to have moved again since the check above.
        if GetForegroundWindow().0 as isize != target {
            return false;
        }
    }
    let ok = send_ctrl_key(0x56);
    if ok {
        // A real keystroke paste just happened into `target` — record it as
        // undoable. Never set for the clipboard-only fallback above (early
        // return): no real text-field edit happened there to undo.
        *LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(LastPaste {
            target_hwnd: target,
            previous_clipboard: previous_clipboard.clone(),
        });

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
                        if let Some(prev) = previous_clipboard {
                            let _ = clipboard.set_text(prev);
                        } else {
                            let _ = clipboard.clear();
                        }
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

#[cfg(not(windows))]
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
    app.primary_monitor().ok().flatten()
}

fn position_capsule_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("capsule") {
        let width = 440.0;
        let top_margin = 8.0;
        let mut x = 0.0;
        let mut y = top_margin;

        if let Some(monitor) = get_cursor_monitor(app) {
            let scale = monitor.scale_factor();
            let mon_pos = monitor.position();
            let mon_size = monitor.size();

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
    let built = WebviewWindowBuilder::new(
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
    .visible(false)
    .build();

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
fn hide_capsule_window(app: tauri::AppHandle) {
    bridge_capsule_show(&app, false);
}

fn handle_hotkey_down(app: &tauri::AppHandle) {
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
    let capsule_hwnd = app
        .get_webview_window("capsule")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .unwrap_or(0);
    let main_hwnd = app
        .get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .unwrap_or(0);
    let target_hwnd = if fg != 0 && fg != capsule_hwnd && fg != main_hwnd && !is_explorer_shell(fg) { fg } else { 0 };
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
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let level = match PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(p) if p.id == id => p.recorder.level(),
            _ => break,
        };
        let _ = app_level.emit("ivy://mic-level", level);
    });
}

fn handle_hotkey_up(app: &tauri::AppHandle) {
    // Taken out of the mutex (and the guard dropped) before any real work —
    // `recorder.stop()` can resample up to 500s of audio and `debug_log`
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
    std::thread::spawn(move || {
        run_dictation_pipeline(
            app_handle,
            samples,
            pending.active_app,
            pending.tone_preset,
            dictation_id,
            target_hwnd,
        );
    });
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

/// Reverts the last real paste Ivy made: sends a real Ctrl+Z into the exact
/// app that received it (every mainstream text field/editor treats a single
/// paste as one undo step), then restores whatever the clipboard held right
/// before that paste. Single-use — consumes `LAST_PASTE`, so nothing
/// happens on a second press with no new paste in between.
fn undo_last_paste(app: &tauri::AppHandle) {
    let Some(last) = LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        debug_log(app, "undo-paste pressed but there's nothing to undo");
        let _ = app.emit("ivy://undo-paste", false);
        return;
    };

    #[cfg(windows)]
    let same_window = unsafe { GetForegroundWindow().0 as isize == last.target_hwnd };
    #[cfg(not(windows))]
    let same_window = false;

    if !same_window {
        // Focus moved to a different app since the paste — sending Ctrl+Z
        // blindly could undo that app's own unrelated work. Only restore
        // the clipboard, which is always safe, and stop there.
        if let Some(prev) = last.previous_clipboard {
            let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(prev));
        }
        debug_log(app, "undo-paste: foreground app changed, restored clipboard only");
        position_capsule_window(app);
        bridge_capsule_show(app, true);
        let _ = app.emit("ivy://undo-paste", false);
        return;
    }

    #[cfg(windows)]
    release_held_modifiers();

    let sent = send_ctrl_key(0x5A); // VK_Z

    if let Some(prev) = last.previous_clipboard {
        let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(prev));
    } else if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.clear();
    }

    debug_log(app, &format!("undo-paste: sent Ctrl+Z to target window ({sent})"));
    position_capsule_window(app);
    bridge_capsule_show(app, true);
    let _ = app.emit("ivy://undo-paste", sent);
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
    let target = 0;

    let capsule_hwnd = app
        .get_webview_window("capsule")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .unwrap_or(0);
    let main_hwnd = app
        .get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .unwrap_or(0);

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
    let previous_clipboard = arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok();
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text.clone()).is_err() {
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
    #[cfg(windows)]
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

    let sent = send_ctrl_key(0x56); // VK_V

    if sent {
        *LAST_PASTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(LastPaste {
            target_hwnd: target,
            previous_clipboard: previous_clipboard.clone(),
        });

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
                        if let Some(prev) = previous_clipboard {
                            let _ = clipboard.set_text(prev);
                        } else {
                            let _ = clipboard.clear();
                        }
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
    debug_log(app, reason);
}

#[tauri::command]
fn cancel_dictation(app: tauri::AppHandle) {
    cancel_dictation_internal(&app, "dictation cancelled by user — discarded audio and state");
}

#[tauri::command]
fn start_manual_dictation(app: tauri::AppHandle) {
    handle_hotkey_down(&app);
}

#[tauri::command]
fn stop_manual_dictation(app: tauri::AppHandle) {
    handle_hotkey_up(&app);
}

#[tauri::command]
fn trigger_undo_paste(app: tauri::AppHandle) {
    undo_last_paste(&app);
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

    let active_engine = if on_battery {
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
                // Three real bindings can fire this one handler: the
                // dictation hotkey (`apply_hotkey` always keeps exactly one
                // registered, so any firing that isn't the undo or manual-
                // paste shortcut is the real user's current dictation
                // hotkey by construction), the undo-paste shortcut
                // (`UNDO_SHORTCUT`), and the manual-paste shortcut
                // (`MANUAL_PASTE_SHORTCUT`).
                .with_handler(move |app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        suppress_alt_menu();
                    }
                    let is_undo = UNDO_SHORTCUT
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .map_or(false, |u| u == *shortcut);
                    if is_undo {
                        if event.state() == ShortcutState::Pressed {
                            undo_last_paste(app);
                        }
                        return;
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
                    // Raw OS event, logged before any of our own routing
                    // logic runs — isolates whether Windows/the global-
                    // shortcut plugin ever delivers a Released event at all
                    // during a stuck-recording episode, versus our own
                    // routing code receiving one and failing to act on it.
                    debug_log(app, &format!("raw hotkey event: {:?}", event.state()));
                    match event.state() {
                        ShortcutState::Pressed => {
                            // `swap` both reads the previous value and sets
                            // true atomically — a duplicate Pressed while
                            // already down (OS key-repeat, confirmed live)
                            // is swallowed here, before it can reach
                            // `route_dictation_press` at all.
                            if HOTKEY_PHYSICALLY_DOWN.swap(true, Ordering::SeqCst) {
                                debug_log(app, "raw hotkey event: Pressed while already down — duplicate/auto-repeat, ignored");
                                return;
                            }
                            route_dictation_press(app);
                        }
                        ShortcutState::Released => {
                            if !HOTKEY_PHYSICALLY_DOWN.swap(false, Ordering::SeqCst) {
                                debug_log(app, "raw hotkey event: Released while already up — duplicate, ignored");
                                return;
                            }
                            route_dictation_release(app);
                        }
                    }
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
            trigger_undo_paste,
            get_hardware_status,
            apply_hardware_mode,
            cancel_dictation,
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
            let first_launch = !settings_path(&app.handle()).exists();
            // Real Windows Run-key launch, not a manual one — see the
            // `--autostart` arg registered with the plugin above.
            let launched_via_autostart = std::env::args().any(|a| a == "--autostart");
            let startup_settings = load_settings(&app.handle());
            sync_autostart(&app.handle(), startup_settings.launch_at_startup);
            // Registers whatever hotkey the user last saved, not a hardcoded
            // Alt+Space — `parse_hotkey` returning `None` for a corrupt
            // settings value falls back to the real default rather than
            // registering nothing.
            let startup_hotkey = parse_hotkey(&startup_settings.hotkey)
                .unwrap_or_else(|| Shortcut::new(Some(Modifiers::ALT), Code::Space));
            if let Err(e) = app.global_shortcut().register(startup_hotkey) {
                log::error!("Ivy: {} didn't register ({e}) — something else on this PC already has it.", startup_settings.hotkey);
            }
            // Same real registration as the dictation hotkey, for the
            // separate undo-paste binding — `UNDO_SHORTCUT` is what the
            // shared handler compares an incoming press against.
            let startup_undo_hotkey = parse_hotkey(&startup_settings.undo_paste_hotkey)
                .unwrap_or_else(|| Shortcut::new(Some(Modifiers::ALT), Code::KeyB));
            if let Err(e) = app.global_shortcut().register(startup_undo_hotkey) {
                log::error!("Ivy: {} (undo paste) didn't register ({e}) — something else on this PC already has it.", startup_settings.undo_paste_hotkey);
            } else {
                *UNDO_SHORTCUT.lock().unwrap_or_else(|e| e.into_inner()) = Some(startup_undo_hotkey);
            }
            // Same real registration again, for the manual-paste binding —
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
                let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
                let separator = PredefinedMenuItem::separator(app)?;
                let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let tray_menu = Menu::with_items(app, &[&dictate_item, &settings_item, &separator, &quit_item])?;

                // `TrayIconBuilder::build` returns a real `TrayIcon` handle
                // whose `Drop` impl removes the icon from the tray — ending
                // this in a bare `;` statement (the original bug: it built,
                // then was destroyed at the end of that same statement, so
                // nothing was ever actually visible) instead of keeping it
                // alive for the app's lifetime via managed state.
                let tray_icon = TrayIconBuilder::new()
                    .icon(app.default_window_icon().cloned().ok_or("no default window icon")?)
                    .menu(&tray_menu)
                    .show_menu_on_left_click(true)
                    .tooltip("Ivy — hold Alt+Space anywhere to dictate")
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "dictate_now" => {
                            // Real toggle through the exact same functions the
                            // hotkey itself uses — start if idle, stop (and run
                            // the real pipeline) if a dictation is already in
                            // flight. No separate simulated "tray dictation".
                            if PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                                handle_hotkey_up(app);
                            } else {
                                handle_hotkey_down(app);
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
            let settings = load_settings(&app_handle);
            let threshold = settings.vram_eviction_threshold;
            gpu_monitor::start_gpu_monitor(threshold, move || {
                lite::unload_engine();
            });

            // Background pre-warm: Load active model into memory/VRAM on startup
            // so the very first dictation starts instantly without initialization delay.
            let app_handle_warm = app.handle().clone();
            std::thread::spawn(move || {
                let models = models_dir(&app_handle_warm);
                let prefer_gpu = should_use_gpu(&app_handle_warm);
                let t_warm = std::time::Instant::now();
                match lite::engine(&models, !prefer_gpu) {
                    Ok(eng) => {
                        let backend = if eng.is_cpu_mode { "CPU" } else { "GPU (Vulkan)" };
                        // One silent practice pass, so the user's first real dictation doesn't pay the
                        // one-time GPU pipeline / buffer setup (Yash: "the first reply takes a lot of time").
                        let _ = eng.transcribe(&vec![0.0; 16_000], &[], std::time::Duration::from_secs(60));
                        debug_log(&app_handle_warm, &format!("lite engine pre-warmed via {backend} in {}ms (incl. a silent warm-up pass)", t_warm.elapsed().as_millis()));
                    }
                    Err(e) => debug_log(&app_handle_warm, &format!("lite engine pre-warm failed: {e}")),
                }
            });

            if let Some(window) = app.get_webview_window("main") {
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
    fn test_validate_settings_valid() {
        let mut settings = SettingsConfig::default();
        settings.hotkey = "Alt+Space".to_string();
        settings.active_tone_preset = "Standard".to_string();
        settings.hardware_mode = "gpu".to_string();
        assert!(validate_settings(&settings).is_ok());
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

    // Regression test for a real bug: an app with no explicit tone-preset
    // match used to always fall back to a hardcoded "Standard", silently
    // ignoring whatever the user picked via the Tone screen's "Make X the
    // default" button — that setting was persisted and shown with a
    // "Default" badge but never actually changed real routing.
    #[test]
    fn tone_for_label_falls_back_to_the_configured_default_not_a_hardcoded_standard() {
        let mut settings = SettingsConfig::default();
        settings.active_tone_preset = "Professional".to_string();
        // A label that matches none of the (default) preset app lists.
        assert_eq!(tone_for_label(&settings, "Some Totally Unlisted App"), "Professional");

        settings.active_tone_preset = "Casual".to_string();
        assert_eq!(tone_for_label(&settings, "Some Totally Unlisted App"), "Casual");

        // An explicit match still wins over the configured default.
        settings.preset_apps.insert("Professional".to_string(), vec!["Outlook".to_string()]);
        settings.active_tone_preset = "Casual".to_string();
        assert_eq!(tone_for_label(&settings, "Outlook"), "Professional");
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
        let dict = vec!["IVY".to_string(), "claude".to_string()];
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

