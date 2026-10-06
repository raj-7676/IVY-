import { DictationSession, SettingsConfig } from './types';

// Ivy ships with no history. Everything on Home and in History is derived
// from dictations the user actually made — there is deliberately no seeded
// sample data, because a number nobody earned is a lie about the product.
export const INITIAL_DICTATIONS: DictationSession[] = [];

// Real starting configuration, not sample content. The app-to-tone routing
// is genuine product behaviour (a message in Discord should not read like a
// contract), so it ships with sensible entries the user can edit or clear.
// The dictionary starts empty — those terms are the user's to teach — and
// microphones are enumerated from the system at runtime, never hardcoded.
export const INITIAL_SETTINGS: SettingsConfig = {
  hotkey: 'Alt + Space',
  activeTonePreset: 'Standard',
  presetApps: {
    Casual: ['Discord', 'WhatsApp', 'Instagram', 'Messages'],
    Standard: ['Code', 'Terminal', 'Notion', 'Obsidian'],
    Professional: ['Outlook', 'Gmail', 'Slack', 'Word'],
  },
  personalDictionary: [],
  selectedMic: '',
  availableMics: [],
  // Higher than Friday's own 72% default — Ivy's darker/warmer ground color
  // read as too see-through at that level against a busy wallpaper.
  glassOpacity: 90,
  glassBlur: 40,
  hardwareMode: 'gpu',
  smartVramEviction: true,
  vramEvictionThreshold: 80,
  launchAtStartup: true,
  // Two keys, not three (Yash's call) — not Alt+Z, which is the Nvidia app
  // overlay's own default hotkey (AMD Adrenalin listens for it too on
  // hybrid-GPU laptops). Alt+B isn't a known global hotkey for any of
  // those, Xbox Game Bar, Discord, or Steam.
  undoPasteHotkey: 'Alt + B',
  // One-handed, verified clear of every major overlay's real default
  // (Nvidia GeForce Experience: Alt+Z, AMD Adrenalin: Alt+R, Discord:
  // Shift+`, Steam: Shift+Tab, Xbox Game Bar: Win+G). Ctrl+Alt+V was
  // considered and rejected — it's Excel's actual current Paste Special
  // shortcut, a real collision, not a guess.
  manualPasteHotkey: 'Alt + V',
  onboardingCompleted: false,
};

