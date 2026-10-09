import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Keyboard, Mic } from 'lucide-react';

// The two permissions a Mac asks the user for (lib.rs `get_permissions`). Windows has none, so nothing here is
// shown there.

export interface Permissions {
  /** Accessibility: Ivy can press Cmd+V in other apps. */
  accessibility: boolean;
  /** "ask" = macOS hasn't asked yet; it asks the first time Ivy records. */
  microphone: 'granted' | 'denied' | 'ask';
}

/** Read again every 1.5 s while shown: the user grants them in System Settings, outside Ivy. */
export function usePermissions(enabled = true): Permissions | null {
  const [perms, setPerms] = useState<Permissions | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    const read = () =>
      invoke<Permissions>('get_permissions')
        .then((p) => alive && setPerms(p))
        .catch(() => {});
    read();
    const id = window.setInterval(read, 1500);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [enabled]);
  return perms;
}

export const requestPermission = (kind: 'accessibility' | 'microphone') => {
  invoke('request_permission', { kind }).catch(() => {});
};

const ACCENT_RGB = '255, 107, 0';

/** Each permission with its live state and the one button that moves it forward. */
export const PermissionRows: React.FC<{ perms: Permissions | null }> = ({ perms }) => {
  const mic = perms?.microphone;
  const rows = [
    {
      kind: 'accessibility' as const,
      icon: <Keyboard className="w-4 h-4" />,
      title: 'Accessibility',
      body: 'Lets Ivy type your words into other apps (it presses ⌘V for you).',
      ok: perms?.accessibility ?? false,
      action: 'Open Accessibility settings',
      hint: "Switch Ivy on in the list. Already on but Ivy still can't paste (normal after an update)? Switch it off and on again.",
    },
    {
      kind: 'microphone' as const,
      icon: <Mic className="w-4 h-4" />,
      title: 'Microphone',
      body: 'Lets Ivy hear you while you hold the shortcut. It listens at no other time.',
      ok: mic === 'granted',
      action: mic === 'ask' ? 'Allow microphone' : 'Open Microphone settings',
      hint: mic === 'ask' ? 'macOS asks once: choose Allow.' : 'Switch Ivy on in the list.',
    },
  ];
  return (
    <div className="flex flex-col gap-2.5 text-left">
      {rows.map((r) => (
        <div
          key={r.kind}
          className="rounded-2xl p-3.5 border flex flex-col gap-2"
          style={{
            backgroundColor: 'rgba(255,255,255,0.03)',
            borderColor: r.ok ? 'rgba(16, 185, 129, 0.3)' : `rgba(${ACCENT_RGB}, 0.3)`,
          }}
        >
          <div className="flex items-center justify-between gap-3">
            <span className="flex items-center gap-2 text-[13px] font-medium text-white/90">
              <span className="text-[#FFA133]">{r.icon}</span>
              {r.title}
            </span>
            <span
              className={`text-[11px] font-medium px-2 py-0.5 rounded-full border ${
                r.ok
                  ? 'bg-emerald-500/15 text-emerald-300 border-emerald-500/30'
                  : 'bg-amber-500/15 text-amber-300 border-amber-500/30'
              }`}
            >
              {perms === null ? 'Checking…' : r.ok ? 'Allowed' : 'Not allowed'}
            </span>
          </div>
          <p className="text-[12px] text-white/50 leading-relaxed">{r.body}</p>
          {perms !== null && !r.ok && (
            <div className="flex flex-wrap items-center gap-3">
              <button
                type="button"
                onClick={() => requestPermission(r.kind)}
                className="px-3.5 py-1.5 rounded-xl text-[12px] font-semibold text-white transition-all hover:brightness-110"
                style={{ backgroundColor: `rgb(${ACCENT_RGB})` }}
              >
                {r.action}
              </button>
              <span className="text-[11px] text-white/45 leading-snug flex-1 min-w-[180px]">{r.hint}</span>
            </div>
          )}
        </div>
      ))}
    </div>
  );
};
