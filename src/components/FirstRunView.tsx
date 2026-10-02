import React, { useState, useEffect, useRef } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  Mic,
  ArrowRight,
  ArrowLeft,
  ShieldCheck,
  Cpu,
  Lock,
  FileText,
  ClipboardCheck,
  Info,
  Check,
  CheckCircle2,
  Zap,
  Sparkles,
  WifiOff,
  Trash2,
  Eye,
  HardDrive,
  Wand2,
} from 'lucide-react';
import { SettingsConfig, TonePreset, DictationMode, HardwareMode } from '../types';
import { StageIndicator, OnboardingStage } from './StageIndicator';
import { ModeMatrix } from './ModeMatrix';
import { HotkeyBadge } from './HotkeyBadge';
import { SoundwaveVisualizer, DictationVisualState } from './SoundwaveVisualizer';
import { audioFeedback, MicrophoneAnalyzer } from '../services/audioFeedback';
import { IvyWordmark } from './IvyWordmark';

interface FirstRunViewProps {
  onDismiss: () => void;
  onComplete: () => void;
  hotkey?: string;
  manualPasteHotkey?: string;
  dictationMode?: DictationMode;
  hardwareMode?: HardwareMode;
  onUpdateSettings?: (newSettings: Partial<SettingsConfig>) => void;
}

const TARGET_PHRASE = 'Hi, my name is James.';
// Only Accuracy + GPU (live Qwen) resolves this; every other mode pastes it as spoken.
const CORRECTION_PHRASE = 'Hi, I am James. I want to order French fries. No, no, I want a burger.';
const ACCENT_RGB = '255, 107, 0';
const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);

export const FirstRunView: React.FC<FirstRunViewProps> = ({
  onDismiss,
  onComplete,
  hotkey = 'Alt + Space',
  manualPasteHotkey = 'Alt + V',
  dictationMode = 'accuracy',
  hardwareMode = 'gpu',
  onUpdateSettings,
}) => {
  const [currentStage, setCurrentStage] = useState<OnboardingStage>(1);
  const [completedStages, setCompletedStages] = useState<Set<OnboardingStage>>(new Set());
  const [selectedMode, setSelectedMode] = useState<DictationMode>(dictationMode);
  const liveAi = selectedMode === 'accuracy' && hardwareMode === 'gpu';

  // Stage 5 (Voice Test) state machine
  const [dictationState, setDictationState] = useState<DictationVisualState>('waiting');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isKeyPressed, setIsKeyPressed] = useState<boolean>(false);
  const [recordingDurationMs, setRecordingDurationMs] = useState<number>(0);

  // Stage 5 Voice Test state
  const [boxText, setBoxText] = useState<string>('');

  // A real attempt limit per phase, not an unbounded loop: 3 genuine tries,
  // then a real terminal fail state (no silent pass, no silent stall — this
  // was a known gap, see the mic-error / 15s-hang note below).
  const MAX_ATTEMPTS = 3;
  const [attemptCount, setAttemptCount] = useState<number>(0);
  const [testExhausted, setTestExhausted] = useState<boolean>(false);

  // Stage 1 Hotkey selection
  const [selectedHotkey, setSelectedHotkey] = useState<string>(hotkey);
  const [testKeyPressed, setTestKeyPressed] = useState<boolean>(false);

  // Stage 6: a real, separate live-mic test (own state, deliberately not
  // sharing Stage 5's — that state machine already carries real complexity
  // of its own, see the mouse-up safety net and 3-attempt cap below). Same
  // real pipeline, real hotkey, real deterministic cleanup — just a different
  // demo sentence, chosen specifically to show off self-correction (IVY.md §7).
  const [demoState, setDemoState] = useState<DictationVisualState>('waiting');
  const [demoResult, setDemoResult] = useState<string>('');
  const [demoError, setDemoError] = useState<string | null>(null);
  const [demoKeyPressed, setDemoKeyPressed] = useState<boolean>(false);
  const [demoDurationMs, setDemoDurationMs] = useState<number>(0);
  const [demoAttempts, setDemoAttempts] = useState<number>(0);
  const [demoExhausted, setDemoExhausted] = useState<boolean>(false);
  const DEMO_MAX_ATTEMPTS = 3;

  // Direct DOM ref for Friday cursor spotlight glow — avoids React re-renders on mouse movement
  const spotlightRef = useRef<HTMLDivElement | null>(null);

  const micAnalyzerRef = useRef<MicrophoneAnalyzer | null>(null);
  const recordStartTimeRef = useRef<number>(0);
  const durationIntervalRef = useRef<number | null>(null);
  const autoAdvanceTimerRef = useRef<number | null>(null);
  const transcribeTimeoutRef = useRef<number | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const hasSpokenRef = useRef<boolean>(false);

  // Stage 6's own refs, mirroring Stage 5's — kept separate for the same
  // isolation reason as the state above.
  const demoMicAnalyzerRef = useRef<MicrophoneAnalyzer | null>(null);
  const demoRecordStartTimeRef = useRef<number>(0);
  const demoDurationIntervalRef = useRef<number | null>(null);
  const demoTranscribeTimeoutRef = useRef<number | null>(null);
  const demoHasSpokenRef = useRef<boolean>(false);

  // Clean up timers & mic analyzer on unmount — including a real recording
  // still in flight (navigating away from Stage 5, or dismissing onboarding
  // entirely, while the on-screen button is still held). Without this the
  // real cpal stream in Rust keeps recording indefinitely and the global
  // hotkey stays dead for the rest of the session (Rust's PENDING never
  // clears without a matching stop).
  useEffect(() => {
    return () => {
      if (durationIntervalRef.current) clearInterval(durationIntervalRef.current);
      if (autoAdvanceTimerRef.current) clearTimeout(autoAdvanceTimerRef.current);
      if (transcribeTimeoutRef.current) clearTimeout(transcribeTimeoutRef.current);
      if (micAnalyzerRef.current) micAnalyzerRef.current.stopListening();
      if (demoDurationIntervalRef.current) clearInterval(demoDurationIntervalRef.current);
      if (demoTranscribeTimeoutRef.current) clearTimeout(demoTranscribeTimeoutRef.current);
      if (demoMicAnalyzerRef.current) demoMicAnalyzerRef.current.stopListening();
      if (isTauri) invoke('stop_manual_dictation').catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Safety net for the on-screen hold-to-talk button: its own onMouseDown/
  // onMouseUp only fire for a press-and-release that both happen over the
  // button. Drag off before releasing, or Alt-Tab away while held, and
  // neither the button nor the old per-element handlers ever see the
  // release — recording (and the real mic stream) would keep running
  // forever. Window-level listeners catch every release, wherever it lands.
  useEffect(() => {
    if (dictationState !== 'listening') return;
    const stop = () => handleStopListening(false);
    window.addEventListener('mouseup', stop);
    window.addEventListener('touchend', stop);
    window.addEventListener('touchcancel', stop);
    window.addEventListener('blur', stop);
    return () => {
      window.removeEventListener('mouseup', stop);
      window.removeEventListener('touchend', stop);
      window.removeEventListener('touchcancel', stop);
      window.removeEventListener('blur', stop);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dictationState]);

  // Same safety net for Stage 6's own manual hold button.
  useEffect(() => {
    if (demoState !== 'listening') return;
    const stop = () => handleDemoStop(false);
    window.addEventListener('mouseup', stop);
    window.addEventListener('touchend', stop);
    window.addEventListener('touchcancel', stop);
    window.addEventListener('blur', stop);
    return () => {
      window.removeEventListener('mouseup', stop);
      window.removeEventListener('touchend', stop);
      window.removeEventListener('touchcancel', stop);
      window.removeEventListener('blur', stop);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [demoState]);

  // Synchronize initial hotkey prop
  useEffect(() => {
    if (hotkey) setSelectedHotkey(hotkey);
  }, [hotkey]);

  // Handle mouse move for Friday cursor spotlight glow via direct DOM manipulation (0 React re-renders)
  const handleMouseMove = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!containerRef.current || !spotlightRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    const x = e.clientX - rect.left + containerRef.current.scrollLeft;
    const y = e.clientY - rect.top + containerRef.current.scrollTop;
    spotlightRef.current.style.background = `radial-gradient(420px circle at ${x}px ${y}px, rgba(${ACCENT_RGB}, 0.12), transparent 75%)`;
    spotlightRef.current.style.opacity = '1';
  };

  const handleMouseLeave = () => {
    if (spotlightRef.current) {
      spotlightRef.current.style.opacity = '0';
    }
  };

  // The wizard runs the real hotkey pipeline for real Stage 5/6 recordings
  // (§9), but the floating capsule overlay is a separate OS window that
  // used to pop up over every wizard stage and fight the wizard's own
  // recording UI for the screen. Tell the backend to suppress it for as
  // long as this view is mounted; it un-suppresses on unmount regardless of
  // how the wizard was left (finished, skipped, or the window closed).
  useEffect(() => {
    if (!isTauri) return;
    invoke('set_wizard_active', { active: true }).catch(() => undefined);
    return () => {
      invoke('set_wizard_active', { active: false }).catch(() => undefined);
    };
  }, []);

  // Real input peak from Rust's recorder, ~20Hz while recording.
  const [micLevel, setMicLevel] = useState(0);
  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<number>('ivy://mic-level', (e) => setMicLevel(e.payload)).catch(() => undefined);
    return () => {
      unlisten.then((f) => f?.());
    };
  }, []);
  const meterBars = [0.25, 0.45, 0.7, 0.95, 1.0, 0.85, 0.65, 0.4, 0.2].map((b) =>
    Math.min(1, b * Math.sqrt(micLevel) * 2),
  );

  // Listen to Tauri backend events broadcasted from Rust.
  //
  // The unlisten handles are kept as the raw Promises themselves (not
  // assigned into a local via `.then()`) — cleanup calls `.then(f =>
  // f?.())` on the Promise directly, matching CapsuleWindow.tsx's own
  // pattern. The `.then()`-assignment version used to store the real
  // unlisten function in a plain local that stayed `undefined` until the
  // async `listen()` IPC round-trip resolved; if this effect's cleanup ran
  // before that resolved (StrictMode's mount→cleanup→mount, or any of
  // `currentStage`/`dictationState`/`selectedHotkey` changing quickly),
  // the stale listener was never actually removed — it just kept running
  // forever with its original `currentStage === 1` closure frozen true,
  // firing `handleStartListening`/`handleDictationSuccess` a second time
  // alongside whatever real stage was now showing (e.g. stage 4's demo).
  useEffect(() => {
    // Only when the wizard is the window in front — closed to the tray or
    // behind another app, a press is a real dictation for that app.
    const unlistenDown = listen<{ activeApp?: string; tonePreset?: TonePreset }>('ivy://hotkey-down', () => {
      if (currentStage === 5 && document.hasFocus()) {
        handleStartListening(true);
      }
    }).catch(() => undefined);

    const unlistenUp = listen('ivy://hotkey-up', () => {
      if (currentStage === 5) {
        handleStopListening(true);
      }
    }).catch(() => undefined);

    const unlistenComplete = listen<{
      success: boolean;
      reason?: string;
      activeApp?: string;
      sessionId?: string;
      pasted?: boolean;
      text?: string;
    }>('ivy://dictation-complete', (e) => {
      if (currentStage === 5 && dictationState !== 'waiting') {
        if (transcribeTimeoutRef.current) {
          clearTimeout(transcribeTimeoutRef.current);
          transcribeTimeoutRef.current = null;
        }
        const text = e.payload?.text?.trim() || '';
        if (e.payload?.success && text.length > 0) {
          handleDictationSuccess(text);
        } else if (e.payload?.reason === 'mic_error') {
          handleDictationFailure("Couldn't reach your microphone. Check Settings → Microphone.");
        } else {
          handleDictationFailure('No speech detected. Please hold Alt + Space and speak into your mic.');
        }
      }
    }).catch(() => undefined);

    return () => {
      unlistenDown.then((f) => f?.());
      unlistenUp.then((f) => f?.());
      unlistenComplete.then((f) => f?.());
    };
  }, [currentStage, dictationState, selectedHotkey]);

  // Same real events, gated on Stage 6 instead — the physical hotkey is a
  // single global registration in Rust, so it fires for whichever stage
  // is actually showing; this just routes it to the demo's own state.
  // Same Promise-held unlisten pattern as the effect above, for the same
  // real reason.
  useEffect(() => {
    const unlistenDown = listen('ivy://hotkey-down', () => {
      if (currentStage === 6 && document.hasFocus()) handleDemoStart(true);
    }).catch(() => undefined);

    const unlistenUp = listen('ivy://hotkey-up', () => {
      if (currentStage === 6) handleDemoStop(true);
    }).catch(() => undefined);

    const unlistenComplete = listen<{
      success: boolean;
      reason?: string;
      text?: string;
    }>('ivy://dictation-complete', (e) => {
      if (currentStage === 6 && demoState !== 'waiting') {
        if (demoTranscribeTimeoutRef.current) {
          clearTimeout(demoTranscribeTimeoutRef.current);
          demoTranscribeTimeoutRef.current = null;
        }
        const text = e.payload?.text?.trim() || '';
        if (e.payload?.success && text.length > 0) {
          handleDemoSuccess(text);
        } else if (e.payload?.reason === 'mic_error') {
          handleDemoFailure("Couldn't reach your microphone. Check Settings → Microphone.");
        } else {
          handleDemoFailure(`No speech detected. Please hold ${selectedHotkey} and speak into your mic.`);
        }
      }
    }).catch(() => undefined);

    return () => {
      unlistenDown.then((f) => f?.());
      unlistenUp.then((f) => f?.());
      unlistenComplete.then((f) => f?.());
    };
  }, [currentStage, demoState, selectedHotkey]);

  // Global keydown/keyup fallback for testing outside Tauri or in browser preview
  useEffect(() => {
    let altPressed = false;
    let spacePressed = false;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Alt' || e.altKey) altPressed = true;
      if (e.code === 'Space') spacePressed = true;

      // Same match rule everywhere a physical key press has to prove it's
      // the one actually selected — Stage 1's tactile tester used to light
      // up for ANY of Alt/Space/Control/CapsLock regardless of which
      // option was picked (Yash: picking Caps Lock but pressing plain Alt
      // "worked"). One shared expression means Stage 1, 5 and 6 can never
      // drift apart on what counts as a match again.
      const matchesSelectedHotkey =
        (selectedHotkey.includes('Alt') && selectedHotkey.includes('Space') && altPressed && spacePressed) ||
        (selectedHotkey === 'Right Alt' && e.code === 'AltRight') ||
        (selectedHotkey === 'Caps Lock' && e.code === 'CapsLock') ||
        (selectedHotkey.includes('Ctrl') && selectedHotkey.includes('Space') && e.ctrlKey && spacePressed);

      // Stage 1 tactile shortcut tester
      if (currentStage === 1) {
        if (matchesSelectedHotkey) setTestKeyPressed(true);
        return;
      }

      if (currentStage !== 5 && currentStage !== 6) return;

      if (currentStage === 5 && matchesSelectedHotkey && dictationState === 'waiting') {
        e.preventDefault();
        handleStartListening();
      } else if (currentStage === 6 && matchesSelectedHotkey && demoState === 'waiting') {
        e.preventDefault();
        handleDemoStart();
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (e.key === 'Alt') altPressed = false;
      if (e.code === 'Space') spacePressed = false;

      if (currentStage === 1) {
        setTestKeyPressed(false);
        return;
      }

      if (currentStage !== 5 && currentStage !== 6) return;

      if (currentStage === 5 && dictationState === 'listening') {
        handleStopListening();
      } else if (currentStage === 6 && demoState === 'listening') {
        handleDemoStop();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);

    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('keyup', handleKeyUp);
    };
  }, [currentStage, dictationState, demoState, selectedHotkey]);

  // Start listening state machine
  const handleStartListening = async (fromHotkey = false) => {
    if (dictationState === 'listening') return;
    if (testExhausted) return; // real cap hit — must click "Try again" first, no silent retries
    if (autoAdvanceTimerRef.current) clearTimeout(autoAdvanceTimerRef.current);
    if (transcribeTimeoutRef.current) clearTimeout(transcribeTimeoutRef.current);

    setErrorMessage(null);
    hasSpokenRef.current = false;
    setIsKeyPressed(true);
    setDictationState('listening');
    audioFeedback.playStartBeep();

    recordStartTimeRef.current = Date.now();
    durationIntervalRef.current = window.setInterval(() => {
      setRecordingDurationMs(Date.now() - recordStartTimeRef.current);
    }, 1000);

    // Fired before the mic-analyzer await below, not after — that await
    // (getUserMedia) can take tens to hundreds of ms, and `dictationState`
    // was already set to 'listening' above it, so a quick tap/release
    // inside that window used to pass `handleStopListening`'s guard and
    // send stop-before-start: a real Rust recorder then started with
    // nothing left to ever stop it, and the global hotkey stayed dead for
    // the rest of the session. This call has no dependency on `started`.
    if (!fromHotkey && isTauri) {
      invoke('start_manual_dictation').catch(() => {});
    }

    // Browser preview only. In the app the meter uses `ivy://mic-level` from
    // Rust's own stream — a second getUserMedia open of the same device
    // delayed and zeroed the real recording.
    if (isTauri) return;
    if (!micAnalyzerRef.current) {
      micAnalyzerRef.current = new MicrophoneAnalyzer();
    }
    await micAnalyzerRef.current.startListening((avg) => {
      if (avg > 0.06) {
        hasSpokenRef.current = true;
      }
    });
  };

  // Stop listening state machine -> transcribing
  const handleStopListening = (fromHotkey = false) => {
    if (dictationState !== 'listening') return;
    setIsKeyPressed(false);
    setDictationState('transcribing');
    audioFeedback.playStopBeep();

    if (durationIntervalRef.current) {
      clearInterval(durationIntervalRef.current);
      durationIntervalRef.current = null;
    }

    if (micAnalyzerRef.current) {
      micAnalyzerRef.current.stopListening();
    }

    if (!fromHotkey && isTauri) {
      invoke('stop_manual_dictation').catch(() => {});
    }

    if (!isTauri) {
      // In web browser dev preview (outside Tauri)
      window.setTimeout(() => {
        if (hasSpokenRef.current) {
          handleDictationSuccess(TARGET_PHRASE);
        } else {
          handleDictationFailure('No speech detected. Please hold Alt + Space and speak into your mic.');
        }
      }, 350);
    } else {
      // Safety net only — the real backend now answers almost instantly for
      // both silence (peak-amplitude check) and a mic that never opened
      // (mic_error, emitted at hotkey-down). This only fires if neither
      // signal ever arrives at all, and it must count as a real failed
      // attempt too, not a silent reset outside the attempt counter.
      transcribeTimeoutRef.current = window.setTimeout(() => {
        setDictationState((curr) => {
          if (curr === 'transcribing') {
            handleDictationFailure("Ivy didn't respond in time. Please try again.");
          }
          return curr;
        });
      }, 15000);
    }
  };

  // Dictation failure trigger — every failure, from any path, counts toward
  // the same real MAX_ATTEMPTS cap. After the cap, this is a genuine
  // terminal state: no auto-reset, no silent pass, just Retry or Skip.
  const handleDictationFailure = (msg: string) => {
    if (autoAdvanceTimerRef.current) clearTimeout(autoAdvanceTimerRef.current);
    if (transcribeTimeoutRef.current) clearTimeout(transcribeTimeoutRef.current);

    const nextAttempt = attemptCount + 1;
    setAttemptCount(nextAttempt);

    if (nextAttempt >= MAX_ATTEMPTS) {
      setDictationState('failed');
      setErrorMessage(msg);
      setTestExhausted(true);
      return;
    }
    setDictationState('failed');
    setErrorMessage(`${msg} (attempt ${nextAttempt} of ${MAX_ATTEMPTS})`);

    // After 2.5s, restore to waiting state so user can try again
    window.setTimeout(() => {
      setDictationState((curr) => (curr === 'failed' ? 'waiting' : curr));
    }, 2500);
  };

  // Explicit, visible reset after the real attempt cap is hit — the user
  // chooses to try again, rather than it happening automatically.
  const handleRetryAfterExhausted = () => {
    setAttemptCount(0);
    setTestExhausted(false);
    setErrorMessage(null);
    setDictationState('waiting');
  };

  // Dictation success trigger — sets boxText and marks Stage 5 (Voice Test) complete
  const handleDictationSuccess = (text: string) => {
    if (autoAdvanceTimerRef.current) clearTimeout(autoAdvanceTimerRef.current);
    if (transcribeTimeoutRef.current) clearTimeout(transcribeTimeoutRef.current);
    setErrorMessage(null);
    setDictationState('success');
    setBoxText(text);
    audioFeedback.playSuccessChime();
    setCompletedStages((prev) => new Set(prev).add(5));

    // After 2.5s auto-advance to Stage 6, or user can click Next Step immediately
    autoAdvanceTimerRef.current = window.setTimeout(() => {
      setCurrentStage(6);
      autoAdvanceTimerRef.current = null;
    }, 2500);
  };

  // Skip test handler
  // Both Stage 5's and Stage 6's `dictation-complete` handlers are gated
  // on `currentStage`, but their 15s safety timeouts are armed
  // unconditionally and only ever cleared from inside those same gated
  // handlers — navigate away (Back, Next, Skip, or the StageIndicator)
  // while a real transcription is in flight and the result that arrives
  // gets silently discarded (wrong stage now), then 15s later the
  // now-stale timeout fires a false "didn't respond in time", burning a
  // real attempt for a dictation that actually succeeded.
  const clearNavigationTimers = () => {
    if (autoAdvanceTimerRef.current) clearTimeout(autoAdvanceTimerRef.current);
    if (transcribeTimeoutRef.current) {
      clearTimeout(transcribeTimeoutRef.current);
      transcribeTimeoutRef.current = null;
    }
    if (demoTranscribeTimeoutRef.current) {
      clearTimeout(demoTranscribeTimeoutRef.current);
      demoTranscribeTimeoutRef.current = null;
    }
  };

  const handleSkipTest = () => {
    clearNavigationTimers();
    setDictationState('success');
    setCompletedStages((prev) => new Set(prev).add(5));
    setCurrentStage(6);
  };

  // Navigation handlers
  const goToNextStage = () => {
    clearNavigationTimers();
    if (currentStage === 1) {
      setCompletedStages((prev) => new Set(prev).add(1));
      setCurrentStage(2);
    } else if (currentStage === 2) {
      setCompletedStages((prev) => new Set(prev).add(2));
      setCurrentStage(3);
    } else if (currentStage === 3) {
      setCompletedStages((prev) => new Set(prev).add(3));
      setCurrentStage(4);
    } else if (currentStage === 4) {
      setCompletedStages((prev) => new Set(prev).add(4));
      setCurrentStage(5);
    } else if (currentStage === 5) {
      setCompletedStages((prev) => new Set(prev).add(5));
      setCurrentStage(6);
    } else if (currentStage === 6) {
      setCompletedStages((prev) => new Set(prev).add(6));
      setCurrentStage(7);
    } else if (currentStage === 7) {
      setCompletedStages((prev) => new Set(prev).add(7));
      onComplete();
    }
  };

  const goToPrevStage = () => {
    clearNavigationTimers();
    if (currentStage === 2) setCurrentStage(1);
    if (currentStage === 3) setCurrentStage(2);
    if (currentStage === 4) setCurrentStage(3);
    if (currentStage === 5) setCurrentStage(4);
    if (currentStage === 6) setCurrentStage(5);
    if (currentStage === 7) setCurrentStage(6);
  };

  // Stage 6: start listening state machine — real mic, real pipeline,
  // same shape as Stage 5's `handleStartListening` but its own state.
  const handleDemoStart = async (fromHotkey = false) => {
    if (demoState === 'listening') return;
    if (demoExhausted) return;
    if (demoTranscribeTimeoutRef.current) clearTimeout(demoTranscribeTimeoutRef.current);

    setDemoError(null);
    demoHasSpokenRef.current = false;
    setDemoKeyPressed(true);
    setDemoState('listening');
    audioFeedback.playStartBeep();

    demoRecordStartTimeRef.current = Date.now();
    demoDurationIntervalRef.current = window.setInterval(() => {
      setDemoDurationMs(Date.now() - demoRecordStartTimeRef.current);
    }, 1000);

    // Same real ordering fix as `handleStartListening` — before the
    // mic-analyzer await, not after.
    if (!fromHotkey && isTauri) {
      invoke('start_manual_dictation').catch(() => {});
    }

    if (isTauri) return; // see handleStartListening
    if (!demoMicAnalyzerRef.current) {
      demoMicAnalyzerRef.current = new MicrophoneAnalyzer();
    }
    await demoMicAnalyzerRef.current.startListening((avg) => {
      if (avg > 0.06) demoHasSpokenRef.current = true;
    });
  };

  const handleDemoStop = (fromHotkey = false) => {
    if (demoState !== 'listening') return;
    setDemoKeyPressed(false);
    setDemoState('transcribing');
    audioFeedback.playStopBeep();

    if (demoDurationIntervalRef.current) {
      clearInterval(demoDurationIntervalRef.current);
      demoDurationIntervalRef.current = null;
    }
    if (demoMicAnalyzerRef.current) demoMicAnalyzerRef.current.stopListening();
    if (!fromHotkey && isTauri) {
      invoke('stop_manual_dictation').catch(() => {});
    }

    if (!isTauri) {
      // Browser dev-preview only — no real backend to actually resolve the
      // correction, so this fakes the already-corrected result. Never the
      // path the shipped app takes (see `isTauri`).
      window.setTimeout(() => {
        if (demoHasSpokenRef.current) {
          handleDemoSuccess('Hi, I am James. I want to order a burger.');
        } else {
          handleDemoFailure(`No speech detected. Please hold ${selectedHotkey} and speak into your mic.`);
        }
      }, 350);
    } else {
      demoTranscribeTimeoutRef.current = window.setTimeout(() => {
        setDemoState((curr) => {
          if (curr === 'transcribing') {
            handleDemoFailure("Ivy didn't respond in time. Please try again.");
          }
          return curr;
        });
      }, 15000);
    }
  };

  const handleDemoFailure = (msg: string) => {
    if (demoTranscribeTimeoutRef.current) clearTimeout(demoTranscribeTimeoutRef.current);
    const next = demoAttempts + 1;
    setDemoAttempts(next);
    if (next >= DEMO_MAX_ATTEMPTS) {
      setDemoState('failed');
      setDemoError(msg);
      setDemoExhausted(true);
      return;
    }
    setDemoState('failed');
    setDemoError(`${msg} (attempt ${next} of ${DEMO_MAX_ATTEMPTS})`);
    window.setTimeout(() => {
      setDemoState((curr) => (curr === 'failed' ? 'waiting' : curr));
    }, 2500);
  };

  const handleDemoRetryAfterExhausted = () => {
    setDemoAttempts(0);
    setDemoExhausted(false);
    setDemoError(null);
    setDemoState('waiting');
  };

  const handleDemoSuccess = (text: string) => {
    if (demoTranscribeTimeoutRef.current) clearTimeout(demoTranscribeTimeoutRef.current);
    setDemoError(null);
    setDemoState('success');
    setDemoResult(text);
    audioFeedback.playSuccessChime();
    setCompletedStages((prev) => new Set(prev).add(6));
  };

  const handleDemoSkip = () => {
    clearNavigationTimers();
    setDemoState('waiting');
    setCompletedStages((prev) => new Set(prev).add(6));
    setCurrentStage(7);
  };

  return (
    <div
      ref={containerRef}
      id="screen-first-run"
      onMouseMove={handleMouseMove}
      onMouseLeave={handleMouseLeave}
      className="relative flex-1 flex flex-col items-center justify-between h-full p-4 sm:p-6 md:p-8 select-none overflow-y-auto overflow-x-hidden"
    >
      {/* Friday Interactive Cursor Spotlight Glow */}
      <div
        ref={spotlightRef}
        className="pointer-events-none absolute inset-0 rounded-3xl transition-opacity duration-150 z-0 opacity-0"
        aria-hidden="true"
      />

      {/* Specular Liquid Glass Top Bevel Highlight */}
      <div
        className="pointer-events-none absolute top-0 left-6 right-6 h-[1.5px] z-20"
        style={{
          background: `linear-gradient(90deg, transparent 0%, rgba(255,255,255,0.6) 20%, rgba(${ACCENT_RGB}, 0.95) 50%, rgba(255,255,255,0.6) 80%, transparent 100%)`,
        }}
      />

      {/* Ambient warm radial glow in the backdrop */}
      <div
        className="pointer-events-none absolute top-12 left-1/2 -translate-x-1/2 w-[600px] h-[350px] rounded-full blur-[110px] opacity-25 z-0"
        style={{
          background: `radial-gradient(circle, rgba(${ACCENT_RGB}, 0.7) 0%, transparent 70%)`,
        }}
      />

      {/* Top Header & Stage Stepper */}
      <header className="relative z-10 w-full max-w-3xl flex flex-col items-center gap-3.5 mt-1 shrink-0">
        {/* Top bar with back button and prominent centered IVY brand */}
        <div className="w-full grid grid-cols-3 items-center px-1">
          {/* Left: Back to Dashboard */}
          <div className="flex items-center justify-start">
            <button
              id="btn-back-to-dashboard"
              onClick={onDismiss}
              className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-xl text-xs font-medium text-white/70 hover:text-white bg-white/[0.05] hover:bg-white/[0.1] border border-white/[0.08] hover:border-white/[0.18] transition-all cursor-pointer shadow-sm"
              title="Return to Main Workspace"
            >
              <ArrowLeft className="w-3.5 h-3.5" />
              <span>Back to Dashboard</span>
            </button>
          </div>

          {/* Center: Prominent IVY Brand Origami Wordmark */}
          <div className="flex items-center justify-center">
            <IvyWordmark height={34} glow={true} />
          </div>

          {/* Right: Optical balance spacer */}
          <div className="flex items-center justify-end" />
        </div>

        {/* Step Indicator Pill */}
        <StageIndicator
          currentStage={currentStage}
          completedStages={completedStages}
          onSelectStage={(stage) => {
            if (stage <= currentStage || completedStages.has(stage)) {
              clearNavigationTimers();
              setCurrentStage(stage);
            }
          }}
        />
      </header>

      {/* Main Interactive Stage Body */}
      <main className="relative z-10 w-full max-w-2xl my-auto py-3">
        <AnimatePresence mode="wait">
          {/* =========================================================================
              STAGE 5: LIVE VOICE TEST
              ========================================================================= */}
          {currentStage === 5 && (
            <motion.div
              key="stage-1"
              initial={{ opacity: 0, y: 14 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -14 }}
              transition={{ duration: 0.3, ease: 'easeOut' }}
              className="w-full flex flex-col items-center text-center"
            >
              {/* Headline */}
              <div className="mb-5">
                <h1
                  id="stage-1-headline"
                  className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white mb-1.5"
                  style={{ fontFamily: 'Syne, sans-serif' }}
                >
                  Let's test your voice
                </h1>
                <p id="stage-1-subtitle" className="text-white/60 text-sm sm:text-base max-w-md mx-auto">
                  Click the box below, hold {selectedHotkey}, and talk — Ivy types it right in.
                </p>
              </div>

              {/* Friday Liquid Glass Card */}
              <div
                id="interactive-hotkey-card"
                className="w-full rounded-3xl p-6 sm:p-7 border flex flex-col items-center gap-5 relative overflow-hidden transition-all duration-300"
                style={{
                  backgroundColor: 'rgba(18, 13, 26, 0.82)',
                  backdropFilter: 'blur(30px) saturate(160%)',
                  WebkitBackdropFilter: 'blur(30px) saturate(160%)',
                  borderColor: 'rgba(255, 107, 0, 0.32)',
                  boxShadow:
                    '0 25px 60px -15px rgba(0,0,0,0.85), 0 0 35px rgba(255,107,0,0.18), inset 0 1.5px 2px 0 rgba(255,255,255,0.3), inset 0 0 0 1px rgba(255,255,255,0.06)',
                }}
              >
                {/* Specular Card Top Highlight */}
                <div
                  className="pointer-events-none absolute top-0 left-8 right-8 h-[1px]"
                  style={{
                    background:
                      'linear-gradient(90deg, transparent 0%, rgba(255,255,255,0.4) 30%, rgba(255,107,0,0.7) 50%, rgba(255,255,255,0.4) 70%, transparent 100%)',
                  }}
                />

                {/* Hotkey Badge Trigger */}
                <div className="flex flex-col items-center gap-2">
                  <span className="text-[11px] uppercase tracking-wider text-white/50 font-semibold">
                    Push-to-talk trigger
                  </span>
                  <HotkeyBadge
                    hotkey={selectedHotkey}
                    isPressed={isKeyPressed || dictationState === 'listening'}
                    size="large"
                    id="stage-1-hotkey-badge"
                  />
                </div>

                {/* Try-saying hint */}
                <div className="text-center">
                  <span className="text-[10.5px] uppercase tracking-wider text-[#FFA133] font-semibold">
                    Try saying
                  </span>
                  <div
                    id="target-phrase-prompt"
                    className="text-sm sm:text-base font-bold text-white/90 tracking-wide font-mono select-text mt-0.5"
                  >
                    "{TARGET_PHRASE}"
                  </div>
                </div>

                {/* Real, focusable text box Ivy types into */}
                <div
                  className="w-full max-w-lg rounded-2xl p-3.5 border shadow-inner flex flex-col items-center gap-1.5 transition-colors"
                  style={{
                    backgroundColor: 'rgba(11, 8, 18, 0.9)',
                    borderColor: dictationState === 'success' ? 'rgba(16, 185, 129, 0.45)' : 'rgba(255, 107, 0, 0.22)',
                  }}
                >
                  <span className="text-[10.5px] uppercase tracking-wider text-white/45 font-semibold self-start">
                    Interactive Text Box
                  </span>
                  <input
                    id="stage1-real-textbox"
                    type="text"
                    value={boxText}
                    onChange={(e) => setBoxText(e.target.value)}
                    placeholder="Ivy will type here once you talk..."
                    className="w-full bg-transparent text-white text-base sm:text-lg font-mono text-center outline-none placeholder:text-white/30 placeholder:italic cursor-text"
                  />
                </div>

                {/* Soundwave Visualizer & State Readout */}
                <div className="w-full flex flex-col items-center min-h-[110px] justify-center">
                  <SoundwaveVisualizer
                    state={dictationState}
                    analyzer={isTauri ? null : micAnalyzerRef.current}
                    levels={isTauri ? meterBars : undefined}
                    audioActive={isTauri}
                  />

                  <div className="mt-1 text-sm font-medium">
                    {dictationState === 'waiting' && !errorMessage && (
                      <span className="text-white/60 flex items-center gap-2">
                        Hold {selectedHotkey} to start speaking...
                      </span>
                    )}

                    {dictationState === 'waiting' && errorMessage && (
                      <motion.span
                        initial={{ opacity: 0, y: 3 }}
                        animate={{ opacity: 1, y: 0 }}
                        className="text-amber-300 font-medium text-xs sm:text-sm flex items-center gap-2 px-3.5 py-1 rounded-full bg-amber-500/10 border border-amber-500/25"
                      >
                        <Info className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                        <span>{errorMessage}</span>
                      </motion.span>
                    )}

                    {dictationState === 'listening' && (
                      <motion.span
                        initial={{ scale: 0.95 }}
                        animate={{ scale: 1 }}
                        className="text-[#FF7A00] font-semibold flex items-center gap-2"
                      >
                        <Mic className="w-4 h-4 text-[#FF7A00] animate-pulse" />
                        Listening... speak now! ({Math.round(recordingDurationMs / 1000)}s)
                      </motion.span>
                    )}

                    {dictationState === 'transcribing' && (
                      <span className="text-amber-300 font-semibold flex items-center gap-2">
                        <Cpu className="w-4 h-4 text-amber-400 animate-spin" />
                        Transcribing on-device...
                      </span>
                    )}

                    {dictationState === 'failed' && !testExhausted && (
                      <motion.span
                        initial={{ opacity: 0, y: 3 }}
                        animate={{ opacity: 1, y: 0 }}
                        className="text-amber-300 font-medium text-xs sm:text-sm flex items-center gap-2 px-3.5 py-1 rounded-full bg-amber-500/15 border border-amber-500/30 shadow-sm"
                      >
                        <Info className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                        <span>{errorMessage || 'No speech detected. Please try again.'}</span>
                      </motion.span>
                    )}

                    {dictationState === 'failed' && testExhausted && (
                      <motion.div
                        initial={{ opacity: 0, y: 3 }}
                        animate={{ opacity: 1, y: 0 }}
                        className="flex flex-col items-center gap-3"
                      >
                        <span className="text-amber-300 font-medium text-xs sm:text-sm flex items-center gap-2 px-3.5 py-1 rounded-full bg-amber-500/15 border border-amber-500/30 shadow-sm">
                          <Info className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                          <span>{MAX_ATTEMPTS} tries, no luck. {errorMessage || 'Check your mic and try again.'}</span>
                        </span>
                        <div className="flex items-center gap-2.5">
                          <button
                            id="stage1-retry-after-exhausted"
                            onClick={handleRetryAfterExhausted}
                            type="button"
                            className="px-4 py-1.5 rounded-full bg-white/[0.08] hover:bg-white/[0.15] text-white text-xs font-semibold border border-white/[0.14] transition-all cursor-pointer"
                          >
                            Try {MAX_ATTEMPTS} more times
                          </button>
                          <button
                            id="stage1-skip-after-exhausted"
                            onClick={handleSkipTest}
                            type="button"
                            className="px-4 py-1.5 rounded-full text-white/50 hover:text-white text-xs font-medium underline underline-offset-4 transition-all cursor-pointer"
                          >
                            Skip test
                          </button>
                        </div>
                      </motion.div>
                    )}

                    {dictationState === 'success' && (
                      <motion.div
                        initial={{ opacity: 0, scale: 0.95 }}
                        animate={{ opacity: 1, scale: 1 }}
                        className="flex flex-col items-center gap-1.5"
                      >
                        <div className="flex items-center gap-2 text-emerald-400 font-semibold text-xs sm:text-sm">
                          <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                          <span>Typed straight into the box above!</span>
                        </div>
                        <p className="text-white/60 text-xs">Voice test verified! Click Next Step to continue.</p>
                      </motion.div>
                    )}
                  </div>
                </div>

                {/* Tactile On-Screen Push-to-Talk Button for Click / Hold */}
                <div className="w-full flex flex-col items-center pt-3 border-t border-white/[0.08]">
                  <button
                    id="manual-hold-test-btn"
                    disabled={testExhausted}
                    onMouseDown={() => handleStartListening(false)}
                    onMouseUp={() => handleStopListening(false)}
                    onTouchStart={() => handleStartListening(false)}
                    onTouchEnd={() => handleStopListening(false)}
                    type="button"
                    className={`w-full max-w-sm py-2.5 px-5 rounded-2xl font-semibold text-xs sm:text-sm transition-all flex items-center justify-center gap-2 select-none active:scale-98 ${
                      testExhausted
                        ? 'bg-[#15101E]/60 text-white/30 border border-white/[0.06] cursor-not-allowed'
                        : isKeyPressed || dictationState === 'listening'
                        ? 'bg-[#FF6B00] text-white shadow-[0_0_24px_#FF6B00] border border-[#FFA133] cursor-pointer'
                        : 'bg-[#15101E] hover:bg-[#1E172B] text-white/80 border border-white/[0.1] hover:border-[#FF6B00]/40 cursor-pointer'
                    }`}
                  >
                    <Mic className="w-4 h-4 text-[#FFA133]" />
                    <span>
                      {testExhausted
                        ? `Attempts used — tap "Try ${MAX_ATTEMPTS} more times" above`
                        : isKeyPressed || dictationState === 'listening'
                        ? 'Release to Transcribe'
                        : dictationState === 'transcribing'
                        ? 'Transcribing on-device...'
                        : 'Or Click & Hold to Test'}
                    </span>
                  </button>
                  <span className="text-[11px] text-white/40 mt-1.5">
                    Physical hotkey: Press & Hold <kbd className="font-mono text-white/70">{selectedHotkey}</kbd>
                  </span>
                </div>
              </div>

              {/* Navigation & Skip Footer */}
              <div className="w-full flex items-center justify-between mt-5 px-1">
                <button
                  id="skip-test-link"
                  onClick={handleSkipTest}
                  type="button"
                  className="text-xs text-white/45 hover:text-[#FFA133] underline underline-offset-4 transition-colors cursor-pointer"
                >
                  Skip test for now
                </button>

                <div className="flex items-center gap-3">
                  {dictationState === 'success' || completedStages.has(5) ? (
                    <button
                      id="stage-1-next-btn"
                      onClick={goToNextStage}
                      type="button"
                      className="px-6 py-2 rounded-full text-white text-xs sm:text-sm font-semibold flex items-center gap-2 transition-all cursor-pointer hover:brightness-110 active:scale-98"
                      style={{
                        background: 'linear-gradient(to right, #FF6B00, #E05300)',
                        boxShadow:
                          '0 0 24px rgba(255, 107, 0, 0.45), inset 0 1px 1px rgba(255, 255, 255, 0.4)',
                        border: '1px solid rgba(255, 161, 51, 0.5)',
                      }}
                    >
                      <span>Next Step</span>
                      <ArrowRight className="w-4 h-4" />
                    </button>
                  ) : (
                    <button
                      id="stage-1-next-disabled"
                      onClick={handleSkipTest}
                      type="button"
                      className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/60 hover:text-white text-xs sm:text-sm font-medium border border-white/10 transition-all cursor-pointer"
                    >
                      Continue →
                    </button>
                  )}
                </div>
              </div>
            </motion.div>
          )}

          {/* =========================================================================
              STAGE 1: HOTKEY CUSTOMIZATION
              ========================================================================= */}
          {currentStage === 1 && (
            <motion.div
              key="stage-2"
              initial={{ opacity: 0, y: 14 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -14 }}
              transition={{ duration: 0.3, ease: 'easeOut' }}
              className="w-full flex flex-col items-center text-center"
            >
              {/* Headline */}
              <div className="mb-5">
                <h1
                  id="stage-2-headline"
                  className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white mb-1.5"
                  style={{ fontFamily: 'Syne, sans-serif' }}
                >
                  You're in control of your keys
                </h1>
                <p id="stage-2-subtitle" className="text-white/60 text-sm sm:text-base max-w-md mx-auto">
                  Don't like Alt + Space? You can customize your push-to-talk trigger anytime.
                </p>
              </div>

              {/* Friday Liquid Glass Card */}
              <div
                id="hotkey-customization-card"
                className="w-full rounded-3xl p-6 sm:p-7 border flex flex-col items-center gap-5 relative overflow-hidden transition-all duration-300"
                style={{
                  backgroundColor: 'rgba(18, 13, 26, 0.82)',
                  backdropFilter: 'blur(30px) saturate(160%)',
                  WebkitBackdropFilter: 'blur(30px) saturate(160%)',
                  borderColor: 'rgba(255, 107, 0, 0.32)',
                  boxShadow:
                    '0 25px 60px -15px rgba(0,0,0,0.85), 0 0 35px rgba(255,107,0,0.18), inset 0 1.5px 2px 0 rgba(255,255,255,0.3), inset 0 0 0 1px rgba(255,255,255,0.06)',
                }}
              >
                {/* Active Key Preview Socket */}
                <div className="flex flex-col items-center gap-2">
                  <span className="text-[11px] uppercase tracking-wider text-white/50 font-semibold">
                    Currently Selected Shortcut
                  </span>
                  <div
                    className="p-3.5 rounded-2xl border shadow-inner flex items-center justify-center min-w-[240px]"
                    style={{
                      backgroundColor: 'rgba(11, 8, 18, 0.9)',
                      borderColor: 'rgba(255, 107, 0, 0.25)',
                    }}
                  >
                    <HotkeyBadge
                      hotkey={selectedHotkey}
                      isPressed={testKeyPressed}
                      size="large"
                      id="custom-hotkey-display"
                    />
                  </div>
                  <span className="text-xs text-[#FFA133]/90">
                    {testKeyPressed
                      ? 'Key registered!'
                      : 'Try pressing your shortcut now to test the tactile feel'}
                  </span>
                </div>

                {/* Popular Triggers Option Grid */}
                <div className="w-full text-left">
                  <label className="text-[11px] uppercase tracking-wider text-white/50 font-semibold mb-2.5 block">
                    Popular push-to-talk triggers
                  </label>
                  <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                    {[
                      { key: 'Alt + Space', label: 'Default', desc: 'Ergonomic thumb trigger' },
                      { key: 'Caps Lock', label: 'Single Key', desc: 'Quick one-finger tap' },
                      { key: 'Ctrl + Space', label: 'Alternative', desc: 'Standard desktop combo' },
                    ].map((item) => {
                      const isSelected = selectedHotkey === item.key;
                      return (
                        <button
                          key={item.key}
                          id={`select-hotkey-${item.key.replace(/\s+/g, '-').toLowerCase()}`}
                          onClick={() => {
                            setSelectedHotkey(item.key);
                            onUpdateSettings?.({ hotkey: item.key });
                            audioFeedback.playStartBeep();
                          }}
                          type="button"
                          className={`p-3.5 rounded-2xl text-left border transition-all cursor-pointer flex flex-col justify-between ${
                            isSelected
                              ? 'bg-[#FF6B00]/15 border-[#FF6B00] shadow-[0_0_20px_rgba(255,107,0,0.25)]'
                              : 'bg-white/[0.03] border-white/[0.08] hover:border-[rgba(255,107,0,0.35)] hover:bg-white/[0.06]'
                          }`}
                        >
                          <div className="flex items-center justify-between mb-2">
                            <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-white/[0.08] text-white/70">
                              {item.label}
                            </span>
                            {isSelected && (
                              <div className="w-4 h-4 rounded-full bg-[#FF6B00] flex items-center justify-center text-white">
                                <Check className="w-2.5 h-2.5 stroke-[3]" />
                              </div>
                            )}
                          </div>
                          <div className="font-mono font-bold text-white text-sm mb-0.5">
                            {item.key}
                          </div>
                          <div className="text-[11px] text-white/45">{item.desc}</div>
                        </button>
                      );
                    })}
                  </div>
                </div>

                {/* Helpful Tip Note Box */}
                <div
                  className="w-full rounded-2xl p-3.5 border flex items-start gap-3 text-left"
                  style={{
                    backgroundColor: 'rgba(11, 8, 18, 0.7)',
                    borderColor: 'rgba(255, 255, 255, 0.08)',
                  }}
                >
                  <Info className="w-4 h-4 text-[#FFA133] shrink-0 mt-0.5" />
                  <div className="text-xs text-white/70 leading-relaxed">
                    <strong className="text-white">Note:</strong> You can change this trigger anytime
                    under <span className="text-[#FFA133] font-semibold">Settings → Shortcut Key</span>.
                    Ivy works system-wide in any application, background window, or full-screen game.
                  </div>
                </div>
              </div>

              {/* Navigation Footer */}
              <div className="w-full flex items-center justify-between mt-5 px-1">
                <button
                  id="stage-2-back-btn"
                  onClick={goToPrevStage}
                  type="button"
                  className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/70 hover:text-white text-xs sm:text-sm font-semibold border border-white/10 transition-all flex items-center gap-2 cursor-pointer"
                >
                  <ArrowLeft className="w-4 h-4" />
                  <span>Back</span>
                </button>

                <button
                  id="stage-2-next-btn"
                  onClick={goToNextStage}
                  type="button"
                  className="px-6 py-2 rounded-full text-white text-xs sm:text-sm font-semibold flex items-center gap-2 transition-all cursor-pointer hover:brightness-110 active:scale-98"
                  style={{
                    background: 'linear-gradient(to right, #FF6B00, #E05300)',
                    boxShadow:
                      '0 0 24px rgba(255, 107, 0, 0.45), inset 0 1px 1px rgba(255, 255, 255, 0.4)',
                    border: '1px solid rgba(255, 161, 51, 0.5)',
                  }}
                >
                  <span>Next: Smart Clipboard</span>
                  <ArrowRight className="w-4 h-4" />
                </button>
              </div>
            </motion.div>
          )}

          {/* =========================================================================
              STAGE 2: SMART CLIPBOARD FALLBACK
              ========================================================================= */}
          {currentStage === 2 && (
            <motion.div
              key="stage-3"
              initial={{ opacity: 0, y: 14 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -14 }}
              transition={{ duration: 0.3, ease: 'easeOut' }}
              className="w-full flex flex-col items-center text-center"
            >
              {/* Headline */}
              <div className="mb-5">
                <h1
                  id="stage-3-headline"
                  className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white mb-1.5"
                  style={{ fontFamily: 'Syne, sans-serif' }}
                >
                  No text box? We've got you covered.
                </h1>
                <p id="stage-3-subtitle" className="text-white/60 text-sm sm:text-base max-w-md mx-auto">
                  Ivy adapts intelligently to wherever you are working on your computer.
                </p>
              </div>

              {/* Two Visual Feature Cards */}
              <div className="w-full grid grid-cols-1 md:grid-cols-2 gap-4 mb-4">
                {/* Card A: Focused Input */}
                <div
                  id="feature-card-focused-input"
                  className="rounded-3xl p-5 border text-left transition-all duration-200 flex flex-col justify-between border-white/[0.08] bg-[#120D1A]/70"
                >
                  <div>
                    <div className="w-9 h-9 rounded-2xl bg-[#FF6B00]/15 border border-[#FF6B00]/30 flex items-center justify-center text-[#FFA133] mb-3.5">
                      <FileText className="w-4 h-4" />
                    </div>
                    <div className="text-[10.5px] uppercase tracking-wider text-[#FFA133] font-semibold mb-1">
                      Mode A · Active Input
                    </div>
                    <h3 className="text-sm sm:text-base font-bold text-white mb-1.5">
                      Direct Cursor Typing
                    </h3>
                    <p className="text-xs text-white/65 leading-relaxed mb-3.5">
                      When a text field is active (Google Docs, Slack, Word) → Ivy pastes your text directly at your cursor.
                    </p>
                  </div>

                  {/* Interactive Mini Mock Application Window */}
                  <div className="bg-[#09070D] rounded-xl p-3 border border-white/[0.08] text-xs font-mono">
                    <div className="flex items-center gap-1.5 mb-2 pb-1.5 border-b border-white/[0.08]">
                      <span className="w-2 h-2 rounded-full bg-red-500/70" />
                      <span className="w-2 h-2 rounded-full bg-yellow-500/70" />
                      <span className="w-2 h-2 rounded-full bg-green-500/70" />
                      <span className="text-[10px] text-white/40 ml-1">Document.docx</span>
                    </div>
                    <div className="text-white/80 min-h-[32px] flex items-center">
                      <span>{TARGET_PHRASE}</span>
                    </div>
                  </div>
                </div>

                {/* Card B: No Input / Desktop */}
                <div
                  id="feature-card-smart-clipboard"
                  className="rounded-3xl p-5 border text-left transition-all duration-200 flex flex-col justify-between border-white/[0.08] bg-[#120D1A]/70"
                >
                  <div>
                    <div className="w-9 h-9 rounded-2xl bg-[#FF6B00]/15 border border-[#FF6B00]/30 flex items-center justify-center text-[#FFA133] mb-3.5">
                      <ClipboardCheck className="w-4 h-4" />
                    </div>
                    <div className="text-[10.5px] uppercase tracking-wider text-[#FFA133] font-semibold mb-1">
                      Mode B · Desktop / Unfocused
                    </div>
                    <h3 className="text-sm sm:text-base font-bold text-white mb-1.5">
                      Held-Back Text
                    </h3>
                    <p className="text-xs text-white/65 leading-relaxed mb-3.5">
                      When no text box is detected → Ivy never touches your real clipboard (it could hold something you meant to keep, like a password). It holds the transcript instead — press <kbd className="font-mono text-white bg-white/[0.1] px-1 rounded">{manualPasteHotkey}</kbd> to paste it wherever you want.
                    </p>
                  </div>

                  {/* Static illustration only — no click action. It used to
                      copy TARGET_PHRASE to the real clipboard on click, which
                      read as an interactive control but did nothing useful
                      and could clobber whatever the user actually had
                      copied. */}
                  <div className="bg-[#09070D] rounded-xl p-3 border border-white/[0.08] text-xs font-mono">
                    <div className="flex items-center justify-between mb-2 pb-1.5 border-b border-white/[0.08]">
                      <span className="text-[10px] text-white/40">Held by Ivy</span>
                      <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-400 font-sans">
                        Waiting
                      </span>
                    </div>
                    <div className="text-white/80 text-[11px] truncate py-1">
                      "{TARGET_PHRASE}"
                    </div>
                  </div>
                </div>
              </div>


              {/* Navigation Footer & Finish CTA */}
              <div className="w-full flex items-center justify-between mt-1 px-1">
                <button
                  id="stage-3-back-btn"
                  onClick={goToPrevStage}
                  type="button"
                  className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/70 hover:text-white text-xs sm:text-sm font-semibold border border-white/10 transition-all flex items-center gap-2 cursor-pointer"
                >
                  <ArrowLeft className="w-4 h-4" />
                  <span>Back</span>
                </button>

                <button
                  id="stage-3-next-btn"
                  onClick={goToNextStage}
                  type="button"
                  className="px-7 py-3 rounded-full text-white text-sm sm:text-base font-bold flex items-center gap-2.5 transition-all cursor-pointer hover:brightness-110 active:scale-98"
                  style={{
                    background: 'linear-gradient(to right, #FF6B00, #E05300)',
                    boxShadow:
                      '0 0 35px rgba(255, 107, 0, 0.65), inset 0 1.5px 2px rgba(255, 255, 255, 0.45)',
                    border: '1px solid rgba(255, 161, 51, 0.6)',
                  }}
                >
                  <span>Next: Choose Your Transcription Mode</span>
                  <ArrowRight className="w-4 h-4 stroke-[2.5]" />
                </button>
              </div>
            </motion.div>
          )}

          {/* =========================================================================
              STAGE 6: SELF-CORRECTION DEMO
              ========================================================================= */}
          {currentStage === 6 && (
            <motion.div
              key="stage-4"
              initial={{ opacity: 0, y: 14 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -14 }}
              transition={{ duration: 0.3, ease: 'easeOut' }}
              className="w-full flex flex-col items-center text-center"
            >
              {/* Headline */}
              <div className="mb-5">
                <h1
                  id="stage-4-headline"
                  className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white mb-1.5"
                  style={{ fontFamily: 'Syne, sans-serif' }}
                >
                  {liveAi ? 'Ivy understands when you change your mind' : 'Changing your mind mid-sentence'}
                </h1>
                <p id="stage-4-subtitle" className="text-white/60 text-sm sm:text-base max-w-md mx-auto">
                  {liveAi
                    ? 'Say the whole line, correction and all — the AI drops what you took back.'
                    : 'Only Accuracy + GPU resolves corrections. Your setup pastes them as spoken — try it and see.'}
                </p>
              </div>

              {/* Friday Liquid Glass Card */}
              <div
                id="self-correction-card"
                className="w-full rounded-3xl p-6 sm:p-7 border flex flex-col items-center gap-5 relative overflow-hidden transition-all duration-300"
                style={{
                  backgroundColor: 'rgba(18, 13, 26, 0.82)',
                  backdropFilter: 'blur(30px) saturate(160%)',
                  WebkitBackdropFilter: 'blur(30px) saturate(160%)',
                  borderColor: 'rgba(255, 107, 0, 0.32)',
                  boxShadow:
                    '0 25px 60px -15px rgba(0,0,0,0.85), 0 0 35px rgba(255,107,0,0.18), inset 0 1.5px 2px 0 rgba(255,255,255,0.3), inset 0 0 0 1px rgba(255,255,255,0.06)',
                }}
              >
                <div
                  className="pointer-events-none absolute top-0 left-8 right-8 h-[1px]"
                  style={{
                    background:
                      'linear-gradient(90deg, transparent 0%, rgba(255,255,255,0.4) 30%, rgba(255,107,0,0.7) 50%, rgba(255,255,255,0.4) 70%, transparent 100%)',
                  }}
                />

                <div className="flex flex-col items-center gap-2">
                  <span className="text-[11px] uppercase tracking-wider text-white/50 font-semibold">
                    Push-to-talk trigger
                  </span>
                  <HotkeyBadge
                    hotkey={selectedHotkey}
                    isPressed={demoKeyPressed || demoState === 'listening'}
                    size="large"
                    id="stage-4-hotkey-badge"
                  />
                </div>

                {/* Real self-correction, said as one continuous line */}
                <div className="text-center">
                  <span className="text-[10.5px] uppercase tracking-wider text-[#FFA133] font-semibold">
                    Say this whole line
                  </span>
                  <div
                    id="correction-phrase-prompt"
                    className="text-sm sm:text-base font-bold text-white/90 tracking-wide font-mono select-text mt-0.5 max-w-lg"
                  >
                    "{CORRECTION_PHRASE}"
                  </div>
                </div>

                <div
                  id="correction-mode-hint"
                  className="w-full max-w-lg rounded-2xl border px-4 py-3 flex flex-col items-center gap-1.5"
                  style={{
                    backgroundColor: 'rgba(255, 107, 0, 0.06)',
                    borderColor: 'rgba(255, 107, 0, 0.25)',
                  }}
                >
                  <span className="text-[10.5px] uppercase tracking-wider text-[#FFA133] font-semibold">
                    {liveAi ? 'No trigger words needed' : `Not active in ${selectedMode === 'speed' ? 'Speed' : 'Accuracy'} + ${hardwareMode.toUpperCase()}`}
                  </span>
                  <p className="text-white/55 text-[11px] max-w-sm">
                    {liveAi
                      ? 'Qwen AI reads the whole line and keeps what you meant. It is AI, so glance at the result.'
                      : 'Your setup has no AI while you dictate. Switch to Accuracy (Step 3) and GPU (Settings → Hardware) to have corrections resolved.'}
                  </p>
                </div>

                {/* Soundwave Visualizer & State Readout */}
                <div className="w-full flex flex-col items-center min-h-[110px] justify-center">
                  <SoundwaveVisualizer
                    state={demoState}
                    analyzer={isTauri ? null : demoMicAnalyzerRef.current}
                    levels={isTauri ? meterBars : undefined}
                    audioActive={isTauri}
                  />

                  <div className="mt-1 text-sm font-medium">
                    {demoState === 'waiting' && !demoError && (
                      <span className="text-white/60 flex items-center gap-2">
                        Hold {selectedHotkey} to start speaking...
                      </span>
                    )}

                    {demoState === 'waiting' && demoError && (
                      <motion.span
                        initial={{ opacity: 0, y: 3 }}
                        animate={{ opacity: 1, y: 0 }}
                        className="text-amber-300 font-medium text-xs sm:text-sm flex items-center gap-2 px-3.5 py-1 rounded-full bg-amber-500/10 border border-amber-500/25"
                      >
                        <Info className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                        <span>{demoError}</span>
                      </motion.span>
                    )}

                    {demoState === 'listening' && (
                      <motion.span
                        initial={{ scale: 0.95 }}
                        animate={{ scale: 1 }}
                        className="text-[#FF7A00] font-semibold flex items-center gap-2"
                      >
                        <Mic className="w-4 h-4 text-[#FF7A00] animate-pulse" />
                        Listening... speak now! ({Math.round(demoDurationMs / 1000)}s)
                      </motion.span>
                    )}

                    {demoState === 'transcribing' && (
                      <span className="text-amber-300 font-semibold flex items-center gap-2">
                        <Cpu className="w-4 h-4 text-amber-400 animate-spin" />
                        Resolving your correction on-device...
                      </span>
                    )}

                    {demoState === 'failed' && !demoExhausted && (
                      <motion.span
                        initial={{ opacity: 0, y: 3 }}
                        animate={{ opacity: 1, y: 0 }}
                        className="text-amber-300 font-medium text-xs sm:text-sm flex items-center gap-2 px-3.5 py-1 rounded-full bg-amber-500/15 border border-amber-500/30 shadow-sm"
                      >
                        <Info className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                        <span>{demoError || 'No speech detected. Please try again.'}</span>
                      </motion.span>
                    )}

                    {demoState === 'failed' && demoExhausted && (
                      <motion.div
                        initial={{ opacity: 0, y: 3 }}
                        animate={{ opacity: 1, y: 0 }}
                        className="flex flex-col items-center gap-3"
                      >
                        <span className="text-amber-300 font-medium text-xs sm:text-sm flex items-center gap-2 px-3.5 py-1 rounded-full bg-amber-500/15 border border-amber-500/30 shadow-sm">
                          <Info className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                          <span>{DEMO_MAX_ATTEMPTS} tries, no luck. {demoError || 'Check your mic and try again.'}</span>
                        </span>
                        <div className="flex items-center gap-2.5">
                          <button
                            id="stage4-retry-after-exhausted"
                            onClick={handleDemoRetryAfterExhausted}
                            type="button"
                            className="px-4 py-1.5 rounded-full bg-white/[0.08] hover:bg-white/[0.15] text-white text-xs font-semibold border border-white/[0.14] transition-all cursor-pointer"
                          >
                            Try {DEMO_MAX_ATTEMPTS} more times
                          </button>
                          <button
                            id="stage4-skip-after-exhausted"
                            onClick={handleDemoSkip}
                            type="button"
                            className="px-4 py-1.5 rounded-full text-white/50 hover:text-white text-xs font-medium underline underline-offset-4 transition-all cursor-pointer"
                          >
                            Skip
                          </button>
                        </div>
                      </motion.div>
                    )}

                    {demoState === 'success' && (() => {
                      // Judged from the real result, never assumed.
                      const correctionCaught = !demoResult.toLowerCase().includes('french fries');
                      return (
                      <motion.div
                        initial={{ opacity: 0, y: 6, scale: 0.92 }}
                        animate={{ opacity: 1, y: 0, scale: 1 }}
                        className="flex flex-col items-center gap-2.5"
                      >
                        <div
                          className={`flex items-center gap-2 font-semibold text-xs sm:text-sm ${
                            correctionCaught ? 'text-emerald-400' : 'text-[#FFA133]'
                          }`}
                        >
                          {correctionCaught ? (
                            <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                          ) : (
                            <Info className="w-4 h-4 text-[#FFA133]" />
                          )}
                          <span>
                            {correctionCaught
                              ? 'Ivy caught the correction!'
                              : liveAi
                              ? "Didn't catch it that time"
                              : 'Pasted as spoken — expected in your setup'}
                          </span>
                        </div>
                        {/* The real cleaned transcript — whatever it actually
                            came back as, not a canned string, so a real miss
                            (the model occasionally doesn't fully resolve it)
                            shows up honestly instead of being papered over. */}
                        <div
                          id="correction-result"
                          className="px-4 py-2 rounded-2xl border text-white text-sm sm:text-base font-mono max-w-lg"
                          style={{
                            backgroundColor: correctionCaught ? 'rgba(16, 185, 129, 0.1)' : 'rgba(255, 107, 0, 0.1)',
                            borderColor: correctionCaught ? 'rgba(16, 185, 129, 0.35)' : 'rgba(255, 107, 0, 0.35)',
                          }}
                        >
                          "{demoResult}"
                        </div>
                        <p className="text-white/50 text-[11px] sm:text-xs max-w-sm">
                          {correctionCaught
                            ? '"French fries" never made it in — that\'s the retracted half of what you said, gone on its own.'
                            : liveAi
                            ? 'The AI can miss sometimes, or the words may have been misheard. Try saying it again.'
                            : 'Only Accuracy + GPU resolves corrections. Every other setup keeps exactly what you said.'}
                        </p>
                      </motion.div>
                      );
                    })()}
                  </div>
                </div>

                {/* Tactile On-Screen Push-to-Talk Button for Click / Hold */}
                <div className="w-full flex flex-col items-center pt-3 border-t border-white/[0.08]">
                  <button
                    id="manual-hold-test-btn-stage4"
                    disabled={demoExhausted}
                    onMouseDown={() => handleDemoStart(false)}
                    onMouseUp={() => handleDemoStop(false)}
                    onTouchStart={() => handleDemoStart(false)}
                    onTouchEnd={() => handleDemoStop(false)}
                    type="button"
                    className={`w-full max-w-sm py-2.5 px-5 rounded-2xl font-semibold text-xs sm:text-sm transition-all flex items-center justify-center gap-2 select-none active:scale-98 ${
                      demoExhausted
                        ? 'bg-[#15101E]/60 text-white/30 border border-white/[0.06] cursor-not-allowed'
                        : demoKeyPressed || demoState === 'listening'
                        ? 'bg-[#FF6B00] text-white shadow-[0_0_24px_#FF6B00] border border-[#FFA133] cursor-pointer'
                        : 'bg-[#15101E] hover:bg-[#1E172B] text-white/80 border border-white/[0.1] hover:border-[#FF6B00]/40 cursor-pointer'
                    }`}
                  >
                    <Mic className="w-4 h-4 text-[#FFA133]" />
                    <span>
                      {demoExhausted
                        ? `Attempts used — tap "Try ${DEMO_MAX_ATTEMPTS} more times" above`
                        : demoKeyPressed || demoState === 'listening'
                        ? 'Release to Transcribe'
                        : demoState === 'transcribing'
                        ? 'Resolving on-device...'
                        : 'Or Click & Hold to Test'}
                    </span>
                  </button>
                  <span className="text-[11px] text-white/40 mt-1.5">
                    Physical hotkey: Press & Hold <kbd className="font-mono text-white/70">{selectedHotkey}</kbd>
                  </span>
                </div>
              </div>

              {/* Navigation & Skip Footer */}
              <div className="w-full flex items-center justify-between mt-5 px-1">
                <button
                  id="stage-4-back-btn"
                  onClick={goToPrevStage}
                  type="button"
                  className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/70 hover:text-white text-xs sm:text-sm font-semibold border border-white/10 transition-all flex items-center gap-2 cursor-pointer"
                >
                  <ArrowLeft className="w-4 h-4" />
                  <span>Back</span>
                </button>

                <div className="flex items-center gap-3">
                  {demoState !== 'success' && (
                    <button
                      id="skip-correction-demo-link"
                      onClick={handleDemoSkip}
                      type="button"
                      className="text-xs text-white/45 hover:text-[#FFA133] underline underline-offset-4 transition-colors cursor-pointer"
                    >
                      Skip for now
                    </button>
                  )}
                  <button
                    id="stage-4-next-btn"
                    onClick={goToNextStage}
                    type="button"
                    className="px-7 py-3 rounded-full text-white text-sm sm:text-base font-bold flex items-center gap-2.5 transition-all cursor-pointer hover:brightness-110 active:scale-98"
                    style={{
                      background: 'linear-gradient(to right, #FF6B00, #E05300)',
                      boxShadow:
                        '0 0 35px rgba(255, 107, 0, 0.65), inset 0 1.5px 2px rgba(255, 255, 255, 0.45)',
                      border: '1px solid rgba(255, 161, 51, 0.6)',
                    }}
                  >
                    <span>Next: Privacy & Safety</span>
                    <ArrowRight className="w-4 h-4 stroke-[2.5]" />
                  </button>
                </div>
              </div>
            </motion.div>
          )}

          {/* Stage 3: AI Speed & Smart Dictation Strategy */}
          {currentStage === 3 && (
            <motion.div
              key="stage-5"
              initial={{ opacity: 0, y: 15 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -15 }}
              transition={{ duration: 0.28, ease: 'easeOut' }}
              className="flex-1 flex flex-col items-center justify-between w-full max-w-xl mx-auto py-2"
            >
              {/* Header Title Area */}
              <div className="text-center flex flex-col items-center gap-2 mb-4">
                <span
                  className="px-3 py-1 rounded-full text-[10px] sm:text-[11px] font-bold uppercase tracking-wider"
                  style={{
                    backgroundColor: 'rgba(255, 107, 0, 0.15)',
                    color: '#FFA133',
                    border: '1px solid rgba(255, 107, 0, 0.35)',
                  }}
                >
                  Step 3 of 7: AI Engine &amp; Strategy
                </span>
                <h3 className="text-xl sm:text-2xl font-bold text-white tracking-tight">
                  Choose Your Transcription Mode
                </h3>
                <p className="text-xs sm:text-sm text-white/60 max-w-md">
                  Select how Ivy balances raw typing and smart speech polish. You can change this anytime later in Settings.
                </p>
              </div>

              {/* Dictation Mode Selectable Cards */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3.5 w-full">
                {/* Instant Speed Card */}
                <div
                  id="mode-card-speed"
                  onClick={() => {
                    setSelectedMode('speed');
                    onUpdateSettings?.({ dictationMode: 'speed' });
                  }}
                  className={`rounded-2xl p-4 border transition-all cursor-pointer flex flex-col justify-between ${
                    selectedMode === 'speed'
                      ? 'bg-[#FF6B00]/10 border-[#FF6B00] shadow-[0_0_25px_rgba(255,107,0,0.25)]'
                      : 'bg-white/[0.03] hover:bg-white/[0.06] border-white/10'
                  }`}
                >
                  <div>
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex items-center gap-2.5">
                        <div
                          className={`p-2 rounded-xl ${
                            selectedMode === 'speed'
                              ? 'bg-[#FF6B00]/20 text-[#FFA133]'
                              : 'bg-white/[0.06] text-white/60'
                          }`}
                        >
                          <Zap className="w-5 h-5" />
                        </div>
                        <div>
                          <h4 className="text-sm font-bold text-white">Instant Speed</h4>
                          <span className="text-[11px] font-semibold text-amber-400">No AI • no extra wait</span>
                        </div>
                      </div>
                      <div
                        className={`w-4 h-4 rounded-full border flex items-center justify-center shrink-0 ${
                          selectedMode === 'speed' ? 'border-[#FF6B00] bg-[#FF6B00]' : 'border-white/30'
                        }`}
                      >
                        {selectedMode === 'speed' && <Check className="w-2.5 h-2.5 text-white stroke-[3]" />}
                      </div>
                    </div>
                    <p className="text-[11.5px] text-white/70 mt-3 leading-relaxed">
                      Core rules only: fillers, spoken punctuation, grammar, numbers. Same on GPU or CPU. Corrections like "scratch that" are pasted as spoken.
                    </p>
                  </div>
                  <span className="text-[10px] text-amber-400/90 font-medium mt-3 block">
                    ⚡ Perfect for fast chat, coding &amp; quick notes
                  </span>
                </div>

                {/* Smart Accuracy Card */}
                <div
                  id="mode-card-accuracy"
                  onClick={() => {
                    setSelectedMode('accuracy');
                    onUpdateSettings?.({ dictationMode: 'accuracy' });
                  }}
                  className={`rounded-2xl p-4 border transition-all cursor-pointer flex flex-col justify-between ${
                    selectedMode === 'accuracy'
                      ? 'bg-[#FF6B00]/10 border-[#FF6B00] shadow-[0_0_25px_rgba(255,107,0,0.25)]'
                      : 'bg-white/[0.03] hover:bg-white/[0.06] border-white/10'
                  }`}
                >
                  <div>
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex items-center gap-2.5">
                        <div
                          className={`p-2 rounded-xl ${
                            selectedMode === 'accuracy'
                              ? 'bg-[#FF6B00]/20 text-[#FFA133]'
                              : 'bg-white/[0.06] text-white/60'
                          }`}
                        >
                          <Sparkles className="w-5 h-5" />
                        </div>
                        <div>
                          <h4 className="text-sm font-bold text-white">Smart Accuracy</h4>
                          <span className="text-[11px] font-semibold text-[#FFA133]">Recommended</span>
                        </div>
                      </div>
                      <div
                        className={`w-4 h-4 rounded-full border flex items-center justify-center shrink-0 ${
                          selectedMode === 'accuracy' ? 'border-[#FF6B00] bg-[#FF6B00]' : 'border-white/30'
                        }`}
                      >
                        {selectedMode === 'accuracy' && <Check className="w-2.5 h-2.5 text-white stroke-[3]" />}
                      </div>
                    </div>
                    <p className="text-[11.5px] text-white/70 mt-3 leading-relaxed">
                      On CPU: the full 50+ rule set, no extra wait. On GPU: Qwen AI also reads every dictation and drops what you took back, then the same formatting rules run.
                    </p>
                  </div>
                  <span className="text-[10px] text-[#FFA133] font-medium mt-3 block">
                    ✨ Worth the wait for anything that matters
                  </span>
                </div>
              </div>

              <div
                className="w-full rounded-2xl p-4 sm:p-5 border mt-4"
                style={{
                  backgroundColor: 'rgba(18, 13, 26, 0.82)',
                  borderColor: 'rgba(255, 161, 51, 0.3)',
                  boxShadow: '0 10px 30px -10px rgba(0,0,0,0.7), inset 0 1px 1px 0 rgba(255,255,255,0.2)',
                }}
              >
                <div className="flex items-center gap-2 mb-2">
                  <span className="px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider bg-[#FF6B00]/20 text-[#FFA133] border border-[#FF6B00]/30">
                    All 4 modes
                  </span>
                  <h4 className="text-xs sm:text-sm font-bold text-white">What each combination does</h4>
                </div>
                <p className="text-[11.5px] text-white/70 leading-relaxed mb-3">
                  Speed or Accuracy is chosen above; GPU or CPU in Settings → Hardware. Your current setup is highlighted.
                </p>
                <ModeMatrix dictationMode={selectedMode} hardwareMode={hardwareMode} />
              </div>

              {/* The Golden Rule of Smart Dictation Card */}
              <div
                id="smart-dictation-pro-tip-card"
                className="w-full rounded-2xl p-4 sm:p-5 border mt-4 relative overflow-hidden"
                style={{
                  backgroundColor: 'rgba(18, 13, 26, 0.82)',
                  borderColor: 'rgba(255, 161, 51, 0.3)',
                  boxShadow: '0 10px 30px -10px rgba(0,0,0,0.7), inset 0 1px 1px 0 rgba(255,255,255,0.2)',
                }}
              >
                <div className="flex items-center gap-2 mb-2">
                  <span className="px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider bg-[#FF6B00]/20 text-[#FFA133] border border-[#FF6B00]/30">
                    Golden Rule
                  </span>
                  <h4 className="text-xs sm:text-sm font-bold text-white">
                    Dictate in 1-Minute Bursts, Not 5-Minute Marathons
                  </h4>
                </div>

                <p className="text-[11.5px] text-white/70 leading-relaxed mb-3">
                  Ivy is designed for fast, high-accuracy dictation. To get the best results:
                </p>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
                  <div className="rounded-xl p-2.5 bg-emerald-500/10 border border-emerald-500/25 flex flex-col gap-1">
                    <span className="text-[11px] font-bold text-emerald-400 flex items-center gap-1.5">
                      <CheckCircle2 className="w-3.5 h-3.5" />
                      The Smart Way: 30–60s Bursts
                    </span>
                    <span className="text-[11px] text-white/70 leading-normal">
                      Speak a few sentences, release the key to let Ivy paste instantly, then press again. Instant results and near-zero errors.
                    </span>
                  </div>

                  <div className="rounded-xl p-2.5 bg-amber-500/10 border border-amber-500/25 flex flex-col gap-1">
                    <span className="text-[11px] font-bold text-amber-400 flex items-center gap-1.5">
                      <Info className="w-3.5 h-3.5" />
                      The Trap: 5+ Minute Monologues
                    </span>
                    <span className="text-[11px] text-white/70 leading-normal">
                      Speaking continuously for 5–6 minutes builds huge audio buffers, delays processing, and increases transcription fatigue.
                    </span>
                  </div>
                </div>
              </div>

              {/* Navigation Footer */}
              <div className="w-full flex items-center justify-between mt-5 px-1">
                <button
                  id="stage-5-back-btn"
                  onClick={goToPrevStage}
                  type="button"
                  className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/70 hover:text-white text-xs sm:text-sm font-semibold border border-white/10 transition-all flex items-center gap-2 cursor-pointer"
                >
                  <ArrowLeft className="w-4 h-4" />
                  <span>Back</span>
                </button>

                <button
                  id="stage-5-next-btn"
                  onClick={goToNextStage}
                  type="button"
                  className="px-7 py-3 rounded-full text-white text-sm sm:text-base font-bold flex items-center gap-2.5 transition-all cursor-pointer hover:brightness-110 active:scale-98"
                  style={{
                    background: 'linear-gradient(to right, #FF6B00, #E05300)',
                    boxShadow:
                      '0 0 35px rgba(255, 107, 0, 0.65), inset 0 1.5px 2px rgba(255, 255, 255, 0.45)',
                    border: '1px solid rgba(255, 161, 51, 0.6)',
                  }}
                >
                  <span>Next: Touch Up</span>
                  <ArrowRight className="w-4 h-4 stroke-[2.5]" />
                </button>
              </div>
            </motion.div>
          )}

          {/* =========================================================================
              STAGE 4: TOUCH UP — optional, on-demand AI proofread
              ========================================================================= */}
          {currentStage === 4 && (
            <motion.div
              key="stage-6"
              initial={{ opacity: 0, y: 15 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -15 }}
              transition={{ duration: 0.28, ease: 'easeOut' }}
              className="flex-1 flex flex-col items-center justify-between w-full max-w-xl mx-auto py-2"
            >
              <div className="text-center flex flex-col items-center gap-2 mb-5">
                <div
                  className="w-14 h-14 rounded-2xl flex items-center justify-center mb-1"
                  style={{
                    background: 'linear-gradient(135deg, rgba(255,107,0,0.2) 0%, rgba(18,13,26,0.8) 100%)',
                    border: '1.5px solid rgba(255,107,0,0.45)',
                    boxShadow: '0 0 40px rgba(255,107,0,0.3), inset 0 1px 1px rgba(255,255,255,0.15)',
                  }}
                >
                  <Wand2 className="w-7 h-7 text-[#FFA133]" />
                </div>
                <span
                  className="px-3 py-1 rounded-full text-[10px] sm:text-[11px] font-bold uppercase tracking-wider"
                  style={{
                    backgroundColor: 'rgba(255, 107, 0, 0.15)',
                    color: '#FFA133',
                    border: '1px solid rgba(255, 107, 0, 0.35)',
                  }}
                >
                  Step 4 of 7: Touch Up
                </span>
                <h3 className="text-xl sm:text-2xl font-bold text-white tracking-tight">
                  Something look off? One click fixes it.
                </h3>
                <p className="text-xs sm:text-sm text-white/60 max-w-md">
                  In Accuracy mode, a small button appears on the capsule for a few seconds right
                  after Ivy pastes. Click it only if you notice something — most of the time you
                  won't need to.
                </p>
              </div>

              {/* Before / after example card — illustrative only, not a live demo:
                  this is what the button looks like and what it does, no mic needed. */}
              <div
                className="w-full rounded-2xl p-4 sm:p-5 flex flex-col gap-3.5"
                style={{
                  backgroundColor: 'rgba(18, 13, 26, 0.85)',
                  border: '1px solid rgba(255, 107, 0, 0.28)',
                  boxShadow: '0 8px 30px rgba(0,0,0,0.5), 0 0 24px rgba(255,107,0,0.1)',
                }}
              >
                <div className="flex flex-col gap-1.5">
                  <span className="text-[10px] font-bold uppercase tracking-wider text-white/40">
                    Pasted
                  </span>
                  <p className="text-[13px] text-white/70 font-mono leading-relaxed">
                    so the the meeting is at 5 pm we need to finish the report before then
                  </p>
                </div>

                <div className="flex items-center gap-2 py-1">
                  <div className="h-px flex-1 bg-white/[0.08]" />
                  <div
                    className="flex items-center gap-1.5 px-3 py-1 rounded-full text-[11px] font-medium text-white/90"
                    style={{ backgroundColor: 'rgba(255,107,0,0.12)', border: '1px solid rgba(255,107,0,0.3)' }}
                  >
                    <Wand2 className="w-3 h-3 text-[#FFA133]" />
                    <span>Touch Up</span>
                  </div>
                  <div className="h-px flex-1 bg-white/[0.08]" />
                </div>

                <div className="flex flex-col gap-1.5">
                  <span className="text-[10px] font-bold uppercase tracking-wider text-emerald-400/80">
                    After clicking
                  </span>
                  <p className="text-[13px] text-white/90 font-mono leading-relaxed">
                    So the meeting is at 5 PM. We need to finish the report before then.
                  </p>
                </div>
              </div>

              <div className="w-full flex flex-col gap-2.5 mt-4">
                <div className="rounded-xl p-2.5 bg-emerald-500/10 border border-emerald-500/25 flex flex-col gap-1">
                  <span className="text-[11px] font-bold text-emerald-400 flex items-center gap-1.5">
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    What it fixes
                  </span>
                  <span className="text-[11px] text-white/70 leading-normal">
                    Missing periods and question marks from natural speaking pauses, and a stray
                    repeated word from a stutter that slipped through.
                  </span>
                </div>
                <div className="rounded-xl p-2.5 bg-amber-500/10 border border-amber-500/25 flex flex-col gap-1">
                  <span className="text-[11px] font-bold text-amber-400 flex items-center gap-1.5">
                    <Info className="w-3.5 h-3.5" />
                    What it never does
                  </span>
                  <span className="text-[11px] text-white/70 leading-normal">
                    Change a single word you actually said. Not a rewrite — if it can't fix
                    something safely, it changes nothing at all.
                  </span>
                </div>
                <div className="rounded-xl p-2.5 bg-blue-500/10 border border-blue-500/25 flex flex-col gap-1">
                  <span className="text-[11px] font-bold text-blue-400 flex items-center gap-1.5">
                    <Cpu className="w-3.5 h-3.5" />
                    Accuracy mode
                  </span>
                  <span className="text-[11px] text-white/70 leading-normal">
                    Offered after real pastes in Accuracy mode on both GPU and CPU. Never runs automatically, never changes your words, and dismisses after 5s if unclicked.
                  </span>
                </div>
              </div>

              {/* Navigation Footer */}
              <div className="w-full flex items-center justify-between mt-5 px-1">
                <button
                  id="stage-6-back-btn"
                  onClick={goToPrevStage}
                  type="button"
                  className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/70 hover:text-white text-xs sm:text-sm font-semibold border border-white/10 transition-all flex items-center gap-2 cursor-pointer"
                >
                  <ArrowLeft className="w-4 h-4" />
                  <span>Back</span>
                </button>

                <button
                  id="stage-6-next-btn"
                  onClick={goToNextStage}
                  type="button"
                  className="px-7 py-3 rounded-full text-white text-sm sm:text-base font-bold flex items-center gap-2.5 transition-all cursor-pointer hover:brightness-110 active:scale-98"
                  style={{
                    background: 'linear-gradient(to right, #FF6B00, #E05300)',
                    boxShadow:
                      '0 0 35px rgba(255, 107, 0, 0.65), inset 0 1.5px 2px rgba(255, 255, 255, 0.45)',
                    border: '1px solid rgba(255, 161, 51, 0.6)',
                  }}
                >
                  <span>Next: Test Your Voice</span>
                  <ArrowRight className="w-4 h-4 stroke-[2.5]" />
                </button>
              </div>
            </motion.div>
          )}

          {/* =========================================================================
              STAGE 7: PRIVACY & SAFETY — dedicated full page
              ========================================================================= */}
          {currentStage === 7 && (
            <motion.div
              key="stage-7"
              initial={{ opacity: 0, y: 15 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -15 }}
              transition={{ duration: 0.28, ease: 'easeOut' }}
              className="flex-1 flex flex-col items-center justify-between w-full max-w-xl mx-auto py-2"
            >
              {/* Hero shield icon */}
              <div className="flex flex-col items-center gap-3 mb-5">
                <div
                  className="w-16 h-16 rounded-2xl flex items-center justify-center"
                  style={{
                    background: 'linear-gradient(135deg, rgba(255,107,0,0.2) 0%, rgba(18,13,26,0.8) 100%)',
                    border: '1.5px solid rgba(255,107,0,0.45)',
                    boxShadow: '0 0 40px rgba(255,107,0,0.3), inset 0 1px 1px rgba(255,255,255,0.15)',
                  }}
                >
                  <ShieldCheck className="w-8 h-8 text-[#FFA133]" />
                </div>

                <div className="text-center">
                  <span
                    className="px-3 py-1 rounded-full text-[10px] sm:text-[11px] font-bold uppercase tracking-wider"
                    style={{
                      backgroundColor: 'rgba(255, 107, 0, 0.15)',
                      color: '#FFA133',
                      border: '1px solid rgba(255, 107, 0, 0.35)',
                    }}
                  >
                    Step 7 of 7: Privacy &amp; Safety
                  </span>
                  <h3
                    className="text-2xl sm:text-3xl font-extrabold text-white mt-2 tracking-tight"
                    style={{ fontFamily: 'Syne, sans-serif' }}
                  >
                    Your data is 100% safe.
                  </h3>
                  <p className="text-sm text-white/60 mt-1 max-w-sm mx-auto">
                    IVY is not a service. It is software that runs entirely on your machine — nothing leaves.
                  </p>
                </div>
              </div>

              {/* Four guarantee cards */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 w-full mb-4">
                {/* Card 1 — 100% Offline */}
                <div
                  className="rounded-2xl p-4 border flex flex-col gap-2"
                  style={{
                    backgroundColor: 'rgba(18,13,26,0.85)',
                    borderColor: 'rgba(255,107,0,0.28)',
                    boxShadow: '0 4px 20px rgba(0,0,0,0.5), 0 0 18px rgba(255,107,0,0.08)',
                  }}
                >
                  <div className="flex items-center gap-2.5">
                    <div className="w-8 h-8 rounded-xl bg-[#FF6B00]/15 border border-[#FF6B00]/30 flex items-center justify-center text-[#FFA133] shrink-0">
                      <WifiOff className="w-4 h-4" />
                    </div>
                    <span className="text-sm font-bold text-white">100% Offline</span>
                  </div>
                  <p className="text-[11.5px] text-white/60 leading-relaxed">
                    Zero internet connection required at runtime. Your voice never touches a server,
                    an API, or a cloud service of any kind.
                  </p>
                  <span className="text-[10px] font-mono text-[#FFA133]/80 flex items-center gap-1 mt-1">
                    <Lock className="w-3 h-3" /> No network calls. Ever.
                  </span>
                </div>

                {/* Card 2 — Auto-deleted every 24 h */}
                <div
                  className="rounded-2xl p-4 border flex flex-col gap-2"
                  style={{
                    backgroundColor: 'rgba(18,13,26,0.85)',
                    borderColor: 'rgba(255,107,0,0.28)',
                    boxShadow: '0 4px 20px rgba(0,0,0,0.5), 0 0 18px rgba(255,107,0,0.08)',
                  }}
                >
                  <div className="flex items-center gap-2.5">
                    <div className="w-8 h-8 rounded-xl bg-[#FF6B00]/15 border border-[#FF6B00]/30 flex items-center justify-center text-[#FFA133] shrink-0">
                      <Trash2 className="w-4 h-4" />
                    </div>
                    <span className="text-sm font-bold text-white">Auto-deleted in 24 h</span>
                  </div>
                  <p className="text-[11.5px] text-white/60 leading-relaxed">
                    Every voice recording and transcript is automatically purged after 24 hours.
                    Nothing lingers. Nothing accumulates.
                  </p>
                  <span className="text-[10px] font-mono text-[#FFA133]/80 flex items-center gap-1 mt-1">
                    <CheckCircle2 className="w-3 h-3" /> Daily auto-purge, always on.
                  </span>
                </div>

                {/* Card 3 — Clear anytime */}
                <div
                  className="rounded-2xl p-4 border flex flex-col gap-2"
                  style={{
                    backgroundColor: 'rgba(18,13,26,0.85)',
                    borderColor: 'rgba(255,107,0,0.28)',
                    boxShadow: '0 4px 20px rgba(0,0,0,0.5), 0 0 18px rgba(255,107,0,0.08)',
                  }}
                >
                  <div className="flex items-center gap-2.5">
                    <div className="w-8 h-8 rounded-xl bg-[#FF6B00]/15 border border-[#FF6B00]/30 flex items-center justify-center text-[#FFA133] shrink-0">
                      <Eye className="w-4 h-4" />
                    </div>
                    <span className="text-sm font-bold text-white">Clear Whenever You Want</span>
                  </div>
                  <p className="text-[11.5px] text-white/60 leading-relaxed">
                    The History page has a one-tap "Clear All" button. Your recordings and
                    transcripts are gone instantly — your streaks and word count stay untouched.
                  </p>
                  <span className="text-[10px] font-mono text-[#FFA133]/80 flex items-center gap-1 mt-1">
                    <CheckCircle2 className="w-3 h-3" /> Recordings clear. Progress stays.
                  </span>
                </div>

                {/* Card 4 — Stays on your machine */}
                <div
                  className="rounded-2xl p-4 border flex flex-col gap-2"
                  style={{
                    backgroundColor: 'rgba(18,13,26,0.85)',
                    borderColor: 'rgba(255,107,0,0.28)',
                    boxShadow: '0 4px 20px rgba(0,0,0,0.5), 0 0 18px rgba(255,107,0,0.08)',
                  }}
                >
                  <div className="flex items-center gap-2.5">
                    <div className="w-8 h-8 rounded-xl bg-[#FF6B00]/15 border border-[#FF6B00]/30 flex items-center justify-center text-[#FFA133] shrink-0">
                      <HardDrive className="w-4 h-4" />
                    </div>
                    <span className="text-sm font-bold text-white">Stays On Your Machine</span>
                  </div>
                  <p className="text-[11.5px] text-white/60 leading-relaxed">
                    All data lives in your local app folder — history, settings, stats. There are
                    no accounts, no logins, no telemetry, and no analytics.
                  </p>
                  <span className="text-[10px] font-mono text-[#FFA133]/80 flex items-center gap-1 mt-1">
                    <CheckCircle2 className="w-3 h-3" /> Open source &amp; auditable.
                  </span>
                </div>
              </div>

              {/* Bottom trust statement */}
              <div
                className="w-full rounded-2xl px-5 py-3.5 border flex items-center gap-3 mb-4"
                style={{
                  background: 'linear-gradient(90deg, rgba(255,107,0,0.08) 0%, rgba(18,13,26,0.6) 100%)',
                  borderColor: 'rgba(255,107,0,0.3)',
                }}
              >
                <ShieldCheck className="w-5 h-5 text-[#FFA133] shrink-0" />
                <p className="text-[12px] text-white/75 leading-relaxed">
                  <span className="font-bold text-white">IVY is offline-first by design.</span>{' '}
                  Your voice recordings are processed locally by Whisper, transcribed on your
                  GPU or CPU, and never transmitted anywhere — not even anonymously.
                </p>
              </div>

              {/* Navigation Footer */}
              <div className="w-full flex items-center justify-between mt-auto pt-2 px-1">
                <button
                  id="stage-7-back-btn"
                  onClick={goToPrevStage}
                  type="button"
                  className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/70 hover:text-white text-xs sm:text-sm font-semibold border border-white/10 transition-all flex items-center gap-2 cursor-pointer"
                >
                  <ArrowLeft className="w-4 h-4" />
                  <span>Back</span>
                </button>

                <button
                  id="start-using-ivy-btn"
                  onClick={goToNextStage}
                  type="button"
                  className="px-9 py-3 rounded-full text-white text-sm sm:text-base font-bold flex items-center gap-2.5 transition-all cursor-pointer hover:brightness-110 active:scale-98"
                  style={{
                    background: 'linear-gradient(to right, #FF6B00, #E05300)',
                    boxShadow:
                      '0 0 40px rgba(255, 107, 0, 0.7), inset 0 1.5px 2px rgba(255, 255, 255, 0.45)',
                    border: '1px solid rgba(255, 161, 51, 0.6)',
                  }}
                >
                  <span>Start Using Ivy</span>
                  <ArrowRight className="w-4 h-4 stroke-[2.5]" />
                </button>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </main>

      {/* Footer Branding */}
      <footer className="relative z-10 w-full max-w-2xl text-center py-1 text-white/30 text-[11px] flex items-center justify-center gap-3 shrink-0">
        <span>IVY Transcriber v0.1.0</span>
        <span>•</span>
        <span>Push-To-Talk Engine</span>
        <span>•</span>
        <span>Tauri v2 + Rust</span>
      </footer>
    </div>
  );
};
