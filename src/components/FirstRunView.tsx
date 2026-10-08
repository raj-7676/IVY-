import React, { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  ArrowLeft,
  ArrowRight,
  Check,
  CheckCircle2,
  ClipboardCheck,
  Cpu,
  Eye,
  FileText,
  HardDrive,
  Info,
  Lock,
  Mic,
  ShieldCheck,
  Trash2,
  Wand2,
  WifiOff,
  Zap,
} from 'lucide-react';
import { SettingsConfig, HardwareMode } from '../types';
import { StageIndicator, OnboardingStage } from './StageIndicator';
import { HotkeyBadge } from './HotkeyBadge';
import { SoundwaveVisualizer, DictationVisualState } from './SoundwaveVisualizer';
import { IvyWordmark } from './IvyWordmark';

// Setup wizard, rebuilt 2026-10-06. Seven steps: shortcut, held-back text, GPU/CPU, Touch Up, voice test,
// self-correction, privacy. Both voice steps share one engine (`useWizardDictation`) that follows the
// backend's own events, so the hotkey, a quick tap, a double-press and the on-screen button all behave the
// same. Practice dictations made with this window in front are never pasted or saved (lib.rs `for_wizard`).

interface FirstRunViewProps {
  onDismiss: () => void;
  onComplete: () => void;
  hotkey?: string;
  manualPasteHotkey?: string;
  hardwareMode?: HardwareMode;
  onUpdateSettings?: (newSettings: Partial<SettingsConfig>) => void;
}

const ACCENT_RGB = '255, 107, 0';
const TEST_PHRASE = 'Hi, my name is James.';
const CORRECTION_PHRASE = 'Hi, I am James. I want to order French fries. No, no, I want a burger.';
const LAST_STAGE: OnboardingStage = 7;
// A minute of speech takes ~20 s on CPU; anything far past that means the backend never answered.
const NO_ANSWER_MS = 90_000;

const HOTKEY_CHOICES = [
  { key: 'Alt + Space', label: 'Default', desc: 'Easy thumb + finger' },
  { key: 'Ctrl + Shift', label: 'Alternative', desc: 'Hold both keys' },
];

// ---------------------------------------------------------------------------------------------------------
// Shared engine for the two voice steps
// ---------------------------------------------------------------------------------------------------------

interface DictationComplete {
  success: boolean;
  reason?: string;
  text?: string;
}

/** One practice dictation at a time, driven only by backend events:
 *  hotkey-down -> listening, hotkey-up -> transcribing, dictation-complete -> success/failed,
 *  dictation-cancelled (a quick tap) -> back to waiting. Only reacts while `active` (its step is on screen);
 *  presses made while another app is in front are real dictations for that app and are ignored here. */
function useWizardDictation(hotkey: string, active: boolean) {
  const [state, setState] = useState<DictationVisualState>('waiting');
  const [text, setText] = useState('');
  const [message, setMessage] = useState<string | null>(null);
  const [seconds, setSeconds] = useState(0);
  const [level, setLevel] = useState(0);
  const stateRef = useRef<DictationVisualState>('waiting');
  const mine = useRef(false); // the dictation in flight was started from this window
  const holding = useRef(false); // the on-screen button is held down
  const tick = useRef<number | undefined>(undefined);
  const noAnswer = useRef<number | undefined>(undefined);
  const hotkeyRef = useRef(hotkey);
  hotkeyRef.current = hotkey;
  const activeRef = useRef(active);
  activeRef.current = active;

  const go = (next: DictationVisualState) => {
    stateRef.current = next;
    setState(next);
    if (next !== 'listening') {
      clearInterval(tick.current);
      tick.current = undefined;
    }
    if (next !== 'transcribing') {
      clearTimeout(noAnswer.current);
      noAnswer.current = undefined;
    }
  };

  useEffect(() => {
    const subs = [
      listen('ivy://hotkey-down', () => {
        if (!activeRef.current || !document.hasFocus() || stateRef.current === 'transcribing') return;
        mine.current = true;
        setMessage(null);
        setSeconds(0);
        setLevel(0);
        go('listening');
        const started = Date.now();
        tick.current = window.setInterval(() => setSeconds(Math.floor((Date.now() - started) / 1000)), 250);
      }),
      listen('ivy://hotkey-up', () => {
        if (!mine.current || stateRef.current !== 'listening') return;
        go('transcribing');
        noAnswer.current = window.setTimeout(() => {
          mine.current = false;
          setMessage("Ivy didn't answer. Please try again.");
          go('failed');
        }, NO_ANSWER_MS);
      }),
      listen('ivy://dictation-cancelled', () => {
        if (!mine.current || stateRef.current !== 'listening') return;
        mine.current = false;
        setMessage(`Keep holding ${hotkeyRef.current} while you speak, then let go.`);
        go('waiting');
      }),
      listen<DictationComplete>('ivy://dictation-complete', (e) => {
        const p = e.payload;
        // A mic that won't open fails before any hotkey-down, so it can't be "mine" yet.
        const micFailedHere = p.reason === 'mic_error' && activeRef.current && document.hasFocus();
        if (!mine.current && !micFailedHere) return;
        mine.current = false;
        const heard = (p.text ?? '').trim();
        if (p.success && heard) {
          setText(heard);
          setMessage(null);
          go('success');
        } else {
          setMessage(
            p.reason === 'mic_error'
              ? "Couldn't open your microphone. Check Settings → Microphone."
              : `Didn't catch any speech. Hold ${hotkeyRef.current}, speak, then let go.`,
          );
          go('failed');
        }
      }),
      listen<number>('ivy://mic-level', (e) => {
        if (stateRef.current === 'listening') setLevel(e.payload);
      }),
    ];
    return () => {
      subs.forEach((s) => s.then((off) => off()).catch(() => undefined));
      clearInterval(tick.current);
      clearTimeout(noAnswer.current);
      // Leaving the wizard mid-recording: same.
      if (mine.current && stateRef.current === 'listening') invoke('cancel_dictation').catch(() => undefined);
    };
  }, []);

  // Leaving the step mid-recording (Back, Next, the step pills): drop it, or the mic would stay open.
  useEffect(() => {
    if (active || !mine.current || stateRef.current !== 'listening') return;
    mine.current = false;
    holding.current = false;
    invoke('cancel_dictation').catch(() => undefined);
    go('waiting');
  }, [active]);

  // On-screen hold-to-talk button: the same backend path as the hotkey (its events drive the state).
  const holdStart = (e: React.PointerEvent<HTMLButtonElement>) => {
    if (holding.current || stateRef.current === 'listening' || stateRef.current === 'transcribing') return;
    e.currentTarget.setPointerCapture(e.pointerId);
    holding.current = true;
    invoke('start_manual_dictation').catch(() => undefined);
  };
  const holdEnd = () => {
    if (!holding.current) return;
    holding.current = false;
    invoke('stop_manual_dictation').catch(() => undefined);
  };
  useEffect(() => {
    window.addEventListener('blur', holdEnd); // Alt-Tab away while holding the button
    return () => window.removeEventListener('blur', holdEnd);
  }, []);

  const bars = [0.25, 0.45, 0.7, 0.95, 1.0, 0.85, 0.65, 0.4, 0.2].map((b) => Math.min(1, b * Math.sqrt(level) * 2));
  return { state, text, setText, message, seconds, bars, holdStart, holdEnd };
}

// ---------------------------------------------------------------------------------------------------------
// Building blocks
// ---------------------------------------------------------------------------------------------------------

const glassCard: React.CSSProperties = {
  backgroundColor: 'rgba(18, 13, 26, 0.82)',
  backdropFilter: 'blur(30px) saturate(160%)',
  WebkitBackdropFilter: 'blur(30px) saturate(160%)',
  borderColor: 'rgba(255, 107, 0, 0.32)',
  boxShadow:
    '0 25px 60px -15px rgba(0,0,0,0.85), 0 0 35px rgba(255,107,0,0.18), inset 0 1.5px 2px 0 rgba(255,255,255,0.3), inset 0 0 0 1px rgba(255,255,255,0.06)',
};

const Card: React.FC<{ children: React.ReactNode; className?: string }> = ({ children, className = '' }) => (
  <div className={`w-full rounded-3xl p-6 sm:p-7 border relative overflow-hidden ${className}`} style={glassCard}>
    <div
      className="pointer-events-none absolute top-0 left-8 right-8 h-[1px]"
      style={{
        background:
          'linear-gradient(90deg, transparent 0%, rgba(255,255,255,0.4) 30%, rgba(255,107,0,0.7) 50%, rgba(255,255,255,0.4) 70%, transparent 100%)',
      }}
    />
    {children}
  </div>
);

const Heading: React.FC<{ title: string; subtitle: React.ReactNode; icon?: React.ReactNode }> = ({ title, subtitle, icon }) => (
  <div className="mb-5 flex flex-col items-center text-center gap-2">
    {icon && (
      <div
        className="w-14 h-14 rounded-2xl flex items-center justify-center mb-1 text-[#FFA133]"
        style={{
          background: 'linear-gradient(135deg, rgba(255,107,0,0.2) 0%, rgba(18,13,26,0.8) 100%)',
          border: '1.5px solid rgba(255,107,0,0.45)',
          boxShadow: '0 0 40px rgba(255,107,0,0.3), inset 0 1px 1px rgba(255,255,255,0.15)',
        }}
      >
        {icon}
      </div>
    )}
    <h1 className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white" style={{ fontFamily: 'Syne, sans-serif' }}>
      {title}
    </h1>
    <p className="text-white/60 text-sm sm:text-base max-w-md">{subtitle}</p>
  </div>
);

const Footer: React.FC<{
  onBack?: () => void;
  onNext: () => void;
  nextLabel: string;
  quietNext?: boolean;
}> = ({ onBack, onNext, nextLabel, quietNext }) => (
  <div className="w-full flex items-center justify-between mt-5 px-1">
    {onBack ? (
      <button
        onClick={onBack}
        type="button"
        className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/70 hover:text-white text-xs sm:text-sm font-semibold border border-white/10 transition-all flex items-center gap-2 cursor-pointer"
      >
        <ArrowLeft className="w-4 h-4" />
        <span>Back</span>
      </button>
    ) : (
      <span />
    )}
    {quietNext ? (
      <button
        onClick={onNext}
        type="button"
        className="px-5 py-2 rounded-full bg-white/[0.06] hover:bg-white/[0.12] text-white/60 hover:text-white text-xs sm:text-sm font-medium border border-white/10 transition-all cursor-pointer"
      >
        {nextLabel}
      </button>
    ) : (
      <button
        onClick={onNext}
        type="button"
        className="px-7 py-3 rounded-full text-white text-sm sm:text-base font-bold flex items-center gap-2.5 transition-all cursor-pointer hover:brightness-110 active:scale-98"
        style={{
          background: 'linear-gradient(to right, #FF6B00, #E05300)',
          boxShadow: '0 0 35px rgba(255, 107, 0, 0.65), inset 0 1.5px 2px rgba(255, 255, 255, 0.45)',
          border: '1px solid rgba(255, 161, 51, 0.6)',
        }}
      >
        <span>{nextLabel}</span>
        <ArrowRight className="w-4 h-4 stroke-[2.5]" />
      </button>
    )}
  </div>
);

const Note: React.FC<{ tone: 'good' | 'warn' | 'info'; title: string; children: React.ReactNode; icon: React.ReactNode }> = ({
  tone,
  title,
  children,
  icon,
}) => {
  const colors = {
    good: 'bg-emerald-500/10 border-emerald-500/25 text-emerald-400',
    warn: 'bg-amber-500/10 border-amber-500/25 text-amber-400',
    info: 'bg-blue-500/10 border-blue-500/25 text-blue-400',
  }[tone];
  return (
    <div className={`rounded-xl p-2.5 border flex flex-col gap-1 text-left ${colors}`}>
      <span className="text-[11px] font-bold flex items-center gap-1.5">
        {icon}
        {title}
      </span>
      <span className="text-[11px] text-white/70 leading-normal">{children}</span>
    </div>
  );
};

/** The live part of both voice steps: the shortcut, the line to say, the meter, status, and the button. */
const VoicePanel: React.FC<{
  hotkey: string;
  phraseLabel: string;
  phrase: string;
  workingText: string;
  dictation: ReturnType<typeof useWizardDictation>;
  result: React.ReactNode;
}> = ({ hotkey, phraseLabel, phrase, workingText, dictation, result }) => {
  const { state, message, seconds, bars, holdStart, holdEnd } = dictation;
  const busy = state === 'listening' || state === 'transcribing';
  return (
    <Card className="flex flex-col items-center gap-5">
      <div className="flex flex-col items-center gap-2">
        <span className="text-[11px] uppercase tracking-wider text-white/50 font-semibold">Hold to talk</span>
        <HotkeyBadge hotkey={hotkey} isPressed={state === 'listening'} size="large" />
      </div>

      <div className="text-center">
        <span className="text-[10.5px] uppercase tracking-wider text-[#FFA133] font-semibold">{phraseLabel}</span>
        <div className="text-sm sm:text-base font-bold text-white/90 tracking-wide font-mono select-text mt-0.5 max-w-lg">
          "{phrase}"
        </div>
      </div>

      {result}

      <div className="w-full flex flex-col items-center min-h-[96px] justify-center">
        <SoundwaveVisualizer state={state} levels={bars} audioActive />
        <div className="mt-1 text-sm font-medium min-h-[24px]">
          {state === 'listening' && (
            <span className="text-[#FF7A00] font-semibold flex items-center gap-2">
              <Mic className="w-4 h-4 animate-pulse" />
              Listening… ({seconds}s)
            </span>
          )}
          {state === 'transcribing' && (
            <span className="text-amber-300 font-semibold flex items-center gap-2">
              <Cpu className="w-4 h-4 animate-spin" />
              {workingText}
            </span>
          )}
          {!busy && message && (
            <span className="text-amber-300 text-xs sm:text-sm flex items-center gap-2 px-3.5 py-1 rounded-full bg-amber-500/10 border border-amber-500/25">
              <Info className="w-3.5 h-3.5 shrink-0" />
              {message}
            </span>
          )}
          {state === 'waiting' && !message && <span className="text-white/60">Hold {hotkey}, say the line, then let go.</span>}
        </div>
      </div>

      <div className="w-full flex flex-col items-center pt-3 border-t border-white/[0.08]">
        <button
          type="button"
          onPointerDown={holdStart}
          onPointerUp={holdEnd}
          onPointerCancel={holdEnd}
          onLostPointerCapture={holdEnd}
          disabled={state === 'transcribing'}
          className={`w-full max-w-sm py-2.5 px-5 rounded-2xl font-semibold text-xs sm:text-sm transition-all flex items-center justify-center gap-2 select-none touch-none ${
            state === 'listening'
              ? 'bg-[#FF6B00] text-white shadow-[0_0_24px_#FF6B00] border border-[#FFA133]'
              : state === 'transcribing'
              ? 'bg-[#15101E]/60 text-white/40 border border-white/[0.06] cursor-wait'
              : 'bg-[#15101E] hover:bg-[#1E172B] text-white/80 border border-white/[0.1] hover:border-[#FF6B00]/40 cursor-pointer'
          }`}
        >
          <Mic className="w-4 h-4 text-[#FFA133]" />
          {state === 'listening' ? 'Let go to finish' : state === 'transcribing' ? 'Working…' : 'Or hold this button and talk'}
        </button>
        <span className="text-[11px] text-white/40 mt-1.5">Keep this window in front while you test.</span>
      </div>
    </Card>
  );
};

// ---------------------------------------------------------------------------------------------------------
// The seven steps
// ---------------------------------------------------------------------------------------------------------

const StepShortcut: React.FC<{ hotkey: string; onPick: (key: string) => void }> = ({ hotkey, onPick }) => {
  // Pressing the real shortcut lights the badge (the backend owns the key; the page never sees it).
  const [down, setDown] = useState(false);
  const [heard, setHeard] = useState(false);
  useEffect(() => {
    const up = () => setDown(false);
    const subs = [
      listen('ivy://hotkey-down', () => {
        if (!document.hasFocus()) return;
        setDown(true);
        setHeard(true);
      }),
      listen('ivy://hotkey-up', up),
      listen('ivy://dictation-cancelled', up),
      listen('ivy://dictation-complete', up),
    ];
    return () => subs.forEach((s) => s.then((off) => off()).catch(() => undefined));
  }, []);
  useEffect(() => setHeard(false), [hotkey]);

  return (
    <>
      <Heading title="Your push-to-talk key" subtitle="Hold it anywhere, talk, let go, and Ivy types what you said." />
      <Card className="flex flex-col items-center gap-5">
        <div className="flex flex-col items-center gap-2">
          <span className="text-[11px] uppercase tracking-wider text-white/50 font-semibold">Your shortcut</span>
          <div
            className="p-3.5 rounded-2xl border shadow-inner flex items-center justify-center min-w-[240px]"
            style={{ backgroundColor: 'rgba(11, 8, 18, 0.9)', borderColor: 'rgba(255, 107, 0, 0.25)' }}
          >
            <HotkeyBadge hotkey={hotkey} isPressed={down} size="large" />
          </div>
          <span className={`text-xs ${heard ? 'text-emerald-400' : 'text-[#FFA133]/90'}`}>
            {heard ? 'Ivy heard your shortcut.' : 'Press it now to try it.'}
          </span>
        </div>

        <div className="w-full text-left">
          <span className="text-[11px] uppercase tracking-wider text-white/50 font-semibold mb-2.5 block">Pick one</span>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            {HOTKEY_CHOICES.map((item) => {
              const picked = hotkey === item.key;
              return (
                <button
                  key={item.key}
                  type="button"
                  onClick={() => !picked && onPick(item.key)}
                  className={`p-3.5 rounded-2xl text-left border transition-all cursor-pointer flex flex-col ${
                    picked
                      ? 'bg-[#FF6B00]/15 border-[#FF6B00] shadow-[0_0_20px_rgba(255,107,0,0.25)]'
                      : 'bg-white/[0.03] border-white/[0.08] hover:border-[rgba(255,107,0,0.35)] hover:bg-white/[0.06]'
                  }`}
                >
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-white/[0.08] text-white/70">{item.label}</span>
                    {picked && (
                      <span className="w-4 h-4 rounded-full bg-[#FF6B00] flex items-center justify-center text-white">
                        <Check className="w-2.5 h-2.5 stroke-[3]" />
                      </span>
                    )}
                  </div>
                  <div className="font-mono font-bold text-white text-sm mb-0.5">{item.key}</div>
                  <div className="text-[11px] text-white/45">{item.desc}</div>
                </button>
              );
            })}
          </div>
        </div>

        <div className="w-full rounded-2xl p-3.5 border flex items-start gap-3 text-left" style={{ backgroundColor: 'rgba(11, 8, 18, 0.7)', borderColor: 'rgba(255,255,255,0.08)' }}>
          <Info className="w-4 h-4 text-[#FFA133] shrink-0 mt-0.5" />
          <div className="text-xs text-white/70 leading-relaxed">
            <strong className="text-white">Hands-free:</strong> press the shortcut twice quickly and talk without holding;
            press it once more to finish. Change the shortcut anytime in{' '}
            <span className="text-[#FFA133] font-semibold">Settings</span>.
          </div>
        </div>
      </Card>
    </>
  );
};

const StepClipboard: React.FC<{ manualPasteHotkey: string }> = ({ manualPasteHotkey }) => (
  <>
    <Heading title="No text box? No problem." subtitle="Ivy types where your cursor is, and never loses what you said." />
    <div className="w-full grid grid-cols-1 md:grid-cols-2 gap-4">
      {[
        {
          icon: <FileText className="w-4 h-4" />,
          tag: 'Cursor in a text box',
          title: 'Typed right in',
          body: 'In Docs, Slack, Word, a browser, anywhere you can type: the text appears at your cursor.',
          window: 'Document.docx',
          badge: null,
        },
        {
          icon: <ClipboardCheck className="w-4 h-4" />,
          tag: 'No text box',
          title: 'Held for you',
          body: (
            <>
              Ivy never overwrites your clipboard (it might hold a password). It keeps the text and you press{' '}
              <kbd className="font-mono text-white bg-white/[0.1] px-1 rounded">{manualPasteHotkey}</kbd> to paste it where you want.
            </>
          ),
          window: 'Held by Ivy',
          badge: 'Waiting',
        },
      ].map((c) => (
        <div key={c.tag} className="rounded-3xl p-5 border text-left flex flex-col justify-between border-white/[0.08] bg-[#120D1A]/70">
          <div>
            <div className="w-9 h-9 rounded-2xl bg-[#FF6B00]/15 border border-[#FF6B00]/30 flex items-center justify-center text-[#FFA133] mb-3.5">
              {c.icon}
            </div>
            <div className="text-[10.5px] uppercase tracking-wider text-[#FFA133] font-semibold mb-1">{c.tag}</div>
            <h3 className="text-sm sm:text-base font-bold text-white mb-1.5">{c.title}</h3>
            <p className="text-xs text-white/65 leading-relaxed mb-3.5">{c.body}</p>
          </div>
          <div className="bg-[#09070D] rounded-xl p-3 border border-white/[0.08] text-xs font-mono">
            <div className="flex items-center justify-between mb-2 pb-1.5 border-b border-white/[0.08]">
              <span className="text-[10px] text-white/40">{c.window}</span>
              {c.badge && <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-400 font-sans">{c.badge}</span>}
            </div>
            <div className="text-white/80 text-[11px] truncate py-1">{TEST_PHRASE}</div>
          </div>
        </div>
      ))}
    </div>
  </>
);

const StepHardware: React.FC<{ mode: HardwareMode; onPick: (m: HardwareMode) => void }> = ({ mode, onPick }) => (
  <>
    <Heading title="Where should Ivy run?" subtitle="Same model and same results either way; the GPU is just faster. Change it anytime in Settings." />
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3.5 w-full">
      {(
        [
          {
            id: 'gpu' as HardwareMode,
            icon: <Zap className="w-5 h-5" />,
            name: 'GPU',
            tag: 'Best if you have a graphics card',
            body: 'Runs on your graphics card (NVIDIA, AMD or Intel). A 1-minute dictation is ready in about 2 seconds.',
            foot: '⚡ Fastest. Steps aside for games and full-screen video.',
          },
          {
            id: 'cpu' as HardwareMode,
            icon: <Cpu className="w-5 h-5" />,
            name: 'CPU',
            tag: 'Works on any PC',
            body: 'Runs on your processor, no graphics card needed. A 1-minute dictation takes about 20 seconds.',
            foot: '🔋 Uses no graphics memory.',
          },
        ]
      ).map((c) => {
        const picked = mode === c.id;
        return (
          <button
            key={c.id}
            type="button"
            onClick={() => !picked && onPick(c.id)}
            className={`rounded-2xl p-4 border transition-all cursor-pointer flex flex-col justify-between text-left ${
              picked ? 'bg-[#FF6B00]/10 border-[#FF6B00] shadow-[0_0_25px_rgba(255,107,0,0.25)]' : 'bg-white/[0.03] hover:bg-white/[0.06] border-white/10'
            }`}
          >
            <div>
              <div className="flex items-start justify-between gap-3">
                <div className="flex items-center gap-2.5">
                  <div className={`p-2 rounded-xl ${picked ? 'bg-[#FF6B00]/20 text-[#FFA133]' : 'bg-white/[0.06] text-white/60'}`}>{c.icon}</div>
                  <div>
                    <h4 className="text-sm font-bold text-white">{c.name}</h4>
                    <span className="text-[11px] font-semibold text-[#FFA133]">{c.tag}</span>
                  </div>
                </div>
                <span className={`w-4 h-4 rounded-full border flex items-center justify-center shrink-0 ${picked ? 'border-[#FF6B00] bg-[#FF6B00]' : 'border-white/30'}`}>
                  {picked && <Check className="w-2.5 h-2.5 text-white stroke-[3]" />}
                </span>
              </div>
              <p className="text-[11.5px] text-white/70 mt-3 leading-relaxed">{c.body}</p>
            </div>
            <span className="text-[10px] text-[#FFA133] font-medium mt-3 block">{c.foot}</span>
          </button>
        );
      })}
    </div>

    <div className="w-full rounded-2xl p-4 sm:p-5 border mt-4" style={{ backgroundColor: 'rgba(18, 13, 26, 0.82)', borderColor: 'rgba(255, 161, 51, 0.3)' }}>
      <div className="flex items-center gap-2 mb-3">
        <span className="px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider bg-[#FF6B00]/20 text-[#FFA133] border border-[#FF6B00]/30">Tip</span>
        <h4 className="text-xs sm:text-sm font-bold text-white">Talk in short bursts</h4>
      </div>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
        <Note tone="good" title="30 to 60 seconds at a time" icon={<CheckCircle2 className="w-3.5 h-3.5" />}>
          Say a few sentences, let go, and Ivy types them right away. Then press again.
        </Note>
        <Note tone="warn" title="Not 5-minute monologues" icon={<Info className="w-3.5 h-3.5" />}>
          Very long recordings take longer to process and are harder to fix if a word is wrong.
        </Note>
      </div>
    </div>
  </>
);

const StepTouchUp: React.FC = () => (
  <>
    <Heading
      icon={<Wand2 className="w-7 h-7" />}
      title="Spot a typo? One click fixes it."
      subtitle="Right after Ivy types, a small Touch Up button shows on the capsule for 7 seconds. Click it only if you see a typo."
    />
    <div
      className="w-full rounded-2xl p-4 sm:p-5 flex flex-col gap-3.5"
      style={{ backgroundColor: 'rgba(18, 13, 26, 0.85)', border: '1px solid rgba(255, 107, 0, 0.28)', boxShadow: '0 8px 30px rgba(0,0,0,0.5)' }}
    >
      <div className="flex flex-col gap-1.5 text-left">
        <span className="text-[10px] font-bold uppercase tracking-wider text-white/40">Typed</span>
        <p className="text-[13px] text-white/70 font-mono">I'll recieve the report tommorow and send it seperately.</p>
      </div>
      <div className="flex items-center gap-2">
        <div className="h-px flex-1 bg-white/[0.08]" />
        <span className="flex items-center gap-1.5 px-3 py-1 rounded-full text-[11px] font-medium text-white/90" style={{ backgroundColor: 'rgba(255,107,0,0.12)', border: '1px solid rgba(255,107,0,0.3)' }}>
          <Wand2 className="w-3 h-3 text-[#FFA133]" />
          Touch Up
        </span>
        <div className="h-px flex-1 bg-white/[0.08]" />
      </div>
      <div className="flex flex-col gap-1.5 text-left">
        <span className="text-[10px] font-bold uppercase tracking-wider text-emerald-400/80">After one click</span>
        <p className="text-[13px] text-white/90 font-mono">I'll receive the report tomorrow and send it separately.</p>
      </div>
    </div>
    <div className="w-full flex flex-col gap-2.5 mt-4">
      <Note tone="good" title="What it fixes" icon={<CheckCircle2 className="w-3.5 h-3.5" />}>
        Misspelled words, using an offline English dictionary. Names, numbers, emails, file names and Indian words like lakh or chai are left alone.
      </Note>
      <Note tone="warn" title="What it never does" icon={<Info className="w-3.5 h-3.5" />}>
        Rephrase or rewrite anything. If there's no typo, it changes nothing and tells you so.
      </Note>
      <Note tone="info" title="Instant and offline" icon={<Cpu className="w-3.5 h-3.5" />}>
        Never runs by itself. Ignore it and it goes away.
      </Note>
    </div>
  </>
);

const StepVoiceTest: React.FC<{ hotkey: string; dictation: ReturnType<typeof useWizardDictation> }> = ({ hotkey, dictation }) => (
  <>
    <Heading title="Let's test your voice" subtitle={`Hold ${hotkey}, say the line below, and let go.`} />
    <VoicePanel
      hotkey={hotkey}
      phraseLabel="Say"
      phrase={TEST_PHRASE}
      workingText="Transcribing on your PC…"
      dictation={dictation}
      result={
        <div
          className="w-full max-w-lg rounded-2xl p-3.5 border flex flex-col gap-1.5"
          style={{
            backgroundColor: 'rgba(11, 8, 18, 0.9)',
            borderColor: dictation.state === 'success' ? 'rgba(16, 185, 129, 0.45)' : 'rgba(255, 107, 0, 0.22)',
          }}
        >
          <span className="text-[10.5px] uppercase tracking-wider text-white/45 font-semibold">What Ivy heard</span>
          <input
            type="text"
            value={dictation.text}
            onChange={(e) => dictation.setText(e.target.value)}
            placeholder="Your words will appear here…"
            className="w-full bg-transparent text-white text-base sm:text-lg font-mono text-center outline-none placeholder:text-white/30 placeholder:italic"
          />
          {dictation.state === 'success' && (
            <span className="text-emerald-400 text-xs font-semibold flex items-center justify-center gap-1.5">
              <CheckCircle2 className="w-3.5 h-3.5" />
              Your mic and Ivy work. In other apps this is typed at your cursor.
            </span>
          )}
        </div>
      }
    />
  </>
);

const StepCorrection: React.FC<{ hotkey: string; dictation: ReturnType<typeof useWizardDictation> }> = ({ hotkey, dictation }) => {
  // Judged from the real result, never assumed.
  const caught = !dictation.text.toLowerCase().includes('french fries');
  return (
    <>
      <Heading title="Change your mind mid-sentence" subtitle="Say the whole line, correction and all. Ivy keeps what you meant." />
      <VoicePanel
        hotkey={hotkey}
        phraseLabel="Say this whole line"
        phrase={CORRECTION_PHRASE}
        workingText="Working out what you meant…"
        dictation={dictation}
        result={
          dictation.state === 'success' ? (
            <div className="flex flex-col items-center gap-2">
              <span className={`flex items-center gap-2 font-semibold text-xs sm:text-sm ${caught ? 'text-emerald-400' : 'text-[#FFA133]'}`}>
                {caught ? <CheckCircle2 className="w-4 h-4" /> : <Info className="w-4 h-4" />}
                {caught ? 'Ivy dropped what you took back' : "Ivy didn't catch it that time"}
              </span>
              <div
                className="px-4 py-2 rounded-2xl border text-white text-sm sm:text-base font-mono max-w-lg"
                style={{
                  backgroundColor: caught ? 'rgba(16, 185, 129, 0.1)' : 'rgba(255, 107, 0, 0.1)',
                  borderColor: caught ? 'rgba(16, 185, 129, 0.35)' : 'rgba(255, 107, 0, 0.35)',
                }}
              >
                "{dictation.text}"
              </div>
              {!caught && <p className="text-white/50 text-[11px] max-w-sm">It's AI, so it can miss. Try saying it again.</p>}
            </div>
          ) : (
            <p className="text-white/55 text-[11px] max-w-sm text-center">No trigger words needed. Ivy hears the whole line. It is AI, so glance at the result.</p>
          )
        }
      />
    </>
  );
};

const StepPrivacy: React.FC = () => (
  <>
    <Heading
      icon={<ShieldCheck className="w-8 h-8" />}
      title="Your voice stays on your PC."
      subtitle="Ivy is not a service. It is software that runs entirely on your machine."
    />
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 w-full">
      {[
        { icon: <WifiOff className="w-4 h-4" />, title: '100% offline', body: 'No internet needed. Your voice never goes to a server, an API or a cloud service.', foot: <><Lock className="w-3 h-3" /> No network calls.</> },
        { icon: <Trash2 className="w-4 h-4" />, title: 'Deleted after 24 hours', body: 'Every recording and transcript is removed automatically after 24 hours.', foot: <><CheckCircle2 className="w-3 h-3" /> Always on.</> },
        { icon: <Eye className="w-4 h-4" />, title: 'Clear anytime', body: 'History has a "Clear all transcriptions" button. Your streak and word count stay.', foot: <><CheckCircle2 className="w-3 h-3" /> Recordings go, progress stays.</> },
        { icon: <HardDrive className="w-4 h-4" />, title: 'Stays on your machine', body: 'History, settings and stats live in your own app folder. No accounts, no telemetry.', foot: <><CheckCircle2 className="w-3 h-3" /> Code is public.</> },
      ].map((c) => (
        <div
          key={c.title}
          className="rounded-2xl p-4 border flex flex-col gap-2 text-left"
          style={{ backgroundColor: 'rgba(18,13,26,0.85)', borderColor: 'rgba(255,107,0,0.28)', boxShadow: '0 4px 20px rgba(0,0,0,0.5)' }}
        >
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-xl bg-[#FF6B00]/15 border border-[#FF6B00]/30 flex items-center justify-center text-[#FFA133] shrink-0">{c.icon}</div>
            <span className="text-sm font-bold text-white">{c.title}</span>
          </div>
          <p className="text-[11.5px] text-white/60 leading-relaxed">{c.body}</p>
          <span className="text-[10px] font-mono text-[#FFA133]/80 flex items-center gap-1 mt-1">{c.foot}</span>
        </div>
      ))}
    </div>
  </>
);

// ---------------------------------------------------------------------------------------------------------
// The wizard
// ---------------------------------------------------------------------------------------------------------

const NEXT_LABEL: Record<OnboardingStage, string> = {
  1: 'Next: No text box?',
  2: 'Next: GPU or CPU',
  3: 'Next: Touch Up',
  4: 'Next: Test your voice',
  5: 'Next: Change your mind',
  6: 'Next: Privacy',
  7: 'Start using Ivy',
};

export const FirstRunView: React.FC<FirstRunViewProps> = ({
  onDismiss,
  onComplete,
  hotkey = 'Alt + Space',
  manualPasteHotkey = 'Alt + V',
  hardwareMode = 'gpu',
  onUpdateSettings,
}) => {
  const [stage, setStage] = useState<OnboardingStage>(1);
  const [furthest, setFurthest] = useState<OnboardingStage>(1);
  const voiceTest = useWizardDictation(hotkey, stage === 5);
  const correction = useWizardDictation(hotkey, stage === 6);

  // The capsule overlay stays hidden while this window is in front (lib.rs `bridge_capsule_show`).
  useEffect(() => {
    invoke('set_wizard_active', { active: true }).catch(() => undefined);
    return () => {
      invoke('set_wizard_active', { active: false }).catch(() => undefined);
    };
  }, []);

  const goTo = (next: OnboardingStage) => {
    setStage(next);
    setFurthest((f) => (next > f ? next : f));
  };
  const back = stage > 1 ? () => goTo((stage - 1) as OnboardingStage) : undefined;
  const next = () => (stage === LAST_STAGE ? onComplete() : goTo((stage + 1) as OnboardingStage));

  const activeDictation = stage === 5 ? voiceTest : stage === 6 ? correction : null;
  const voiceDone = activeDictation ? activeDictation.state === 'success' : true;

  const done = new Set<OnboardingStage>();
  for (let s = 1; s < furthest; s++) done.add(s as OnboardingStage);
  if (voiceTest.state === 'success') done.add(5);
  if (correction.state === 'success') done.add(6);

  return (
    <div id="screen-first-run" className="relative flex-1 flex flex-col items-center h-full p-4 sm:p-6 md:p-8 select-none overflow-y-auto overflow-x-hidden">
      <div
        className="pointer-events-none absolute top-0 left-6 right-6 h-[1.5px] z-20"
        style={{
          background: `linear-gradient(90deg, transparent 0%, rgba(255,255,255,0.6) 20%, rgba(${ACCENT_RGB}, 0.95) 50%, rgba(255,255,255,0.6) 80%, transparent 100%)`,
        }}
      />
      <div
        className="pointer-events-none absolute top-12 left-1/2 -translate-x-1/2 w-[600px] h-[350px] rounded-full blur-[110px] opacity-25 z-0"
        style={{ background: `radial-gradient(circle, rgba(${ACCENT_RGB}, 0.7) 0%, transparent 70%)` }}
      />

      <header className="relative z-10 w-full max-w-3xl flex flex-col items-center gap-3.5 shrink-0">
        <div className="w-full grid grid-cols-3 items-center px-1">
          <div>
            <button
              onClick={onDismiss}
              type="button"
              className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-xl text-xs font-medium text-white/70 hover:text-white bg-white/[0.05] hover:bg-white/[0.1] border border-white/[0.08] transition-all cursor-pointer"
            >
              <ArrowLeft className="w-3.5 h-3.5" />
              Skip setup
            </button>
          </div>
          <div className="flex justify-center">
            <IvyWordmark height={34} glow />
          </div>
          <div />
        </div>
        <StageIndicator currentStage={stage} completedStages={done} onSelectStage={(s) => s <= furthest && goTo(s)} />
      </header>

      <main className="relative z-10 w-full max-w-2xl my-auto py-4">
        <AnimatePresence mode="wait">
          <motion.div
            key={stage}
            initial={{ opacity: 0, y: 14 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -14 }}
            transition={{ duration: 0.25, ease: 'easeOut' }}
            className="w-full flex flex-col items-center"
          >
            {stage === 1 && <StepShortcut hotkey={hotkey} onPick={(key) => onUpdateSettings?.({ hotkey: key })} />}
            {stage === 2 && <StepClipboard manualPasteHotkey={manualPasteHotkey} />}
            {stage === 3 && <StepHardware mode={hardwareMode} onPick={(m) => onUpdateSettings?.({ hardwareMode: m })} />}
            {stage === 4 && <StepTouchUp />}
            {stage === 5 && <StepVoiceTest hotkey={hotkey} dictation={voiceTest} />}
            {stage === 6 && <StepCorrection hotkey={hotkey} dictation={correction} />}
            {stage === 7 && <StepPrivacy />}
            <Footer onBack={back} onNext={next} nextLabel={voiceDone ? NEXT_LABEL[stage] : 'Skip this test'} quietNext={!voiceDone} />
          </motion.div>
        </AnimatePresence>
      </main>
    </div>
  );
};
