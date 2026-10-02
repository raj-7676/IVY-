export type TonePreset = 'Casual' | 'Standard' | 'Professional';

export interface DictationSession {
  id: string;
  preview: string;
  fullTranscript: string;
  /** Display string, e.g. "Today, 2:42 PM". Not parseable — use createdAt. */
  timestamp: string;
  /** Epoch ms, stamped by the Rust backend on insert. 0 for pre-v0.2 entries. */
  createdAt: number;
  duration: string;
  durationSec: number;
  tonePreset: TonePreset;
  appTarget: string;
  wordsCount: number;
  audioDuration: number;
  /** Absolute path to the saved recording, or "" if none was kept. */
  audioPath: string;
  /** Real, opt-in summary — only present once the user explicitly asked
   *  History to summarize this transcript. Never auto-generated. */
  summary?: string;
  /** Short auto-generated title, filled in by a background pass after the
   *  dictation has already pasted. Absent until that finishes — fall back
   *  to `preview` as the row headline until it does. */
  title?: string;
}

export type ScreenState = 'home' | 'history' | 'dictionary' | 'tone' | 'settings' | 'first-run';

export type CapsuleMode = 'idle' | 'recording' | 'transcribing' | 'pasted' | 'failed' | 'undone' | 'launched' | 'manual-pasted' | 'touch-up';

export type HardwareMode = 'gpu' | 'cpu';

export interface HardwareStatus {
  activeEngine: 'gpu' | 'cpu' | 'evicted';
  configuredMode: HardwareMode;
  gpuName: string;
  gpuUsagePercent: number;
  vramUsedMb: number;
  vramTotalMb: number;
  isEvicted: boolean;
  /** True when the machine is genuinely running on battery (AC unplugged),
   *  read from Windows' own power status — not a guess. Dictation forces
   *  CPU whenever this is true, regardless of the configured mode. */
  onBattery: boolean;
}

export type DictationMode = 'speed' | 'accuracy';

export interface SettingsConfig {
  hotkey: string;
  activeTonePreset: TonePreset;
  presetApps: Record<TonePreset, string[]>;
  personalDictionary: string[];
  cleanupPass: boolean;
  /** Speed (<0.1s core rule cleaner) vs Accuracy (instant full 50+ deterministic rule set; on-demand Touch Up). */
  dictationMode: DictationMode;
  selectedMic: string;
  availableMics: string[];
  /** Main window glass background opacity, 30-95 (%). */
  glassOpacity: number;
  /** Main window backdrop blur, 12-50 (px). */
  glassBlur: number;
  /** Compute acceleration: GPU (DirectML speech recognition) or CPU (processor speech recognition; 0% VRAM). */
  hardwareMode: HardwareMode;
  /** Automatically unload models from VRAM when GPU usage >= threshold (e.g. gaming). */
  smartVramEviction: boolean;
  /** GPU utilization threshold (percentage, e.g. 90) to trigger eviction. */
  vramEvictionThreshold: number;
  /** Register Ivy to launch in the background at Windows sign-in (real
   *  registry Run-key entry via the OS, not a simulated switch). */
  launchAtStartup: boolean;
  /** Separate, user-remappable shortcut that reverts the last real paste:
   *  sends Ctrl+Z into the app it was pasted into, restores the clipboard
   *  to whatever it held before. */
  undoPasteHotkey: string;
  /** Separate, user-remappable shortcut that pastes a dictation's transcript
   *  when Ivy found no text field to paste into automatically — Ivy never
   *  touches the real clipboard on its own in that case (it could hold
   *  something the user actually meant to keep, like a password or an API
   *  key); this hotkey injects the held-back text on demand instead. */
  manualPasteHotkey: string;
  /** True when user has completed or dismissed the initial onboarding wizard. */
  onboardingCompleted?: boolean;
  /** Active transcription engine: 'voxtral' (default, end-to-end multimodal) | 'whisper' (legacy fallback). */
  engine?: string;
}

export interface UserStats {
  totalWords: number;
  wordsPerMinute: number;
  dayStreak: number;
  lastActiveDate: string;
  totalDurationSec: number;
  sessionCount: number;
  dailyWords: Record<string, number>;
}

