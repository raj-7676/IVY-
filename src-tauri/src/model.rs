//! Ivy's speech model: two files (2.4 GB) that ship outside the installer, because NSIS can't hold more than
//! 2 GB and GitHub caps each release file at 2 GB. Setup copies them in when they lie next to it (an offline
//! install); otherwise Ivy downloads them itself on first start, the way apps that fetch their content after
//! install do, with progress in its own window (src/components/ModelDownload.tsx).
//!
//! 32 MB pieces, 8 at a time, so one slow or dropped connection can't stall or kill the rest (a friend's
//! one-connection install died with curl exit 18 at ~570 KB/s, 2026-10-07). Failed pieces are fetched again,
//! finished ones survive a restart (`<file>.part` plus the `<file>.pieces` list), and each file must match its
//! pinned SHA-256 before it gets its real name and Ivy loads it.

use std::collections::{HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tauri::Emitter;

/// The model files live on the v0.2.6 release: the first one under Ivy's current license (MIT + Commons Clause;
/// the model itself Apache 2.0 + Commons Clause). Older releases were removed (Yash, 2026-10-08). The files
/// themselves are unchanged. A new model means new names, sizes and hashes here AND in
/// src-tauri/installer/hooks.nsh (package-installer.mjs checks both against the real files).
const BASE_URL: &str = "https://github.com/raj-7676/IVY-/releases/download/v0.2.6";
pub const FILES: [(&str, u64, &str); 2] = [
    ("mmproj-ivy-lite-f16.gguf", 641_774_016, "07ed1cc9c96c19aba84354b9135c69332747a98d50363a321bf1418aececfc00"),
    ("ivy-lite-Q8_0.gguf", 1_834_422_208, "da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed"),
];
const PIECE: u64 = 32 << 20;
const WORKERS: usize = 8;
const ROUNDS: u32 = 20;

/// What the window and the capsule show. `state`: checking (only until `check` runs at startup), missing,
/// downloading, retrying, verifying, ready, failed.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: &'static str,
    pub done: u64,
    pub total: u64,
    /// Bytes per second, smoothed.
    pub speed: f64,
    pub message: String,
}

static STATUS: Mutex<Status> =
    Mutex::new(Status { state: "checking", done: 0, total: 0, speed: 0.0, message: String::new() });
static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn status() -> Status {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn is_ready() -> bool {
    status().state == "ready"
}

fn set(app: &tauri::AppHandle, s: Status) {
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = s.clone();
    let _ = app.emit("ivy://model-download", s);
}

fn simple(state: &'static str, message: impl Into<String>) -> Status {
    Status { state, done: 0, total: 0, speed: 0.0, message: message.into() }
}

/// In place with the right size. Files only get their real name after their hash matched (or setup checked
/// them), so the size is enough here; hashing 2.4 GB at every start would cost seconds.
fn in_place(dir: &Path, name: &str, size: u64) -> bool {
    fs::metadata(dir.join(name)).map(|m| m.len() == size).unwrap_or(false)
}

/// At startup, before the window loads: "ready" or "missing" from the files on disk, so the window never
/// guesses. Only "missing" ever shows the download screen (Yash: one time only, never a flash).
pub fn check(dir: &Path) {
    let state = if FILES.iter().all(|(name, size, _)| in_place(dir, name, *size)) { "ready" } else { "missing" };
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = simple(state, "");
}

/// After `check`, and from the Retry button: ready if both files are in place, else downloads them on a
/// background thread. `on_ready` runs once the model is usable, with `true` only right after a download.
pub fn ensure(app: &tauri::AppHandle, dir: PathBuf, on_ready: fn(&tauri::AppHandle, bool)) {
    if FILES.iter().all(|(name, size, _)| in_place(&dir, name, *size)) {
        set(app, simple("ready", ""));
        on_ready(app, false);
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let result = download(&app, &dir);
        RUNNING.store(false, Ordering::SeqCst);
        match result {
            Ok(()) => {
                log::info!("Ivy: speech model downloaded and checked");
                set(&app, simple("ready", ""));
                on_ready(&app, true);
            }
            Err(e) => {
                log::error!("Ivy: speech model download failed: {e}");
                set(&app, simple("failed", e));
            }
        }
    });
}

struct Part {
    url: String,
    size: u64,
    file: File,
    /// Indices of the pieces already written, one per line; appended as each piece lands.
    pieces: Mutex<File>,
}

fn download(app: &tauri::AppHandle, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let todo: Vec<_> = FILES.iter().filter(|(name, size, _)| !in_place(dir, name, *size)).collect();
    let total: u64 = todo.iter().map(|(_, size, _)| size).sum();

    let mut parts = Vec::new();
    let mut queue = VecDeque::new();
    let mut have = 0u64;
    for (i, (name, size, _)) in todo.iter().enumerate() {
        let part_path = dir.join(format!("{name}.part"));
        let list_path = dir.join(format!("{name}.pieces"));
        let done: HashSet<u64> = fs::read_to_string(&list_path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect();
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&part_path)
            .map_err(|e| format!("Couldn't write {}: {e}", part_path.display()))?;
        let fresh = file.metadata().map(|m| m.len() != *size).unwrap_or(true);
        let done = if fresh { HashSet::new() } else { done };
        if fresh {
            // ponytail: no disk-space check up front; set_len below fails straight away when the drive is too
            // small, before anything is downloaded.
            file.set_len(*size).map_err(|e| {
                format!("Not enough free disk space for Ivy's speech model (it needs {:.1} GB). {e}", total as f64 / 1e9)
            })?;
        }
        // Emptied first when starting over: an append-only handle can't be truncated on Windows (Access denied).
        if fresh {
            fs::write(&list_path, "").map_err(|e| format!("Couldn't write {}: {e}", list_path.display()))?;
        }
        let pieces = OpenOptions::new().create(true).append(true).open(&list_path)
            .map_err(|e| format!("Couldn't write {}: {e}", list_path.display()))?;
        for p in 0..size.div_ceil(PIECE) {
            if done.contains(&p) {
                have += piece_len(*size, p);
            } else {
                queue.push_back((i, p));
            }
        }
        parts.push(Part { url: format!("{BASE_URL}/{name}"), size: *size, file, pieces: Mutex::new(pieces) });
    }

    let parts = Arc::new(parts);
    let done = Arc::new(AtomicU64::new(have));
    let agent = ureq::AgentBuilder::new()
        // Windows' own TLS and certificate store, like curl.exe: antivirus "web shields" that check HTTPS
        // install their own root certificate there, and a built-in list of roots would reject them.
        .tls_connector(Arc::new(native_tls::TlsConnector::new().map_err(|e| e.to_string())?))
        .try_proxy_from_env(true)
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(30))
        .user_agent(concat!("Ivy/", env!("CARGO_PKG_VERSION")))
        .build();

    let progress = Progress::start(app.clone(), done.clone(), total);
    let mut last_error = String::new();
    for round in 1..=ROUNDS {
        if queue.is_empty() {
            break;
        }
        if round > 1 {
            let wait = (3 * round).min(30);
            progress.pause(&format!("Connection dropped. Trying again in {wait} s… ({last_error})"));
            std::thread::sleep(Duration::from_secs(wait as u64));
            progress.resume();
        }
        let work = Arc::new(Mutex::new(std::mem::take(&mut queue)));
        let failed = Arc::new(Mutex::new((VecDeque::new(), String::new())));
        let workers: Vec<_> = (0..WORKERS)
            .map(|_| {
                let (work, failed, parts, done, agent) = (work.clone(), failed.clone(), parts.clone(), done.clone(), agent.clone());
                std::thread::spawn(move || loop {
                    let Some((i, p)) = work.lock().unwrap_or_else(|e| e.into_inner()).pop_front() else { break };
                    let part = &parts[i];
                    match fetch_piece(&agent, part, p, &done) {
                        Ok(()) => {
                            let _ = writeln!(part.pieces.lock().unwrap_or_else(|e| e.into_inner()), "{p}");
                        }
                        Err(e) => {
                            let mut f = failed.lock().unwrap_or_else(|e| e.into_inner());
                            f.0.push_back((i, p));
                            f.1 = e;
                        }
                    }
                })
            })
            .collect();
        for w in workers {
            let _ = w.join();
        }
        let mut f = failed.lock().unwrap_or_else(|e| e.into_inner());
        queue = std::mem::take(&mut f.0);
        last_error = std::mem::take(&mut f.1);
    }
    progress.stop();
    if !queue.is_empty() {
        return Err(format!(
            "Couldn't download Ivy's speech model ({last_error}). Check your internet connection, then press Retry: \
             the finished part is kept."
        ));
    }

    for (part, (name, size, sha)) in parts.iter().zip(&todo) {
        let part_path = dir.join(format!("{name}.part"));
        let list_path = dir.join(format!("{name}.pieces"));
        set(app, Status { state: "verifying", done: 0, total: *size, speed: 0.0, message: String::new() });
        let hash = sha256(&part.file, *size, |n| {
            if n % (64 << 20) < (1 << 20) || n == *size {
                set(app, Status { state: "verifying", done: n, total: *size, speed: 0.0, message: String::new() });
            }
        })?;
        if hash != *sha {
            let _ = fs::remove_file(&part_path);
            let _ = fs::remove_file(&list_path);
            return Err(format!("{name} arrived damaged (its checksum didn't match), so it was deleted. Press Retry."));
        }
        let _ = part.file.sync_all();
        let final_path = dir.join(name);
        let _ = fs::remove_file(&final_path);
        fs::rename(&part_path, &final_path).map_err(|e| format!("Couldn't finish {name}: {e}"))?;
        let _ = fs::remove_file(&list_path);
    }
    Ok(())
}

fn piece_len(size: u64, p: u64) -> u64 {
    PIECE.min(size - p * PIECE)
}

/// One range request, written in place. On failure the piece's bytes come back off the progress count, and
/// the whole piece is fetched again later.
fn fetch_piece(agent: &ureq::Agent, part: &Part, p: u64, done: &AtomicU64) -> Result<(), String> {
    let from = p * PIECE;
    let end = from + piece_len(part.size, p);
    let resp = agent
        .get(&part.url)
        .set("Range", &format!("bytes={from}-{}", end - 1))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("the server answered {code}"),
            ureq::Error::Transport(t) => t.to_string(),
        })?;
    // 200 would be the whole file from byte 0, not this piece.
    if resp.status() != 206 {
        return Err(format!("the server answered {} instead of a piece", resp.status()));
    }
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 256 << 10];
    let mut pos = from;
    let result = loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break if pos == end { Ok(()) } else { Err("the connection closed early".to_string()) },
            Ok(n) if pos + n as u64 > end => break Err("the server sent too much".to_string()),
            Ok(n) => n,
            Err(e) => break Err(e.to_string()),
        };
        let mut chunk = &buf[..n];
        while !chunk.is_empty() {
            match part.file.seek_write(chunk, pos) {
                Ok(w) => {
                    chunk = &chunk[w..];
                    pos += w as u64;
                    done.fetch_add(w as u64, Ordering::Relaxed);
                }
                Err(e) => return Err(format!("couldn't write to disk: {e}")),
            }
        }
    };
    if result.is_err() {
        done.fetch_sub(pos - from, Ordering::Relaxed);
    }
    result
}

fn sha256(file: &File, size: u64, mut progress: impl FnMut(u64)) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut pos = 0u64;
    while pos < size {
        let n = file.seek_read(&mut buf, pos).map_err(|e| format!("Couldn't read the download back: {e}"))?;
        if n == 0 {
            return Err("The download is shorter than it should be. Press Retry.".to_string());
        }
        hasher.update(&buf[..n]);
        pos += n as u64;
        progress(pos);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Sends the status twice a second while pieces download.
struct Progress {
    stop: Arc<AtomicBool>,
    paused: Arc<Mutex<Option<String>>>,
    thread: std::thread::JoinHandle<()>,
}

impl Progress {
    fn start(app: tauri::AppHandle, done: Arc<AtomicU64>, total: u64) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let paused: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (stop2, paused2) = (stop.clone(), paused.clone());
        let thread = std::thread::spawn(move || {
            let mut prev = done.load(Ordering::Relaxed);
            let mut speed = 0.0;
            while !stop2.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(500));
                let now = done.load(Ordering::Relaxed);
                let message = paused2.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let state = if message.is_some() { "retrying" } else { "downloading" };
                speed = if message.is_some() { 0.0 } else { 0.8 * speed + 0.2 * (now.saturating_sub(prev) as f64 * 2.0) };
                prev = now;
                set(&app, Status { state, done: now, total, speed, message: message.unwrap_or_default() });
            }
        });
        Progress { stop, paused, thread }
    }

    fn pause(&self, message: &str) {
        *self.paused.lock().unwrap_or_else(|e| e.into_inner()) = Some(message.to_string());
    }

    fn resume(&self) {
        *self.paused.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Waits for the last report, so nothing it sends can land after the next state.
    fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.join();
    }
}
