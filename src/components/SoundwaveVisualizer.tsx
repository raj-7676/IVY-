import React, { useEffect, useRef } from 'react';
import { motion } from 'motion/react';
import { MicrophoneAnalyzer } from '../services/audioFeedback';

export type DictationVisualState = 'waiting' | 'listening' | 'transcribing' | 'success' | 'failed';

interface SoundwaveVisualizerProps {
  state: DictationVisualState;
  levels?: number[];
  audioActive?: boolean;
  analyzer?: MicrophoneAnalyzer | null;
}

export const SoundwaveVisualizer: React.FC<SoundwaveVisualizerProps> = React.memo(({
  state,
  levels = [],
  audioActive = false,
  analyzer,
}) => {
  // 9 default bar heights when idle or waiting
  const defaultBars = [0.25, 0.45, 0.7, 0.95, 1.0, 0.85, 0.65, 0.4, 0.2];
  const barHeights = levels.length > 0 ? levels : defaultBars;
  const barElementsRef = useRef<(HTMLDivElement | null)[]>([]);

  // When analyzer is provided, update bar styles directly via DOM refs.
  // This bypasses React reconciliation completely during 60-144fps microphone recording.
  useEffect(() => {
    if (state !== 'listening' || !analyzer) return;

    const unsubscribe = analyzer.subscribe((avg, bars) => {
      const active = avg > 0.04;
      const elements = barElementsRef.current;
      for (let idx = 0; idx < 9; idx++) {
        const el = elements[idx];
        if (!el) continue;
        const val = bars[idx] ?? 0.2;
        const heightMultiplier = active ? Math.max(0.2, val) : 0.18;
        const barHeight = Math.round(heightMultiplier * 58);
        el.style.height = `${barHeight}px`;
        el.style.backgroundColor = '#FF7A00';
        el.style.boxShadow = '0 0 14px rgba(255, 122, 0, 0.85), 0 0 4px rgba(255, 255, 255, 0.5)';
      }
    });

    return () => {
      unsubscribe();
      const elements = barElementsRef.current;
      for (let idx = 0; idx < 9; idx++) {
        const el = elements[idx];
        if (!el) continue;
        el.style.height = '8px';
        el.style.backgroundColor = 'rgba(255, 107, 0, 0.22)';
        el.style.boxShadow = '0 0 0px transparent';
      }
    };
  }, [state, analyzer]);

  if (state === 'transcribing') {
    return (
      <div className="relative flex items-center justify-center h-28 w-28 mx-auto my-2">
        {/* Pulsing outer ambient aura */}
        <motion.div
          animate={{
            scale: [1, 1.25, 1],
            opacity: [0.35, 0.75, 0.35],
          }}
          transition={{
            duration: 1.6,
            repeat: Infinity,
            ease: 'easeInOut',
          }}
          className="absolute inset-0 rounded-full bg-gradient-to-tr from-[#FF6B00] via-[#FFA133] to-amber-500 blur-xl pointer-events-none"
        />

        {/* Shimmer orb spinning ring */}
        <motion.div
          animate={{ rotate: 360 }}
          transition={{
            duration: 2.0,
            repeat: Infinity,
            ease: 'linear',
          }}
          className="relative w-20 h-20 rounded-full border-2 border-transparent border-t-[#FF8A00] border-r-[#FFA133] border-b-amber-400 p-1"
        >
          <div className="w-full h-full rounded-full bg-[#120D1A] flex items-center justify-center border border-[rgba(255,107,0,0.35)] shadow-inner">
            <motion.div
              animate={{
                scale: [0.85, 1.12, 0.85],
                opacity: [0.75, 1, 0.75],
              }}
              transition={{
                duration: 1.2,
                repeat: Infinity,
                ease: 'easeInOut',
              }}
              className="w-8 h-8 rounded-full bg-gradient-to-br from-[#FF6B00] to-amber-400 shadow-[0_0_24px_#FF6B00]"
            />
          </div>
        </motion.div>
      </div>
    );
  }

  return (
    <div className="flex items-center justify-center gap-1.5 h-20 px-6 py-2 mx-auto">
      {barHeights.map((val, idx) => {
        const isListening = state === 'listening';
        const heightMultiplier = isListening
          ? audioActive
            ? Math.max(0.2, val)
            : 0.35 + Math.sin(idx * 0.8 + Date.now() * 0.005) * 0.25
          : 0.18;
        const barHeight = Math.round(heightMultiplier * 58);

        return (
          <div
            key={idx}
            ref={(el) => {
              barElementsRef.current[idx] = el;
            }}
            className="w-2 rounded-full origin-center"
            style={{
              height: isListening ? `${barHeight}px` : '8px',
              backgroundColor: isListening ? '#FF7A00' : 'rgba(255, 107, 0, 0.22)',
              boxShadow: isListening
                ? '0 0 14px rgba(255, 122, 0, 0.85), 0 0 4px rgba(255, 255, 255, 0.5)'
                : '0 0 0px transparent',
              transition: 'height 60ms ease-out, background-color 150ms ease, box-shadow 150ms ease',
              minHeight: '6px',
            }}
          />
        );
      })}
    </div>
  );
});
