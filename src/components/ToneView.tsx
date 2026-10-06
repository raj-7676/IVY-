import React, { useState } from 'react';
import { Plus, X } from 'lucide-react';
import { SettingsConfig, TonePreset } from '../types';

interface ToneViewProps {
  settings: SettingsConfig;
  onUpdateSettings: (newSettings: Partial<SettingsConfig>) => void;
}

const ACCENT_RGB = '255, 107, 0';

const TONES: { id: TonePreset; blurb: string; sample: string }[] = [
  {
    id: 'Casual',
    blurb: 'Your words exactly as you said them: slang, contractions and all.',
    sample: "Yeah, I'm gonna push that fix before standup, thanks!",
  },
  {
    id: 'Standard',
    blurb: 'Slang written out, filler "like" and "you know" dropped. Your voice, tidied.',
    sample: "Yeah, I'm going to push that fix before standup, thanks!",
  },
  {
    id: 'Professional',
    blurb: 'No contractions, no slang or chat words, no exclamation marks.',
    sample: 'Yes, I am going to push that fix before standup, thank you.',
  },
];

export const ToneView: React.FC<ToneViewProps> = ({ settings, onUpdateSettings }) => {
  const [selected, setSelected] = useState<TonePreset>(settings.activeTonePreset);
  const [newApp, setNewApp] = useState('');

  const apps = settings.presetApps[selected] || [];

  const addApp = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = newApp.trim();
    if (!trimmed) return;
    if (!apps.includes(trimmed)) {
      onUpdateSettings({
        presetApps: { ...settings.presetApps, [selected]: [...apps, trimmed] },
      });
    }
    setNewApp('');
  };

  const removeApp = (app: string) => {
    onUpdateSettings({
      presetApps: { ...settings.presetApps, [selected]: apps.filter((a) => a !== app) },
    });
  };

  const active = TONES.find((t) => t.id === selected)!;

  return (
    <div id="screen-tone" className="flex-1 flex flex-col h-full overflow-y-auto">
      <div className="px-8 pt-7 pb-10 max-w-2xl space-y-7">
        <div>
          <h1 className="text-[22px] font-semibold tracking-tight text-white/95">Tone</h1>
          <p className="text-[12.5px] text-white/40 mt-1.5 leading-relaxed max-w-md">
            Ivy writes differently depending on where you are. Whichever app has focus when you
            let go of the key picks the tone.
          </p>
        </div>

        {/* Tone picker */}
        <div className="space-y-2">
          {TONES.map((tone) => {
            const isSelected = selected === tone.id;
            return (
              <button
                key={tone.id}
                id={`tone-${tone.id.toLowerCase()}`}
                onClick={() => setSelected(tone.id)}
                className="w-full text-left px-4 py-3.5 rounded-2xl transition-colors duration-150"
                style={
                  isSelected
                    ? {
                        backgroundColor: `rgba(${ACCENT_RGB}, 0.1)`,
                        border: `1px solid rgba(${ACCENT_RGB}, 0.3)`,
                      }
                    : {
                        backgroundColor: 'rgba(255,255,255,0.03)',
                        border: '1px solid rgba(255,255,255,0.07)',
                      }
                }
              >
                <div className="flex items-center justify-between gap-3">
                  <span
                    className={`text-[13.5px] font-medium ${
                      isSelected ? 'text-white' : 'text-white/70'
                    }`}
                  >
                    {tone.id}
                  </span>
                  {settings.activeTonePreset === tone.id && (
                    <span
                      className="text-[10px] px-2 py-0.5 rounded-full font-medium"
                      style={{
                        backgroundColor: `rgba(${ACCENT_RGB}, 0.16)`,
                        color: `rgb(${ACCENT_RGB})`,
                      }}
                    >
                      Default
                    </span>
                  )}
                </div>
                <p className="text-[12px] text-white/40 mt-1 leading-relaxed">{tone.blurb}</p>
              </button>
            );
          })}
        </div>

        {/* Sample */}
        <div>
          <div className="text-[11px] uppercase tracking-wider text-white/30 mb-2.5">
            {active.id} sounds like
          </div>
          <p className="text-[13px] text-white/70 italic leading-relaxed pl-3 border-l-2 border-white/[0.12]">
            “{active.sample}”
          </p>
        </div>

        {/* App routing */}
        <div className="space-y-3">
          <div>
            <div className="text-[13px] font-medium text-white/85">
              Apps that use {active.id}
            </div>
            {selected === 'Standard' && (
              <p className="text-[11.5px] text-white/30 mt-1">
                Also the fallback for anything not listed under another tone.
              </p>
            )}
          </div>

          <div className="flex flex-wrap gap-2 items-center">
            {apps.map((app) => (
              <span
                key={app}
                className="inline-flex items-center gap-2 pl-3.5 pr-2 py-1.5 rounded-full text-[12px] text-white/85"
                style={{
                  backgroundColor: 'rgba(255,255,255,0.05)',
                  border: '1px solid rgba(255,255,255,0.09)',
                }}
              >
                {app}
                <button
                  onClick={() => removeApp(app)}
                  className="text-white/30 hover:text-red-400 transition-colors duration-150"
                  title={`Remove ${app}`}
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </span>
            ))}

            <form onSubmit={addApp} className="relative flex items-center">
              <input
                type="text"
                value={newApp}
                onChange={(e) => setNewApp(e.target.value)}
                placeholder="Add an app"
                className="bg-transparent border border-dashed border-white/[0.16] rounded-full px-3.5 py-1.5 pr-8 text-[12px] text-white/85 focus:outline-none focus:border-white/[0.3] transition-colors duration-150 w-36"
              />
              {newApp.trim() && (
                <button type="submit" className="absolute right-2.5" title="Add">
                  <Plus className="w-3.5 h-3.5" style={{ color: `rgb(${ACCENT_RGB})` }} />
                </button>
              )}
            </form>
          </div>
        </div>

        <button
          onClick={() => onUpdateSettings({ activeTonePreset: selected })}
          disabled={settings.activeTonePreset === selected}
          className="px-4 py-2 rounded-xl text-[12.5px] font-medium text-white/85 bg-white/[0.06] hover:bg-white/[0.11] disabled:opacity-30 transition-colors duration-150"
        >
          {settings.activeTonePreset === selected
            ? `${selected} is the default`
            : `Make ${selected} the default`}
        </button>
      </div>
    </div>
  );
};
