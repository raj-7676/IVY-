//! ivy-keys: the one part of Ivy that may read the keyboard and type on a Wayland desktop (src/linux.rs runs it).
//!
//! Wayland lets no app watch keys or type into another app. Ivy's Linux package installs this small program
//! setgid "input", plus a udev rule that gives that group the virtual keyboard device /dev/uinput (see
//! src-tauri/linux/), the way ydotool's and keyd's helpers work. Any program may run it, so what it tells and what
//! it accepts are kept narrow, and only for the person at the computer (`at_the_keyboard`):
//! - out `ready <devices> <denied> <uinput>`: how many keyboards and mice it opened, whether one was refused (1/0),
//!   whether its virtual keyboard works (1/0).
//! - out `keys <bits>` whenever Ctrl (1), Shift (2), Alt (4) or Super (8) goes down or up, and (16) while Ctrl and
//!   Shift are both held, whether any other key or mouse button is too: all Ivy's Ctrl + Shift key needs to tell
//!   itself from a shortcut like Ctrl+Shift+T. Never which key, nor the rhythm of typing.
//! - in `paste`: presses Shift+Insert, the paste key of Linux text boxes and terminals alike; answered `pasted 1`
//!   (sent) or `pasted 0`. Its virtual keyboard has those two keys only.
//! It exits when Ivy closes its input.

#[cfg(target_os = "linux")]
fn main() {
    helper::run();
}

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
mod helper {
    use std::collections::{HashMap, HashSet};
    use std::fs::{File, OpenOptions};
    use std::io::{BufRead, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const EV_SYN: u16 = 0;
    const EV_KEY: u16 = 1;
    const KEY_LEFTCTRL: usize = 29;
    const KEY_A: usize = 30;
    const KEY_LEFTSHIFT: usize = 42;
    const KEY_RIGHTSHIFT: usize = 54;
    const KEY_LEFTALT: usize = 56;
    const KEY_RIGHTCTRL: usize = 97;
    const KEY_RIGHTALT: usize = 100;
    const KEY_INSERT: usize = 110;
    const KEY_LEFTMETA: usize = 125;
    const KEY_RIGHTMETA: usize = 126;
    const BTN_LEFT: usize = 0x110;
    const BTN_TASK: usize = 0x117;
    const MODIFIERS: [usize; 8] =
        [KEY_LEFTCTRL, KEY_RIGHTCTRL, KEY_LEFTSHIFT, KEY_RIGHTSHIFT, KEY_LEFTALT, KEY_RIGHTALT, KEY_LEFTMETA, KEY_RIGHTMETA];
    /// KEY_MAX (0x2ff) bits.
    const KEY_BYTES: usize = 0x2ff / 8 + 1;
    const NAME: &[u8] = b"Ivy virtual keyboard";
    /// The same 15 ms the Ctrl + Shift watcher polls at (src/modifier_hotkey.rs).
    const TICK: Duration = Duration::from_millis(15);

    /// Linux's _IOC request numbers (the asm-generic layout of x86-64 and arm64).
    fn ioc(dir: u64, kind: u8, nr: u8, size: usize) -> u64 {
        dir << 30 | (size as u64) << 16 | (kind as u64) << 8 | nr as u64
    }
    const READ: u64 = 2;
    const WRITE: u64 = 1;

    fn bit(bits: &[u8], n: usize) -> bool {
        bits.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
    }

    /// EVIOCGBIT: the event types (`ev` 0) or the key codes (`ev` EV_KEY) a device has.
    fn caps(f: &File, ev: u8, buf: &mut [u8]) -> bool {
        unsafe { libc::ioctl(f.as_raw_fd(), ioc(READ, b'E', 0x20 + ev, buf.len()) as _, buf.as_mut_ptr()) >= 0 }
    }

    /// A keyboard or a mouse: has keys, and letters or a left button. Ivy's own virtual keyboard is left out, so its
    /// Shift+Insert never reads as the user's.
    fn watchable(f: &File) -> bool {
        let (mut types, mut keys, mut name) = ([0u8; 4], [0u8; KEY_BYTES], [0u8; 64]);
        // EVIOCGNAME
        let named = unsafe { libc::ioctl(f.as_raw_fd(), ioc(READ, b'E', 0x06, name.len()) as _, name.as_mut_ptr()) };
        let ours = named > 0 && name.starts_with(NAME);
        !ours
            && caps(f, 0, &mut types)
            && bit(&types, EV_KEY as usize)
            && caps(f, EV_KEY as u8, &mut keys)
            && (bit(&keys, KEY_A) || bit(&keys, BTN_LEFT))
    }

    /// Opens the devices not open yet (new ones appear when a keyboard is plugged in). `skip` remembers the others
    /// (power button, lid, webcam) by device number and node time. True when one was refused: this program wasn't
    /// installed with its "input" group.
    fn scan(open: &mut HashMap<u64, File>, skip: &mut HashSet<(u64, i64)>) -> bool {
        let mut denied = false;
        let Ok(dir) = std::fs::read_dir("/dev/input") else { return false };
        for entry in dir.flatten() {
            if !entry.file_name().as_encoded_bytes().starts_with(b"event") {
                continue;
            }
            let path = entry.path();
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            let id = (meta.rdev(), meta.ctime_nsec() + meta.ctime() * 1_000_000_000);
            if open.contains_key(&id.0) || skip.contains(&id) {
                continue;
            }
            match OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(&path) {
                Ok(f) if watchable(&f) => {
                    open.insert(id.0, f);
                }
                Ok(_) => {
                    skip.insert(id);
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied = true,
                Err(_) => {}
            }
        }
        denied
    }

    /// What is held right now on every keyboard and mouse together, as the `keys` bits (EVIOCGKEY: the state
    /// itself, so no event can be missed). A device that was unplugged drops out.
    fn held(open: &mut HashMap<u64, File>) -> u8 {
        let mut all = [0u8; KEY_BYTES];
        open.retain(|_, f| {
            let mut keys = [0u8; KEY_BYTES];
            let ok = unsafe { libc::ioctl(f.as_raw_fd(), ioc(READ, b'E', 0x18, keys.len()) as _, keys.as_mut_ptr()) } >= 0;
            if ok {
                all.iter_mut().zip(keys).for_each(|(a, k)| *a |= k);
            }
            ok
        });
        let any = |codes: &[usize]| codes.iter().any(|&c| bit(&all, c));
        let (ctrl, shift) = (any(&[KEY_LEFTCTRL, KEY_RIGHTCTRL]), any(&[KEY_LEFTSHIFT, KEY_RIGHTSHIFT]));
        let other = ctrl
            && shift
            && ((1..0x100).any(|c| !MODIFIERS.contains(&c) && bit(&all, c)) || (BTN_LEFT..=BTN_TASK).any(|c| bit(&all, c)));
        ctrl as u8
            | (shift as u8) << 1
            | (any(&[KEY_LEFTALT, KEY_RIGHTALT]) as u8) << 2
            | (any(&[KEY_LEFTMETA, KEY_RIGHTMETA]) as u8) << 3
            | (other as u8) << 4
    }

    /// The virtual keyboard (uinput): Shift and Insert, nothing else.
    fn virtual_keyboard() -> std::io::Result<File> {
        let f = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open("/dev/uinput")?;
        let fd = f.as_raw_fd();
        let int = std::mem::size_of::<libc::c_int>();
        let check = |r: libc::c_int| if r < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) };
        unsafe {
            check(libc::ioctl(fd, ioc(WRITE, b'U', 100, int) as _, EV_KEY as libc::c_int))?; // UI_SET_EVBIT
            for key in [KEY_LEFTSHIFT, KEY_INSERT] {
                check(libc::ioctl(fd, ioc(WRITE, b'U', 101, int) as _, key as libc::c_int))?; // UI_SET_KEYBIT
            }
            let mut setup: libc::uinput_setup = std::mem::zeroed();
            setup.id.bustype = 0x06; // BUS_VIRTUAL
            setup.name.iter_mut().zip(NAME).for_each(|(d, s)| *d = *s as libc::c_char);
            check(libc::ioctl(fd, ioc(WRITE, b'U', 3, std::mem::size_of::<libc::uinput_setup>()) as _, &setup))?; // UI_DEV_SETUP
            check(libc::ioctl(fd, ioc(0, b'U', 1, 0) as _))?; // UI_DEV_CREATE
        }
        Ok(f)
    }

    fn send(f: &mut File, code: usize, value: i32) -> bool {
        let event = |type_: u16, code: u16, value: i32| {
            let mut e: libc::input_event = unsafe { std::mem::zeroed() };
            (e.type_, e.code, e.value) = (type_, code, value);
            e
        };
        let events = [event(EV_KEY, code as u16, value), event(EV_SYN, 0, 0)];
        let bytes = unsafe { std::slice::from_raw_parts(events.as_ptr() as *const u8, std::mem::size_of_val(&events)) };
        f.write_all(bytes).is_ok()
    }

    /// Whether the person at this computer started this helper: logind's record of the seat's active user (what
    /// sd_seat_get_active reads). Someone logged in over SSH, or a user switched away from, gets neither keys nor
    /// pastes. A record without an active user (no logind, nobody logged in at the screen) doesn't stop it.
    fn at_the_keyboard() -> bool {
        let seat = std::fs::read_to_string("/run/systemd/seats/seat0").unwrap_or_default();
        seat_allows(&seat, unsafe { libc::getuid() })
    }

    pub(crate) fn seat_allows(seat: &str, uid: u32) -> bool {
        seat.lines().find_map(|l| l.strip_prefix("ACTIVE_UID=")).map_or(true, |active| active.trim() == uid.to_string())
    }

    /// Shift+Insert, a moment apart like real keys. Shift is always let go again.
    fn paste(f: &mut File) -> bool {
        let pause = || std::thread::sleep(Duration::from_millis(12));
        let sent = send(f, KEY_LEFTSHIFT, 1) && { pause(); send(f, KEY_INSERT, 1) } && { pause(); send(f, KEY_INSERT, 0) };
        pause();
        send(f, KEY_LEFTSHIFT, 0) && sent
    }

    pub fn run() {
        let out = Arc::new(Mutex::new(std::io::stdout()));
        let say = move |line: String| {
            let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
            if writeln!(o, "{line}").and_then(|_| o.flush()).is_err() {
                std::process::exit(0); // Ivy is gone
            }
        };
        // The keyboards first: the virtual keyboard's own device node belongs to root for a moment after it appears
        // (until udev gives it the group), which would read as a refusal.
        let (mut open, mut skip) = (HashMap::new(), HashSet::new());
        let denied = scan(&mut open, &mut skip);
        let keyboard = virtual_keyboard().ok();
        let uinput = keyboard.is_some() as u8;
        let mut ready = (open.len(), denied);
        say(format!("ready {} {} {uinput}", ready.0, ready.1 as u8));

        let reply = say.clone();
        std::thread::spawn(move || {
            let mut keyboard = keyboard;
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if line.trim() == "paste" {
                    let sent = at_the_keyboard() && keyboard.as_mut().is_some_and(paste);
                    reply(format!("pasted {}", sent as u8));
                }
            }
            std::process::exit(0); // Ivy closed its end
        });

        let (mut last, mut scanned, mut here) = (u8::MAX, Instant::now(), at_the_keyboard());
        loop {
            std::thread::sleep(TICK);
            if scanned.elapsed() >= Duration::from_secs(2) {
                let denied = scan(&mut open, &mut skip);
                scanned = Instant::now();
                here = at_the_keyboard();
                // Said again when a first keyboard turns up (or the last one goes): Ivy shows that state.
                if (open.is_empty(), denied) != (ready.0 == 0, ready.1) {
                    ready = (open.len(), denied);
                    say(format!("ready {} {} {uinput}", ready.0, ready.1 as u8));
                }
            }
            let now = if here { held(&mut open) } else { 0 };
            if now != last {
                say(format!("keys {now}"));
                last = now;
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::helper::seat_allows;

    #[test]
    fn only_the_person_at_the_screen() {
        let seat = "# This is private data. Do not parse.\nIS_SEAT0=1\nACTIVE=2\nACTIVE_UID=1000\nSESSIONS=2 c1\n";
        assert!(seat_allows(seat, 1000));
        assert!(!seat_allows(seat, 1001)); // logged in over SSH, or switched away from
        assert!(seat_allows("IS_SEAT0=1\nCAN_GRAPHICAL=0\n", 1001)); // nobody at the screen (WSL, a server)
        assert!(seat_allows("", 1001)); // no logind
    }
}
