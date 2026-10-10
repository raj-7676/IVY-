import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow, Effect, EffectState } from '@tauri-apps/api/window';
import { Sidebar } from './components/Sidebar';
import { HomeView } from './components/HomeView';
import { HistoryView } from './components/HistoryView';
import { DictionaryView } from './components/DictionaryView';
import { ToneView } from './components/ToneView';
import { SettingsView } from './components/SettingsView';
import { FirstRunView } from './components/FirstRunView';
import { PauseControl } from './components/PauseControl';
import { GlassParticles } from './components/GlassParticles';
import { IvyLaunchIntro } from './components/IvyLaunchIntro';
import { IvyLogo } from './components/IvyLogo';
import { IntroFilm, ModelDownloadBanner, ModelDownloadScreen, needsDownload, useModelStatus } from './components/ModelDownload';
import { INITIAL_DICTATIONS, INITIAL_SETTINGS } from './defaults';
import { ScreenState, DictationSession, SettingsConfig, UserStats } from './types';
import { Film, Minus, Square, X } from 'lucide-react';
import { IS_LINUX, IS_MAC, keyLabel, useDesktop } from './utils/platform';
import { requestPermission, usePermissions } from './components/MacPermissions';
import { LinuxKeyboardNotice } from './components/LinuxKeyboard';

const ACCENT_RGB = '255, 107, 0';

export default function App() {
  // 'wait' = intro not seen yet but the window isn't on screen: a startup (Run-key) launch loads this page
  // hidden, and playing the intro there made its sound at boot (Yash, 2026-10-07). It plays on first show.
  const [intro, setIntro] = useState<'wait' | 'play' | 'off'>(() => {
    try {
      return sessionStorage.getItem('ivy_intro_seen') === 'true' ? 'off' : 'wait';
    } catch {
      return 'wait';
    }
  });

  useEffect(() => {
    if (intro !== 'wait') return;
    let win: ReturnType<typeof getCurrentWindow>;
    try {
      win = getCurrentWindow();
    } catch {
      setIntro('play'); // plain browser tab
      return;
    }
    let done = false;
    let unlisten: (() => void) | undefined;
    let timer: number | undefined;
    const start = () => {
      if (done) return;
      done = true;
      unlisten?.();
      window.clearInterval(timer);
      setIntro('play');
    };
    // Polled too: opening Ivy while it already runs hidden (started with Windows) shows the window with
    // set_focus but no focus event reaches the page, which left it on this black backdrop for good (a
    // tester's PC, 2026-10-09).
    const check = () => win.isVisible().then((v) => v && start()).catch(start);
    check();
    timer = window.setInterval(check, 500);
    // Tray, hotkey and second launch all show the window with set_focus.
    win
      .onFocusChanged(({ payload }) => payload && start())
      .then((u) => (done ? u() : (unlisten = u)))
      .catch(() => {});
    return () => {
      done = true;
      unlisten?.();
      window.clearInterval(timer);
    };
  }, [intro]);

  const handleIntroComplete = useCallback(() => {
    setIntro('off');
    try {
      sessionStorage.setItem('ivy_intro_seen', 'true');
    } catch {}
  }, []);
  const [currentScreen, setCurrentScreen] = useState<ScreenState>(() => {
    const completed = typeof window !== 'undefined' && localStorage.getItem('ivy_onboarding_completed') === 'true';
    return completed ? 'home' : 'first-run';
  });
  // First start: the speech model downloads first, then the setup wizard opens by itself (Yash, 2026-10-08;
  // the backend brings the window back, lib.rs model_ready). Only when the backend says the model really is
  // missing (it checks the files before this page loads), so a model that is already here never shows the
  // download screen, not even for a moment.
  const model = useModelStatus();
  // The intro film on the download screen holds it open after the download, until the film ends or the user
  // continues, so the wizard never cuts it off mid-sentence.
  const [introFilmPlaying, setIntroFilmPlaying] = useState(false);
  // Title-bar "Intro": replays the film for anyone who never saw it (offline installs, people who updated).
  const [replayFilm, setReplayFilm] = useState(false);
  const noop = useCallback(() => undefined, []);
  const closeFilm = useCallback(() => setReplayFilm(false), []);
  const waitingForModel = currentScreen === 'first-run' && (needsDownload(model) || introFilmPlaying);
  const [sessions, setSessions] = useState<DictationSession[]>(INITIAL_DICTATIONS);
  const [userStats, setUserStats] = useState<UserStats | undefined>(undefined);
  const [settings, setSettings] = useState<SettingsConfig>(INITIAL_SETTINGS);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  // The dictation shortcut didn't register at startup (another app holds it; lib.rs HOTKEY_PROBLEM).
  const [hotkeyProblem, setHotkeyProblem] = useState<string | null>(null);
  const readHotkeyProblem = useCallback(() => {
    invoke<string | null>('get_hotkey_problem')
      .then(setHotkeyProblem)
      .catch(() => {});
  }, []);
  useEffect(readHotkeyProblem, [readHotkeyProblem]);
  const settingsLoaded = useRef(false);
  const settingsRef = useRef<SettingsConfig>(INITIAL_SETTINGS);
  // What the backend last accepted (the revert target if a save fails), and the pending save.
  const savedSettingsRef = useRef<SettingsConfig>(INITIAL_SETTINGS);
  const saveTimer = useRef<number | undefined>(undefined);

  // Real, persisted settings + history + decoupled stats from the Rust backend.
  useEffect(() => {
    invoke<SettingsConfig>('get_settings')
      .then((s) => {
        settingsRef.current = s;
        savedSettingsRef.current = s;
        setSettings(s);
        settingsLoaded.current = true;
        if (s.onboardingCompleted) {
          try {
            localStorage.setItem('ivy_onboarding_completed', 'true');
          } catch {}
          setCurrentScreen((prev) => (prev === 'first-run' ? 'home' : prev));
        }
      })
      .catch(() => {});
    invoke<DictationSession[]>('get_history')
      .then((h) => {
        if (h.length > 0) setSessions(h);
      })
      .catch(() => {});
    invoke<UserStats>('get_user_stats')
      .then((stats) => setUserStats(stats))
      .catch(() => {});

    // Registration fails outside Tauri (a plain browser tab, used for checking
    // the UI) — that must not take the whole window down.
    let unlistenHistory: (() => void) | undefined;
    let unlistenStats: (() => void) | undefined;
    let unlistenSettings: (() => void) | undefined;
    try {
      listen('ivy://history-updated', () => {
        invoke<DictationSession[]>('get_history')
          .then((h) => setSessions(h))
          .catch(() => {});
      })
        .then((f) => {
          unlistenHistory = f;
        })
        .catch(() => {});
      listen('ivy://stats-updated', () => {
        invoke<UserStats>('get_user_stats')
          .then((stats) => setUserStats(stats))
          .catch(() => {});
      })
        .then((f) => {
          unlistenStats = f;
        })
        .catch(() => {});
      // Real tray menu action ("Settings"), not just a UI nicety — the tray
      // is the only way to reach Ivy at all once the main window is closed.
      listen('ivy://open-settings', () => {
        setCurrentScreen('settings');
      })
        .then((f) => {
          unlistenSettings = f;
        })
        .catch(() => {});
    } catch {
      /* no Tauri host */
    }
    return () => {
      unlistenHistory?.();
      unlistenStats?.();
      unlistenSettings?.();
    };
  }, []);

  // Real desktop blur comes from Windows (Acrylic), or on a Mac from its own dark glass material, kept on while Ivy
  // isn't the active app; CSS backdrop-filter can't see behind the window. Linux desktops offer apps no blur
  // (Settings hides the switch there).
  const blurOn = settings.glassBlur > 0;
  useEffect(() => {
    if (IS_LINUX) return;
    try {
      const win = getCurrentWindow();
      const effects = IS_MAC
        ? { effects: [Effect.HudWindow], state: EffectState.Active }
        : { effects: [Effect.Acrylic] };
      (blurOn ? win.setEffects(effects) : win.clearEffects()).catch(() => {});
    } catch {
      /* no Tauri host */
    }
  }, [blurOn]);

  // macOS: pasting needs the Accessibility permission, which every update asks for again (Ivy isn't signed
  // with an Apple certificate), so the main window says when it's missing.
  const perms = usePermissions(IS_MAC);
  // Linux on Wayland: the same kind of banner when Ivy's keyboard helper doesn't work.
  const desktop = useDesktop();
  const minimize = () => {
    window.dispatchEvent(new Event('ivy:window-hiding'));
    invoke('minimize_main');
  };
  const toggleMaximize = () => invoke('toggle_maximize_main');
  const close = () => {
    window.dispatchEvent(new Event('ivy:window-hiding'));
    invoke('close_main');
  };

  const handleDeleteSession = useCallback((id: string) => {
    setSessions((prev) => prev.filter((s) => s.id !== id));
    invoke('delete_history_entry', { id }).catch(() => {});
  }, []);

  const handleUpdateSession = useCallback((updated: DictationSession) => {
    setSessions((prev) => prev.map((s) => (s.id === updated.id ? updated : s)));
  }, []);

  // Persisted to disk so the real capsule window (a separate window, same
  // backend) sees tone presets, hotkey, dictionary, etc. as soon as they change.
  const handleUpdateSettings = useCallback((newSettings: Partial<SettingsConfig>) => {
    // Merged from a ref, not inside a setState updater: React can defer an
    // updater until the next render, which left `merged` null here and
    // silently skipped the save while the screen still showed the change.
    const merged = { ...settingsRef.current, ...newSettings };
    settingsRef.current = merged;
    setSettings(merged);
    if (!settingsLoaded.current) return;
    // One save per burst of changes: dragging a slider fires many updates, and the backend refuses
    // saves closer than 100ms apart ("rate limit exceeded"), which used to revert the slider.
    clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      const toSave = settingsRef.current;
      invoke('save_settings', { settings: toSave })
        .then(() => {
          savedSettingsRef.current = toSave;
          readHotkeyProblem();
        })
        .catch((err) => {
          // A real failure here (most commonly: the new hotkey is already
          // taken by something else) must not leave the UI showing a
          // setting that was never actually applied — revert and say so.
          settingsRef.current = savedSettingsRef.current;
          setSettings(savedSettingsRef.current);
          setSettingsError(typeof err === 'string' ? err : 'Could not save that setting.');
        });
    }, 150);
  }, []);

  return (
    <div
      id="ivy-window"
      // No CSS border or rim shadows: Windows draws the window's own thin neutral border. The old orange
      // border and top highlight showed only on the top and left edges, which looked like a stray orange
      // line (Yash, 2026-10-07).
      style={{ backgroundColor: `rgba(10, 8, 14, ${settings.glassOpacity / 100})` }}
      className="relative flex flex-col h-screen w-screen overflow-hidden antialiased select-none"
    >
      {/* Cinematic Launch Intro with smooth zoom-out-and-dock */}
      {intro === 'play' && (
        <IvyLaunchIntro onComplete={handleIntroComplete} />
      )}
      {/* Same backdrop as the intro, so the app doesn't flash before it starts. */}
      {intro === 'wait' && <div className="fixed inset-0 z-50 bg-[#050508]" />}

      {/* Ambient warm wash + drifting glass motes */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 z-0"
        style={{
          background: `radial-gradient(900px 520px at 78% -8%, rgba(${ACCENT_RGB}, 0.14), transparent 62%),
                       radial-gradient(700px 600px at 6% 108%, rgba(${ACCENT_RGB}, 0.07), transparent 58%)`,
        }}
      />
      <GlassParticles accentRgb={ACCENT_RGB} />

      {/* Titlebar Header - on top (z-[100]) so window controls stay reachable, except during the launch intro,
          which has its own Skip and close buttons (two sets overlapped in the corner; Yash, 2026-10-08), and the
          replayed film: it sits under the header, whose buttons took the clicks meant for its Close. */}
      <header
        data-tauri-drag-region
        // macOS draws its own red, yellow and green window buttons at the left of this bar (tauri.macos.conf.json:
        // native title bar as an overlay), so the bar starts after them there.
        className={`relative z-[100] h-11 flex items-center justify-between px-4 shrink-0 border-b border-white/[0.07] ${IS_MAC ? 'pl-[88px]' : ''} ${intro !== 'off' || replayFilm ? 'invisible' : ''}`}
      >
        <button
          type="button"
          onClick={() => setIntro('play')}
          title="Replay intro"
          className="flex items-center gap-2 cursor-pointer"
        >
          <IvyLogo size={18} glow={true} />
          <span className="text-[13px] font-semibold tracking-tight text-white/90">Ivy</span>
          <span className="text-[11px] font-medium text-white/35">v{__IVY_VERSION__}</span>
        </button>

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => setReplayFilm(true)}
            title="Watch Ivy's intro film"
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-[11px] font-medium text-white/60 hover:text-white bg-white/[0.05] hover:bg-white/[0.1] border border-white/[0.08] transition-colors"
          >
            <Film className="w-3 h-3" />
            Intro
          </button>
          <PauseControl />
          <div className="hidden md:flex items-center gap-1.5 text-[11px] text-white/45">
            <span>Hold</span>
            <kbd className="px-1.5 py-0.5 rounded-md bg-white/[0.07] border border-white/[0.1] text-white/80 text-[10px]">
              {keyLabel(settings.hotkey || INITIAL_SETTINGS.hotkey)}
            </kbd>
            <span>anywhere to dictate</span>
          </div>

          {!IS_MAC && (
            <div className="flex items-center gap-0.5">
              <button
                id="win-minimize"
                onClick={minimize}
                className="w-8 h-7 flex items-center justify-center rounded-lg text-white/45 hover:bg-white/[0.1] hover:text-white transition-colors duration-150"
                title="Minimize"
              >
                <Minus className="w-3.5 h-3.5" />
              </button>
              <button
                id="win-maximize"
                onClick={toggleMaximize}
                className="w-8 h-7 flex items-center justify-center rounded-lg text-white/45 hover:bg-white/[0.1] hover:text-white transition-colors duration-150"
                title="Maximize"
              >
                <Square className="w-3 h-3" />
              </button>
              <button
                id="win-close"
                onClick={close}
                className="w-8 h-7 flex items-center justify-center rounded-lg text-white/45 hover:bg-red-500 hover:text-white transition-colors duration-150"
                title="Close"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          )}
        </div>
      </header>

      {IS_MAC && perms && !perms.accessibility && currentScreen !== 'first-run' && (
        <div className="relative z-10 mx-4 mt-2 shrink-0 flex items-center justify-between gap-3 px-3.5 py-2 rounded-xl text-xs font-medium text-amber-300 bg-amber-500/10 border border-amber-500/25">
          <span>
            Ivy can't type into other apps until you allow it in System Settings › Privacy &amp; Security ›
            Accessibility. After an update, switch Ivy off and on there.
          </span>
          <button
            onClick={() => requestPermission('accessibility')}
            className="shrink-0 px-2.5 py-1 rounded-lg bg-amber-500/20 hover:bg-amber-500/30 text-amber-200"
          >
            Open settings
          </button>
        </div>
      )}

      {currentScreen !== 'first-run' && <LinuxKeyboardNotice desktop={desktop} className="relative z-10 mx-4 mt-2 shrink-0" />}

      {hotkeyProblem && (
        <div className="relative z-10 mx-4 mt-2 shrink-0 flex items-center justify-between gap-3 px-3.5 py-2 rounded-xl text-xs font-medium text-amber-300 bg-amber-500/10 border border-amber-500/25">
          <span>{hotkeyProblem}</span>
          {currentScreen !== 'first-run' && (
            <button
              onClick={() => setCurrentScreen('settings')}
              className="shrink-0 px-2.5 py-1 rounded-lg bg-amber-500/20 hover:bg-amber-500/30 text-amber-200"
            >
              Change shortcut
            </button>
          )}
        </div>
      )}

      {settingsError && (
        <div className="relative z-10 mx-4 mt-2 shrink-0 flex items-center justify-between gap-3 px-3.5 py-2 rounded-xl text-xs font-medium text-amber-300 bg-amber-500/10 border border-amber-500/25">
          <span>{settingsError}</span>
          <button
            onClick={() => setSettingsError(null)}
            className="text-amber-300/70 hover:text-amber-200 shrink-0"
          >
            Dismiss
          </button>
        </div>
      )}

      {currentScreen !== 'first-run' && <ModelDownloadBanner hotkey={settings.hotkey} />}

      {/* App Body */}
      <div className="relative z-10 flex-1 flex min-h-0 overflow-hidden">
        {currentScreen !== 'first-run' && (
          <Sidebar
            currentScreen={currentScreen}
            onSelectScreen={setCurrentScreen}
            sessionCount={sessions.length}
          />
        )}

        <main className="flex-1 flex flex-col min-w-0 h-full overflow-hidden">
          {currentScreen === 'home' && (
            <HomeView
              sessions={sessions}
              userStats={userStats}
              hotkey={settings.hotkey}
              onSelectScreen={setCurrentScreen}
            />
          )}
          {currentScreen === 'history' && (
            <HistoryView
              sessions={sessions}
              historyDays={settings.historyDays}
              hotkey={settings.hotkey}
              onHistoryDays={(historyDays) => handleUpdateSettings({ historyDays })}
              onDeleteSession={handleDeleteSession}
              onUpdateSession={handleUpdateSession}
            />
          )}
          {currentScreen === 'dictionary' && (
            <DictionaryView settings={settings} onUpdateSettings={handleUpdateSettings} />
          )}
          {currentScreen === 'tone' && (
            <ToneView settings={settings} onUpdateSettings={handleUpdateSettings} />
          )}
          {currentScreen === 'settings' && (
            <SettingsView
              settings={settings}
              onUpdateSettings={handleUpdateSettings}
              onReplayOnboarding={() => setCurrentScreen('first-run')}
            />
          )}
          {replayFilm && <IntroFilm skippable onPlaying={noop} onDone={closeFilm} />}
          {waitingForModel && <ModelDownloadScreen status={model} launchDone={intro === 'off'} onIntroPlaying={setIntroFilmPlaying} />}
          {currentScreen === 'first-run' && !waitingForModel && (
            <FirstRunView
              hotkey={settings.hotkey}
              manualPasteHotkey={settings.manualPasteHotkey}
              hardwareMode={settings.hardwareMode}
              onUpdateSettings={handleUpdateSettings}
              onDismiss={() => {
                try {
                  localStorage.setItem('ivy_onboarding_completed', 'true');
                } catch {}
                handleUpdateSettings({ onboardingCompleted: true });
                setCurrentScreen('home');
              }}
              onComplete={() => {
                try {
                  localStorage.setItem('ivy_onboarding_completed', 'true');
                } catch {}
                handleUpdateSettings({ onboardingCompleted: true });
                setCurrentScreen('home');
              }}
            />
          )}
        </main>
      </div>
    </div>
  );
}
