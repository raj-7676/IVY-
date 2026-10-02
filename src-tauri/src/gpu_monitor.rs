use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
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
    pub usage_percent: u32,
    pub is_evicted: bool,
}

static IS_EVICTED: AtomicBool = AtomicBool::new(false);
static LAST_EVICTION_TIME: Mutex<Option<Instant>> = Mutex::new(None);

pub fn is_vram_evicted() -> bool {
    IS_EVICTED.load(Ordering::Relaxed)
}

pub fn set_vram_evicted(evicted: bool) {
    IS_EVICTED.store(evicted, Ordering::Relaxed);
    if evicted {
        if let Ok(mut t) = LAST_EVICTION_TIME.lock() {
            *t = Some(Instant::now());
        }
    }
}

/// Queries the primary high-performance GPU adapter on Windows via DXGI.
pub fn query_gpu_telemetry() -> GpuTelemetry {
    #[cfg(windows)]
    {
        unsafe {
            if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
                let mut best_adapter: Option<(String, u64, u64, u32)> = None;
                let mut i = 0;
                while let Ok(adapter) = factory.EnumAdapters1(i) {
                    i += 1;
                    if let Ok(desc) = adapter.GetDesc1() {
                        // Skip Microsoft Basic Render Driver (software fallback)
                        if (desc.Flags & 2) != 0 {
                            continue;
                        }

                        let name = String::from_utf16_lossy(&desc.Description)
                            .trim_matches('\0')
                            .to_string();
                        let total_vram_bytes = desc.DedicatedVideoMemory;
                        let total_mb = (total_vram_bytes / (1024 * 1024)) as u64;

                        let mut used_mb = 0u64;
                        let mut pct = 0u32;

                        if let Ok(adapter3) = adapter.cast::<IDXGIAdapter3>() {
                            let mut mem_info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                            if adapter3
                                .QueryVideoMemoryInfo(
                                    0,
                                    DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
                                    &mut mem_info,
                                )
                                .is_ok()
                            {
                                used_mb = mem_info.CurrentUsage / (1024 * 1024);
                                let denom = if mem_info.Budget > 0 {
                                    mem_info.Budget
                                } else if total_vram_bytes > 0 {
                                    total_vram_bytes as u64
                                } else {
                                    1
                                };
                                pct = ((mem_info.CurrentUsage * 100) / denom).min(100) as u32;
                            }
                        }

                        // Pick the dedicated GPU with the largest VRAM
                        match &best_adapter {
                            Some((_, prev_total, _, _)) if *prev_total >= total_mb => {}
                            _ => {
                                best_adapter = Some((name, total_mb, used_mb, pct));
                            }
                        }
                    }
                }

                if let Some((name, total_mb, used_mb, pct)) = best_adapter {
                    return GpuTelemetry {
                        adapter_name: name,
                        total_vram_mb: total_mb,
                        used_vram_mb: used_mb,
                        usage_percent: pct,
                        is_evicted: is_vram_evicted(),
                    };
                }
            }
        }
    }

    GpuTelemetry {
        adapter_name: "Default Graphics Adapter".to_string(),
        total_vram_mb: 0,
        used_vram_mb: 0,
        usage_percent: 0,
        is_evicted: is_vram_evicted(),
    }
}

/// Spawns the low-overhead background thread that evaluates GPU load every 3 seconds.
/// When usage reaches or exceeds threshold_percent (e.g. 90%), triggers Smart VRAM Eviction.
pub fn start_gpu_monitor<F>(threshold_percent: u32, on_evict: F)
where
    F: Fn() + Send + Sync + 'static,
{
    std::thread::Builder::new()
        .name("ivy-gpu-monitor".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(3));
                let telemetry = query_gpu_telemetry();
                let evicted = is_vram_evicted();

                if telemetry.usage_percent >= threshold_percent {
                    if !evicted {
                        log::warn!(
                            "Ivy: High GPU usage detected ({}% >= {}%). Triggering Smart VRAM Eviction.",
                            telemetry.usage_percent,
                            threshold_percent
                        );
                        set_vram_evicted(true);
                        on_evict();
                    }
                } else if telemetry.usage_percent < 70 && evicted {
                    let can_restore = if let Ok(guard) = LAST_EVICTION_TIME.lock() {
                        guard.map_or(true, |t| t.elapsed() >= Duration::from_secs(15))
                    } else {
                        true
                    };

                    if can_restore {
                        log::info!(
                            "Ivy: GPU load normalized ({}% < 70%). Re-enabling GPU allocation.",
                            telemetry.usage_percent
                        );
                        set_vram_evicted(false);
                    }
                }
            }
        })
        .expect("failed to spawn ivy-gpu-monitor thread");
}
