import React from 'react';
import { DictationMode, HardwareMode } from '../types';

interface ModeCombo {
  title: string;
  points: string[];
}

// Mode matrix describing runtime behavior for each mode combination.
const COMBOS: Record<DictationMode, Record<HardwareMode, ModeCombo>> = {
  speed: {
    gpu: {
      title: 'Speed + GPU',
      points: [
        'Speech recognition runs on your graphics card.',
        'Core rulebooks only: fillers, spoken commands, numbers as digits, names and capitals.',
        'Fast response, minimal latency.',
        'Self-corrections are pasted as spoken.',
      ],
    },
    cpu: {
      title: 'Speed + CPU',
      points: [
        'Speech recognition runs on your processor. No graphics memory used.',
        'Same core rulebooks as Speed + GPU.',
        'Fast response, minimal latency.',
        'Self-corrections are pasted as spoken.',
      ],
    },
  },
  accuracy: {
    gpu: {
      title: 'Accuracy + GPU',
      points: [
        'Voxtral multimodal AI runs on your graphics card (Vulkan).',
        'Directly hears speech and rewrites it: fixes punctuation, strips fillers, and resolves self-corrections in ~1s.',
        'Formatting rulebooks (numbers as digits, links, code, names) run after transcription.',
        'On battery or high gaming VRAM usage, falls back to CPU automatically.',
      ],
    },
    cpu: {
      title: 'Accuracy + CPU',
      points: [
        'Voxtral multimodal AI runs on your processor. No graphics memory used.',
        'Full self-correction resolution and formatting, identical to GPU accuracy.',
        'Runs reliably on CPU with no GPU required, taking a few extra seconds.',
        'All rulebooks run after transcription.',
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
