//! macOS side of Ivy's system integration (IVY.md §19): which app is in front, pressing Cmd+V in it, the two
//! permissions macOS asks for (Accessibility to type into other apps, Microphone to listen), and the capsule
//! overlay window. Windows does the same through Win32 in lib.rs.
//!
//! Plain C calls (Core Foundation, Accessibility, Core Graphics) plus a few Objective-C messages; everything
//! here is safe to call from any thread except the two `capsule_*` functions (main thread only).

use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::time::{Duration, Instant};

use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{class, msg_send};

type CFTypeRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    // Only their addresses are used.
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
    fn CFStringCreateWithCString(allocator: CFTypeRef, c_str: *const c_char, encoding: u32) -> CFTypeRef;
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFRelease(cf: CFTypeRef);
}

const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(element: CFTypeRef, attribute: CFTypeRef, value: *mut CFTypeRef) -> i32;
    fn AXUIElementGetPid(element: CFTypeRef, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> i32;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state: i32) -> *mut c_void;
    fn CGEventCreateKeyboardEvent(source: *mut c_void, key: u16, down: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CGEventSourceFlagsState(state: i32) -> u64;
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: *const AnyObject;
}

extern "C" {
    fn sysctlbyname(name: *const c_char, old: *mut c_void, old_len: *mut usize, new: *mut c_void, new_len: usize) -> i32;
}

/// A Core Foundation object Ivy created or copied, released when dropped.
struct Cf(CFTypeRef);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

fn cf_str(s: &CStr) -> Cf {
    Cf(unsafe { CFStringCreateWithCString(ptr::null(), s.as_ptr(), UTF8) })
}

/// One attribute of an accessibility element, or None (no such attribute, no permission, or the app didn't
/// answer within a quarter second: a hung app must never stall a dictation).
fn ax_attr(element: CFTypeRef, name: &CStr) -> Option<Cf> {
    if element.is_null() {
        return None;
    }
    let attr = cf_str(name);
    let mut value: CFTypeRef = ptr::null();
    let err = unsafe {
        AXUIElementSetMessagingTimeout(element, 0.25);
        AXUIElementCopyAttributeValue(element, attr.0, &mut value)
    };
    (err == 0 && !value.is_null()).then_some(Cf(value))
}

/// The text of a CFString ("" for any other kind of value). A CFString is also an NSString.
fn cf_string(value: &Cf) -> String {
    unsafe {
        if CFGetTypeID(value.0) != CFStringGetTypeID() {
            return String::new();
        }
        ns_string(&*(value.0 as *const AnyObject))
    }
}

fn ns_string(s: &AnyObject) -> String {
    let p: *const c_char = unsafe { msg_send![s, UTF8String] };
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

pub fn own_pid() -> i32 {
    std::process::id() as i32
}

/// Accessibility permission: what lets Ivy press Cmd+V in other apps and see which text box has focus.
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Shows macOS's own "Ivy would like to control this computer" prompt, which also puts Ivy in the
/// Accessibility list in System Settings (switched off until the user turns it on).
pub fn prompt_trust() -> bool {
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = Cf(CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
            ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
        ));
        AXIsProcessTrustedWithOptions(options.0) != 0
    }
}

/// Microphone permission as macOS reports it: "granted", "denied", or "ask" (not asked yet: the first
/// recording brings up macOS's own prompt).
pub fn mic_permission() -> &'static str {
    let Some(device) = AnyClass::get(c"AVCaptureDevice") else { return "granted" };
    let status: isize = unsafe { msg_send![device, authorizationStatusForMediaType: AVMediaTypeAudio] };
    match status {
        0 => "ask",
        3 => "granted",
        _ => "denied", // 1 restricted (parental controls, a company profile), 2 denied
    }
}

/// Opens System Settings › Privacy & Security at that list ("Privacy_Accessibility", "Privacy_Microphone").
pub fn open_privacy_settings(pane: &str) {
    let _ = std::process::Command::new("open")
        .arg(format!("x-apple.systempreferences:com.apple.preference.security?{pane}"))
        .spawn();
}

/// The app in front (its process id), 0 if none. With Accessibility it comes from the accessibility system,
/// which is current even while Ivy's main thread is busy; NSWorkspace only updates between main-loop turns.
pub fn frontmost_pid() -> i32 {
    if is_trusted() {
        let system = Cf(unsafe { AXUIElementCreateSystemWide() });
        if let Some(app) = ax_attr(system.0, c"AXFocusedApplication") {
            let mut pid = 0;
            if unsafe { AXUIElementGetPid(app.0, &mut pid) } == 0 && pid > 0 {
                return pid;
            }
        }
    }
    autoreleasepool(|_| unsafe {
        let workspace: Option<Retained<AnyObject>> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let Some(workspace) = workspace else { return 0 };
        let app: Option<Retained<AnyObject>> = msg_send![&*workspace, frontmostApplication];
        let Some(app) = app else { return 0 };
        let pid: i32 = msg_send![&*app, processIdentifier];
        pid
    })
}

fn running_app(pid: i32) -> Option<Retained<AnyObject>> {
    if pid <= 0 {
        return None;
    }
    unsafe { msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid] }
}

/// The app's name as macOS shows it ("Google Chrome", "Notes"); "" if unknown.
pub fn app_name(pid: i32) -> String {
    autoreleasepool(|_| unsafe {
        let Some(app) = running_app(pid) else { return String::new() };
        let name: Option<Retained<AnyObject>> = msg_send![&*app, localizedName];
        name.map_or_else(String::new, |n| ns_string(&n))
    })
}

/// The app's bundle as named on disk ("Slack.app"): what the Tone screen's app lists hold.
pub fn app_bundle_name(pid: i32) -> String {
    autoreleasepool(|_| unsafe {
        let Some(app) = running_app(pid) else { return String::new() };
        let url: Option<Retained<AnyObject>> = msg_send![&*app, bundleURL];
        let Some(url) = url else { return String::new() };
        let name: Option<Retained<AnyObject>> = msg_send![&*url, lastPathComponent];
        name.map_or_else(String::new, |n| ns_string(&n))
    })
}

/// "com.apple.finder" and the like; "" if unknown.
pub fn app_bundle_id(pid: i32) -> String {
    autoreleasepool(|_| unsafe {
        let Some(app) = running_app(pid) else { return String::new() };
        let id: Option<Retained<AnyObject>> = msg_send![&*app, bundleIdentifier];
        id.map_or_else(String::new, |i| ns_string(&i))
    })
}

/// Title of the app's focused window ("Inbox - Gmail - Google Chrome"); "" without Accessibility. Never asked of
/// Ivy itself: the asking thread may be its main thread, which would also have to answer.
pub fn window_title(pid: i32) -> String {
    if pid <= 0 || pid == own_pid() || !is_trusted() {
        return String::new();
    }
    let app = Cf(unsafe { AXUIElementCreateApplication(pid) });
    ax_attr(app.0, c"AXFocusedWindow")
        .and_then(|window| ax_attr(window.0, c"AXTitle"))
        .map_or_else(String::new, |title| cf_string(&title))
}

/// Role of the element with keyboard focus in that app ("AXTextField", "AXGroup"); None if unknown.
pub fn focused_role(pid: i32) -> Option<String> {
    if pid <= 0 || pid == own_pid() || !is_trusted() {
        return None;
    }
    let app = Cf(unsafe { AXUIElementCreateApplication(pid) });
    let focused = ax_attr(app.0, c"AXFocusedUIElement")?;
    ax_attr(focused.0, c"AXRole").map(|role| cf_string(&role))
}

/// Brings that app to the front. Allowed while Ivy is the active app itself, which is the case after a click
/// on the capsule.
pub fn activate(pid: i32) -> bool {
    autoreleasepool(|_| unsafe {
        let Some(app) = running_app(pid) else { return false };
        // NSApplicationActivateIgnoringOtherApps: needed before macOS 14, ignored (and not needed) since.
        let ok: bool = msg_send![&*app, activateWithOptions: 2usize];
        ok
    })
}

/// Whether `pid` is in front, or back in front within `wait`. A click on the capsule makes Ivy the active app;
/// the app the text is for is then brought back first. Another app in front means the user switched: no.
pub fn bring_to_front(pid: i32, wait: Duration) -> bool {
    if pid <= 0 {
        return false;
    }
    let front = frontmost_pid();
    if front == pid {
        return true;
    }
    if front == own_pid() {
        activate(pid);
    }
    let start = Instant::now();
    while start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(30));
        if frontmost_pid() == pid {
            return true;
        }
    }
    false
}

pub const KEY_V: u16 = 0x09; // kVK_ANSI_V
pub const KEY_Z: u16 = 0x06; // kVK_ANSI_Z
const KEY_COMMAND: u16 = 0x37;
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_OPTION: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;
/// kCGEventSourceStateHIDSystemState: the keys the user is physically holding.
const HID_STATE: i32 = 1;

/// Presses Cmd+`key` in the app in front: Command down, the key down and up, Command up. The events carry
/// "only Command is held", whatever the user may still be holding. Without Accessibility permission macOS
/// drops them silently, so callers check `is_trusted` first.
// ponytail: QWERTY key positions (as every Cmd+V tool sends them); on a plain Dvorak layout that position is
// another letter. Look the key up with UCKeyTranslate if a Dvorak user reports it.
pub fn press_cmd(key: u16) -> bool {
    unsafe {
        let source = CGEventSourceCreate(-1); // kCGEventSourceStatePrivate: none of the user's keys mixed in
        let steps = [(KEY_COMMAND, true, FLAG_COMMAND), (key, true, FLAG_COMMAND), (key, false, FLAG_COMMAND), (KEY_COMMAND, false, 0)];
        let events: Vec<*mut c_void> = steps.iter().map(|&(code, down, _)| CGEventCreateKeyboardEvent(source, code, down)).collect();
        let ok = events.iter().all(|e| !e.is_null());
        if ok {
            for (&event, &(_, _, flags)) in events.iter().zip(&steps) {
                CGEventSetFlags(event, flags);
                CGEventPost(0, event); // kCGHIDEventTap: as if typed on the keyboard
                std::thread::sleep(Duration::from_millis(4));
            }
        }
        for event in events.into_iter().filter(|e| !e.is_null()) {
            CFRelease(event as CFTypeRef);
        }
        if !source.is_null() {
            CFRelease(source as CFTypeRef);
        }
        ok
    }
}

/// Waits, up to `max`, until the user has let go of Option, Control, Shift and Command (still holding Option
/// from Option + Space would otherwise turn Cmd+V into Cmd+Option+V in some apps).
pub fn wait_modifiers_released(max: Duration) {
    let held = FLAG_SHIFT | FLAG_CONTROL | FLAG_OPTION | FLAG_COMMAND;
    let start = Instant::now();
    while start.elapsed() < max && unsafe { CGEventSourceFlagsState(HID_STATE) } & held != 0 {
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// For the Control + Shift dictation key: Control down, Shift down, and whether any other key or mouse button
/// is down (looked at only while both are held, so the idle poll stays one cheap call).
pub fn ctrl_shift_state() -> (bool, bool, bool) {
    let flags = unsafe { CGEventSourceFlagsState(HID_STATE) };
    let ctrl = flags & FLAG_CONTROL != 0;
    let shift = flags & FLAG_SHIFT != 0;
    let other = ctrl
        && shift
        // 0x38/0x3C Shift, 0x3B/0x3E Control, 0x39 Caps Lock (a toggle, not a held key)
        && ((0u16..=0x7E).filter(|k| !matches!(k, 0x38 | 0x3C | 0x3B | 0x3E | 0x39)).any(|k| unsafe { CGEventSourceKeyState(HID_STATE, k) })
            || (0u32..3).any(|button| unsafe { CGEventSourceButtonState(HID_STATE, button) }));
    (ctrl, shift, other)
}

/// Bundle names of the apps running with a window or a Dock icon ("Notes.app"), Ivy left out: the choices for
/// the Tone screen's "Add app".
pub fn running_apps() -> Vec<String> {
    let own = app_bundle_name(own_pid());
    let mut names = autoreleasepool(|_| unsafe {
        let mut names = Vec::new();
        let workspace: Option<Retained<AnyObject>> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let Some(workspace) = workspace else { return names };
        let apps: Option<Retained<AnyObject>> = msg_send![&*workspace, runningApplications];
        let Some(apps) = apps else { return names };
        let count: usize = msg_send![&*apps, count];
        for i in 0..count {
            let app: Option<Retained<AnyObject>> = msg_send![&*apps, objectAtIndex: i];
            let Some(app) = app else { continue };
            let policy: isize = msg_send![&*app, activationPolicy];
            if policy != 0 {
                continue; // only NSApplicationActivationPolicyRegular: no menu bar helpers or background agents
            }
            let url: Option<Retained<AnyObject>> = msg_send![&*app, bundleURL];
            let Some(url) = url else { continue };
            let name: Option<Retained<AnyObject>> = msg_send![&*url, lastPathComponent];
            if let Some(name) = name {
                names.push(ns_string(&name));
            }
        }
        names
    });
    names.retain(|n| n.ends_with(".app") && *n != own);
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// Opts Ivy out of App Nap for as long as it runs. With no window on screen macOS would otherwise slow its timers,
/// and the Control + Shift watcher (a 15 ms poll) would notice the keys late. The Mac can still sleep when idle.
pub fn disable_app_nap() {
    autoreleasepool(|_| unsafe {
        let info: Option<Retained<AnyObject>> = msg_send![class!(NSProcessInfo), processInfo];
        let reason: Option<Retained<AnyObject>> =
            msg_send![class!(NSString), stringWithUTF8String: c"Ivy listens for its dictation key".as_ptr()];
        if let (Some(info), Some(reason)) = (info, reason) {
            // NSActivityUserInitiatedAllowingIdleSystemSleep
            let token: Option<Retained<AnyObject>> =
                msg_send![&*info, beginActivityWithOptions: 0x00EF_FFFFu64, reason: &*reason];
            std::mem::forget(token); // the activity lasts as long as its token: all of Ivy's run
        }
    })
}

/// The Mac's chip ("Apple M2"), for Settings; "" if unknown.
pub fn chip_name() -> String {
    let mut buf = [0u8; 128];
    let mut len = buf.len();
    let found = unsafe {
        sysctlbyname(c"machdep.cpu.brand_string".as_ptr(), buf.as_mut_ptr().cast(), &mut len, ptr::null_mut(), 0)
    } == 0;
    if !found {
        return String::new();
    }
    CStr::from_bytes_until_nul(&buf[..len.min(buf.len())])
        .map(|s| s.to_string_lossy().trim().to_string())
        .unwrap_or_default()
}

/// Capsule overlay set-up (main thread only): on every Space and over full-screen apps, left out of window
/// cycling, above other apps' windows, and still shown while Ivy isn't the active app.
///
/// # Safety
/// `ns_window` is the capsule's NSWindow (or null), and this runs on the main thread.
pub unsafe fn capsule_setup(ns_window: *mut c_void) {
    let window = ns_window as *mut AnyObject;
    if window.is_null() {
        return;
    }
    // CanJoinAllSpaces | Stationary | IgnoresCycle | FullScreenAuxiliary
    let behavior: usize = 1 << 0 | 1 << 4 | 1 << 6 | 1 << 8;
    let _: () = msg_send![window, setCollectionBehavior: behavior];
    let _: () = msg_send![window, setLevel: 25isize]; // NSStatusWindowLevel
    let _: () = msg_send![window, setHidesOnDeactivate: false];
    let _: () = msg_send![window, setCanHide: false];
}

/// Shows the capsule without making it the key window or activating Ivy (what SW_SHOWNA does on Windows), or
/// hides it.
///
/// # Safety
/// `ns_window` is the capsule's NSWindow (or null), and this runs on the main thread.
pub unsafe fn capsule_show(ns_window: *mut c_void, visible: bool) {
    let window = ns_window as *mut AnyObject;
    if window.is_null() {
        return;
    }
    if visible {
        let _: () = msg_send![window, orderFrontRegardless];
    } else {
        let _: () = msg_send![window, orderOut: ptr::null_mut::<AnyObject>()];
    }
}
