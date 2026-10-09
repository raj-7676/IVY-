// Which OS Ivy runs on, for words and key names on screen (the backend does the real per-OS work). WKWebView on
// a Mac reports "Macintosh" in its user agent; WebView2 on Windows never does.
export const IS_MAC = typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent);

/** "PC" or "Mac", as in "Your voice stays on your PC." */
export const DEVICE = IS_MAC ? 'Mac' : 'PC';

/** The paste and undo modifier: Ctrl on Windows, Cmd on a Mac. */
export const MOD = IS_MAC ? 'Cmd' : 'Ctrl';

const MAC_KEYS: Record<string, string> = { Alt: '⌥ Option', Ctrl: '⌃ Control', Shift: '⇧ Shift' };

/** A saved shortcut as the keyboard in front of the user labels it: "Alt + Space" reads "⌥ Option + Space" on a
 *  Mac. Settings keep the saved form ("Alt + Space"); only what is shown changes. */
export const keyLabel = (spec: string): string =>
  IS_MAC ? spec.split('+').map((k) => MAC_KEYS[k.trim()] ?? k.trim()).join(' + ') : spec;
