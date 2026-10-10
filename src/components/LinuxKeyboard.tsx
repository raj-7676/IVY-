import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Desktop } from '../utils/platform';

// Linux, Wayland only: Ivy's keyboard helper (lib.rs `get_desktop`, src-tauri/linux/ivy-keys) isn't working, so
// the dictation key and pasting don't either. Says why in plain words, with the one fix there is.

const WHY: Record<Exclude<Desktop['keyboard'], 'ready' | 'starting'>, { text: string; fix: boolean }> = {
  denied: {
    text: "Ivy can't use your keyboard yet, so Ctrl + Shift and pasting don't work. Its keyboard helper is missing the rights its installer gives it.",
    fix: true,
  },
  'no-paste': {
    text: "Ivy hears Ctrl + Shift but can't paste: its virtual keyboard isn't allowed yet.",
    fix: true,
  },
  missing: {
    text: "This Ivy wasn't installed from its .deb or .rpm package, which sets up the keyboard helper Ivy needs on this desktop (Wayland). Install the package to dictate.",
    fix: false,
  },
  'no-keyboard': {
    text: 'Ivy found no keyboard to watch for Ctrl + Shift. Plug one in, or use "Dictate Now" in the tray menu.',
    fix: false,
  },
};

export const LinuxKeyboardNotice: React.FC<{ desktop: Desktop | null; className?: string }> = ({ desktop, className }) => {
  const [fixing, setFixing] = useState(false);
  if (!desktop?.wayland || desktop.keyboard === 'ready' || desktop.keyboard === 'starting') return null;
  const why = WHY[desktop.keyboard];
  // The system's own password prompt (pkexec) asks first: changing a program's rights needs the administrator.
  const fix = () => {
    setFixing(true);
    invoke('request_permission', { kind: 'keyboard' })
      .catch(() => {})
      .finally(() => setFixing(false));
  };
  return (
    <div
      className={`flex items-center justify-between gap-3 px-3.5 py-2 rounded-xl text-xs font-medium text-amber-300 bg-amber-500/10 border border-amber-500/25 ${className ?? ''}`}
    >
      <span>{why.text}</span>
      {why.fix && (
        <button
          onClick={fix}
          disabled={fixing}
          className="shrink-0 px-2.5 py-1 rounded-lg bg-amber-500/20 hover:bg-amber-500/30 text-amber-200 disabled:opacity-50"
        >
          {fixing ? 'Fixing…' : 'Fix keyboard access'}
        </button>
      )}
    </div>
  );
};
