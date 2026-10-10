import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { AlertTriangle, CheckCircle2, Download, RefreshCw, ShieldCheck, VolumeX, WifiOff, X } from 'lucide-react';
import { IvyWordmark } from './IvyWordmark';
import INTRO_VIDEO from '../assets/ivy-intro.mp4';
import { IS_LINUX, IS_MAC, keyLabel } from '../utils/platform';
import { INITIAL_SETTINGS } from '../defaults';

// Ivy downloads its speech model itself on first start (src-tauri/src/model.rs); setup only copies it in for
// an offline install. First start: a full screen (ModelDownloadScreen), then the setup wizard opens by itself
// (App.tsx). A model missing after setup was done shows as a banner at the top instead.
// Both show ONLY when the backend says the model is missing or downloading: it checks the files before the
// window loads, and until it answers ("checking") nothing shows (Yash: one time only, never a flash).

export interface ModelStatus {
  state: 'checking' | 'missing' | 'downloading' | 'retrying' | 'verifying' | 'ready' | 'failed';
  done: number;
  total: number;
  /** Bytes per second. */
  speed: number;
  message: string;
}

const CHECKING: ModelStatus = { state: 'checking', done: 0, total: 0, speed: 0, message: '' };

/** The backend's download status, live. Outside Tauri (a plain browser tab) it reports ready. */
export function useModelStatus(): ModelStatus {
  const [status, setStatus] = useState<ModelStatus>(CHECKING);
  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    // Asked again whenever the window comes back, in case an event was missed while it was hidden.
    const ask = () =>
      invoke<ModelStatus>('model_status')
        .then(setStatus)
        .catch(() => setStatus({ ...CHECKING, state: 'ready' }));
    try {
      listen<ModelStatus>('ivy://model-download', (e) => setStatus(e.payload))
        .then((f) => (gone ? f() : (off = f)))
        .catch(() => {});
      ask();
      window.addEventListener('focus', ask);
    } catch {
      setStatus({ ...CHECKING, state: 'ready' });
    }
    return () => {
      gone = true;
      off?.();
      window.removeEventListener('focus', ask);
    };
  }, []);
  return status;
}

export const percent = (s: ModelStatus) => (s.total > 0 ? Math.floor((s.done * 100) / s.total) : 0);

/** The model really isn't usable yet: the only states that show the download screen or banner. */
export const needsDownload = (s: ModelStatus) =>
  s.state === 'missing' || s.state === 'downloading' || s.state === 'retrying' || s.state === 'verifying' || s.state === 'failed';

const mb = (bytes: number) => Math.round(bytes / 1048576).toLocaleString();

function timeLeft(s: ModelStatus): string {
  if (s.speed < 1024) return '';
  const secs = (s.total - s.done) / s.speed;
  if (secs < 60) return 'under a minute left';
  if (secs < 3600) return `about ${Math.round(secs / 60)} min left`;
  return `about ${(secs / 3600).toFixed(1)} h left`;
}

/** "840 of 2,361 MB · 12.3 MB/s · about 2 min left", or the retry message, or the check's percentage. */
function detail(s: ModelStatus): string {
  if (s.state === 'verifying') return `${percent(s)}%`;
  if (s.state === 'retrying') return s.message;
  if (s.total === 0) return 'starting…';
  return [`${mb(s.done)} of ${mb(s.total)} MB`, s.speed >= 1024 ? `${(s.speed / 1048576).toFixed(1)} MB/s` : '', timeLeft(s)]
    .filter(Boolean)
    .join(' · ');
}

const retry = () => invoke('retry_model_download').catch(() => undefined);

/** After setup was done, if the model ever has to be downloaded again (deleted, or a download that failed). */
export const ModelDownloadBanner: React.FC<{ hotkey: string }> = ({ hotkey }) => {
  const status = useModelStatus();
  // "Ready" shows for a few seconds, and only right after a download seen in this window; a model that was
  // already here shows nothing at all.
  const [justFinished, setJustFinished] = useState(false);
  const wasBusy = useRef(false);
  useEffect(() => {
    if (needsDownload(status)) {
      wasBusy.current = true;
    } else if (status.state === 'ready' && wasBusy.current) {
      wasBusy.current = false;
      setJustFinished(true);
      const t = window.setTimeout(() => setJustFinished(false), 6000);
      return () => clearTimeout(t);
    }
  }, [status.state]);

  if (!needsDownload(status) && !(status.state === 'ready' && justFinished)) return null;

  if (status.state === 'ready') {
    return (
      <Shell tone="ok">
        <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
        <span className="text-white/90 font-semibold">Speech model ready.</span>
        <span className="text-white/60">Hold {keyLabel(hotkey || INITIAL_SETTINGS.hotkey)} anywhere and talk.</span>
      </Shell>
    );
  }

  if (status.state === 'failed') {
    return (
      <Shell tone="warn">
        <AlertTriangle className="w-4 h-4 text-amber-300 shrink-0" />
        <span className="flex-1 min-w-0 text-amber-200">{status.message}</span>
        <button
          type="button"
          onClick={retry}
          className="shrink-0 flex items-center gap-1.5 px-3 py-1 rounded-full bg-[#FF6B00]/25 hover:bg-[#FF6B00]/40 border border-[#FF6B00]/50 text-white font-semibold transition-colors"
        >
          <RefreshCw className="w-3.5 h-3.5" />
          Retry
        </button>
      </Shell>
    );
  }

  const verifying = status.state === 'verifying';
  const retrying = status.state === 'retrying';
  return (
    <Shell tone={retrying ? 'warn' : 'busy'}>
      <div className="flex-1 min-w-0 flex flex-col gap-1.5">
        <div className="flex items-center gap-2 min-w-0">
          {verifying ? (
            <ShieldCheck className="w-4 h-4 text-[#FFA133] shrink-0" />
          ) : retrying ? (
            <RefreshCw className="w-4 h-4 text-amber-300 shrink-0 animate-spin" />
          ) : (
            <Download className="w-4 h-4 text-[#FFA133] shrink-0" />
          )}
          <span className="text-white/90 font-semibold shrink-0">
            {verifying ? 'Checking the speech model' : 'Downloading Ivy’s speech model'}
          </span>
          <span className={`truncate ${retrying ? 'text-amber-200' : 'text-white/55'}`}>{detail(status)}</span>
        </div>
        <div className="h-1 rounded-full bg-white/[0.08] overflow-hidden">
          <div
            className="h-full rounded-full bg-[#FF6B00] transition-[width] duration-500 ease-out"
            style={{ width: `${percent(status)}%` }}
          />
        </div>
        {!verifying && (
          <span className="text-[10.5px] text-white/40">One time only, about 2.5 GB. After this Ivy works fully offline.</span>
        )}
      </div>
    </Shell>
  );
};

const Shell: React.FC<{ tone: 'busy' | 'warn' | 'ok'; children: React.ReactNode }> = ({ tone, children }) => (
  <div
    role="status"
    className={`relative z-10 mx-4 mt-2 shrink-0 flex items-center gap-2.5 px-3.5 py-2.5 rounded-xl text-xs border ${
      tone === 'warn'
        ? 'bg-amber-500/10 border-amber-500/25'
        : tone === 'ok'
        ? 'bg-emerald-500/10 border-emerald-500/25'
        : 'bg-[#FF6B00]/[0.07] border-[#FF6B00]/25'
    }`}
  >
    {children}
  </div>
);

/**
 * Ivy's 74 s intro film: full screen and unskippable, once, on the first-run download screen after the launch
 * animation (the download takes about that long on a normal connection). It shows new users what Ivy does and
 * tells them not to skip the setup wizard that follows. Plays with sound; if WebView2 ever blocks that, it plays
 * muted with a "Sound on" button. Closing or minimizing Ivy (title bar, Alt+F4, tray) only hides the window, so
 * the film pauses with it and carries on where it was when Ivy comes back (Yash: nothing but the transcriber
 * may run in the background). A thin bar keeps the download visible underneath.
 */
export const IntroFilm: React.FC<{
  /** The first-run download, shown as a thin bar; left out when replayed from the title bar. */
  status?: ModelStatus;
  onPlaying: (playing: boolean) => void;
  onDone: () => void;
  /** Replays (title bar "Intro") can be closed; the first-run showing can't. */
  skippable?: boolean;
}> = ({ status, onPlaying, onDone, skippable = false }) => {
  const ref = useRef<HTMLVideoElement>(null);
  const [muted, setMuted] = useState(false);
  // Read through a ref: the download screen re-renders on every progress tick with a new onDone, and the
  // effect below must not restart (it would leave and re-enter full screen each time).
  const doneRef = useRef(onDone);
  doneRef.current = onDone;
  useEffect(() => {
    const v = ref.current;
    if (!v) return;
    let win: ReturnType<typeof getCurrentWindow> | null = null;
    try {
      win = getCurrentWindow();
    } catch {}
    let onScreen = true;
    let stopped = false;
    const play = () =>
      v.play().catch(() => {
        v.muted = true;
        setMuted(true);
        return v.play().catch(() => undefined);
      });
    onPlaying(true);
    win?.setFullscreen(true).catch(() => undefined);
    play();
    // Polled: a hidden WebView2 page gets no visibilitychange, which is how the first try kept playing in the tray.
    const check = async () => {
      if (stopped || !win) return;
      const visible = (await win.isVisible().catch(() => true)) && !(await win.isMinimized().catch(() => false));
      if (stopped) return;
      if (!visible && onScreen) {
        onScreen = false;
        v.pause();
      } else if (visible && !onScreen && !v.ended) {
        onScreen = true;
        win.setFullscreen(true).catch(() => undefined);
        play();
      }
    };
    const timer = window.setInterval(check, 250);
    // The title-bar close and minimize buttons say so first, so the sound stops at once.
    const hiding = () => {
      onScreen = false;
      v.pause();
    };
    window.addEventListener('ivy:window-hiding', hiding);
    const esc = (e: KeyboardEvent) => skippable && e.key === 'Escape' && doneRef.current();
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('keydown', esc);
      stopped = true;
      window.clearInterval(timer);
      window.removeEventListener('ivy:window-hiding', hiding);
      v.pause();
      win?.setFullscreen(false).catch(() => undefined);
      onPlaying(false);
    };
  }, [onPlaying, skippable]);
  const soundOn = () => {
    const v = ref.current;
    if (!v) return;
    v.muted = false;
    setMuted(false);
  };
  const ready = status?.state === 'ready';
  return (
    <div className="fixed inset-0 z-[80] bg-black flex items-center justify-center select-none" onContextMenu={(e) => e.preventDefault()}>
      {/* A system that can't decode the film (a Linux without an H.264 decoder for GStreamer) moves on rather
          than holding a black screen that can't be skipped on first start. */}
      <video
        ref={ref}
        src={INTRO_VIDEO}
        playsInline
        preload="auto"
        disablePictureInPicture
        className="w-full h-full object-contain"
        onEnded={onDone}
        onError={onDone}
      />
      {muted && (
        <button
          type="button"
          onClick={soundOn}
          className="absolute bottom-6 right-6 flex items-center gap-2 px-4 py-2 rounded-full bg-black/70 hover:bg-black/85 border border-white/20 text-white text-sm font-semibold"
        >
          <VolumeX className="w-4 h-4" />
          Sound on
        </button>
      )}
      {skippable && (
        <button
          type="button"
          onClick={onDone}
          title="Close (Esc)"
          className="absolute top-5 right-5 flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-black/60 hover:bg-black/80 border border-white/15 text-white/80 text-xs font-semibold"
        >
          <X className="w-3.5 h-3.5" />
          Close
        </button>
      )}
      {status && (
        <>
          <div className="absolute left-4 bottom-3 text-[11px] font-medium text-white/50">
            {ready ? 'Speech model downloaded' : `Downloading Ivy's speech model · ${percent(status)}%`}
          </div>
          <div className="absolute left-0 right-0 bottom-0 h-[3px] bg-white/10">
            <div className="h-full bg-[#FF6B00] transition-[width] duration-500" style={{ width: `${ready ? 100 : percent(status)}%` }} />
          </div>
        </>
      )}
    </div>
  );
};

/**
 * First start, in place of the setup wizard, while `needsDownload` (and, once the model is ready, until the
 * intro film ends). App.tsx opens the wizard after.
 */
export const ModelDownloadScreen: React.FC<{
  status: ModelStatus;
  /** The launch animation is over: the film may start. */
  launchDone: boolean;
  onIntroPlaying: (playing: boolean) => void;
}> = ({ status, launchDone, onIntroPlaying }) => {
  const [filmDone, setFilmDone] = useState(false);
  const failed = status.state === 'failed';
  const retrying = status.state === 'retrying';
  const verifying = status.state === 'verifying';
  const ready = status.state === 'ready';
  const starting = status.state === 'missing' || status.total === 0;
  return (
    <div className="relative flex-1 flex flex-col items-center justify-center px-6 py-6 overflow-y-auto">
      <div
        aria-hidden="true"
        className="pointer-events-none absolute top-12 left-1/2 -translate-x-1/2 w-[600px] h-[350px] rounded-full blur-[110px] opacity-25"
        style={{ background: 'radial-gradient(circle, rgba(255, 107, 0, 0.7) 0%, transparent 70%)' }}
      />
      {launchDone && !filmDone && <IntroFilm status={status} onPlaying={onIntroPlaying} onDone={() => setFilmDone(true)} />}
      <div className="relative w-full max-w-xl flex flex-col items-center gap-7">
        <IvyWordmark height={40} glow />
        <div className="text-center flex flex-col gap-2">
          <h1 className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white" style={{ fontFamily: 'Syne, sans-serif' }}>
            {failed ? 'Download stopped' : ready ? 'Ivy is ready' : verifying ? 'Almost ready' : 'Getting Ivy ready'}
          </h1>
          <p className="text-sm text-white/60 max-w-md">
            {failed
              ? 'Ivy couldn’t finish downloading its speech model. What it already has is kept.'
              : ready
                ? 'The speech model is downloaded. Ivy works fully offline from now on.'
                : 'Ivy is downloading its speech model. One time only, about 2.5 GB. After this, Ivy works fully offline.'}
          </p>
        </div>

        <div
          role="status"
          className="w-full rounded-3xl p-6 border flex flex-col gap-3"
          style={{
            background: 'linear-gradient(160deg, rgba(28, 20, 38, 0.85) 0%, rgba(12, 9, 18, 0.92) 100%)',
            borderColor: failed || retrying ? 'rgba(245, 158, 11, 0.3)' : 'rgba(255, 107, 0, 0.25)',
            boxShadow: '0 25px 60px -15px rgba(0,0,0,0.85), 0 0 35px rgba(255,107,0,0.12)',
          }}
        >
          <div className="flex items-center gap-2.5 text-sm">
            {failed ? (
              <WifiOff className="w-4 h-4 text-amber-300 shrink-0" />
            ) : retrying ? (
              <RefreshCw className="w-4 h-4 text-amber-300 shrink-0 animate-spin" />
            ) : verifying ? (
              <ShieldCheck className="w-4 h-4 text-[#FFA133] shrink-0" />
            ) : ready ? (
              <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
            ) : (
              <Download className="w-4 h-4 text-[#FFA133] shrink-0" />
            )}
            <span className="font-semibold text-white/90">
              {failed ? 'Not downloaded' : ready ? 'Downloaded' : verifying ? 'Checking the download' : starting ? 'Starting' : `${percent(status)}%`}
            </span>
            {!failed && !starting && !ready && (
              <span className={`ml-auto text-xs truncate ${retrying ? 'text-amber-200' : 'text-white/55'}`}>{detail(status)}</span>
            )}
          </div>
          <div className="h-2 rounded-full bg-white/[0.08] overflow-hidden">
            <div
              className={`h-full rounded-full transition-[width] duration-500 ease-out ${failed ? 'bg-amber-500/60' : 'bg-[#FF6B00]'}`}
              style={{ width: `${percent(status)}%`, boxShadow: failed ? undefined : '0 0 12px rgba(255, 107, 0, 0.6)' }}
            />
          </div>
          {failed && (
            <div className="flex items-center gap-3 pt-1">
              <span className="flex-1 text-xs text-amber-200/90">{status.message}</span>
              <button
                type="button"
                onClick={retry}
                className="shrink-0 flex items-center gap-1.5 px-4 py-2 rounded-xl bg-[#FF6B00] hover:bg-[#FF7A1A] text-white text-sm font-semibold transition-colors shadow-[0_0_20px_rgba(255,107,0,0.4)]"
              >
                <RefreshCw className="w-4 h-4" />
                Retry
              </button>
            </div>
          )}
        </div>

        {!failed && !ready && (
          <p className="text-xs text-white/40 text-center max-w-md">
            Setup opens by itself when the download is done. You can close this window meanwhile: Ivy keeps
            downloading {IS_MAC ? 'from the menu bar' : IS_LINUX ? 'in the background' : 'from the tray'} and comes back when it is ready.
          </p>
        )}
      </div>
    </div>
  );
};
