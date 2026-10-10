import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { motion, AnimatePresence } from 'motion/react';
import { AlertTriangle, Check, ExternalLink, Mic, RefreshCw, Wand2, X } from 'lucide-react';
import { CapsuleMode, TonePreset } from '../types';

interface FloatingCapsuleProps {
  mode: CapsuleMode;
  activeTone: TonePreset;
  activeApp: string;
  /** Real Ctrl+V happened vs. clipboard-only fallback — never claim "Pasted"
   *  when this is false (Yash: no gimmicks, don't claim what didn't happen). */
  pasted?: boolean;
  /** Whether the last failure has a saved recording it can retry against —
   *  false means literal silence, nothing to retry ("Didn't hear anything"). */
  canRetry?: boolean;
  /** Real, user-remappable shortcut for pasting a held-back transcript —
   *  shown so the "no text field found" message tells the user something
   *  they can actually act on. */
  manualPasteHotkey: string;
  /** Where the optional Touch Up click currently stands — only meaningful
   *  while `mode === 'touch-up'`. */
  touchUpStatus?: 'offer' | 'loading' | 'done' | 'clean' | 'error';
  /** Text for `mode === 'notice'` (GPU too busy, speech model not ready). */
  notice?: string;
  onRetry?: () => void;
  onCancel?: () => void;
  onTouchUp?: () => void;
}

const BAR_COUNT = 18;
const ENVELOPE = Array.from({ length: BAR_COUNT }, (_, i) => {
  const t = i / (BAR_COUNT - 1);
  return 0.25 + Math.sin(t * Math.PI) * 0.8;
});

// Purely presentational — CapsuleWindow.tsx owns the real state machine,
// driven by the real Alt+Space hotkey and the real
// transcribe→cleanup→paste pipeline in Rust. This component has no
// simulated audio or transcript, only the recording elapsed-time display
// and decorative waveform motion.
//
// Compact single-row pill (Yash: the previous 470x116 center-bottom card
// read as "large" and "in the middle of the screen") — layout and the
// two-mode soundwave (organic while listening, a traveling wave while
// transcribing) are ported from Friday's own Dynamic Island dictation
// pill, `jarvis_v2/friday-ui/src/dynamic-island/DynamicIsland.tsx`.
export const FloatingCapsule: React.FC<FloatingCapsuleProps> = ({
  mode,
  activeTone,
  activeApp,
  pasted = true,
  canRetry,
  manualPasteHotkey,
  touchUpStatus = 'offer',
  notice = '',
  onRetry,
  onCancel,
  onTouchUp,
}) => {
  const [elapsedSec, setElapsedSec] = useState(0);
  // State, not a plain ref: `AnimatePresence mode="wait"` delays mounting
  // the incoming child (e.g. the 'transcribing' canvas) until the outgoing
  // one (e.g. 'recording') finishes its exit animation, ~150ms later. A
  // plain ref read inside the draw effect below (which fires on the
  // render where `mode` changes) would still point at the *old*,
  // about-to-unmount canvas at that moment — the new canvas mounting
  // later never re-triggers the effect (its deps don't include the DOM
  // node), so the waveform stayed blank for the entire 'transcribing'
  // state, every time. A callback ref updates this state exactly when a
  // canvas element actually mounts, which the effect below is keyed on.
  const [waveformCanvas, setWaveformCanvas] = useState<HTMLCanvasElement | null>(null);

  useEffect(() => {
    if (mode !== 'recording') {
      setElapsedSec(0);
      return;
    }
    const start = Date.now();
    const timer = setInterval(() => setElapsedSec(Math.floor((Date.now() - start) / 1000)), 1000);
    return () => clearInterval(timer);
  }, [mode]);

  useEffect(() => {
    if (mode !== 'recording' && mode !== 'transcribing') return;
    const canvas = waveformCanvas;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Backing store at the screen's real pixel density: an 80x24 bitmap stretched to 125-150% Windows
    // scaling looked blurry (Yash, 2026-10-07). Drawing stays in CSS pixels through the transform.
    const w = canvas.clientWidth || 80;
    const h = canvas.clientHeight || 24;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const params = Array.from({ length: BAR_COUNT }).map(() => ({
      freq1: 4.8 + Math.random() * 4,
      freq2: 10.5 + Math.random() * 4,
      phase1: Math.random() * Math.PI * 2,
      phase2: Math.random() * Math.PI * 2,
      depth: 0.25 + Math.random() * 0.2,
    }));

    let animId: number;
    const startTime = performance.now();

    const render = (now: number) => {
      const elapsed = (now - startTime) / 1000;
      ctx.clearRect(0, 0, w, h);
      const barWidth = 2.5;
      // 18 bars needed 96px at gap=3 (18*2.5 + 17*3) but the canvas is
      // only 80px wide, so the outer bars drew off-canvas and the wave
      // looked abruptly cut off instead of tapering. gap=2 fits in 79px.
      const gap = 2;
      const totalWidth = BAR_COUNT * barWidth + (BAR_COUNT - 1) * gap;
      const startX = (w - totalWidth) / 2;

      for (let i = 0; i < BAR_COUNT; i++) {
        const p = params[i];
        const envelope = ENVELOPE[i] ?? 0.6;
        let amp: number;
        if (mode === 'recording') {
          const wave1 = Math.sin(elapsed * p.freq1 + p.phase1);
          const wave2 = Math.cos(elapsed * p.freq2 + p.phase2) * 0.45;
          const organic = Math.max(0.18, 1 + (wave1 + wave2) * p.depth);
          amp = envelope * organic;
        } else {
          // Transcribing: a smooth traveling wave sweeping across the bars,
          // not a live audio reading — Friday's own dictation-pill pattern
          // for "the system is working on it", ported verbatim.
          const travel = Math.sin(elapsed * 7.5 - i * 0.52);
          const pulse = (travel + 1) * 0.5;
          amp = (0.22 + pulse * 0.72) * envelope;
        }

        const minH = 3;
        const maxH = h - 4;
        const barHeight = Math.max(minH, Math.min(maxH, minH + amp * (maxH - minH)));
        const x = startX + i * (barWidth + gap);
        const y = (h - barHeight) / 2;

        const grad = ctx.createLinearGradient(0, y, 0, y + barHeight);
        grad.addColorStop(0, '#ffffff');
        grad.addColorStop(0.35, '#E59530');
        grad.addColorStop(1, 'rgba(229, 149, 48, 0.75)');

        ctx.save();
        ctx.shadowColor = '#E59530';
        ctx.shadowBlur = (barHeight > 6 ? 5 : 2) * dpr; // shadowBlur ignores the transform
        ctx.fillStyle = grad;
        ctx.beginPath();
        ctx.roundRect(x, y, barWidth, barHeight, barWidth / 2);
        ctx.fill();
        ctx.restore();
      }

      animId = requestAnimationFrame(render);
    };

    animId = requestAnimationFrame(render);
    return () => cancelAnimationFrame(animId);
  }, [mode, waveformCanvas]);

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs < 10 ? '0' : ''}${secs}`;
  };

  const openApp = () => void invoke('show_main_window');

  // Alt+Space-only visibility (IVY.md §13): nothing renders until the real
  // hotkey fires, and it disappears again once the result confirmation clears.
  if (mode === 'idle') return null;

  return (
    <div className="w-full h-full flex flex-col items-center justify-start pt-1.5 select-none bg-transparent">
      <motion.div
        layout
        transition={{ type: 'spring', stiffness: 280, damping: 27, mass: 0.75 }}
        className="relative select-none overflow-hidden w-[352px] h-[54px] rounded-full flex items-center bg-black shrink-0"
        style={{
          backgroundColor: '#000000',
          border: '1px solid rgba(229, 149, 48, 0.35)',
          boxShadow: 'inset 0 1px 0 rgba(255, 255, 255, 0.12)',
        }}
      >
        <button
          onClick={openApp}
          title="Open Ivy"
          className="shrink-0 w-8 h-8 ml-2 rounded-full flex items-center justify-center text-white/70 hover:text-white bg-white/[0.08] hover:bg-white/[0.16] transition-colors duration-150"
        >
          <ExternalLink className="w-3.5 h-3.5" />
        </button>

        <AnimatePresence mode="wait">
          {mode === 'recording' && (
            <motion.div
              key="recording"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="flex-1 h-full pl-2.5 pr-2 flex items-center justify-between gap-2 min-w-0"
            >
              <div className="flex items-center gap-1.5 min-w-0">
                <div className="flex flex-col min-w-0 leading-none">
                  <span className="text-[11px] font-semibold text-white truncate flex items-center gap-1">
                    {formatTime(elapsedSec)} · {activeTone}
                    {elapsedSec >= 290 ? (
                      <span className="text-[8.5px] font-semibold text-red-200 bg-red-500/30 px-1.5 py-0.5 rounded-full animate-pulse" title="Ivy finishes and types your words at 5:00">
                        {Math.max(0, 300 - elapsedSec)}s left
                      </span>
                    ) : elapsedSec >= 60 && (
                      <span className="text-[8.5px] font-medium text-amber-300 bg-amber-400/20 px-1.5 py-0.5 rounded-full" title="Audio is safely buffered">
                        Buffered
                      </span>
                    )}
                  </span>
                  <span className="text-[9px] text-white/50 font-mono mt-0.5 truncate">{activeApp}</span>
                </div>
              </div>
              <div className="flex items-center gap-1.5 shrink-0">
                <canvas ref={setWaveformCanvas} width={80} height={24} className="w-[80px] h-[24px] shrink-0" />
                <button
                  onClick={onCancel}
                  title="Cancel recording"
                  className="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-white/50 hover:text-white bg-white/[0.08] hover:bg-red-500/25 border border-white/10 hover:border-red-500/40 transition-colors duration-150"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            </motion.div>
          )}

          {mode === 'transcribing' && (
            <motion.div
              key="transcribing"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="flex-1 h-full pl-2.5 pr-2 flex items-center justify-between gap-2"
            >
              <div className="flex items-center gap-1.5 min-w-0">
                <div className="w-6 h-6 rounded-full flex items-center justify-center bg-[#E59530] text-black shrink-0">
                  <Mic className="w-3 h-3" strokeWidth={2.5} />
                </div>
                <span className="text-[11.5px] font-semibold text-white truncate">Transcribing…</span>
              </div>
              <div className="flex items-center gap-1.5 shrink-0">
                <canvas ref={setWaveformCanvas} width={80} height={24} className="w-[80px] h-[24px] shrink-0" />
                <button
                  onClick={onCancel}
                  title="Cancel transcription"
                  className="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-white/50 hover:text-white bg-white/[0.08] hover:bg-red-500/25 border border-white/10 hover:border-red-500/40 transition-colors duration-150"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            </motion.div>
          )}

          {mode === 'pasted' && (
            <motion.div
              key="pasted"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="flex-1 h-full pl-2.5 pr-3 flex items-center gap-2"
            >
              <div
                className={`w-5 h-5 rounded-full flex items-center justify-center shrink-0 border ${
                  pasted ? 'bg-emerald-500/20 border-emerald-400/40' : 'bg-white/[0.12] border-white/25'
                }`}
              >
                <Check className={`w-3 h-3 stroke-[2.5] ${pasted ? 'text-emerald-400' : 'text-white/80'}`} />
              </div>
              {/* Shows clear confirmation: pasted directly, or held back — Ivy
                  never touches the real clipboard when it can't find a
                  text field (that used to silently destroy whatever the
                  user had actually copied, e.g. a password or API key). */}
              {/* No app name: an app on Linux's Wayland that Ivy can't see. No held-back key there either (no
                  Alt + V on Wayland): the text waits in History. */}
              <span className="text-[11.5px] font-medium text-white truncate">
                {pasted
                  ? activeApp ? `Pasted to ${activeApp}` : 'Pasted'
                  : manualPasteHotkey
                  ? `Couldn't paste automatically — press ${manualPasteHotkey} to paste it`
                  : "Couldn't paste here — it's saved in History"}
              </span>
            </motion.div>
          )}

          {mode === 'manual-pasted' && (
            <motion.div
              key="manual-pasted"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="flex-1 h-full pl-2.5 pr-3 flex items-center gap-2"
            >
              <div
                className={`w-5 h-5 rounded-full flex items-center justify-center shrink-0 border ${
                  pasted ? 'bg-emerald-500/20 border-emerald-400/40' : 'bg-white/[0.12] border-white/25'
                }`}
              >
                <Check className={`w-3 h-3 stroke-[2.5] ${pasted ? 'text-emerald-400' : 'text-white/80'}`} />
              </div>
              <span className="text-[11.5px] font-medium text-white truncate">
                {pasted ? (activeApp ? `Pasted to ${activeApp}` : 'Pasted') : "Couldn't paste there — copied instead"}
              </span>
            </motion.div>
          )}

          {mode === 'touch-up' && (
            <motion.div
              key="touch-up"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="flex-1 h-full pl-2.5 pr-3 flex items-center gap-2"
            >
              {touchUpStatus === 'offer' && (
                <button
                  id="btn-capsule-touch-up"
                  onClick={onTouchUp}
                  className="flex items-center gap-1.5 text-[11.5px] font-medium text-white/90 hover:text-white transition-colors cursor-pointer"
                >
                  <Wand2 className="w-3.5 h-3.5 text-[#FFA133]" />
                  <span>Typos? Touch Up</span>
                </button>
              )}
              {touchUpStatus === 'loading' && (
                <>
                  <Wand2 className="w-3.5 h-3.5 text-[#FFA133] shrink-0 animate-pulse" />
                  {/* No visible countdown — a precise number here would
                      just be one more thing to be wrong when the real
                      call runs a beat over or under it. */}
                  <span className="text-[11.5px] font-medium text-white/80 truncate">Touching up…</span>
                </>
              )}
              {touchUpStatus === 'done' && (
                <>
                  <div className="w-5 h-5 rounded-full flex items-center justify-center shrink-0 border bg-emerald-500/20 border-emerald-400/40">
                    <Check className="w-3 h-3 stroke-[2.5] text-emerald-400" />
                  </div>
                  <span className="text-[11.5px] font-medium text-white truncate">Typos fixed</span>
                </>
              )}
              {touchUpStatus === 'clean' && (
                <>
                  <div className="w-5 h-5 rounded-full flex items-center justify-center shrink-0 border bg-emerald-500/20 border-emerald-400/40">
                    <Check className="w-3 h-3 stroke-[2.5] text-emerald-400" />
                  </div>
                  <span className="text-[11.5px] font-medium text-white truncate">No typos found</span>
                </>
              )}
              {touchUpStatus === 'error' && (
                <>
                  <div className="w-5 h-5 rounded-full flex items-center justify-center shrink-0 border bg-white/[0.12] border-white/25">
                    <X className="w-3 h-3 stroke-[2.5] text-white/70" />
                  </div>
                  <span className="text-[11.5px] font-medium text-white/80 truncate">Couldn't touch it up</span>
                </>
              )}
            </motion.div>
          )}

          {mode === 'launched' && (
            <motion.div
              key="launched"
              initial={{ clipPath: 'inset(0 100% 0 0 round 999px)' }}
              animate={{ clipPath: 'inset(0 0% 0 0 round 999px)' }}
              // Exit deliberately much faster than the 0.6s entrance (and
              // than the exit above) — this toast can be on screen for up
              // to 3.2s (`CapsuleWindow.tsx`'s auto-hide delay), and a real
              // hotkey press during that window switches `mode` to
              // 'recording' immediately. Under `AnimatePresence mode="wait"`
              // the incoming child waits for this exit to finish, so a
              // slow one left the capsule a bare black pill — no dot, no
              // timer, no waveform — for 600ms while Ivy was already
              // recording, reading as "the hotkey didn't work".
              exit={{ clipPath: 'inset(0 0 0 100% round 999px)', transition: { duration: 0.15 } }}
              transition={{ duration: 0.6, ease: [0.6, 0, 0.2, 1] }}
              className="flex-1 h-full pl-2.5 pr-3 flex items-center gap-2"
            >
              <div className="w-5 h-5 rounded-full flex items-center justify-center shrink-0 bg-[#E59530]/20 border border-[#E59530]/50">
                <span className="w-2 h-2 rounded-full bg-[#E59530]" />
              </div>
              <span className="text-[11.5px] font-medium text-white truncate">Ivy is running in the background</span>
            </motion.div>
          )}

          {mode === 'notice' && (
            <motion.div
              key="notice"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0, transition: { duration: 0.15 } }}
              transition={{ duration: 0.2 }}
              className="flex-1 h-full pl-2.5 pr-3 flex items-center gap-2"
            >
              <div className="w-5 h-5 rounded-full flex items-center justify-center shrink-0 bg-[#E59530]/20 border border-[#E59530]/50">
                <AlertTriangle className="w-3 h-3 text-[#E59530]" />
              </div>
              <span className="text-[11.5px] font-medium text-white truncate">{notice}</span>
            </motion.div>
          )}

          {mode === 'failed' && (
            <motion.div
              key="failed"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="flex-1 h-full pl-3 pr-2.5 flex items-center justify-between gap-2 min-w-0"
            >
              <div className="flex items-center gap-2 min-w-0">
                <div className="w-5 h-5 rounded-full bg-amber-500/20 border border-amber-400/40 flex items-center justify-center shrink-0">
                  <AlertTriangle className="w-3 h-3 text-amber-400 stroke-[2.5]" />
                </div>
                <div className="flex flex-col min-w-0 leading-tight">
                  <span className="text-[11px] font-semibold text-white truncate">
                    {canRetry ? "Audio saved safely" : "Didn't hear anything"}
                  </span>
                  {canRetry && (
                    <span className="text-[9px] text-white/60 truncate">
                      Retry here or check in App
                    </span>
                  )}
                </div>
              </div>
              {canRetry && (
                <div className="flex items-center gap-1.5 shrink-0">
                  <button
                    onClick={onRetry}
                    title="Retry transcription now"
                    className="flex items-center gap-1 px-2.5 py-1 rounded-full bg-[#E59530]/25 hover:bg-[#E59530]/40 border border-[#E59530]/50 text-[10.5px] font-semibold text-white transition-colors duration-150 shadow-sm"
                  >
                    <RefreshCw className="w-3 h-3" />
                    Retry
                  </button>
                  <button
                    onClick={openApp}
                    title="Open Ivy app to view full recording history & retry"
                    className="flex items-center gap-0.5 px-2 py-1 rounded-full bg-white/[0.1] hover:bg-white/[0.18] text-[10.5px] font-medium text-white transition-colors duration-150"
                  >
                    App
                  </button>
                </div>
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>

      <AnimatePresence>
        {mode === 'transcribing' && (
          <motion.div
            initial={{ opacity: 0, y: -2 }}
            animate={{ opacity: 0.7, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="mt-2 text-[11px] font-medium text-white/70 tracking-wide text-center drop-shadow-[0_1px_3px_rgba(0,0,0,0.85)] pointer-events-none select-none"
          >
            Local model can sometimes take a few more seconds, please be patient
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};
