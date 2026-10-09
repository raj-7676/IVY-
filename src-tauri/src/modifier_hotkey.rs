//! "Ctrl + Shift" as the dictation key (Yash, 2026-10-06: two choices only, Alt + Space or Ctrl + Shift).
//! Windows (and macOS) can't register a modifier-only combo as a hotkey, so a small thread looks at the keyboard
//! state every 15 ms. (A low-level keyboard hook was tried first and never fired on Yash's laptop.) Nothing is
//! swallowed: Ctrl+Shift+T and other shortcuts still work, because any other key pressed while both are
//! held cancels the dictation instead of finishing it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const SPEC: &str = "Ctrl + Shift";

static ENABLED: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
const TICK: Duration = Duration::from_millis(15);
/// Both keys must be held this long with no other key before a recording starts, so a Ctrl+Shift+X
/// shortcut never flashes the capsule. A release within it is still a tap (double-tap = hands-free).
const SETTLE: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Press,
    Release,
    Cancel,
}

/// Turns the Ctrl + Shift key on or off. The watcher starts the first time it's turned on; `on_key` runs on
/// the watcher thread.
pub fn set_enabled<F: Fn(Key) + Send + 'static>(on: bool, on_key: F) {
    ENABLED.store(on, Ordering::SeqCst);
    if on && !STARTED.swap(true, Ordering::SeqCst) {
        let _ = std::thread::Builder::new()
            .name("ivy-ctrl-shift".into())
            .spawn(move || watch(&on_key, &read_keyboard, &|| ENABLED.load(Ordering::SeqCst)));
    }
}

/// The keyboard right now: Ctrl down, Shift down, any other key or mouse button down.
#[derive(Clone, Copy, Default)]
struct Keys {
    ctrl: bool,
    shift: bool,
    other: bool,
}

fn read_keyboard() -> Keys {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetAsyncKeyState(vk: i32) -> i16;
        }
        let held = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
        // 0x10/0x11 and their left/right forms are Shift/Ctrl themselves; 0xFF means "no key".
        let other = (0x01..=0xFE).filter(|vk| !matches!(vk, 0x10 | 0x11 | 0xA0..=0xA3)).any(held);
        Keys { ctrl: held(0x11), shift: held(0x10), other }
    }
    // macOS: the same poll over the keyboard state (Control + Shift is free there too: Mac shortcuts use Command).
    #[cfg(target_os = "macos")]
    {
        let (ctrl, shift, other) = crate::macos::ctrl_shift_state();
        Keys { ctrl, shift, other }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    Keys::default()
}

/// The watcher loop. Returns only when `read` panics (tests); keyboard and switch are passed in so the
/// rules can be tested without a keyboard.
fn watch(on_key: &dyn Fn(Key), read: &dyn Fn() -> Keys, enabled: &dyn Fn() -> bool) {
    let mut spoiled = false; // another key joined in: ignore until both are let go
    loop {
        std::thread::sleep(TICK);
        if !enabled() {
            continue;
        }
        let k = read();
        if !k.ctrl && !k.shift {
            spoiled = false;
        }
        if !(k.ctrl && k.shift) || spoiled {
            continue;
        }
        if k.other {
            spoiled = true;
            continue;
        }
        // Both down: settle, then record until either is let go.
        let started = Instant::now();
        let mut released = false;
        while started.elapsed() < SETTLE {
            std::thread::sleep(TICK);
            let k = read();
            if k.other {
                spoiled = true;
                break;
            }
            if !(k.ctrl && k.shift) {
                released = true;
                break;
            }
        }
        if spoiled {
            continue;
        }
        on_key(Key::Press);
        if released {
            on_key(Key::Release);
            continue;
        }
        loop {
            std::thread::sleep(TICK);
            let k = read();
            if k.other && k.ctrl && k.shift {
                on_key(Key::Cancel);
                spoiled = true;
                break;
            }
            if !(k.ctrl && k.shift) || !enabled() {
                on_key(Key::Release);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Plays a scripted keyboard, one state per 15 ms tick, through the real watcher loop.
    fn run(script: Vec<(Keys, usize)>) -> Vec<Key> {
        let ticks: Vec<Keys> = script.into_iter().flat_map(|(k, n)| std::iter::repeat(k).take(n)).collect();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let _ = std::thread::spawn(move || {
            let pos = Mutex::new(0usize);
            let read = || {
                let mut i = pos.lock().unwrap();
                *i += 1;
                assert!(*i <= ticks.len(), "script finished"); // ends the endless loop
                ticks[*i - 1]
            };
            watch(&|key| s.lock().unwrap().push(key), &read, &|| true);
        })
        .join();
        let out = seen.lock().unwrap().clone();
        out
    }

    const BOTH: Keys = Keys { ctrl: true, shift: true, other: false };
    const CTRL: Keys = Keys { ctrl: true, shift: false, other: false };
    const NONE: Keys = Keys { ctrl: false, shift: false, other: false };
    const BOTH_T: Keys = Keys { ctrl: true, shift: true, other: true };

    #[test]
    fn hold_records_and_release_finishes() {
        assert_eq!(run(vec![(NONE, 2), (CTRL, 3), (BOTH, 60), (CTRL, 2), (NONE, 3)]), vec![Key::Press, Key::Release]);
    }

    #[test]
    fn quick_tap_is_still_a_tap() {
        assert_eq!(run(vec![(NONE, 2), (BOTH, 3), (NONE, 5)]), vec![Key::Press, Key::Release]);
    }

    #[test]
    fn shortcut_inside_the_settle_time_never_starts() {
        assert_eq!(run(vec![(NONE, 2), (BOTH, 3), (BOTH_T, 4), (BOTH, 20), (NONE, 3)]), vec![]);
    }

    #[test]
    fn shortcut_after_a_long_hold_cancels() {
        assert_eq!(run(vec![(NONE, 2), (BOTH, 30), (BOTH_T, 2), (BOTH, 10), (NONE, 3)]), vec![Key::Press, Key::Cancel]);
    }
}
