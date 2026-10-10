import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

// Which OS Ivy runs on, for words and key names on screen (the backend does the real per-OS work). WKWebView on
// a Mac reports "Macintosh" in its user agent; WebView2 on Windows never does; WebKitGTK on Linux says "Linux" in
// its platform.
const PLATFORM = typeof navigator === 'undefined' ? '' : `${navigator.platform} ${navigator.userAgent}`;
export const IS_LINUX = /Linux/.test(PLATFORM) && !/Android/.test(PLATFORM);
export const IS_MAC = !IS_LINUX && /Mac/.test(PLATFORM);
export const IS_WINDOWS = !IS_MAC && !IS_LINUX;

/** "PC" or "Mac", as in "Your voice stays on your PC." */
export const DEVICE = IS_MAC ? 'Mac' : 'PC';

/** The paste and undo modifier: Ctrl on Windows and Linux, Cmd on a Mac. */
export const MOD = IS_MAC ? 'Cmd' : 'Ctrl';

const MAC_KEYS: Record<string, string> = { Alt: '⌥ Option', Ctrl: '⌃ Control', Shift: '⇧ Shift' };

/** A saved shortcut as the keyboard in front of the user labels it: "Alt + Space" reads "⌥ Option + Space" on a
 *  Mac. Settings keep the saved form ("Alt + Space"); only what is shown changes. */
export const keyLabel = (spec: string): string =>
  IS_MAC ? spec.split('+').map((k) => MAC_KEYS[k.trim()] ?? k.trim()).join(' + ') : spec;

/** What this desktop lets Ivy do (lib.rs `get_desktop`). Only a Linux "Wayland" desktop (Ubuntu's and Fedora's
 *  default) differs: there Ivy can tell apart only some apps (X11 ones), can't claim Alt + Space or Alt + V, and
 *  watches its Ctrl + Shift key through its keyboard helper, whose state is `keyboard`. */
export interface Desktop {
  wayland: boolean;
  keyboard: 'ready' | 'starting' | 'missing' | 'denied' | 'no-keyboard' | 'no-paste';
}

const ELSEWHERE: Desktop = { wayland: false, keyboard: 'ready' };

/** Read again every 2 s on Linux: the keyboard helper starts, and can be repaired, while Ivy runs. */
export function useDesktop(): Desktop | null {
  const [desktop, setDesktop] = useState<Desktop | null>(IS_LINUX ? null : ELSEWHERE);
  useEffect(() => {
    if (!IS_LINUX) return;
    let alive = true;
    const read = () =>
      invoke<Desktop>('get_desktop')
        .then((d) => alive && setDesktop(d))
        .catch(() => {});
    read();
    const id = window.setInterval(read, 2000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);
  return desktop;
}
