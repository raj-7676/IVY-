import React from 'react';
import { Check } from 'lucide-react';
import { IS_MAC } from '../utils/platform';

export type OnboardingStage = 1 | 2 | 3 | 4 | 5 | 6 | 7;

interface StageIndicatorProps {
  currentStage: OnboardingStage;
  completedStages: Set<OnboardingStage>;
  onSelectStage?: (stage: OnboardingStage) => void;
}

export const StageIndicator: React.FC<StageIndicatorProps> = React.memo(({
  currentStage,
  completedStages,
  onSelectStage,
}) => {
  const steps: { stage: OnboardingStage; label: string }[] = [
    { stage: 1, label: '1. Shortcut' },
    { stage: 2, label: '2. No text box' },
    { stage: 3, label: IS_MAC ? '3. Permissions' : '3. GPU or CPU' },
    { stage: 4, label: '4. Touch Up' },
    { stage: 5, label: '5. Voice test' },
    { stage: 6, label: '6. Self-correction' },
    { stage: 7, label: '7. Privacy' },
  ];

  return (
    <nav
      id="stage-indicator-pill"
      aria-label="Onboarding Progress"
      className="inline-flex items-center gap-1 sm:gap-2 px-3 py-1.5 rounded-full bg-[#120D1A]/90 border border-white/[0.1] backdrop-blur-xl shadow-[0_8px_30px_rgba(0,0,0,0.6)]"
      style={{
        boxShadow:
          '0 8px 30px rgba(0,0,0,0.6), inset 0 1px 1.5px 0 rgba(255,255,255,0.2), 0 0 20px rgba(255,107,0,0.1)',
      }}
    >
      {steps.map((step, idx) => {
        const isActive = currentStage === step.stage;
        const isCompleted = completedStages.has(step.stage);

        return (
          <div key={step.stage} className="flex items-center">
            <button
              id={`stage-indicator-btn-${step.stage}`}
              onClick={() => onSelectStage && onSelectStage(step.stage)}
              type="button"
              className={`flex items-center gap-2 px-3 py-1 rounded-full text-xs sm:text-[13px] font-medium transition-all duration-200 cursor-pointer ${
                isActive
                  ? 'bg-gradient-to-r from-[#FF6B00] to-[#E05300] text-white shadow-[0_0_18px_rgba(255,107,0,0.65),inset_0_1px_1px_rgba(255,255,255,0.4)] border border-[#FFA133]/60'
                  : isCompleted
                  ? 'bg-[#FF6B00]/15 text-[#FFA133] hover:text-white border border-[#FF6B00]/30 shadow-[0_0_10px_rgba(255,107,0,0.2)]'
                  : 'text-white/40 hover:text-white/80 hover:bg-white/[0.04] border border-transparent'
              }`}
            >
              <span
                className={`w-4 h-4 rounded-full flex items-center justify-center text-[10px] transition-transform ${
                  isActive
                    ? 'bg-white text-[#FF6B00] font-extrabold shadow-sm'
                    : isCompleted
                    ? 'bg-[#FF6B00] text-white font-bold'
                    : 'border border-white/20 text-white/40'
                }`}
              >
                {isCompleted && !isActive ? (
                  <Check className="w-2.5 h-2.5 stroke-[3]" />
                ) : (
                  step.stage
                )}
              </span>
              <span className="whitespace-nowrap">{step.label}</span>
            </button>

            {idx < steps.length - 1 && (
              <span className="text-white/20 mx-1 sm:mx-1.5 select-none text-xs">/</span>
            )}
          </div>
        );
      })}
    </nav>
  );
});
