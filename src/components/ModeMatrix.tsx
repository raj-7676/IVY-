import React from 'react';
import { HardwareMode } from '../types';

interface ModeCombo {
  title: string;
  points: string[];
}

// GPU vs CPU for Ivy's one model (lite: Qwen3-ASR-1.7B, fine-tuned). Times are measured
// (IVY.md section 23.7 and the lab's 60-second paragraph timing), not estimates.
const COMBOS: Record<HardwareMode, ModeCombo> = {
  gpu: {
    title: 'GPU',
    points: [
      'Runs on your graphics card (NVIDIA, AMD or Intel, through Vulkan).',
      'A 1-minute dictation is ready in about 2 seconds; a short one in a fraction of a second.',
      'On battery, or when other apps keep the GPU busy, Ivy switches to CPU by itself. During games and full-screen video it steps aside.',
    ],
  },
  cpu: {
    title: 'CPU',
    points: [
      'Runs on your processor. Works on any PC, no graphics memory used.',
      'A short dictation takes 2 to 5 seconds; a 1-minute one about 10 to 20 seconds.',
      'Same model and same results as GPU, just slower.',
    ],
  },
};

export const modeCombo = (hardwareMode: HardwareMode): ModeCombo => COMBOS[hardwareMode];

export const ModeMatrix: React.FC<{ hardwareMode: HardwareMode }> = ({ hardwareMode }) => (
  <div className="flex flex-col gap-2.5 text-left">
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
      {(['gpu', 'cpu'] as HardwareMode[]).map((h) => {
        const active = h === hardwareMode;
        const combo = COMBOS[h];
        return (
          <div
            key={h}
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
      })}
    </div>
    <p className="text-[10.5px] text-white/40 leading-relaxed">
      Both apply your self-corrections ("no wait", "sorry, I mean") and your tone. Touch Up (offered after a paste) fixes misspelled words.
    </p>
  </div>
);
