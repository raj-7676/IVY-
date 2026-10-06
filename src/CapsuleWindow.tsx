import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { FloatingCapsule } from './components/FloatingCapsule';
import { CapsuleMode, SettingsConfig, TonePreset } from './types';
import { audioFeedback } from './services/audioFeedback';

// The always-on-top, system-wide overlay window. Every state transition is
// driven by real events from the Rust backend — the real Alt+Space global
// shortcut, and the real record→transcribe→cleanup→paste pipeline — never
// by browser keydown/keyup or a simulated timer.
export default function CapsuleWindow() {
  const [mode, setMode] = useState<CapsuleMode>('idle');
  const [activeTone, setActiveTone] = useState<TonePreset>('Standard');
  const [activeApp, setActiveApp] = useState<string>('Desktop');
  const [sessionId, setSessionId] = useState<string>('');
  const [pasted, setPasted] = useState<boolean>(true);
  // Real value from Settings, only for display in the "no text field
  // found" message — fetched once since the capsule window has no other
  // reason to hold live settings, and the hotkey rarely changes mid-session.
  const [manualPasteHotkey, setManualPasteHotkey] = useState<string>('Alt + V');
  const [touchUpStatus, setTouchUpStatus] = useState<'offer' | 'loading' | 'done' | 'clean' | 'error'>('offer');
  const idleTimer = useRef<number | undefined>(undefined);
  const [notice, setNotice] = useState('');

  // A real OS-level hide, not just rendering nothing — WebView2 can leave a
  // faint rectangular background/shadow visible even over fully transparent
  // content while the window itself stays shown (Yash: "the background is
  // in square shape" — the exact issue Friday's own Dynamic Island had
  // until it started actually hiding the window between uses).
  const goIdle = () => {
    setMode('idle');
    void invoke('hide_capsule_window');
  };

  useEffect(() => {
    const loadSettings = () => {
      invoke<SettingsConfig>('get_settings')
        .then((s) => {
          setManualPasteHotkey(s.manualPasteHotkey);
        })
        .catch(() => {});
    };
    loadSettings();

    // This window is created once at startup and never remounts (it only
    // ever shows/hides), so it re-fetches on a real settings change.
    let unlistenSettings: (() => void) | undefined;
    try {
      listen('ivy://settings-updated', loadSettings)
        .then((f) => {
          unlistenSettings = f;
        })
        .catch(() => {});
    } catch {
      /* no Tauri host */
    }
    return () => unlistenSettings?.();
  }, []);

  useEffect(() => {
    const unlistenDown = listen<{ activeApp: string; tonePreset: TonePreset }>(
      'ivy://hotkey-down',
      (e) => {
        clearTimeout(idleTimer.current);
        setActiveApp(e.payload.activeApp);
        setActiveTone(e.payload.tonePreset);
        setSessionId('');
        setMode('recording');
        audioFeedback.playStartBeep();
      }
    );
    const unlistenUp = listen('ivy://hotkey-up', () => {
      setMode('transcribing');
    });
    const unlistenUndo = listen<boolean>('ivy://undo-paste', (e) => {
      clearTimeout(idleTimer.current);
      setMode('undone');
      setPasted(e.payload);
      idleTimer.current = window.setTimeout(goIdle, 1800);
    });
    const unlistenManualPaste = listen<boolean | { pasted: boolean; activeApp?: string }>('ivy://manual-paste', (e) => {
      clearTimeout(idleTimer.current);
      setMode('manual-pasted');
      if (typeof e.payload === 'object' && e.payload !== null) {
        setPasted(e.payload.pasted);
        if (e.payload.activeApp) setActiveApp(e.payload.activeApp);
      } else {
        setPasted(Boolean(e.payload));
      }
      audioFeedback.playSuccessChime();
      idleTimer.current = window.setTimeout(goIdle, 1800);
    });
    // Fires once, 90s after a real autostart launch — see `setup()` in
    // lib.rs. Never on a manual launch or the first-ever run.
    // Smart eviction moved dictation to CPU because other programs were loading the GPU (gpu_monitor.rs).
    const unlistenGpuEvicted = listen<{ load: number; threshold: number }>('ivy://gpu-evicted', (e) => {
      clearTimeout(idleTimer.current);
      setNotice(`GPU above your ${e.payload.threshold}% limit · Ivy switched to CPU`);
      setMode('gpu-evicted');
      idleTimer.current = window.setTimeout(goIdle, 3500);
    });
    const unlistenBackgroundReady = listen('ivy://background-ready', () => {
      clearTimeout(idleTimer.current);
      setMode('launched');
      idleTimer.current = window.setTimeout(goIdle, 3200);
    });
    const unlistenComplete = listen<{
      success: boolean;
      activeApp: string;
      sessionId: string;
      pasted: boolean;
    }>('ivy://dictation-complete', (e) => {
      setActiveApp(e.payload.activeApp);
      setSessionId(e.payload.sessionId);
      setPasted(e.payload.pasted);
      setTouchUpStatus('offer');
      setMode(e.payload.success ? 'pasted' : 'failed');
      if (e.payload.success) audioFeedback.playSuccessChime();
      // A real session to retry against gets more time on screen than a
      // plain "didn't hear anything" with nothing to retry. The held-back
      // case (no text field found) gets real reading time too — it's
      // telling the user a hotkey to remember (Alt+V), not just confirming
      // a paste that already happened, so a quick glance isn't enough.
      const delay = !e.payload.success
        ? e.payload.sessionId ? 7000 : 2500
        : e.payload.pasted ? 1600 : 3000;
      // Touch Up (spell-fix) is only ever offered after a real paste —
      // never for the held-back-text case or a failed dictation. After the normal
      // "Pasted to X" confirmation window, the capsule shows the optional
      // button for a further 7s (Yash: time to let go of the hotkey and read it), then dismisses if unclicked.
      if (e.payload.success && e.payload.pasted) {
        idleTimer.current = window.setTimeout(() => {
          setMode('touch-up');
          idleTimer.current = window.setTimeout(goIdle, 7000);
        }, delay);
      } else {
        idleTimer.current = window.setTimeout(goIdle, delay);
      }
    });
    return () => {
      unlistenDown.then((f) => f());
      unlistenUp.then((f) => f());
      unlistenUndo.then((f) => f());
      unlistenManualPaste.then((f) => f());
      unlistenBackgroundReady.then((f) => f());
      unlistenGpuEvicted.then((f) => f());
      unlistenComplete.then((f) => f());
      clearTimeout(idleTimer.current);
    };
  }, []);

  // Retries against the audio already saved for this session — no
  // re-recording. Reuses the 'transcribing' visual for the retry itself.
  const retry = async () => {
    if (!sessionId) return;
    clearTimeout(idleTimer.current);
    setMode('transcribing');
    try {
      const res = await invoke<{ pasted: boolean }>('retry_transcription', { id: sessionId });
      setPasted(res.pasted);
      setMode('pasted');
      idleTimer.current = window.setTimeout(goIdle, 1600);
    } catch {
      setMode('failed');
      idleTimer.current = window.setTimeout(goIdle, 7000);
    }
  };

  const cancel = () => {
    clearTimeout(idleTimer.current);
    setMode('idle');
    void invoke('cancel_dictation');
  };

  // Touch Up: reads the session's own already-pasted transcript, asks the
  // backend to fix misspelled words (spell-check only, never rephrasing, see
  // `spellcheck.rs`), and swaps the pasted text for the result.
  // Never runs automatically; only this explicit click starts it.
  const touchUp = async () => {
    if (!sessionId) return;
    clearTimeout(idleTimer.current);
    setTouchUpStatus('loading');
    try {
      const swapped = await invoke<boolean>('touch_up_transcript', { id: sessionId });
      setTouchUpStatus(swapped ? 'done' : 'clean');
    } catch {
      setTouchUpStatus('error');
    }
    idleTimer.current = window.setTimeout(goIdle, 1800);
  };

  return (
    <FloatingCapsule
      mode={mode}
      activeTone={activeTone}
      activeApp={activeApp}
      pasted={pasted}
      manualPasteHotkey={manualPasteHotkey}
      canRetry={!!sessionId}
      touchUpStatus={touchUpStatus}
      notice={notice}
      onRetry={retry}
      onCancel={cancel}
      onTouchUp={touchUp}
    />
  );
}
