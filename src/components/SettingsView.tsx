import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ChevronDown, Check, Cpu, Zap, Activity, RefreshCw } from 'lucide-react';
import { SettingsConfig, HardwareMode, HardwareStatus } from '../types';
import { ModeMatrix, modeCombo } from './ModeMatrix';
import { useEscape } from '../utils/useEscape';
import { IS_MAC, keyLabel } from '../utils/platform';
import { PermissionRows, usePermissions } from './MacPermissions';

interface SettingsViewProps {
  settings: SettingsConfig;
  onUpdateSettings: (newSettings: Partial<SettingsConfig>) => void;
  onReplayOnboarding?: () => void;
}

const ACCENT_RGB = '255, 107, 0';
// Two choices only (Yash, 2026-10-06). Ctrl + Shift is watched by modifier_hotkey.rs.
const DICTATION_KEYS = ['Alt + Space', 'Ctrl + Shift'];

type UpdateState =
  | { s: 'idle' }
  | { s: 'checking' }
  | { s: 'current'; current: string }
  | { s: 'available'; latest: string }
  | { s: 'downloading'; pct: number }
  | { s: 'error'; msg: string };

/** Ivy goes online here only when the button is pressed (update.rs). */
const UpdatesControl: React.FC = () => {
  const [st, setSt] = useState<UpdateState>({ s: 'idle' });
  useEffect(() => {
    let off: (() => void) | undefined;
    listen<number>('ivy://update-progress', (e) => setSt({ s: 'downloading', pct: e.payload }))
      .then((f) => (off = f))
      .catch(() => {});
    return () => off?.();
  }, []);
  const check = () => {
    setSt({ s: 'checking' });
    invoke<{ current: string; latest: string; newer: boolean }>('check_for_update')
      .then((r) => setSt(r.newer ? { s: 'available', latest: r.latest } : { s: 'current', current: r.current }))
      .catch((e) => setSt({ s: 'error', msg: `Couldn't check (${e}). Try again later.` }));
  };
  const install = () => {
    setSt({ s: 'downloading', pct: 0 });
    invoke('install_update').catch((e) => setSt({ s: 'error', msg: `Update stopped: ${e}.` }));
  };
  const btn = 'px-3.5 py-1.5 rounded-xl text-[12px] font-medium transition-all';
  return (
    <div className="flex items-center gap-3">
      {st.s === 'current' && <span className="text-[12px] text-emerald-400">Up to date (v{st.current})</span>}
      {st.s === 'error' && <span className="text-[12px] text-amber-300 max-w-[220px]">{st.msg}</span>}
      {st.s === 'downloading' && (
        <span className="text-[12px] text-white/70">
          {IS_MAC
            ? `Downloading ${st.pct}%… Then drag Ivy into Applications in the window that opens.`
            : `Downloading ${st.pct}%… Ivy restarts by itself.`}
        </span>
      )}
      {st.s === 'available' ? (
        <button onClick={install} className={`${btn} text-white`} style={{ backgroundColor: `rgb(${ACCENT_RGB})` }}>
          Download & install v{st.latest}
        </button>
      ) : (
        st.s !== 'downloading' && (
          <button
            onClick={check}
            disabled={st.s === 'checking'}
            className={`${btn} text-white/80 bg-white/[0.06] hover:bg-white/[0.12] border border-white/[0.08] hover:text-white disabled:opacity-50`}
          >
            {st.s === 'checking' ? 'Checking…' : 'Check for updates'}
          </button>
        )
      )}
    </div>
  );
};

const Row: React.FC<{
  title: string;
  description: string;
  children: React.ReactNode;
}> = ({ title, description, children }) => (
  <div className="flex items-start justify-between gap-6 py-5">
    <div className="min-w-0">
      <div className="text-[13.5px] font-medium text-white/90">{title}</div>
      <p className="text-[12px] text-white/40 mt-1 leading-relaxed max-w-md">{description}</p>
    </div>
    <div className="shrink-0 flex items-center gap-3">{children}</div>
  </div>
);

const SliderRow: React.FC<{
  title: string;
  value: number;
  unit: string;
  min: number;
  max: number;
  minLabel: string;
  maxLabel: string;
  onChange: (value: number) => void;
}> = ({ title, value, unit, min, max, minLabel, maxLabel, onChange }) => (
  <div className="py-5">
    <div className="flex items-baseline justify-between">
      <span className="text-[13.5px] font-medium text-white/90">{title}</span>
      <span className="text-[12.5px] font-mono tabular" style={{ color: `rgb(${ACCENT_RGB})` }}>
        {value}
        {unit}
      </span>
    </div>
    <input
      type="range"
      min={min}
      max={max}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
      className="w-full mt-3 accent-current"
      style={{ accentColor: `rgb(${ACCENT_RGB})` }}
    />
    <div className="flex items-center justify-between mt-1.5 text-[11px] text-white/30">
      <span>{minLabel}</span>
      <span>{maxLabel}</span>
    </div>
  </div>
);

export const SettingsView: React.FC<SettingsViewProps> = ({
  settings,
  onUpdateSettings,
  onReplayOnboarding,
}) => {
  // Which hotkey field is being captured, if any — `null` means neither
  // capture box is open. Shared so only one capture can be active at a time.
  const [micDropdownOpen, setMicDropdownOpen] = useState(false);
  const [mics, setMics] = useState<string[]>([]);
  const [micsBlocked, setMicsBlocked] = useState(false);
  const [pendingModeSwitch, setPendingModeSwitch] = useState<HardwareMode | null>(null);
  const [isRestarting, setIsRestarting] = useState(false);
  useEscape(micDropdownOpen, () => setMicDropdownOpen(false));
  const perms = usePermissions(IS_MAC);
  useEscape(!!pendingModeSwitch && !isRestarting, () => setPendingModeSwitch(null));
  const [hardwareStatus, setHardwareStatus] = useState<HardwareStatus>({
    activeEngine: 'cpu',
    configuredMode: settings.hardwareMode || 'gpu',
    gpuName: 'Detecting...',
    gpuUsagePercent: 0,
    vramUsedMb: 0,
    vramTotalMb: 0,
    isEvicted: false,
    onBattery: false,
  });

  const loadHardwareStatus = useCallback(async () => {
    try {
      const status = await invoke<HardwareStatus>('get_hardware_status');
      setHardwareStatus(status);
    } catch {
      // Fallback if command not ready
    }
  }, []);

  useEffect(() => {
    void loadHardwareStatus();
    const interval = setInterval(() => {
      void loadHardwareStatus();
    }, 1000);

    let batteryRef: any = null;
    let handleBatteryChange: (() => void) | null = null;

    if (typeof navigator !== 'undefined' && 'getBattery' in navigator) {
      (navigator as any)
        .getBattery()
        .then((bat: any) => {
          batteryRef = bat;
          handleBatteryChange = () => {
            void loadHardwareStatus();
          };
          bat.addEventListener('chargingchange', handleBatteryChange);
        })
        .catch(() => {});
    }

    return () => {
      clearInterval(interval);
      if (batteryRef && handleBatteryChange) {
        batteryRef.removeEventListener('chargingchange', handleBatteryChange);
      }
    };
  }, [loadHardwareStatus]);

  // Real devices from the OS, via the same cpal host the Rust backend
  // actually records from — not the browser's WebRTC device list, which
  // could name a different device than the one dictation really uses. An
  // empty list means no input device or Windows has blocked mic access for
  // the app, never invented names.
  const loadMics = useCallback(async () => {
    try {
      const names = await invoke<string[]>('list_audio_input_devices');
      setMics(names);
      setMicsBlocked(names.length === 0);
    } catch {
      setMics([]);
      setMicsBlocked(true);
    }
  }, []);

  useEffect(() => {
    void loadMics();
    // Real bug (Yash, 2026-09-23): a newly plugged-in mic didn't show up
    // until the whole app was restarted — this only ever fetched once, on
    // mount. `list_audio_input_devices` itself re-enumerates fresh from
    // cpal every call (no caching on the Rust side), so refetching here is
    // enough — no restart needed. Refresh on window focus (covers "plugged
    // it in, tabbed back to Ivy") and whenever the picker is actually opened.
    const onFocus = () => void loadMics();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [loadMics]);

  // Windows' own mic privacy prompt appears the first time cpal opens the
  // device, not through a JS permission API — retrying the real list is all
  // there is to "grant access" from this screen.
  const grantMicAccess = () => void loadMics();

  // No process restart: the STT engine already reloads itself in the
  // requested mode on the very next dictation (`stt::engine()` compares
  // the loaded engine's `is_gpu` against the setting and swaps if it
  // differs — see `src-tauri/src/stt.rs`). `apply_hardware_mode` just
  // unloads both engines now so the old one's VRAM/RAM is freed
  // immediately instead of sitting there until that next dictation.
  // A prior version called `restart_app` (kills and relaunches the whole
  // process) here, but Tauri's `app.restart()` races the single-instance
  // plugin's OS mutex on Windows: the new process can see the old one
  // still holding it and just refocus that stale window instead of
  // actually relaunching, so the "restart" sometimes silently did nothing.
  const handleConfirmModeSwitch = async () => {
    if (!pendingModeSwitch) return;
    setIsRestarting(true);
    try {
      // One save only: a second save_settings within 100ms hits the backend
      // rate limit, throws, and skips apply_hardware_mode below.
      onUpdateSettings({ hardwareMode: pendingModeSwitch });
      await invoke('apply_hardware_mode');
    } catch (e) {
      console.error('Failed to apply hardware mode:', e);
    } finally {
      setIsRestarting(false);
      setPendingModeSwitch(null);
    }
  };

  const chipStyle = {
    backgroundColor: 'rgba(255,255,255,0.05)',
    border: '1px solid rgba(255,255,255,0.09)',
  };

  return (
    <div id="screen-settings" className="flex-1 flex flex-col h-full overflow-y-auto">
      <header className="px-8 pt-7 pb-2 shrink-0">
        <h1 className="text-[22px] font-semibold tracking-tight text-white/95">Settings</h1>
        <p className="text-[12.5px] text-white/40 mt-1">How Ivy listens, writes, and pastes.</p>
      </header>

      <div className="px-8 pb-10 max-w-2xl w-full mx-auto">
        {IS_MAC && (
          <div className="mb-6">
            <div className="text-[11px] uppercase tracking-wider text-white/30 mb-2.5">Permissions</div>
            <PermissionRows perms={perms} />
          </div>
        )}
        <div className="pb-2">
          <div className="text-[11px] uppercase tracking-wider text-white/30 mb-1">Appearance</div>
        </div>
        <div className="divide-y divide-white/[0.06] mb-2">
          <SliderRow
            title="Glass opacity"
            value={settings.glassOpacity}
            unit="%"
            min={30}
            max={95}
            minLabel="30% (high transparency)"
            maxLabel="95% (nearly solid)"
            onChange={(v) => onUpdateSettings({ glassOpacity: v })}
          />
          {/* On/off, not a slider: Windows Acrylic has one fixed blur strength. Glass opacity above
              sets how much of the blurred desktop shows through. */}
          <Row
            title="Background blur"
            description="Frosted glass: the desktop behind Ivy shows through blurred. Lower Glass opacity to see more of it."
          >
            <button
              id="background-blur-toggle"
              role="switch"
              aria-checked={settings.glassBlur > 0}
              onClick={() => onUpdateSettings({ glassBlur: settings.glassBlur > 0 ? 0 : 40 })}
              className="w-11 h-6 rounded-full transition-colors duration-200 relative shrink-0 p-0.5"
              style={{
                backgroundColor: settings.glassBlur > 0 ? `rgb(${ACCENT_RGB})` : 'rgba(255,255,255,0.12)',
                boxShadow: settings.glassBlur > 0 ? `0 0 14px rgba(${ACCENT_RGB}, 0.45)` : 'none',
              }}
            >
              <span
                className={`block w-5 h-5 rounded-full bg-white transition-transform duration-200 ${
                  settings.glassBlur > 0 ? 'translate-x-5' : 'translate-x-0'
                }`}
              />
            </button>
          </Row>
        </div>
        <div className="divide-y divide-white/[0.06]">
          <Row
            title="Dictation shortcut"
            description="Hold to record, let go to paste. Press twice quickly for hands-free, and once more to stop."
          >
            <div className="flex items-center p-1 rounded-xl" style={chipStyle}>
              {DICTATION_KEYS.map((key) => {
                const picked = settings.hotkey === key;
                return (
                  <button
                    key={key}
                    onClick={() => !picked && onUpdateSettings({ hotkey: key })}
                    className={`px-3.5 py-1.5 rounded-lg text-[12px] font-medium transition-all ${
                      picked ? 'text-white' : 'text-white/45 hover:text-white/80'
                    }`}
                    style={picked ? { backgroundColor: `rgba(${ACCENT_RGB}, 0.25)`, border: `1px solid rgba(${ACCENT_RGB}, 0.5)` } : undefined}
                  >
                    {keyLabel(key)}
                  </button>
                );
              })}
            </div>
          </Row>
          {(
            [
              ['Alt + V', 'Paste held-back text', `No text box when you finished speaking? Ivy keeps your words in its own clipboard (your normal clipboard is never touched). Click where you want them and press ${keyLabel('Alt + V')}.`],
            ] as const
          ).map(([keys, title, description]) => (
            <Row key={keys} title={title} description={description}>
              <div className="flex items-center gap-1.5">
                {keyLabel(keys).split(' + ').map((part) => (
                  <kbd key={part} className="px-2.5 py-1 rounded-lg text-[11px] text-white/85 font-medium" style={chipStyle}>
                    {part}
                  </kbd>
                ))}
              </div>
            </Row>
          ))}
          {/* Microphone */}
          <Row title="Microphone" description="The input Ivy records from.">
            {micsBlocked || mics.length === 0 ? (
              <button
                onClick={grantMicAccess}
                className="px-3.5 py-2 rounded-xl text-[12px] text-white/85 hover:bg-white/[0.08] transition-colors duration-150"
                style={chipStyle}
              >
                No microphone found — retry
              </button>
            ) : (
              <div className="relative">
                <button
                  id="mic-select-dropdown"
                  onClick={() => {
                    if (!micDropdownOpen) void loadMics();
                    setMicDropdownOpen(!micDropdownOpen);
                  }}
                  className="flex items-center gap-2.5 px-3.5 py-2 rounded-xl text-[12px] text-white/85 hover:bg-white/[0.08] transition-colors duration-150 min-w-[200px] justify-between"
                  style={chipStyle}
                >
                  <span className="flex items-center gap-2 truncate">
                    <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 shrink-0" />
                    <span className="truncate">{settings.selectedMic || mics[0]}</span>
                  </span>
                  <ChevronDown className="w-3.5 h-3.5 text-white/35 shrink-0" />
                </button>

                {micDropdownOpen && (
                <>
                <div className="fixed inset-0 z-10" onClick={() => setMicDropdownOpen(false)} />
                <div
                  className="absolute z-20 top-full right-0 mt-1.5 min-w-full rounded-2xl overflow-hidden py-1"
                  style={{
                    backgroundColor: 'rgba(16, 13, 20, 0.97)',
                    border: '1px solid rgba(255,255,255,0.1)',
                    backdropFilter: 'blur(20px)',
                  }}
                >
                    {mics.map((mic) => (
                      <button
                        key={mic}
                        onClick={() => {
                          onUpdateSettings({ selectedMic: mic, availableMics: mics });
                          setMicDropdownOpen(false);
                        }}
                        className="w-full flex items-center justify-between gap-4 px-3.5 py-2 text-[12px] text-left text-white/80 hover:bg-white/[0.07] transition-colors duration-150 whitespace-nowrap"
                      >
                        <span>{mic}</span>
                        {settings.selectedMic === mic && (
                          <Check className="w-3.5 h-3.5" style={{ color: `rgb(${ACCENT_RGB})` }} />
                        )}
                      </button>
                    ))}
                  </div>
                </>
                )}
              </div>
            )}
          </Row>

          {/* A Mac has one chip and no GPU or CPU mode to pick: the model always runs on the chip's GPU (lib.rs
              should_use_gpu). */}
          {IS_MAC && (
            <Row
              title="Runs on your Mac's chip"
              description={
                hardwareStatus.gpuName !== 'Detecting...' && hardwareStatus.activeEngine === 'cpu'
                  ? "Your Mac's graphics gave wrong results in Ivy's start-up check, so this time the model runs on the chip's processor cores instead (slower). Ivy checks again the next time it starts."
                  : "Ivy's model runs on the graphics built into your Mac's chip, through Metal. There's no CPU or GPU mode to choose, and nothing to set."
              }
            >
              <span className="flex items-center gap-2 px-3.5 py-2 rounded-xl text-[12px] text-white/85" style={chipStyle}>
                <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 shrink-0" />
                {hardwareStatus.gpuName === 'Detecting...' ? 'Apple silicon' : hardwareStatus.gpuName || 'Apple silicon'}
              </span>
            </Row>
          )}

          {/* Compute Acceleration (GPU vs CPU) */}
          {!IS_MAC && (
          <div className="py-5 border-t border-white/[0.06]">
            <div className="flex items-start justify-between gap-6 mb-3">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-white/90 flex items-center gap-2">
                  <span>Hardware Acceleration</span>
                  <span
                    className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[11px] font-medium ${
                      hardwareStatus.activeEngine === 'gpu'
                        ? 'bg-emerald-500/15 text-emerald-300 border border-emerald-500/30'
                        : hardwareStatus.activeEngine === 'evicted'
                        ? 'bg-amber-500/15 text-amber-300 border border-amber-500/30'
                        : 'bg-blue-500/15 text-blue-300 border border-blue-500/30'
                    }`}
                  >
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${
                        hardwareStatus.activeEngine === 'gpu'
                          ? 'bg-emerald-400 animate-pulse'
                          : hardwareStatus.activeEngine === 'evicted'
                          ? 'bg-amber-400'
                          : 'bg-blue-400'
                      }`}
                    />
                    {hardwareStatus.activeEngine === 'gpu'
                      ? 'GPU Active'
                      : hardwareStatus.activeEngine === 'evicted'
                      ? 'On CPU (GPU busy)'
                      : 'CPU Mode'}
                  </span>
                  {hardwareStatus.onBattery && (
                    <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[11px] font-medium bg-blue-500/15 text-blue-300 border border-blue-500/30">
                      On Battery — CPU Forced
                    </span>
                  )}
                </div>
                <p className="text-[12px] text-white/40 mt-1 leading-relaxed max-w-md">
                  Where Ivy's model runs. GPU is fast on any graphics card (NVIDIA, AMD, Intel); CPU works on every PC, just slower.
                  {hardwareStatus.onBattery && ' Currently on battery, so dictation runs on CPU to save power regardless of the mode below.'}
                </p>
              </div>

              {/* Mode Switcher */}
              <div
                className="flex items-center p-1 rounded-xl bg-white/[0.04] border border-white/[0.08]"
                style={chipStyle}
              >
                {(['gpu', 'cpu'] as HardwareMode[]).map((mode) => {
                  const effectiveMode: HardwareMode = hardwareStatus.onBattery
                    ? 'cpu'
                    : (settings.hardwareMode || 'gpu');
                  const active = effectiveMode === mode;
                  const isGpuOnBattery = hardwareStatus.onBattery && mode === 'gpu';

                  return (
                    <button
                      key={mode}
                      disabled={isGpuOnBattery}
                      title={
                        isGpuOnBattery
                          ? 'GPU acceleration paused while running on battery'
                          : undefined
                      }
                      onClick={() => {
                        if (isGpuOnBattery) return;
                        if (mode !== effectiveMode) {
                          setPendingModeSwitch(mode);
                        }
                      }}
                      className={`px-3.5 py-1.5 rounded-lg text-[12px] font-medium transition-all uppercase tracking-wide ${
                        active
                          ? 'text-white shadow-sm'
                          : isGpuOnBattery
                          ? 'text-white/20 cursor-not-allowed'
                          : 'text-white/45 hover:text-white/80 cursor-pointer'
                      }`}
                      style={
                        active
                          ? {
                              backgroundColor: mode === 'gpu' ? `rgba(${ACCENT_RGB}, 0.25)` : 'rgba(59, 130, 246, 0.25)',
                              border: mode === 'gpu' ? `1px solid rgba(${ACCENT_RGB}, 0.5)` : '1px solid rgba(59, 130, 246, 0.5)',
                            }
                          : undefined
                      }
                    >
                      {mode}
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="mt-3">
              <ModeMatrix hardwareMode={hardwareStatus.onBattery ? 'cpu' : (settings.hardwareMode || 'gpu')} />
            </div>

            {/* Live GPU Telemetry pill */}
            <div className="mt-3 flex items-center justify-between px-3.5 py-2 rounded-xl bg-white/[0.02] border border-white/[0.06] text-[11.5px]">
              <div className="flex items-center gap-2 text-white/60 truncate">
                <Activity className="w-3.5 h-3.5 text-white/40 shrink-0" />
                <span className="truncate">{hardwareStatus.gpuName || 'System Graphics Adapter'}</span>
              </div>
              <div className="flex items-center gap-3 shrink-0">
                <span className="text-white/40">Other apps' load:</span>
                <span className="font-mono text-white/80 tabular-nums">
                  {hardwareStatus.gpuUsagePercent}%
                </span>
                <div className="w-16 h-1.5 bg-white/10 rounded-full overflow-hidden">
                  <div
                    className={`h-full transition-all duration-300 ${
                      hardwareStatus.gpuUsagePercent >= 90
                        ? 'bg-amber-400'
                        : hardwareStatus.gpuUsagePercent >= 60
                        ? 'bg-orange-400'
                        : 'bg-emerald-400'
                    }`}
                    style={{ width: `${Math.min(100, hardwareStatus.gpuUsagePercent)}%` }}
                  />
                </div>
              </div>
            </div>
          </div>

          )}

          {/* Smart VRAM Eviction */}
          {!IS_MAC && (
          <Row
            title="Smart GPU sharing (games, videos & 3D)"
            description={`While a game or full-screen video player is in front, Ivy sleeps: model unloaded, Alt+Space left to the game. Full-screen browsers and terminals don't count. When other programs keep the GPU at ${settings.vramEvictionThreshold}% or more, Ivy switches to CPU (a short note shows on the overlay) and returns to the GPU when it calms down.`}
          >
            <button
              id="smart-vram-eviction-toggle"
              role="switch"
              aria-checked={settings.smartVramEviction}
              onClick={() =>
                onUpdateSettings({ smartVramEviction: !settings.smartVramEviction })
              }
              className="w-11 h-6 rounded-full transition-colors duration-200 relative shrink-0 p-0.5"
              style={{
                backgroundColor: settings.smartVramEviction
                  ? `rgb(${ACCENT_RGB})`
                  : 'rgba(255,255,255,0.12)',
                boxShadow: settings.smartVramEviction
                  ? `0 0 14px rgba(${ACCENT_RGB}, 0.45)`
                  : 'none',
              }}
            >
              <span
                className={`block w-5 h-5 rounded-full bg-white transition-transform duration-200 ${
                  settings.smartVramEviction ? 'translate-x-5' : 'translate-x-0'
                }`}
              />
            </button>
          </Row>
          )}

          {/* VRAM eviction threshold — was persisted, validated (50-100) and
              read by the backend's GPU monitor already, with no way to
              actually change it from its 90% default anywhere in this UI. */}
          {!IS_MAC && settings.smartVramEviction && (
            <SliderRow
              title="GPU usage limit"
              value={settings.vramEvictionThreshold}
              unit="%"
              min={50}
              max={100}
              minLabel="50% (evict early)"
              maxLabel="100% (evict only when maxed out)"
              onChange={(v) => onUpdateSettings({ vramEvictionThreshold: v })}
            />
          )}

          {/* Launch at Startup */}
          <Row
            title="Launch at Startup"
            description={
              IS_MAC
                ? 'Run Ivy in the background as soon as you log in to your Mac — no window opens, only the menu bar icon and the hotkey are live. Open the app anytime from the menu bar or the Dock.'
                : 'Run Ivy in the background the moment you sign in to Windows — no window opens, only the tray icon and the hotkey are live. Open the app anytime from the tray.'
            }
          >
            <button
              id="launch-at-startup-toggle"
              role="switch"
              aria-checked={settings.launchAtStartup}
              onClick={() =>
                onUpdateSettings({ launchAtStartup: !settings.launchAtStartup })
              }
              className="w-11 h-6 rounded-full transition-colors duration-200 relative shrink-0 p-0.5"
              style={{
                backgroundColor: settings.launchAtStartup
                  ? `rgb(${ACCENT_RGB})`
                  : 'rgba(255,255,255,0.12)',
                boxShadow: settings.launchAtStartup
                  ? `0 0 14px rgba(${ACCENT_RGB}, 0.45)`
                  : 'none',
              }}
            >
              <span
                className={`block w-5 h-5 rounded-full bg-white transition-transform duration-200 ${
                  settings.launchAtStartup ? 'translate-x-5' : 'translate-x-0'
                }`}
              />
            </button>
          </Row>

          <Row
            title="Updates"
            description={`You're on Ivy v${__IVY_VERSION__}. Ivy checks GitHub for a newer version only when you press the button, and never sends anything about you.`}
          >
            <UpdatesControl />
          </Row>

          {/* Replay Onboarding Guide */}
          {onReplayOnboarding && (
            <Row
              title="Interactive Onboarding Guide"
              description="Re-run the first-time setup wizard, test your mic with 'Hi, my name is James', and review tips."
            >
              <button
                onClick={onReplayOnboarding}
                className="px-3.5 py-1.5 rounded-xl text-[12px] font-medium text-white/80 bg-white/[0.06] hover:bg-white/[0.12] border border-white/[0.08] transition-all hover:text-white"
              >
                Replay Tutorial
              </button>
            </Row>
          )}
        </div>

      </div>

      {/* Hardware Switch & Complete Restart Confirmation Popup */}
      {pendingModeSwitch && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/75 backdrop-blur-md animate-in fade-in duration-200">
          <div
            className="w-full max-w-md p-6 rounded-3xl border border-white/[0.14] shadow-[0_30px_70px_-15px_rgba(0,0,0,0.95)]"
            style={{
              backgroundColor: 'rgba(16, 12, 20, 0.98)',
            }}
          >
            <div className="flex items-center gap-3.5 mb-2">
              <div
                className={`w-11 h-11 rounded-2xl flex items-center justify-center shrink-0 ${
                  pendingModeSwitch === 'gpu'
                    ? 'bg-[#ff6b00]/15 border border-[#ff6b00]/30 text-[#ff6b00]'
                    : 'bg-blue-500/15 border border-blue-500/30 text-blue-400'
                }`}
              >
                {pendingModeSwitch === 'gpu' ? (
                  <Zap className="w-5 h-5" />
                ) : (
                  <Cpu className="w-5 h-5" />
                )}
              </div>
              <div>
                <h3 className="text-[15.5px] font-semibold text-white/95 tracking-tight">
                  {pendingModeSwitch === 'gpu' ? 'Switch to GPU Acceleration?' : 'Switch to CPU Mode?'}
                </h3>
                <p className="text-[11.5px] text-white/40 mt-0.5">
                  {pendingModeSwitch === 'gpu'
                    ? 'Graphics card (Vulkan)'
                    : 'Processor (no graphics memory used)'}
                </p>
              </div>
            </div>

            <div className="mt-3">
              <p className="text-[12px] text-white/55">On {modeCombo(pendingModeSwitch).title}:</p>
              <ul className="mt-1.5 flex flex-col gap-1">
                {modeCombo(pendingModeSwitch).points.map((point) => (
                  <li key={point} className="text-[12px] text-white/75 leading-snug">
                    • {point}
                  </li>
                ))}
              </ul>
            </div>

            <div className="mt-4 p-3.5 rounded-2xl bg-white/[0.04] border border-white/[0.08] flex items-start gap-3 text-[12px] text-white/65 leading-relaxed">
              <RefreshCw className="w-4 h-4 text-amber-400 shrink-0 mt-0.5" />
              <span>
                <strong className="text-white/95 font-medium">No restart needed.</strong> Ivy unloads the current model now; the next time you dictate, it loads in {pendingModeSwitch.toUpperCase()} mode.
              </span>
            </div>

            <div className="flex items-center justify-end gap-3 mt-6 pt-1">
              <button
                disabled={isRestarting}
                onClick={() => setPendingModeSwitch(null)}
                className="px-4 py-2 rounded-xl text-[12px] font-medium text-white/70 hover:text-white bg-white/[0.06] hover:bg-white/[0.1] border border-white/[0.08] transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                disabled={isRestarting}
                onClick={handleConfirmModeSwitch}
                className={`px-4 py-2 rounded-xl text-[12.5px] font-semibold text-white transition-all flex items-center gap-2 shadow-lg disabled:opacity-50 ${
                  pendingModeSwitch === 'gpu'
                    ? 'bg-[#ff6b00] hover:bg-[#ff6b00]/90 shadow-[#ff6b00]/30'
                    : 'bg-blue-600 hover:bg-blue-500 shadow-blue-500/30'
                }`}
              >
                {isRestarting ? (
                  <>
                    <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                    <span>Switching...</span>
                  </>
                ) : (
                  <>
                    <RefreshCw className="w-3.5 h-3.5" />
                    <span>Switch Mode</span>
                  </>
                )}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
