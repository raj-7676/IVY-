import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronDown, Check, Cpu, Zap, Sparkles, Activity, RefreshCw } from 'lucide-react';
import { SettingsConfig, HardwareMode, HardwareStatus } from '../types';
import { ModeMatrix, modeCombo } from './ModeMatrix';

interface SettingsViewProps {
  settings: SettingsConfig;
  onUpdateSettings: (newSettings: Partial<SettingsConfig>) => void;
  onReplayOnboarding?: () => void;
}

const ACCENT_RGB = '255, 107, 0';

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
  const [changingHotkeyField, setChangingHotkeyField] = useState<'hotkey' | 'undoPasteHotkey' | 'manualPasteHotkey' | null>(null);
  const [micDropdownOpen, setMicDropdownOpen] = useState(false);
  const [mics, setMics] = useState<string[]>([]);
  const [micsBlocked, setMicsBlocked] = useState(false);
  const [pendingModeSwitch, setPendingModeSwitch] = useState<HardwareMode | null>(null);
  const [isRestarting, setIsRestarting] = useState(false);
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

  // Named-key spellings must match `parse_hotkey` in lib.rs exactly — a
  // mismatch here (e.g. `e.key.toUpperCase()` producing "ARROWUP" instead
  // of "ArrowUp") makes the backend fail to parse the saved string, which
  // used to unregister the real OS hotkey before discovering that (fixed
  // separately in lib.rs's `apply_hotkey`, register-before-unregister —
  // but this UI still shouldn't be able to save an unparseable spec).
  const NAMED_KEYS: Record<string, string> = {
    ' ': 'Space',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    ArrowUp: 'ArrowUp',
    ArrowDown: 'ArrowDown',
    ArrowLeft: 'ArrowLeft',
    ArrowRight: 'ArrowRight',
    CapsLock: 'Caps Lock',
  };
  const isModifierKey = (key: string) =>
    key === 'Alt' || key === 'Control' || key === 'Shift' || key === 'Meta';

  const handleHotkeyKeyDown = (e: React.KeyboardEvent) => {
    e.preventDefault();
    const field = changingHotkeyField;
    if (!field) return;
    if (e.key === 'Escape') {
      setChangingHotkeyField(null);
      return;
    }
    // A modifier alone isn't a complete hotkey yet — wait for the real key
    // instead of committing "Alt" the instant Alt goes down, which used to
    // close the capture box before the user ever reached the second key.
    if (isModifierKey(e.key)) return;

    let mainKey: string | null = null;
    if (NAMED_KEYS[e.key]) mainKey = NAMED_KEYS[e.key];
    else if (/^F([1-9]|1[0-2])$/.test(e.key)) mainKey = e.key;
    else if (/^[a-zA-Z]$/.test(e.key)) mainKey = e.key.toUpperCase();
    else if (/^[0-9]$/.test(e.key)) mainKey = e.key;
    else return; // unrecognized key (media keys, IME composition, …) — ignore, keep waiting

    const keys: string[] = [];
    if (e.altKey) keys.push('Alt');
    if (e.ctrlKey) keys.push('Ctrl');
    if (e.shiftKey) keys.push('Shift');
    if (e.metaKey) keys.push('Cmd');

    // A bare letter/digit with no modifier would register a real
    // system-wide hotkey on that character alone, swallowing every normal
    // keystroke of it anywhere on the PC — only Caps Lock is a legitimate
    // single-key trigger (Right/Left Alt can't be registered as a standalone
    // hotkey at all: the Windows RegisterHotKey backend this app uses has no
    // VK mapping for a bare modifier key, confirmed in global-hotkey 0.8.0's
    // Windows key_to_vk table — isModifierKey() above already filters Alt
    // out before this point, so this branch never actually saw it anyway).
    if (keys.length === 0 && /^[A-Z0-9]$/.test(mainKey)) return;

    keys.push(mainKey);
    onUpdateSettings({ [field]: keys.join(' + ') });
    setChangingHotkeyField(null);
  };

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

      <div className="px-8 pb-10 max-w-2xl">
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
            maxLabel="95% (dense frosted glass)"
            onChange={(v) => onUpdateSettings({ glassOpacity: v })}
          />
          <SliderRow
            title="Backdrop blur"
            value={settings.glassBlur}
            unit="px"
            min={12}
            max={50}
            minLabel="12px (clear glass)"
            maxLabel="50px (deep frost)"
            onChange={(v) => onUpdateSettings({ glassBlur: v })}
          />
        </div>
        <div className="divide-y divide-white/[0.06]">
          {/* Hotkey */}
          <Row
            title="Dictation shortcut"
            description="Hold to record, release to paste. Or double-press to start recording hands-free — press once more to stop."
          >
            {changingHotkeyField === 'hotkey' ? (
              <div
                tabIndex={0}
                autoFocus
                onKeyDown={handleHotkeyKeyDown}
                className="px-3 py-1.5 rounded-xl text-[11.5px] outline-none"
                style={{
                  backgroundColor: `rgba(${ACCENT_RGB}, 0.12)`,
                  border: `1px solid rgba(${ACCENT_RGB}, 0.4)`,
                  color: `rgb(${ACCENT_RGB})`,
                }}
              >
                Press keys… Esc to cancel
              </div>
            ) : (
              <div className="flex items-center gap-1.5">
                {settings.hotkey.split(' + ').map((part) => (
                  <kbd
                    key={part}
                    className="px-2.5 py-1 rounded-lg text-[11px] text-white/85 font-medium"
                    style={chipStyle}
                  >
                    {part}
                  </kbd>
                ))}
              </div>
            )}

            <button
              id="change-hotkey-btn"
              onClick={() => setChangingHotkeyField(changingHotkeyField === 'hotkey' ? null : 'hotkey')}
              className="px-3 py-1.5 rounded-xl bg-white/[0.06] hover:bg-white/[0.11] text-[12px] font-medium text-white/85 transition-colors duration-150"
            >
              {changingHotkeyField === 'hotkey' ? 'Cancel' : 'Change'}
            </button>
          </Row>

          {/* Undo-paste shortcut */}
          <Row
            title="Undo paste shortcut"
            description="Reverts the last thing Ivy pasted — sends Undo into that app and restores your previous clipboard."
          >
            {changingHotkeyField === 'undoPasteHotkey' ? (
              <div
                tabIndex={0}
                autoFocus
                onKeyDown={handleHotkeyKeyDown}
                className="px-3 py-1.5 rounded-xl text-[11.5px] outline-none"
                style={{
                  backgroundColor: `rgba(${ACCENT_RGB}, 0.12)`,
                  border: `1px solid rgba(${ACCENT_RGB}, 0.4)`,
                  color: `rgb(${ACCENT_RGB})`,
                }}
              >
                Press keys… Esc to cancel
              </div>
            ) : (
              <div className="flex items-center gap-1.5">
                {settings.undoPasteHotkey.split(' + ').map((part) => (
                  <kbd
                    key={part}
                    className="px-2.5 py-1 rounded-lg text-[11px] text-white/85 font-medium"
                    style={chipStyle}
                  >
                    {part}
                  </kbd>
                ))}
              </div>
            )}

            <button
              onClick={() => setChangingHotkeyField(changingHotkeyField === 'undoPasteHotkey' ? null : 'undoPasteHotkey')}
              className="px-3 py-1.5 rounded-xl bg-white/[0.06] hover:bg-white/[0.11] text-[12px] font-medium text-white/85 transition-colors duration-150"
            >
              {changingHotkeyField === 'undoPasteHotkey' ? 'Cancel' : 'Change'}
            </button>
          </Row>

          {/* Manual-paste shortcut */}
          <Row
            title="Paste held-back text shortcut"
            description="When Ivy can't find a text field, it never touches your real clipboard — it holds the transcript instead. Press this to paste it into whatever's focused now."
          >
            {changingHotkeyField === 'manualPasteHotkey' ? (
              <div
                tabIndex={0}
                autoFocus
                onKeyDown={handleHotkeyKeyDown}
                className="px-3 py-1.5 rounded-xl text-[11.5px] outline-none"
                style={{
                  backgroundColor: `rgba(${ACCENT_RGB}, 0.12)`,
                  border: `1px solid rgba(${ACCENT_RGB}, 0.4)`,
                  color: `rgb(${ACCENT_RGB})`,
                }}
              >
                Press keys… Esc to cancel
              </div>
            ) : (
              <div className="flex items-center gap-1.5">
                {settings.manualPasteHotkey.split(' + ').map((part) => (
                  <kbd
                    key={part}
                    className="px-2.5 py-1 rounded-lg text-[11px] text-white/85 font-medium"
                    style={chipStyle}
                  >
                    {part}
                  </kbd>
                ))}
              </div>
            )}

            <button
              onClick={() => setChangingHotkeyField(changingHotkeyField === 'manualPasteHotkey' ? null : 'manualPasteHotkey')}
              className="px-3 py-1.5 rounded-xl bg-white/[0.06] hover:bg-white/[0.11] text-[12px] font-medium text-white/85 transition-colors duration-150"
            >
              {changingHotkeyField === 'manualPasteHotkey' ? 'Cancel' : 'Change'}
            </button>
          </Row>

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
                )}
              </div>
            )}
          </Row>

          {/* Cleanup */}
          <Row
            title="Clean up as I speak"
            description="Strips fillers, fixes punctuation, and resolves self-corrections before pasting."
          >
            <button
              id="cleanup-pass-toggle"
              role="switch"
              aria-checked={settings.cleanupPass}
              onClick={() => onUpdateSettings({ cleanupPass: !settings.cleanupPass })}
              className="w-11 h-6 rounded-full transition-colors duration-200 relative shrink-0 p-0.5"
              style={{
                backgroundColor: settings.cleanupPass
                  ? `rgb(${ACCENT_RGB})`
                  : 'rgba(255,255,255,0.12)',
                boxShadow: settings.cleanupPass ? `0 0 14px rgba(${ACCENT_RGB}, 0.45)` : 'none',
              }}
            >
              <span
                className={`block w-5 h-5 rounded-full bg-white transition-transform duration-200 ${
                  settings.cleanupPass ? 'translate-x-5' : 'translate-x-0'
                }`}
              />
            </button>
          </Row>

          {/* Speed vs Accuracy Preference */}
          {settings.cleanupPass && (
            <Row
              title="Transcription Priority"
              description="Speed: core rules, no AI, no extra wait. Accuracy: the full 50+ rule set on CPU; on GPU, live Qwen AI plus the formatting rules. All four combinations are spelled out under Hardware Acceleration below."
            >
              <div
                className="flex items-center p-1 rounded-xl"
                style={{
                  backgroundColor: 'rgba(255, 255, 255, 0.05)',
                  border: '1px solid rgba(255, 255, 255, 0.08)',
                }}
              >
                <button
                  id="dictation-mode-speed-btn"
                  type="button"
                  onClick={() => onUpdateSettings({ dictationMode: 'speed' })}
                  className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                    settings.dictationMode === 'speed'
                      ? 'bg-[#FF6B00] text-white shadow-[0_0_12px_rgba(255,107,0,0.5)]'
                      : 'text-white/50 hover:text-white/85'
                  }`}
                  title='Core rules only. No AI, no extra wait, GPU or CPU. Corrections like "scratch that" are pasted as spoken.'
                >
                  <Zap className="w-3.5 h-3.5" />
                  <span>Speed</span>
                </button>
                <button
                  id="dictation-mode-accuracy-btn"
                  type="button"
                  onClick={() => onUpdateSettings({ dictationMode: 'accuracy' })}
                  className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                    settings.dictationMode === 'accuracy'
                      ? 'bg-[#FF6B00] text-white shadow-[0_0_12px_rgba(255,107,0,0.5)]'
                      : 'text-white/50 hover:text-white/85'
                  }`}
                  title="CPU: full 50+ rule set, no AI. GPU: live Qwen AI on every dictation plus the formatting rules — handles corrections, adds a little time."
                >
                  <Sparkles className="w-3.5 h-3.5" />
                  <span>Accuracy</span>
                </button>
              </div>
            </Row>
          )}

          {/* Compute Acceleration (GPU vs CPU) */}
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
                      ? 'VRAM Evicted (Gaming)'
                      : 'CPU Mode'}
                  </span>
                  {hardwareStatus.onBattery && (
                    <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[11px] font-medium bg-blue-500/15 text-blue-300 border border-blue-500/30">
                      On Battery — CPU Forced
                    </span>
                  )}
                </div>
                <p className="text-[12px] text-white/40 mt-1 leading-relaxed max-w-md">
                  Where speech recognition runs — DirectML works on any GPU (NVIDIA, AMD, Intel). In Accuracy mode, GPU also turns on live AI. See all four combinations below.
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
              {settings.cleanupPass ? (
                <ModeMatrix
                  dictationMode={settings.dictationMode}
                  hardwareMode={hardwareStatus.onBattery ? 'cpu' : (settings.hardwareMode || 'gpu')}
                />
              ) : (
                <p className="text-[11.5px] text-white/50 leading-relaxed">
                  Cleanup is off, so Ivy pastes the raw transcript. GPU or CPU only changes where speech recognition runs.
                </p>
              )}
            </div>

            {/* Live GPU Telemetry pill */}
            <div className="mt-3 flex items-center justify-between px-3.5 py-2 rounded-xl bg-white/[0.02] border border-white/[0.06] text-[11.5px]">
              <div className="flex items-center gap-2 text-white/60 truncate">
                <Activity className="w-3.5 h-3.5 text-white/40 shrink-0" />
                <span className="truncate">{hardwareStatus.gpuName || 'System Graphics Adapter'}</span>
              </div>
              <div className="flex items-center gap-3 shrink-0">
                <span className="text-white/40">Load:</span>
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

          {/* Smart VRAM Eviction */}
          <Row
            title="Smart VRAM Eviction (Gaming & 3D)"
            description={`Automatically unloads models from VRAM when games or 3D applications hit >= ${settings.vramEvictionThreshold}% GPU usage. Frees 100% of graphics memory while keeping dictation hotkeys active.`}
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

          {/* VRAM eviction threshold — was persisted, validated (50-100) and
              read by the backend's GPU monitor already, with no way to
              actually change it from its 90% default anywhere in this UI. */}
          {settings.smartVramEviction && (
            <SliderRow
              title="Eviction threshold"
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
            description="Run Ivy in the background the moment you sign in to Windows — no window opens, only the tray icon and the hotkey are live. Open the app anytime from the tray."
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
                    ? 'DirectML GPU Engine'
                    : 'Universal CPU Engine (0% VRAM usage)'}
                </p>
              </div>
            </div>

            <div className="mt-3">
              <p className="text-[12px] text-white/55">
                {settings.cleanupPass
                  ? `You're in ${settings.dictationMode === 'speed' ? 'Speed' : 'Accuracy'} mode, so you'll get ${modeCombo(settings.dictationMode, pendingModeSwitch).title}:`
                  : 'Cleanup is off, so only speech recognition moves.'}
              </p>
              {settings.cleanupPass && (
                <ul className="mt-1.5 flex flex-col gap-1">
                  {modeCombo(settings.dictationMode, pendingModeSwitch).points.map((point) => (
                    <li key={point} className="text-[12px] text-white/75 leading-snug">
                      • {point}
                    </li>
                  ))}
                </ul>
              )}
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
