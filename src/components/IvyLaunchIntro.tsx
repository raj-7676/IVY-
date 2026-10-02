import React, { useEffect, useState, useMemo, useRef } from 'react';
import {
  motion,
  Variants,
  useMotionValue,
  useSpring,
  useTransform,
} from 'motion/react';
import { invoke } from '@tauri-apps/api/core';
import { X } from 'lucide-react';
import { soundEngine } from '../utils/audio';
import { AtmosphericDust } from './AtmosphericDust';

interface IvyLaunchIntroProps {
  onComplete?: () => void;
}

interface ShardDefinition {
  id: string;
  points: string;
  fill: string;
  stroke?: string;
  // Origami unfold parameters relative to final slot
  origamiOriginX: number;
  origamiOriginY: number;
  origamiRotate: number;
  origamiScale: number;
  delaySec: number;
  unfoldDurationSec: number;
}

// ─── 1. THE CENTRAL 'V' LOGO (Center X: 400, Width: 180, Span: 310 to 490) ───
// Awakes first at the dead center of the 800-wide viewBox.
const V_LOGO_SHARDS: ShardDefinition[] = [
  // Glowing Keystone Apex
  {
    id: 'v-apex-wedge',
    points: '360,156 400,152 440,156 400,200',
    fill: '#FF5500',
    stroke: '#FFAA66',
    origamiOriginX: 0,
    origamiOriginY: 45,
    origamiRotate: 180,
    origamiScale: 0.2,
    delaySec: 0.15,
    unfoldDurationSec: 0.85,
  },
  // Lower Left Fold
  {
    id: 'v-left-lower',
    points: '332,112 366,112 388,156 360,156',
    fill: '#F1F5F9',
    origamiOriginX: 20,
    origamiOriginY: 30,
    origamiRotate: -45,
    origamiScale: 0.4,
    delaySec: 0.35,
    unfoldDurationSec: 0.8,
  },
  // Lower Right Fold
  {
    id: 'v-right-lower',
    points: '440,156 412,156 434,112 468,112',
    fill: '#F1F5F9',
    origamiOriginX: -20,
    origamiOriginY: 30,
    origamiRotate: 45,
    origamiScale: 0.4,
    delaySec: 0.42,
    unfoldDurationSec: 0.8,
  },
  // Center Facet
  {
    id: 'v-center-facet',
    points: '345,50 376,96 400,132 424,96 455,50 400,75',
    fill: '#FF7722',
    origamiOriginX: 0,
    origamiOriginY: -40,
    origamiRotate: 90,
    origamiScale: 0.3,
    delaySec: 0.55,
    unfoldDurationSec: 0.8,
  },
  // Upper Left Wing
  {
    id: 'v-left-upper',
    points: '310,50 345,50 366,112 332,112',
    fill: '#FFFFFF',
    origamiOriginX: 45,
    origamiOriginY: -25,
    origamiRotate: -60,
    origamiScale: 0.5,
    delaySec: 0.65,
    unfoldDurationSec: 0.8,
  },
  // Upper Right Wing
  {
    id: 'v-right-upper',
    points: '434,112 468,112 490,50 455,50',
    fill: '#FFFFFF',
    origamiOriginX: -45,
    origamiOriginY: -25,
    origamiRotate: 60,
    origamiScale: 0.5,
    delaySec: 0.72,
    unfoldDurationSec: 0.8,
  },
];

// ─── 2. LETTER 'I' (Center X: 175, Width: 150, Span: 100 to 250) ───
// Gap between I (ends at 250) and V (starts at 310) = EXACTLY 60px.
// Unfolds laterally to the left from the central 'V' keystone.
const I_LETTER_SHARDS: ShardDefinition[] = [
  {
    id: 'i-top-left',
    points: '100,50 175,50 168,72 100,72',
    fill: '#FFFFFF',
    origamiOriginX: 225,
    origamiOriginY: -10,
    origamiRotate: -35,
    origamiScale: 0.4,
    delaySec: 1.35,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'i-top-right',
    points: '175,50 250,50 250,72 182,72',
    fill: '#E2E8F0',
    origamiOriginX: 200,
    origamiOriginY: -15,
    origamiRotate: 25,
    origamiScale: 0.45,
    delaySec: 1.42,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'i-stem-upper',
    points: '160,72 190,72 182,125 160,125',
    fill: '#F8FAFC',
    origamiOriginX: 215,
    origamiOriginY: 0,
    origamiRotate: -20,
    origamiScale: 0.5,
    delaySec: 1.48,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'i-stem-lower',
    points: '160,125 182,125 190,178 160,178',
    fill: '#FFF1E8',
    origamiOriginX: 215,
    origamiOriginY: 10,
    origamiRotate: 20,
    origamiScale: 0.5,
    delaySec: 1.54,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'i-bottom-left',
    points: '100,178 168,178 175,200 100,200',
    fill: '#FFFFFF',
    origamiOriginX: 225,
    origamiOriginY: 20,
    origamiRotate: -30,
    origamiScale: 0.4,
    delaySec: 1.60,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'i-bottom-right',
    points: '182,178 250,178 250,200 175,200',
    fill: '#E2E8F0',
    origamiOriginX: 200,
    origamiOriginY: 25,
    origamiRotate: 30,
    origamiScale: 0.45,
    delaySec: 1.66,
    unfoldDurationSec: 0.75,
  },
];

// ─── 3. LETTER 'Y' (Center X: 625, Width: 150, Span: 550 to 700) ───
// Gap between V (ends at 490) and Y (starts at 550) = EXACTLY 60px.
// Symmetrical counterpart to 'I' with identical 60px gap and 150px width.
// Unfolds laterally to the right from the central 'V' keystone.
const Y_LETTER_SHARDS: ShardDefinition[] = [
  {
    id: 'y-nexus-diamond',
    points: '590,124 625,102 660,124 625,140',
    fill: '#FF9944',
    origamiOriginX: -225,
    origamiOriginY: 0,
    origamiRotate: 45,
    origamiScale: 0.4,
    delaySec: 1.35,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'y-left-fork-top',
    points: '550,50 580,50 600,92 570,92',
    fill: '#FFFFFF',
    origamiOriginX: -200,
    origamiOriginY: -20,
    origamiRotate: -25,
    origamiScale: 0.45,
    delaySec: 1.42,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'y-left-fork-bottom',
    points: '570,92 600,92 615,124 590,124',
    fill: '#E2E8F0',
    origamiOriginX: -215,
    origamiOriginY: -10,
    origamiRotate: 20,
    origamiScale: 0.5,
    delaySec: 1.48,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'y-right-fork-top',
    points: '670,50 700,50 680,92 650,92',
    fill: '#FFFFFF',
    origamiOriginX: -245,
    origamiOriginY: -25,
    origamiRotate: 35,
    origamiScale: 0.4,
    delaySec: 1.54,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'y-right-fork-bottom',
    points: '650,92 680,92 660,124 635,124',
    fill: '#E2E8F0',
    origamiOriginX: -235,
    origamiOriginY: -10,
    origamiRotate: -20,
    origamiScale: 0.5,
    delaySec: 1.60,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'y-stem-upper',
    points: '610,136 640,136 640,170 610,170',
    fill: '#F8FAFC',
    origamiOriginX: -225,
    origamiOriginY: 15,
    origamiRotate: -15,
    origamiScale: 0.5,
    delaySec: 1.66,
    unfoldDurationSec: 0.75,
  },
  {
    id: 'y-stem-anchor',
    points: '610,170 640,170 650,200 600,200',
    fill: '#FFFFFF',
    origamiOriginX: -225,
    origamiOriginY: 30,
    origamiRotate: 25,
    origamiScale: 0.45,
    delaySec: 1.72,
    unfoldDurationSec: 0.75,
  },
];

export const IvyLaunchIntro: React.FC<IvyLaunchIntroProps> = ({ onComplete }) => {
  const [animationStarted, setAnimationStarted] = useState(false);
  const [isExiting, setIsExiting] = useState(false);
  const [isIdle, setIsIdle] = useState(false);
  const durationSec = 3.5;

  const onCompleteRef = useRef(onComplete);
  useEffect(() => {
    onCompleteRef.current = onComplete;
  }, [onComplete]);

  const triggerDismiss = () => {
    if (isExiting) return;
    setIsExiting(true);
    soundEngine.stopIntro();
    onCompleteRef.current?.();
  };

  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || e.key === ' ' || e.key === 'Enter') {
        triggerDismiss();
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, []);


  // Normalized mouse coordinates from center: -1 to +1
  const mouseX = useMotionValue(0);
  const mouseY = useMotionValue(0);

  // Screen shake completely removed as requested

  // Smooth responsive spring for physical fluidity
  const smoothMouseX = useSpring(mouseX, { stiffness: 90, damping: 22, mass: 0.6 });
  const smoothMouseY = useSpring(mouseY, { stiffness: 90, damping: 22, mass: 0.6 });

  // Idle activation progression spring (0 during animation -> smoothly blends to 1 when idle)
  const idleProgress = useMotionValue(0);
  const smoothIdle = useSpring(idleProgress, { stiffness: 60, damping: 18 });

  useEffect(() => {
    idleProgress.set(isIdle ? 1 : 0);
  }, [isIdle, idleProgress]);

  // Subtle 3D tilt angles (only active during idle)
  const rotateX = useTransform([smoothMouseY, smoothIdle], ([y, idle]: number[]) => {
    return (y as number) * -8 * (idle as number);
  });
  const rotateY = useTransform([smoothMouseX, smoothIdle], ([x, idle]: number[]) => {
    return (x as number) * 11 * (idle as number);
  });

  // Subtle foreground text parallax
  const translateX = useTransform([smoothMouseX, smoothIdle], ([x, idle]: number[]) => {
    return (x as number) * 16 * (idle as number);
  });
  const translateY = useTransform([smoothMouseY, smoothIdle], ([y, idle]: number[]) => {
    return (y as number) * 10 * (idle as number);
  });

  // Background subtle counter-parallax for depth
  const bgTranslateX = useTransform([smoothMouseX, smoothIdle], ([x, idle]: number[]) => {
    return (x as number) * -12 * (idle as number);
  });
  const bgTranslateY = useTransform([smoothMouseY, smoothIdle], ([y, idle]: number[]) => {
    return (y as number) * -8 * (idle as number);
  });

  // Listen to window mouse movement
  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      const { innerWidth, innerHeight } = window;
      if (innerWidth === 0 || innerHeight === 0) return;
      const nx = (e.clientX / innerWidth) * 2 - 1;
      const ny = (e.clientY / innerHeight) * 2 - 1;
      mouseX.set(Math.max(-1, Math.min(1, nx)));
      mouseY.set(Math.max(-1, Math.min(1, ny)));
    };

    const handleMouseLeave = () => {
      mouseX.set(0);
      mouseY.set(0);
    };

    window.addEventListener('mousemove', handleMouseMove, { passive: true });
    window.addEventListener('mouseleave', handleMouseLeave);

    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseleave', handleMouseLeave);
    };
  }, [mouseX, mouseY]);

  useEffect(() => {
    setAnimationStarted(true);

    // Synchronized acoustic sequence mapped to origami unfolding & crystallization
    soundEngine.playIntroSequence();

    // 3. Transition into idle phase at 3.3s once unfold animation finishes
    const idleTimeout = setTimeout(() => {
      setIsIdle(true);
    }, 3300);

    // 4. Smooth cinematic fade-out after the animation settles
    const exitTimeout = setTimeout(() => {
      setIsExiting(true);
    }, 2650);

    const completeTimeout = setTimeout(() => {
      onCompleteRef.current?.();
    }, 3550);

    return () => {
      soundEngine.stopIntro();
      clearTimeout(idleTimeout);
      clearTimeout(exitTimeout);
      clearTimeout(completeTimeout);
    };
  }, []); // Run strictly once on mount

  // Master container fade-in
  const containerVariants: Variants = useMemo(
    () => ({
      initial: { opacity: 0 },
      animate: {
        opacity: 1,
        transition: { duration: 0.4 },
      },
      exit: {
        opacity: 0,
        transition: { duration: 0.9, ease: [0.16, 1, 0.3, 1] },
      },
    }),
    []
  );

  return (
    <motion.div
      id="ivy-launch-intro"
      onClick={triggerDismiss}
      className="fixed inset-0 z-50 flex flex-col items-center justify-center overflow-hidden select-none bg-[#050508] cursor-pointer"
      variants={containerVariants}
      initial="initial"
      animate={animationStarted && !isExiting ? 'animate' : 'exit'}
    >
      {/* Top right quick actions: Skip and Direct Window Close */}
      <div className="absolute top-3 right-4 z-[60] flex items-center gap-2">
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            triggerDismiss();
          }}
          className="px-2.5 py-1 rounded-lg text-xs font-medium text-white/60 hover:text-white hover:bg-white/10 transition-colors duration-150 border border-white/10 flex items-center gap-1.5"
          title="Skip intro (Esc)"
        >
          <span>Skip</span>
          <kbd className="text-[10px] px-1 py-0.5 rounded bg-white/10 text-white/70">Esc</kbd>
        </button>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            invoke('close_main').catch(() => {});
          }}
          className="w-7 h-7 flex items-center justify-center rounded-lg text-white/60 hover:text-white hover:bg-red-500/80 transition-colors duration-150 border border-white/10"
          title="Close Ivy"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>
      {/* Viewport stage (shake completely removed) */}
      <div
        id="ivy-viewport-stage"
        className="relative w-full h-full flex flex-col items-center justify-center overflow-hidden"
      >
        {/* Subtle, atmospheric ambient grid with counter-parallax */}
        <motion.div
        className="absolute inset-0 pointer-events-none opacity-20"
        style={{
          x: bgTranslateX,
          y: bgTranslateY,
          backgroundImage: `
            linear-gradient(to right, rgba(255, 85, 0, 0.08) 1px, transparent 1px),
            linear-gradient(to bottom, rgba(255, 85, 0, 0.08) 1px, transparent 1px)
          `,
          backgroundSize: '48px 48px',
          maskImage: 'radial-gradient(ellipse 65% 55% at 50% 50%, black 20%, transparent 80%)',
          WebkitMaskImage: 'radial-gradient(ellipse 65% 55% at 50% 50%, black 20%, transparent 80%)',
        }}
      />

      {/* ─── CENTRAL HERO: IVY ANIMATION WITH 3D PERSPECTIVE TILT & PARALLAX ─── */}
      <div
        className="relative flex flex-col items-center justify-center w-full max-w-4xl px-4 z-10"
        style={{ perspective: 1200 }}
      >
        <motion.div
          className="relative flex items-center justify-center w-full will-change-transform"
          style={{
            transformStyle: 'preserve-3d',
            rotateX,
            rotateY,
            x: translateX,
            y: translateY,
          }}
        >
          <svg
            viewBox="0 0 800 260"
            className="w-[310px] xs:w-[380px] sm:w-[540px] md:w-[680px] lg:w-[780px] h-auto overflow-visible"
            style={{
              filter:
                'drop-shadow(0 12px 28px rgba(0, 0, 0, 0.75)) drop-shadow(0 0 35px rgba(255, 85, 0, 0.28))',
            }}
          >
            <defs>
              {/* Radial gradient for glowing keystone shard of 'V' */}
              <radialGradient id="apexGlow" cx="50%" cy="50%" r="50%">
                <stop offset="0%" stopColor="#FFAA66" />
                <stop offset="100%" stopColor="#FF5500" />
              </radialGradient>

              {/* Central 'V' logo ambient radial glow (Locked inside SVG coordinates at (400, 130)) */}
              <radialGradient id="vCenterGlow" cx="50%" cy="50%" r="50%">
                <stop offset="0%" stopColor="rgba(255, 120, 0, 0.45)" />
                <stop offset="50%" stopColor="rgba(255, 85, 0, 0.15)" />
                <stop offset="100%" stopColor="rgba(255, 85, 0, 0)" />
              </radialGradient>

              {/* Specular Gleam gradient across seam joints */}
              <linearGradient id="seamGleamGrad" x1="0%" y1="0%" x2="100%" y2="0%">
                <stop offset="0%" stopColor="rgba(255, 255, 255, 0)" />
                <stop offset="45%" stopColor="rgba(255, 120, 0, 0.7)" />
                <stop offset="50%" stopColor="#FFFFFF" />
                <stop offset="55%" stopColor="rgba(255, 120, 0, 0.7)" />
                <stop offset="100%" stopColor="rgba(255, 255, 255, 0)" />
              </linearGradient>

              {/* Physical paper fold drop shadow */}
              <filter id="paperShadow" x="-15%" y="-15%" width="130%" height="130%">
                <feDropShadow
                  dx="0"
                  dy="2"
                  stdDeviation="2.5"
                  floodColor="#000000"
                  floodOpacity="0.5"
                />
              </filter>
            </defs>

            {/* A. Resonance aura centered exactly on 'V' (400, 130) */}
            <motion.circle
              cx="400"
              cy="130"
              r="160"
              fill="url(#vCenterGlow)"
              initial={{ scale: 0.2, opacity: 0 }}
              animate={{
                scale: [0.2, 0.8, 1.3, 1.0],
                opacity: [0, 0.65, 0.85, 0.3],
              }}
              transition={{
                times: [0, 0.24, 0.35, 0.5],
                duration: durationSec,
                ease: 'easeOut',
              }}
            />

            {/* B. Vector acoustic pulse waves emitted by the 'V' logo at 1.15s */}
            <motion.circle
              cx="400"
              cy="130"
              fill="none"
              stroke="rgba(255, 85, 0, 0.75)"
              strokeWidth="1.5"
              initial={{ r: 25, opacity: 0 }}
              animate={{
                r: [25, 25, 150, 210],
                opacity: [0, 0, 0.85, 0],
              }}
              transition={{
                times: [0, 0.23, 0.34, 0.46],
                duration: durationSec,
                ease: 'easeOut',
              }}
            />

            {/* C. Specular Seam Gleam line flashing across all letters at 2.05s */}
            <motion.line
              x1="90"
              y1="130"
              x2="710"
              y2="130"
              stroke="url(#seamGleamGrad)"
              strokeWidth="2"
              initial={{ pathLength: 0, opacity: 0 }}
              animate={{
                pathLength: [0, 0, 1, 1],
                opacity: [0, 0, 1, 0],
              }}
              transition={{
                times: [0, 0.41, 0.46, 0.53],
                duration: durationSec,
                ease: 'easeInOut',
              }}
            />

            {/* 1. CENTRAL LOGO 'V' SHARDS (Center X: 400) */}
            {V_LOGO_SHARDS.map((shard) => {
              const isApex = shard.id === 'v-apex-wedge';
              return (
                <motion.polygon
                  key={shard.id}
                  points={shard.points}
                  fill={isApex ? 'url(#apexGlow)' : shard.fill}
                  stroke={shard.stroke || 'rgba(255, 107, 0, 0.45)'}
                  strokeWidth="0.85"
                  strokeLinejoin="round"
                  filter="url(#paperShadow)"
                  initial={{
                    x: shard.origamiOriginX,
                    y: shard.origamiOriginY,
                    rotate: shard.origamiRotate,
                    scale: shard.origamiScale,
                    opacity: 0,
                  }}
                  animate={{
                    x: 0,
                    y: 0,
                    rotate: 0,
                    scale: [shard.origamiScale, 1.05, 0.98, 1.0],
                    opacity: 1,
                  }}
                  transition={{
                    duration: shard.unfoldDurationSec,
                    delay: shard.delaySec,
                    ease: [0.16, 1, 0.3, 1],
                  }}
                />
              );
            })}

            {/* 2. LETTER 'I' SHARDS (Center X: 175, Span: 100 to 250) */}
            {I_LETTER_SHARDS.map((shard) => (
              <motion.polygon
                key={shard.id}
                points={shard.points}
                fill={shard.fill}
                stroke={shard.stroke || 'rgba(255, 107, 0, 0.35)'}
                strokeWidth="0.85"
                strokeLinejoin="round"
                filter="url(#paperShadow)"
                initial={{
                  x: shard.origamiOriginX,
                  y: shard.origamiOriginY,
                  rotate: shard.origamiRotate,
                  scale: shard.origamiScale,
                  opacity: 0,
                }}
                animate={{
                  x: 0,
                  y: 0,
                  rotate: 0,
                  scale: [shard.origamiScale, 1.06, 0.98, 1.0],
                  opacity: 1,
                }}
                transition={{
                  duration: shard.unfoldDurationSec,
                  delay: shard.delaySec,
                  ease: [0.16, 1, 0.3, 1],
                }}
              />
            ))}

            {/* 3. LETTER 'Y' SHARDS (Center X: 625, Span: 550 to 700) */}
            {Y_LETTER_SHARDS.map((shard) => (
              <motion.polygon
                key={shard.id}
                points={shard.points}
                fill={shard.fill}
                stroke={shard.stroke || 'rgba(255, 107, 0, 0.35)'}
                strokeWidth="0.85"
                strokeLinejoin="round"
                filter="url(#paperShadow)"
                initial={{
                  x: shard.origamiOriginX,
                  y: shard.origamiOriginY,
                  rotate: shard.origamiRotate,
                  scale: shard.origamiScale,
                  opacity: 0,
                }}
                animate={{
                  x: 0,
                  y: 0,
                  rotate: 0,
                  scale: [shard.origamiScale, 1.06, 0.98, 1.0],
                  opacity: 1,
                }}
                transition={{
                  duration: shard.unfoldDurationSec,
                  delay: shard.delaySec,
                  ease: [0.16, 1, 0.3, 1],
                }}
              />
            ))}
          </svg>
        </motion.div>
      </div>
    </div>
    {/* Atmospheric dust particles layer */}
    <AtmosphericDust />
  </motion.div>
);
};
