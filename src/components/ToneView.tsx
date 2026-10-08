import React from 'react';
import { Plus, X } from 'lucide-react';
import { SettingsConfig, TonePreset } from '../types';

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
            {app}
            <button
              onClick={() => onChange(apps.filter((a) => a !== app))}
              className="text-white/30 hover:text-red-400 transition-colors duration-150"
              title={`Remove ${app}`}
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </span>
        ))}
        <label
          className="inline-flex items-center gap-1.5 cursor-pointer border border-dashed border-white/[0.16] hover:border-white/[0.3] rounded-full px-3.5 py-1.5 text-[12px] text-white/70 transition-colors duration-150"
          title="Pick the app's .exe file"
        >
          <Plus className="w-3.5 h-3.5" style={{ color: `rgb(${ACCENT_RGB})` }} />
          Add app
          <input type="file" accept=".exe" onChange={pick} className="hidden" />
        </label>
      </div>
    </div>
  );
};

export const ToneView: React.FC<ToneViewProps> = ({ settings, onUpdateSettings }) => {
  const inUse = settings.activeTonePreset;
  const shown = TONES.find((t) => t.id === inUse);
  // Three modes (Yash, 2026-10-06): a click takes effect at once for every app not added below.
  const use = (tone: TonePreset) => {
    if (inUse !== tone) onUpdateSettings({ activeTonePreset: tone });
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
              Click "Add app" and pick the program's .exe (for example brave.exe, usually in
              C:\Program Files). That program then always uses that mode.
            </p>
          </div>
          {TONES.map((tone) => (
            <AppList
              key={tone.id}
              tone={tone.id}
              apps={settings.presetApps[tone.id] || []}
              onChange={(apps) => onUpdateSettings({ presetApps: { ...settings.presetApps, [tone.id]: apps } })}
            />
          ))}
        </div>
      </div>
    </div>
  );
};
