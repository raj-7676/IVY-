//! Everything Linux-only (IVY.md §19). Linux desktops come in two kinds:
//! - X11 (Linux Mint, "Xorg" logins): Ivy asks the X server which window is in front (`_NET_ACTIVE_WINDOW`), reads
//!   the keyboard (`QueryKeymap`) and presses keys itself (XTest), the way it does on Windows.
//! - Wayland (Ubuntu's and Fedora's default): no app may do any of that. Ivy still runs as an X11 window there
//!   (main.rs sets GDK_BACKEND=x11, so the capsule can float over other apps), can see and type into other X11
//!   windows, and leaves the rest of the keyboard to its helper `ivy-keys` (src-tauri/linux/ivy-keys), which the
//!   Linux package installs with the rights for it.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// The window in front on Wayland when it isn't an X11 window: some Wayland app, which can't be named or checked.
pub const WAYLAND_APP: isize = -1;

/// Keyboard bits, the same as ivy-keys sends: Ctrl, Shift, Alt, Super, any other key or mouse button.
pub const CTRL: u8 = 1;
pub const SHIFT: u8 = 2;
pub const ALT: u8 = 4;
pub const SUPER: u8 = 8;
pub const OTHER: u8 = 16;

pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
}

struct X11 {
    conn: RustConnection,
    root: Window,
    active: u32,
    pid: u32,
    net_name: u32,
    utf8: u32,
    window_type: u32,
    desktop: u32,
    clients: u32,
}

/// One connection to the X server for the whole run (on Wayland: XWayland's). None without an X server.
fn x11() -> Option<&'static X11> {
    static X: OnceLock<Option<X11>> = OnceLock::new();
    X.get_or_init(|| {
        let (conn, screen) = RustConnection::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atom = |name: &[u8]| conn.intern_atom(false, name).ok()?.reply().ok().map(|r| r.atom);
        let (active, pid, net_name, utf8) =
            (atom(b"_NET_ACTIVE_WINDOW")?, atom(b"_NET_WM_PID")?, atom(b"_NET_WM_NAME")?, atom(b"UTF8_STRING")?);
        let (window_type, desktop, clients) =
            (atom(b"_NET_WM_WINDOW_TYPE")?, atom(b"_NET_WM_WINDOW_TYPE_DESKTOP")?, atom(b"_NET_CLIENT_LIST")?);
        Some(X11 { conn, root, active, pid, net_name, utf8, window_type, desktop, clients })
    })
    .as_ref()
}

impl X11 {
    fn words(&self, w: Window, prop: u32, kind: u32, len: u32) -> Vec<u32> {
        self.conn
            .get_property(false, w, prop, kind, 0, len)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn bytes(&self, w: Window, prop: u32, kind: u32) -> Vec<u8> {
        self.conn
            .get_property(false, w, prop, kind, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.value)
            .unwrap_or_default()
    }
}

/// The X11 window in front, 0 for none. On Wayland only X11 windows can be seen; any other app reads as 0.
fn active_window() -> u32 {
    x11().and_then(|x| x.words(x.root, x.active, AtomEnum::WINDOW.into(), 1).first().copied()).unwrap_or(0)
}

/// The window in front as Ivy keeps it (`capture_foreground_hwnd`): an X11 window, `WAYLAND_APP`, or 0 (nothing).
pub fn foreground() -> isize {
    match active_window() {
        0 if is_wayland() => WAYLAND_APP,
        w => w as isize,
    }
}

fn window_pid(w: u32) -> u32 {
    x11().and_then(|x| x.words(w, x.pid, AtomEnum::CARDINAL.into(), 1).first().copied()).unwrap_or(0)
}

/// One of Ivy's own windows (its process put it there).
pub fn is_own(w: isize) -> bool {
    w > 0 && window_pid(w as u32) == std::process::id()
}

/// The program, from the window's class: "Slack", "firefox", "Code" ("" if unknown). Tone app lists match on it.
pub fn window_class(w: isize) -> String {
    let Some(x) = x11().filter(|_| w > 0) else { return String::new() };
    let raw = x.bytes(w as u32, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());
    // "instance\0Class\0"
    let parts: Vec<&[u8]> = raw.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
    parts.get(1).or(parts.first()).map(|c| String::from_utf8_lossy(c).into_owned()).unwrap_or_default()
}

/// The program's name for the capsule and history: "Slack", "Firefox", "Code".
pub fn app_label(w: isize) -> String {
    let class = window_class(w);
    let mut chars = class.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

pub fn window_title(w: isize) -> String {
    let Some(x) = x11().filter(|_| w > 0) else { return String::new() };
    let utf8 = x.bytes(w as u32, x.net_name, x.utf8);
    let raw = if utf8.is_empty() { x.bytes(w as u32, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()) } else { utf8 };
    String::from_utf8_lossy(&raw).into_owned()
}

/// The desktop itself (the window with the icons), which takes no typing, like Explorer on Windows.
pub fn is_desktop(w: isize) -> bool {
    x11().filter(|_| w > 0).is_some_and(|x| x.words(w as u32, x.window_type, AtomEnum::ATOM.into(), 16).contains(&x.desktop))
}

/// The apps with an X11 window open now (their classes), for the Tone screen's "Add app". On Wayland only X11 apps
/// show up here, which is honest: those are the only ones Ivy can tell apart.
pub fn open_apps() -> Vec<String> {
    let Some(x) = x11() else { return Vec::new() };
    let mut apps: Vec<String> = x
        .words(x.root, x.clients, AtomEnum::WINDOW.into(), 4096)
        .into_iter()
        .map(|w| w as isize)
        .filter(|&w| !is_own(w) && !is_desktop(w))
        .map(window_class)
        .filter(|c| !c.is_empty())
        .collect();
    apps.sort_by_key(|a| a.to_lowercase());
    apps.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    apps
}

/// What's held on the keyboard and mouse now (the bits above). Wayland: from ivy-keys while it runs (X11 can only
/// see keys while an X11 window is in front), which tells OTHER only while Ctrl and Shift are both held, the one
/// time the Ctrl + Shift watcher looks at it.
pub fn keys() -> u8 {
    if is_wayland() && matches!(helper(), Helper::Ready { .. }) {
        return HELPER_KEYS.load(Ordering::Relaxed);
    }
    let Some(x) = x11() else { return 0 };
    let map = match x.conn.query_keymap().map_err(|e| e.to_string()).and_then(|c| c.reply().map_err(|e| e.to_string())) {
        Ok(map) => map,
        Err(e) => {
            // Once: the Ctrl + Shift watcher asks 66 times a second, and a dead X connection is why it went quiet.
            static SAID: AtomicBool = AtomicBool::new(false);
            if !SAID.swap(true, Ordering::Relaxed) {
                log::warn!("Ivy: the X server didn't answer the keyboard query ({e})");
            }
            return 0;
        }
    };
    // X key codes are the kernel's plus 8.
    let down = |code: usize| map.keys.get((code + 8) / 8).is_some_and(|b| b & (1 << ((code + 8) % 8)) != 0);
    const MODIFIERS: [usize; 8] = [29, 97, 42, 54, 56, 100, 125, 126];
    let buttons = x.conn.query_pointer(x.root).ok().and_then(|c| c.reply().ok()).is_some_and(|p| u16::from(p.mask) & 0x700 != 0);
    let other = buttons || (1..248).any(|c| !MODIFIERS.contains(&c) && down(c));
    (down(29) || down(97)) as u8 * CTRL
        | (down(42) || down(54)) as u8 * SHIFT
        | (down(56) || down(100)) as u8 * ALT
        | (down(125) || down(126)) as u8 * SUPER
        | other as u8 * OTHER
}

const XK_SHIFT_L: u32 = 0xffe1;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_ALT_L: u32 = 0xffe9;
const XK_SUPER_L: u32 = 0xffeb;
const XK_INSERT: u32 = 0xff63;
const XK_Z: u32 = 0x007a;

/// The key code that types `keysym` in the keyboard layout in use, so Ctrl+Z is the Z of an AZERTY keyboard too.
fn keycode(x: &X11, keysym: u32) -> Option<u8> {
    let (min, max) = (x.conn.setup().min_keycode, x.conn.setup().max_keycode);
    let map = x.conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
    let per = (map.keysyms_per_keycode as usize).max(1);
    map.keysyms.chunks(per).position(|syms| syms.contains(&keysym)).map(|i| min + i as u8)
}

/// Presses the keys in order and lets go in reverse, through XTest (the X server's own synthetic input). Reaches X11
/// windows only.
fn x11_press(keysyms: &[u32]) -> bool {
    let Some(x) = x11() else { return false };
    let Some(codes) = keysyms.iter().map(|&k| keycode(x, k)).collect::<Option<Vec<u8>>>() else { return false };
    let fake = |kind: u8, code: u8| x.conn.xtest_fake_input(kind, code, 0, x.root, 0, 0, 0).is_ok();
    let pressed = codes.iter().all(|&c| fake(KEY_PRESS_EVENT, c));
    let released = codes.iter().rev().fold(true, |ok, &c| fake(KEY_RELEASE_EVENT, c) && ok);
    pressed && released && x.conn.sync().is_ok()
}

/// Linux's paste: Shift+Insert, which text boxes and terminals alike take (Ctrl+V types ^V in a terminal). The text
/// sits on both the clipboard and the selection (`set_clipboard`) because some terminals paste the selection.
/// An X11 window in front gets it through XTest, a Wayland app through ivy-keys.
pub fn press_paste() -> bool {
    if active_window() != 0 {
        x11_press(&[XK_SHIFT_L, XK_INSERT])
    } else {
        is_wayland() && helper_paste()
    }
}

/// Ctrl+Z, for Touch Up: only into an X11 window, the only kind Ivy can check is still the one it pasted into.
pub fn press_undo() -> bool {
    active_window() != 0 && x11_press(&[XK_CONTROL_L, XK_Z])
}

/// Waits up to `max` for Ctrl, Shift, Alt and Super to be let go, so they can't merge into Ivy's keys (Alt held from
/// Alt + V would turn Shift+Insert into Alt+Shift+Insert). On X11 a modifier still held after that gets a synthetic
/// release, like Windows' `release_held_modifiers`.
pub fn release_modifiers(max: Duration) {
    let start = std::time::Instant::now();
    while keys() & (CTRL | SHIFT | ALT | SUPER) != 0 && start.elapsed() < max {
        std::thread::sleep(Duration::from_millis(15));
    }
    let held = keys();
    if active_window() != 0 && held & (CTRL | SHIFT | ALT | SUPER) != 0 {
        let Some(x) = x11() else { return };
        for (bit, sym) in [(CTRL, XK_CONTROL_L), (SHIFT, XK_SHIFT_L), (ALT, XK_ALT_L), (SUPER, XK_SUPER_L)] {
            if held & bit != 0 {
                if let Some(code) = keycode(x, sym) {
                    let _ = x.conn.xtest_fake_input(KEY_RELEASE_EVENT, code, 0, x.root, 0, 0, 0);
                }
            }
        }
        let _ = x.conn.sync();
    }
}

/// arboard empties what it put on the clipboard when its last handle closes (it offers it to a clipboard manager,
/// which a default GNOME doesn't run), so Ivy keeps one handle for its whole run.
pub fn keep_clipboard() {
    if let Ok(clipboard) = arboard::Clipboard::new() {
        std::mem::forget(clipboard);
    }
}

/// The transcript on the clipboard and on the selection (Shift+Insert in xterm, kitty or Konsole pastes the
/// selection), both marked for clipboard managers to skip (KDE's password-manager hint).
pub fn set_clipboard(text: &str) -> Result<(), arboard::Error> {
    use arboard::{LinuxClipboardKind, SetExtLinux};
    let mut clipboard = arboard::Clipboard::new()?;
    let _ = clipboard.set().clipboard(LinuxClipboardKind::Primary).exclude_from_history().text(text.to_string());
    clipboard.set().exclude_from_history().text(text.to_string())
}

/// The selection from before Ivy's paste, put back afterwards like the clipboard (`SavedClipboard`).
pub struct Selection(Option<String>);

pub fn save_selection() -> Selection {
    use arboard::{GetExtLinux, LinuxClipboardKind};
    Selection(arboard::Clipboard::new().ok().and_then(|mut c| c.get().clipboard(LinuxClipboardKind::Primary).text().ok()))
}

impl Selection {
    /// Only if the selection still holds Ivy's text: the user may have selected something new since.
    pub fn restore_if(self, ivy_text: &str) {
        use arboard::{ClearExtLinux, GetExtLinux, LinuxClipboardKind, SetExtLinux};
        let Ok(mut c) = arboard::Clipboard::new() else { return };
        if c.get().clipboard(LinuxClipboardKind::Primary).text().ok().as_deref() != Some(ivy_text) {
            return;
        }
        let _ = match self.0 {
            Some(previous) => c.set().clipboard(LinuxClipboardKind::Primary).text(previous),
            None => c.clear_with().clipboard(LinuxClipboardKind::Primary),
        };
    }
}

/// Laptop on battery: a battery (not a mouse's) is discharging and no charger is online (/sys/class/power_supply).
pub fn on_battery() -> bool {
    let Ok(supplies) = std::fs::read_dir("/sys/class/power_supply") else { return false };
    let (mut discharging, mut powered) = (false, false);
    for supply in supplies.flatten() {
        let read = |name: &str| std::fs::read_to_string(supply.path().join(name)).unwrap_or_default().trim().to_string();
        if read("scope") == "Device" {
            continue; // a wireless mouse's or keyboard's own battery
        }
        if read("type") == "Battery" {
            discharging |= read("status") == "Discharging";
        } else {
            powered |= read("online") == "1";
        }
    }
    discharging && !powered
}

/// Where ivy-keys stands. Wayland only; on X11 Ivy doesn't need it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Helper {
    /// Not started yet, or X11.
    Off,
    /// Not installed (a build from source): Linux's package puts it in /usr/libexec.
    Missing,
    /// Installed without its "input" group, so it may not open the keyboard.
    Denied,
    /// Runs, but found no keyboard (a virtual machine without one, or WSL).
    NoKeyboard,
    /// Watching the keyboard; `paste`: its virtual keyboard works too.
    Ready { paste: bool },
}

static HELPER_STATE: Mutex<Helper> = Mutex::new(Helper::Off);
static HELPER_KEYS: AtomicU8 = AtomicU8::new(0);
static HELPER_PIPE: Mutex<Option<(ChildStdin, mpsc::Receiver<bool>)>> = Mutex::new(None);
static HELPER_LOG: OnceLock<Box<dyn Fn(&str) + Send + Sync>> = OnceLock::new();
static SUPERVISING: AtomicBool = AtomicBool::new(false);

pub fn helper() -> Helper {
    *HELPER_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn set_helper(state: Helper) {
    *HELPER_STATE.lock().unwrap_or_else(|e| e.into_inner()) = state;
}

fn log(line: &str) {
    if let Some(log) = HELPER_LOG.get() {
        log(line);
    }
}

/// Where the Linux package installs it, else beside Ivy's own program (a build from source).
fn helper_path() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join("ivy-keys")));
    [PathBuf::from("/usr/libexec/ivy-keys")].into_iter().chain(beside).find(|p| p.is_file())
}

/// Wayland: runs ivy-keys for as long as Ivy runs. `log` (debug.log) gets each change.
pub fn start_helper(log: impl Fn(&str) + Send + Sync + 'static) {
    if is_wayland() {
        let _ = HELPER_LOG.set(Box::new(log));
        supervise();
    }
}

/// Starts ivy-keys, and again whenever it stops, unless it can't work (not installed, no rights, no keyboard).
fn supervise() {
    if SUPERVISING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let mut wait = 1;
        loop {
            let Some(path) = helper_path() else {
                set_helper(Helper::Missing);
                log("Wayland: Ivy's keyboard helper (ivy-keys) isn't installed");
                break;
            };
            run_helper(&path);
            if matches!(helper(), Helper::Missing | Helper::Denied | Helper::NoKeyboard) {
                break; // starting it again won't change that; `repair_helper` may
            }
            std::thread::sleep(Duration::from_secs(wait));
            wait = (wait * 2).min(30);
        }
        SUPERVISING.store(false, Ordering::SeqCst);
    });
}

/// "Fix keyboard access" (Settings, setup wizard): the Linux package's own repair (ivy-keys' group, the udev rule
/// for the virtual keyboard), run as root through the system's password prompt, then a fresh ivy-keys.
pub fn repair_helper() {
    let fix = std::path::Path::new("/usr/libexec/ivy-keys-setup");
    if fix.is_file() && Command::new("pkexec").arg(fix).status().is_ok_and(|s| s.success()) {
        log("Wayland: keyboard access repaired");
    }
    // Closing its input stops the running helper; the supervisor (or a new one) starts it again.
    *HELPER_PIPE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    set_helper(Helper::Off);
    std::thread::sleep(Duration::from_millis(300));
    supervise();
}

fn run_helper(path: &std::path::Path) {
    let child = Command::new(path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            log(&format!("Wayland: ivy-keys didn't start ({e})"));
            set_helper(Helper::Missing);
            return;
        }
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return };
    let (replies, answers) = mpsc::channel();
    *HELPER_PIPE.lock().unwrap_or_else(|e| e.into_inner()) = Some((stdin, answers));
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["ready", devices, denied, uinput] => {
                let state = match (*devices, *denied) {
                    ("0", "1") => Helper::Denied,
                    ("0", _) => Helper::NoKeyboard,
                    _ => Helper::Ready { paste: *uinput == "1" },
                };
                set_helper(state);
                log(&format!("Wayland: ivy-keys {state:?} ({devices} keyboards and mice)"));
            }
            ["keys", bits] => HELPER_KEYS.store(bits.parse().unwrap_or(0), Ordering::Relaxed),
            ["pasted", sent] => {
                let _ = replies.send(*sent == "1");
            }
            _ => {}
        }
    }
    *HELPER_PIPE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    HELPER_KEYS.store(0, Ordering::Relaxed);
    let _ = child.wait();
    if matches!(helper(), Helper::Ready { .. }) {
        set_helper(Helper::Off);
        log("Wayland: ivy-keys stopped");
    }
}

fn helper_paste() -> bool {
    let mut pipe = HELPER_PIPE.lock().unwrap_or_else(|e| e.into_inner());
    let Some((stdin, answers)) = pipe.as_mut() else { return false };
    while answers.try_recv().is_ok() {} // a late answer to an earlier paste
    if writeln!(stdin, "paste").and_then(|_| stdin.flush()).is_err() {
        return false;
    }
    answers.recv_timeout(Duration::from_millis(500)).unwrap_or(false)
}

/// Whether Ivy can watch its keys and paste on this desktop: always on X11; on Wayland once ivy-keys works.
pub fn keyboard_ready() -> bool {
    !is_wayland() || helper() == Helper::Ready { paste: true }
}

/// The GPU llama.cpp would use (Vulkan), e.g. "NVIDIA GeForce RTX 4060 Laptop GPU"; None without one. Software
/// Vulkan (llvmpipe) doesn't count: llama.cpp skips it and runs on the CPU.
pub fn gpu() -> Option<&'static str> {
    static GPU: OnceLock<Option<String>> = OnceLock::new();
    GPU.get_or_init(|| {
        llama_cpp_2::list_llama_ggml_backend_devices()
            .into_iter()
            .find(|d| {
                matches!(d.device_type, llama_cpp_2::LlamaBackendDeviceType::Gpu | llama_cpp_2::LlamaBackendDeviceType::IntegratedGpu)
            })
            .map(|d| d.description)
    })
    .as_deref()
}
