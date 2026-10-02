import React from 'react';
import { DictationMode, HardwareMode } from '../types';

interface ModeCombo {
  title: string;
  points: string[];
}

// Mirrors `clean_transcript` in src-tauri/src/cleanup.rs. Change both together.
const COMBOS: Record<DictationMode, Record<HardwareMode, ModeCombo>> = {
  speed: {
    gpu: {
      title: 'Speed + GPU',
      points: [
        'Speech recognition runs on your graphics card.',
        'Rulebooks only: fillers, spoken commands, numbers as digits, names and capitals. A word you said is never swapped for another.',
        'No AI, no extra wait.',
        'Corrections like "scratch that" are pasted exactly as spoken.',
      ],
    },
    cpu: {
      title: 'Speed + CPU',
      points: [
        'Speech recognition runs on your processor. No graphics memory used.',
        'Same rulebooks as Speed + GPU.',
        'No AI, no extra wait.',
        'Corrections like "scratch that" are pasted exactly as spoken.',
      ],
    },
  },
  accuracy: {
    gpu: {
      title: 'Accuracy + GPU',
      points: [
        'Speech recognition runs on your graphics card.',
        'Qwen AI reads every dictation: fixes punctuation, removes fillers, and drops what you took back when you correct yourself.',
        'The formatting rulebooks (numbers, links, code, names) run after the AI.',
        'If the AI adds a word or number you never said, answers instead of transcribing, drops a sentence, or runs out of time, Ivy uses the rulebooks alone instead.',
        'Adds a little time per dictation, capped at about 6 seconds.',
        'On battery, or when a game pushes the GPU past your eviction threshold, it behaves like Accuracy + CPU.',
      ],
    },
    cpu: {
      title: 'Accuracy + CPU',
      points: [
        'Speech recognition runs on your processor. No graphics memory used.',
        'All rulebooks: fillers, spoken commands, numbers as digits, dates, links, emails, file names, code casing and more.',
        'No live AI, no extra wait.',
        'Corrections like "scratch that" are pasted exactly as spoken.',
      ],
    },
  },
};

export const modeCombo = (dictationMode: DictationMode, hardwareMode: HardwareMode): ModeCombo =>
  COMBOS[dictationMode][hardwareMode];

export const ModeMatrix: React.FC<{ dictationMode: DictationMode; hardwareMode: HardwareMode }> = ({
  dictationMode,
  hardwareMode,
}) => (
  <div className="flex flex-col gap-2.5 text-left">
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
      {(['speed', 'accuracy'] as DictationMode[]).flatMap((d) =>
        (['gpu', 'cpu'] as HardwareMode[]).map((h) => {
          const active = d === dictationMode && h === hardwareMode;
          const combo = COMBOS[d][h];
          return (
            <div
              key={`${d}-${h}`}
              className={`rounded-xl p-3 border flex flex-col gap-1.5 transition-colors ${
                active ? 'bg-[#FF6B00]/10 border-[#FF6B00]/60' : 'bg-white/[0.02] border-white/[0.08]'
              }`}
            >
              <div className="flex items-center justify-between gap-2">
                <span className={`text-[11.5px] font-bold ${active ? 'text-[#FFA133]' : 'text-white/80'}`}>
                  {combo.title}
                </span>
                {active && (
                  <span className="px-1.5 py-0.5 rounded-full text-[9.5px] font-bold uppercase tracking-wider bg-[#FF6B00]/20 text-[#FFA133] border border-[#FF6B00]/30">
                    Your setup
                  </span>
                )}
              </div>
              <ul className="flex flex-col gap-1">
                {combo.points.map((point) => (
                  <li key={point} className={`text-[11px] leading-snug ${active ? 'text-white/75' : 'text-white/45'}`}>
                    • {point}
                  </li>
                ))}
              </ul>
            </div>
          );
        })
      )}
    </div>
    <p className="text-[10.5px] text-white/40 leading-relaxed">
      Touch Up (offered after a paste) works in Accuracy mode on GPU and CPU. Summarize in History works in every mode.
    </p>
  </div>
);
