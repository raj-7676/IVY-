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
    if (!settings.personalDictionary.includes(trimmed)) {
      onUpdateSettings({ personalDictionary: [...settings.personalDictionary, trimmed] });
    }
    setNewWord('');
  };

  const removeWord = (word: string) => {
    onUpdateSettings({
      personalDictionary: settings.personalDictionary.filter((w) => w !== word),
    });
  };

  return (
    <div id="screen-dictionary" className="flex-1 flex flex-col h-full overflow-y-auto">
      <div className="px-8 pt-7 pb-10 max-w-2xl space-y-7">
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
      </div>
    </div>
  );
};
