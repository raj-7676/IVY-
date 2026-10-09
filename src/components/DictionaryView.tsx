import React, { useState } from 'react';
import { X } from 'lucide-react';
import { SettingsConfig } from '../types';

interface DictionaryViewProps {
  settings: SettingsConfig;
  onUpdateSettings: (newSettings: Partial<SettingsConfig>) => void;
}

const ACCENT_RGB = '255, 107, 0';

export const DictionaryView: React.FC<DictionaryViewProps> = ({ settings, onUpdateSettings }) => {
  const [newWord, setNewWord] = useState('');

  const addWord = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = newWord.trim();
    if (!trimmed) return;
    if (!settings.personalDictionary.some((w) => w.toLowerCase() === trimmed.toLowerCase())) {
      onUpdateSettings({ personalDictionary: [...settings.personalDictionary, trimmed] });
    }
    setNewWord('');
  };

  const [trigger, setTrigger] = useState('');
  const [snippetText, setSnippetText] = useState('');
  const snippets = settings.snippets ?? [];
  const addSnippet = (e: React.FormEvent) => {
    e.preventDefault();
    const t = trigger.trim();
    const body = snippetText.trim();
    if (!t || !body) return;
    const others = snippets.filter((s) => s.trigger.toLowerCase() !== t.toLowerCase());
    onUpdateSettings({ snippets: [...others, { trigger: t, text: body }] });
    setTrigger('');
    setSnippetText('');
  };

  const removeWord = (word: string) => {
    onUpdateSettings({
      personalDictionary: settings.personalDictionary.filter((w) => w !== word),
    });
  };

  return (
    <div id="screen-dictionary" className="flex-1 flex flex-col h-full overflow-y-auto">
      <div className="px-8 pt-7 pb-10 max-w-2xl w-full mx-auto space-y-7">
        <div>
          <h1 className="text-[22px] font-semibold tracking-tight text-white/95">Dictionary</h1>
          <p className="text-[12.5px] text-white/40 mt-1.5 leading-relaxed max-w-md">
            Names, jargon and spellings Ivy keeps getting wrong. Teach each one once and it stops
            guessing.
          </p>
        </div>

        <form onSubmit={addWord} className="flex gap-2 max-w-md">
          <input
            id="dictionary-input"
            type="text"
            value={newWord}
            onChange={(e) => setNewWord(e.target.value)}
            maxLength={100}
            placeholder="Add a word or phrase"
            className="flex-1 bg-white/[0.04] border border-white/[0.08] rounded-xl px-3.5 py-2.5 text-[13px] text-white/90 focus:outline-none focus:border-white/[0.18] transition-colors duration-150"
          />
          <button
            id="dictionary-add-btn"
            type="submit"
            disabled={!newWord.trim()}
            className="px-4 py-2.5 rounded-xl text-[12.5px] font-semibold text-white disabled:opacity-30 transition-opacity duration-150"
            style={{ backgroundColor: `rgb(${ACCENT_RGB})` }}
          >
            Add
          </button>
        </form>

        {settings.personalDictionary.length === 0 ? (
          <p className="text-[12.5px] text-white/35">Nothing taught yet.</p>
        ) : (
          <div>
            <div className="text-[11px] uppercase tracking-wider text-white/30 mb-3">
              {settings.personalDictionary.length} term
              {settings.personalDictionary.length === 1 ? '' : 's'}
            </div>
            <div className="flex flex-wrap gap-2">
              {settings.personalDictionary.map((word) => (
                <span
                  key={word}
                  className="inline-flex items-center gap-2 pl-3.5 pr-2 py-1.5 rounded-full font-mono text-[12px] text-white/85"
                  style={{
                    backgroundColor: 'rgba(255,255,255,0.05)',
                    border: '1px solid rgba(255,255,255,0.09)',
                  }}
                >
                  {word}
                  <button
                    onClick={() => removeWord(word)}
                    className="text-white/30 hover:text-red-400 transition-colors duration-150"
                    title={`Remove ${word}`}
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                </span>
              ))}
            </div>
          </div>
        )}

        <div className="pt-6 border-t border-white/[0.06] space-y-4">
          <div>
            <h2 className="text-[16px] font-semibold tracking-tight text-white/90">Snippets</h2>
            <p className="text-[12.5px] text-white/40 mt-1.5 leading-relaxed max-w-md">
              Say a trigger phrase anywhere in a sentence, and Ivy types the full text you saved in its place.
              "Send it to my email address" becomes "Send it to you@example.com".
            </p>
            <p className="text-[12px] text-white/30 mt-1 leading-relaxed max-w-md">
              Pick a phrase you'd never say by accident. Ctrl + Z undoes a paste if one slips through.
            </p>
          </div>
          <form onSubmit={addSnippet} className="flex flex-col gap-2 max-w-md">
            <input
              type="text"
              value={trigger}
              maxLength={80}
              onChange={(e) => setTrigger(e.target.value)}
              placeholder='When I say… (e.g. "my email address")'
              className="bg-white/[0.04] border border-white/[0.08] rounded-xl px-3.5 py-2.5 text-[13px] text-white/90 focus:outline-none focus:border-white/[0.18]"
            />
            <textarea
              value={snippetText}
              maxLength={5000}
              rows={3}
              onChange={(e) => setSnippetText(e.target.value)}
              placeholder="…type this (e.g. you@example.com)"
              className="bg-white/[0.04] border border-white/[0.08] rounded-xl px-3.5 py-2.5 text-[13px] text-white/90 focus:outline-none focus:border-white/[0.18] resize-none"
            />
            <button
              type="submit"
              disabled={!trigger.trim() || !snippetText.trim()}
              className="self-start px-4 py-2.5 rounded-xl text-[12.5px] font-semibold text-white disabled:opacity-30 transition-opacity"
              style={{ backgroundColor: `rgb(${ACCENT_RGB})` }}
            >
              Save snippet
            </button>
          </form>
          {snippets.length > 0 && (
            <div className="flex flex-col gap-2">
              {snippets.map((s) => (
                <div
                  key={s.trigger}
                  className="flex items-start justify-between gap-3 px-3.5 py-2.5 rounded-xl"
                  style={{ backgroundColor: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)' }}
                >
                  <div className="min-w-0">
                    <div className="text-[12.5px] font-medium text-white/90">"{s.trigger}"</div>
                    <div className="text-[12px] text-white/50 whitespace-pre-wrap break-words">{s.text}</div>
                  </div>
                  <button
                    onClick={() => onUpdateSettings({ snippets: snippets.filter((x) => x.trigger !== s.trigger) })}
                    className="text-white/30 hover:text-red-400 transition-colors shrink-0 mt-0.5"
                    title={`Remove "${s.trigger}"`}
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
