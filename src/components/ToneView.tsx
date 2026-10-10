import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Plus, X } from 'lucide-react';
import { SettingsConfig, TonePreset } from '../types';
import { IS_LINUX, IS_MAC, useDesktop } from '../utils/platform';
import { useEscape } from '../utils/useEscape';

interface ToneViewProps {
  settings: SettingsConfig;
  onUpdateSettings: (newSettings: Partial<SettingsConfig>) => void;
}

const ACCENT_RGB = '255, 107, 0';

// Samples are the rulebooks' real output for the same dictation
// (rulebooks::tone::tests::tone_screen_samples_are_real).
const TONES: { id: TonePreset; blurb: string; sample: string }[] = [
  {
    id: 'Casual',
    blurb: 'Texting style: your words as you said them, no full stops, one sentence per line.',
    sample: "Yeah, I'm gonna push that fix before standup\nThanks!",
  },
  {
    id: 'Standard',
    blurb: 'Slang written out, filler "like" and "you know" dropped. Your voice, tidied.',
    sample: "Yeah, I'm going to push that fix before standup. Thanks!",
  },
  {
    id: 'Professional',
    blurb: 'No contractions, no slang or chat words, no exclamation marks.',
    sample: 'Yes, I am going to push that fix before standup. Thank you.',
  },
];

const cardStyle = (on: boolean): React.CSSProperties =>
  on
    ? { backgroundColor: `rgba(${ACCENT_RGB}, 0.1)`, border: `1px solid rgba(${ACCENT_RGB}, 0.3)` }
    : { backgroundColor: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.07)' };

const InUse: React.FC = () => (
  <span
    className="text-[10px] px-2 py-0.5 rounded-full font-medium"
    style={{ backgroundColor: `rgba(${ACCENT_RGB}, 0.16)`, color: `rgb(${ACCENT_RGB})` }}
  >
    In use
  </span>
);

/** "Slack.app" reads "Slack", Linux's "discord" reads "Discord"; the list keeps the bundle's full name or the
 *  window class, which is what Ivy matches on (lib.rs). */
const appLabel = (app: string) => {
  const name = app.replace(/\.app$/i, '');
  return IS_LINUX ? name.charAt(0).toUpperCase() + name.slice(1) : name;
};

const addAppStyle =
  'inline-flex items-center gap-1.5 cursor-pointer border border-dashed border-white/[0.16] hover:border-white/[0.3] rounded-full px-3.5 py-1.5 text-[12px] text-white/70 transition-colors duration-150';

/** macOS and Linux: an app is picked from the ones running now; an .app bundle can't go through a file picker, and a
 *  Linux program is known by its window. */
const RunningAppPicker: React.FC<{ apps: string[]; onPick: (app: string) => void }> = ({ apps, onPick }) => {
  const [open, setOpen] = useState(false);
  const [running, setRunning] = useState<string[] | null>(null);
  useEscape(open, () => setOpen(false));
  const show = () => {
    setRunning(null);
    setOpen(true);
    invoke<string[]>('list_running_apps')
      .then(setRunning)
      .catch(() => setRunning([]));
  };
  const choices = (running ?? []).filter((r) => !apps.some((a) => a.toLowerCase() === r.toLowerCase()));
  return (
    <div className="relative">
      <button type="button" onClick={show} className={addAppStyle} title="Pick from the apps open right now">
        <Plus className="w-3.5 h-3.5" style={{ color: `rgb(${ACCENT_RGB})` }} />
        Add app
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
          <div
            className="absolute z-20 top-full left-0 mt-1.5 min-w-[220px] max-h-64 overflow-y-auto rounded-2xl py-1"
            style={{ backgroundColor: 'rgba(16, 13, 20, 0.97)', border: '1px solid rgba(255,255,255,0.1)' }}
          >
            {running === null ? (
              <div className="px-3.5 py-2 text-[12px] text-white/50">Looking…</div>
            ) : choices.length === 0 ? (
              <div className="px-3.5 py-2 text-[12px] text-white/50 max-w-[260px]">No other apps are open. Open the app first.</div>
            ) : (
              choices.map((app) => (
                <button
                  key={app}
                  type="button"
                  onClick={() => {
                    onPick(app);
                    setOpen(false);
                  }}
                  className="w-full text-left px-3.5 py-2 text-[12px] text-white/80 hover:bg-white/[0.07] transition-colors duration-150 whitespace-nowrap"
                >
                  {appLabel(app)}
                </button>
              ))
            )}
          </div>
        </>
      )}
    </div>
  );
};

const AppList: React.FC<{
  tone: TonePreset;
  apps: string[];
  onChange: (apps: string[]) => void;
}> = ({ tone, apps, onChange }) => {
  // Windows' own file picker: only the program's name (e.g. "brave.exe") is kept, never its path.
  const pick = (e: React.ChangeEvent<HTMLInputElement>) => {
    const name = e.target.files?.[0]?.name;
    if (name && !apps.some((a) => a.toLowerCase() === name.toLowerCase())) onChange([...apps, name]);
    e.target.value = '';
  };
  return (
    <div className="space-y-2">
      <div className="text-[12.5px] font-medium text-white/75">{tone}</div>
      <div className="flex flex-wrap gap-2 items-center">
        {apps.map((app) => (
          <span
            key={app}
            className="inline-flex items-center gap-2 pl-3.5 pr-2 py-1.5 rounded-full text-[12px] text-white/85"
            style={{ backgroundColor: 'rgba(255,255,255,0.05)', border: '1px solid rgba(255,255,255,0.09)' }}
          >
            {appLabel(app)}
            <button
              onClick={() => onChange(apps.filter((a) => a !== app))}
              className="text-white/30 hover:text-red-400 transition-colors duration-150"
              title={`Remove ${app}`}
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </span>
        ))}
        {IS_MAC || IS_LINUX ? (
          <RunningAppPicker apps={apps} onPick={(app) => onChange([...apps, app])} />
        ) : (
          <label className={addAppStyle} title="Pick the app's .exe file">
            <Plus className="w-3.5 h-3.5" style={{ color: `rgb(${ACCENT_RGB})` }} />
            Add app
            <input type="file" accept=".exe" onChange={pick} className="hidden" />
          </label>
        )}
      </div>
    </div>
  );
};

export const ToneView: React.FC<ToneViewProps> = ({ settings, onUpdateSettings }) => {
  const desktop = useDesktop();
  const inUse = settings.activeTonePreset;
  const shown = TONES.find((t) => t.id === inUse);
  // Three modes (Yash, 2026-10-06): a click takes effect at once for every app not added below.
  const use = (tone: TonePreset) => {
    if (inUse !== tone) onUpdateSettings({ activeTonePreset: tone });
  };
  // An app belongs to one mode only: adding it to a mode takes it out of the others (lib.rs would
  // otherwise silently pick Casual first).
  const setApps = (tone: TonePreset, apps: string[]) => {
    const names = apps.map((a) => a.toLowerCase());
    const next = Object.fromEntries(
      Object.entries(settings.presetApps).map(([t, list]) => [t, list.filter((a) => !names.includes(a.toLowerCase()))]),
    ) as typeof settings.presetApps;
    onUpdateSettings({ presetApps: { ...next, [tone]: apps } });
  };

  return (
    <div id="screen-tone" className="flex-1 flex flex-col h-full overflow-y-auto">
      <div className="px-8 pt-7 pb-10 max-w-2xl w-full mx-auto space-y-7">
        <div>
          <h1 className="text-[22px] font-semibold tracking-tight text-white/95">Tone</h1>
          <p className="text-[12.5px] text-white/40 mt-1.5 leading-relaxed max-w-md">
            Click a mode and Ivy uses it right away. Apps you add below always get their own mode.
          </p>
        </div>

        <div className="space-y-2">
          {TONES.map((tone) => (
            <button
              key={tone.id}
              id={`tone-${tone.id.toLowerCase()}`}
              onClick={() => use(tone.id)}
              className="w-full text-left px-4 py-3.5 rounded-2xl transition-colors duration-150"
              style={cardStyle(inUse === tone.id)}
            >
              <div className="flex items-center justify-between gap-3">
                <span className={`text-[13.5px] font-medium ${inUse === tone.id ? 'text-white' : 'text-white/70'}`}>
                  {tone.id}
                </span>
                {inUse === tone.id && <InUse />}
              </div>
              <p className="text-[12px] text-white/40 mt-1 leading-relaxed">{tone.blurb}</p>
            </button>
          ))}
        </div>

        {shown && (
          <div>
            <div className="text-[11px] uppercase tracking-wider text-white/30 mb-2.5">{shown.id} sounds like</div>
            <p className="text-[13px] text-white/70 italic leading-relaxed pl-3 border-l-2 border-white/[0.12] whitespace-pre-line">
              “{shown.sample}”
            </p>
          </div>
        )}

        <div className="space-y-4">
          <div>
            <div className="text-[13px] font-medium text-white/85">Add your apps</div>
            <p className="text-[11.5px] text-white/30 mt-1">
              {IS_MAC || IS_LINUX
                ? 'Open the app, then click "Add app" and pick it from the apps open right now. That app then always uses that mode.'
                : `Click "Add app" and pick the program's .exe (for example brave.exe, usually in C:\\Program Files). That program then always uses that mode.`}
              {desktop?.wayland &&
                ' On this desktop (Wayland) Ivy can tell apart only the apps it lists there; every other app gets the mode clicked above.'}
            </p>
          </div>
          {TONES.map((tone) => (
            <AppList
              key={tone.id}
              tone={tone.id}
              apps={settings.presetApps[tone.id] || []}
              onChange={(apps) => setApps(tone.id, apps)}
            />
          ))}
        </div>
      </div>
    </div>
  );
};
