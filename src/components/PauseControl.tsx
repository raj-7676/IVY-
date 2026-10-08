import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Pause, Play } from 'lucide-react';

const ACCENT_RGB = '255, 107, 0';
const MAX_MINUTES = 24 * 60;

/** Title-bar Pause (Yash, 2026-10-06): one click pauses for 1 hour; a custom length goes up to 24 hours.
 *  The backend (lib.rs `pause_ivy`) frees the dictation key and resumes by itself when time is up. */
export const PauseControl: React.FC = () => {
  const [until, setUntil] = useState(0);
  const [open, setOpen] = useState(false);
  const [amount, setAmount] = useState('30');
  const [unit, setUnit] = useState<'minutes' | 'hours'>('minutes');
  const [error, setError] = useState<string | null>(null);
  const box = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    invoke<number>('get_pause').then(setUntil).catch(() => undefined);
    const sub = listen<number>('ivy://pause-changed', (e) => setUntil(e.payload));
    return () => {
      sub.then((off) => off()).catch(() => undefined);
    };
  }, []);

  // Click outside closes the menu.
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', close);
    return () => window.removeEventListener('mousedown', close);
  }, [open]);

  const pause = (minutes: number) => {
    if (!Number.isFinite(minutes) || minutes < 1 || minutes > MAX_MINUTES) {
      setError('Pick from 1 minute up to 24 hours.');
      return;
    }
    setError(null);
    invoke<number>('pause_ivy', { minutes: Math.round(minutes) })
      .then((u) => {
        setUntil(u);
        setOpen(false);
      })
      .catch((e) => setError(String(e)));
  };

  if (until) {
    const end = new Date(until);
    const day = end.toDateString() === new Date().toDateString() ? '' : 'tomorrow ';
    return (
      <button
        onClick={() => invoke('resume_ivy').catch(() => undefined)}
        title="Resume Ivy now"
        className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-[11px] font-medium text-amber-200 bg-amber-500/15 border border-amber-500/30 hover:bg-amber-500/25 transition-colors"
      >
        <Play className="w-3 h-3" />
        Paused until {day}
        {end.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })} · Resume
      </button>
    );
  }

  return (
    <div ref={box} className="relative">
      <button
        onClick={() => setOpen(!open)}
        title="Pause Ivy"
        className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-[11px] font-medium text-white/60 hover:text-white bg-white/[0.05] hover:bg-white/[0.1] border border-white/[0.08] transition-colors"
      >
        <Pause className="w-3 h-3" />
        Pause
      </button>
      {open && (
        <div
          className="absolute right-0 top-full mt-2 w-64 rounded-2xl p-3 z-[200] flex flex-col gap-2.5"
          style={{ backgroundColor: 'rgba(16, 13, 20, 0.97)', border: '1px solid rgba(255,255,255,0.1)', backdropFilter: 'blur(20px)' }}
        >
          <p className="text-[11px] text-white/50 leading-relaxed">
            Turns dictation off for a meeting or a screen share. It comes back on by itself.
          </p>
          <button
            onClick={() => pause(60)}
            className="w-full py-2 rounded-xl text-[12px] font-semibold text-white"
            style={{ backgroundColor: `rgba(${ACCENT_RGB}, 0.25)`, border: `1px solid rgba(${ACCENT_RGB}, 0.5)` }}
          >
            Pause for 1 hour
          </button>
          <div className="flex items-center gap-1.5">
            <input
              type="number"
              min={1}
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && pause(Number(amount) * (unit === 'hours' ? 60 : 1))}
              className="w-14 px-2 py-1.5 rounded-lg text-[12px] text-white/85 bg-white/[0.05] border border-white/[0.09] outline-none text-center"
            />
            <select
              value={unit}
              onChange={(e) => setUnit(e.target.value as 'minutes' | 'hours')}
              className="flex-1 px-2 py-1.5 rounded-lg text-[12px] text-white/85 bg-[#16121c] border border-white/[0.09] outline-none"
            >
              <option value="minutes">minutes</option>
              <option value="hours">hours</option>
            </select>
            <button
              onClick={() => pause(Number(amount) * (unit === 'hours' ? 60 : 1))}
              className="px-3 py-1.5 rounded-lg bg-white/[0.08] hover:bg-white/[0.14] text-[12px] font-medium text-white/85"
            >
              Pause
            </button>
          </div>
          <span className={`text-[10.5px] ${error ? 'text-amber-300' : 'text-white/30'}`}>{error ?? 'Up to 24 hours.'}</span>
        </div>
      )}
    </div>
  );
};
