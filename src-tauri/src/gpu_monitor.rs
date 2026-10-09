use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows::core::Interface;
#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory1,
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
};
#[cfg(windows)]
use windows::Win32::System::Power::GetSystemPowerStatus;

/// Real read of Windows' own power state (`GetSystemPowerStatus`) — true
/// when running on battery with AC unplugged. `ACLineStatus == 0` means
/// offline; `1` means online; `255` means "unknown" (desktops with no
/// battery report this), which must not be treated as "on battery" or a
/// desktop with no battery would wrongly get forced to CPU forever.
pub fn is_on_battery() -> bool {
    #[cfg(windows)]
    unsafe {
        let mut status = Default::default();
        if GetSystemPowerStatus(&mut status).is_ok() {
            return status.ACLineStatus == 0;
        }
    }
    false
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTelemetry {
    pub adapter_name: String,
    pub total_vram_mb: u64,
    pub used_vram_mb: u64,
    /// GPU load from other programs, % (see `GpuLoad`); what the eviction threshold compares against.
    pub usage_percent: u32,
    pub is_evicted: bool,
}

static IS_EVICTED: AtomicBool = AtomicBool::new(false);
/// Latest GPU load from other programs, written by the monitor thread every 3 s.
static OTHERS_LOAD: AtomicU32 = AtomicU32::new(0);

pub fn is_vram_evicted() -> bool {
    IS_EVICTED.load(Ordering::Relaxed)
}

/// The dedicated GPU (largest VRAM): name, total MB, Ivy's own VRAM use MB, and its LUID as Windows'
/// counter instance names spell it ("luid_0x00000000_0x00012b5c").
#[cfg(windows)]
fn best_adapter() -> Option<(String, u64, u64, String)> {
    unsafe {
        let factory = CreateDXGIFactory1::<IDXGIFactory1>().ok()?;
        let mut best: Option<(String, u64, u64, String)> = None;
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if (desc.Flags & 2) != 0 {
                continue; // Microsoft Basic Render Driver (software fallback)
            }
            let name = String::from_utf16_lossy(&desc.Description).trim_matches('\0').to_string();
            let total_mb = (desc.DedicatedVideoMemory / (1024 * 1024)) as u64;
            let mut used_mb = 0u64;
            if let Ok(adapter3) = adapter.cast::<IDXGIAdapter3>() {
                let mut mem = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                if adapter3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut mem).is_ok() {
                    used_mb = mem.CurrentUsage / (1024 * 1024);
                }
            }
            let luid = format!("luid_0x{:08x}_0x{:08x}", desc.AdapterLuid.HighPart as u32, desc.AdapterLuid.LowPart);
            if best.as_ref().map_or(true, |b| total_mb > b.1) {
                best = Some((name, total_mb, used_mb, luid));
            }
        }
        best
    }
}

/// For Settings: the GPU, Ivy's VRAM use, and the current load from other programs.
pub fn query_gpu_telemetry() -> GpuTelemetry {
    #[cfg(windows)]
    if let Some((name, total_mb, used_mb, _)) = best_adapter() {
        return GpuTelemetry {
            adapter_name: name,
            total_vram_mb: total_mb,
            used_vram_mb: used_mb,
            usage_percent: OTHERS_LOAD.load(Ordering::Relaxed),
            is_evicted: is_vram_evicted(),
        };
    }
    GpuTelemetry {
        adapter_name: default_adapter_name(),
        total_vram_mb: 0,
        used_vram_mb: 0,
        usage_percent: OTHERS_LOAD.load(Ordering::Relaxed),
        is_evicted: is_vram_evicted(),
    }
}

/// What Settings shows when no adapter could be read; on a Mac, the chip Ivy runs on ("Apple M2").
fn default_adapter_name() -> String {
    #[cfg(target_os = "macos")]
    {
        let chip = crate::macos::chip_name();
        if !chip.is_empty() {
            return chip;
        }
    }
    "Default Graphics Adapter".to_string()
}

/// GPU load the way Task Manager shows it, from Windows' "GPU Engine" counter (NVIDIA, AMD and Intel all
/// report it): per engine, the utilisation of every process except Ivy itself is summed, and the busiest
/// engine wins. Ivy's own transcription runs the GPU near 100% for a moment and must not evict itself.
#[cfg(windows)]
struct GpuLoad {
    query: isize,
    counter: isize,
}

#[cfg(windows)]
impl GpuLoad {
    fn open() -> Option<Self> {
        use windows::core::w;
        use windows::Win32::System::Performance::{PdhAddEnglishCounterW, PdhCollectQueryData, PdhOpenQueryW};
        unsafe {
            let mut query = 0isize;
            if PdhOpenQueryW(windows::core::PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut load = GpuLoad { query, counter: 0 };
            if PdhAddEnglishCounterW(query, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut load.counter) != 0 {
                return None; // dropping `load` closes the query
            }
            let _ = PdhCollectQueryData(query); // a rate counter needs a first sample to diff against
            Some(load)
        }
    }

    /// Load (%) on adapter `luid` from every process except `own_pid`; None if the read fails.
    fn others_percent(&self, luid: &str, own_pid: u32) -> Option<f64> {
        use std::collections::HashMap;
        use windows::Win32::System::Performance::{
            PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA,
        };
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) != PDH_MORE_DATA {
                return None;
            }
            let n = size as usize / std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>() + 1;
            let mut items: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = Vec::with_capacity(n);
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items.as_mut_ptr())) != 0 {
                return None;
            }
            items.set_len(count as usize);
            let own = format!("pid_{own_pid}_");
            let mut engines: HashMap<String, f64> = HashMap::new();
            for item in &items {
                // e.g. "pid_1234_luid_0x00000000_0x00012b5c_phys_0_eng_0_engtype_3D"
                let name = item.szName.to_string().unwrap_or_default().to_lowercase();
                if item.FmtValue.CStatus > 1 || name.starts_with(&own) || !name.contains(luid) {
                    continue;
                }
                let engine = name.split_once("_phys_").map_or(name.as_str(), |(_, e)| e).to_string();
                *engines.entry(engine).or_default() += item.FmtValue.Anonymous.doubleValue;
            }
            Some(engines.values().fold(0.0_f64, |m, &v| m.max(v)).min(100.0))
        }
    }
}

#[cfg(windows)]
impl Drop for GpuLoad {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Performance::PdhCloseQuery(self.query);
        }
    }
}

#[cfg(windows)]
/// Programs that may be full screen while the user still wants to dictate (Yash, 2026-10-06: "if I full
/// screen Brave, Ivy should not close"): browsers, terminals, editors, and the desktop/Explorer.
const KEEP_AWAKE_IN_FULL_SCREEN: &[&str] = &[
    "brave.exe", "chrome.exe", "msedge.exe", "firefox.exe", "opera.exe", "vivaldi.exe", "arc.exe", "zen.exe",
    "windowsterminal.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "conhost.exe", "wezterm-gui.exe",
    "alacritty.exe", "code.exe", "cursor.exe", "notepad.exe", "notepad++.exe", "winword.exe", "explorer.exe",
];

/// True when Windows itself reports a full-screen app in front (the signal it uses to hold back
/// notifications: a game, a full-screen video player, a slideshow) and that app isn't on the keep-awake list.
#[cfg(windows)]
fn fullscreen_app_in_front() -> bool {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let full = unsafe { SHQueryUserNotificationState() }
        .map_or(false, |s| s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE);
    // Ivy's own intro film plays full screen on first start; that's not a game to sleep for.
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    full && pid != std::process::id() && !KEEP_AWAKE_IN_FULL_SCREEN.contains(&crate::foreground_exe_name().to_lowercase().as_str())
}

/// What the monitor tells Ivy.
#[cfg_attr(not(windows), allow(dead_code))] // the monitor only runs on Windows
pub enum Event {
    /// A full-screen app came to the front: unload the model, stop listening for the hotkey, stay silent.
    Sleep,
    /// The full-screen app is gone: listen for the hotkey again and load the model back.
    Wake,
    /// Other programs pushed the GPU past the threshold: unload and dictate on CPU (load %, threshold %).
    GpuBusy(u32, u32),
}

static SLEEPING: AtomicBool = AtomicBool::new(false);

pub fn is_sleeping() -> bool {
    SLEEPING.load(Ordering::Relaxed)
}

/// Smart GPU sharing (Yash, 2026-10-06), checked every 3 s while the Settings switch is on:
/// - Rule 1: a full-screen app (game, video player) in front for 6 s -> `Event::Sleep`; when it leaves the
///   front -> `Event::Wake`. Full screen wins over rule 2.
/// - Rule 2, GPU mode only: OTHER programs keeping the GPU at or above the user's threshold (Settings,
///   50-100%) for 6 s -> `Event::GpuBusy`, and dictation stays on CPU until their load is 15 points below
///   the threshold for 15 s. One rule for every card: a strong GPU's owner can set 50%, a weak one's 100%.
/// `settings` returns (enabled, threshold %, GPU mode) live, so Settings changes apply without a restart.
/// `on_event` returns false when it couldn't act (a dictation was running); the monitor then tries again.
pub fn start_gpu_monitor<S, F>(settings: S, on_event: F)
where
    S: Fn() -> (bool, u32, bool) + Send + 'static,
    F: Fn(Event) -> bool + Send + Sync + 'static,
{
    // Windows only: full-screen detection and GPU load come from Windows' own counters. On a Mac neither rule
    // applies (Settings hides the switch there).
    #[cfg(not(windows))]
    {
        drop((settings, on_event));
        return;
    }
    #[allow(unreachable_code)]
    std::thread::Builder::new()
        .name("ivy-gpu-monitor".to_string())
        .spawn(move || {
            #[cfg(windows)]
            {
                let load = GpuLoad::open();
                if load.is_none() {
                    log::warn!("Ivy: GPU load counter unavailable; only the full-screen rule is on");
                }
                let luid = best_adapter().map(|a| a.3).unwrap_or_default();
                let own_pid = std::process::id();
                let (mut high_samples, mut low_since): (u32, Option<Instant>) = (0, None);
                let mut fullscreen_samples = 0u32;
                loop {
                    std::thread::sleep(Duration::from_secs(3));
                    let pct = load.as_ref().and_then(|l| l.others_percent(&luid, own_pid));
                    if let Some(p) = pct {
                        OTHERS_LOAD.store(p.round() as u32, Ordering::Relaxed);
                    }
                    let (enabled, threshold, gpu_mode) = settings();
                    let full = enabled && fullscreen_app_in_front();
                    fullscreen_samples = if full { fullscreen_samples + 1 } else { 0 };
                    if !is_sleeping() && fullscreen_samples >= 2 && on_event(Event::Sleep) {
                        log::info!("Ivy: a full-screen app is in front. Sleeping (model unloaded, hotkey off).");
                        SLEEPING.store(true, Ordering::Relaxed);
                    } else if is_sleeping() && !full && on_event(Event::Wake) {
                        log::info!("Ivy: full screen ended. Awake again.");
                        SLEEPING.store(false, Ordering::Relaxed);
                    }
                    if !enabled || !gpu_mode {
                        IS_EVICTED.store(false, Ordering::Relaxed);
                        continue;
                    }
                    let (Some(pct), false) = (pct, is_sleeping()) else { continue };
                    let threshold = threshold as f64;
                    if !is_vram_evicted() {
                        high_samples = if pct >= threshold { high_samples + 1 } else { 0 };
                        if high_samples >= 2 && on_event(Event::GpuBusy(pct.round() as u32, threshold as u32)) {
                            log::warn!("Ivy: other programs are using the GPU at {pct:.0}% (>= {threshold:.0}%). Dictating on CPU.");
                            IS_EVICTED.store(true, Ordering::Relaxed);
                            high_samples = 0;
                            low_since = None;
                        }
                    } else if pct < threshold - 15.0 {
                        let since = *low_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= Duration::from_secs(15) {
                            log::info!("Ivy: GPU load from other programs is down to {pct:.0}%. Using the GPU again.");
                            IS_EVICTED.store(false, Ordering::Relaxed);
                            low_since = None;
                        }
                    } else {
                        low_since = None;
                    }
                }
            }
        })
        .expect("failed to spawn ivy-gpu-monitor thread");
}
