import React from 'react';

interface IvyWordmarkProps {
  className?: string;
  height?: number | string;
  glow?: boolean;
}

export const IvyWordmark: React.FC<IvyWordmarkProps> = React.memo(({
  className = '',
  height = 36,
  glow = true,
}) => {
  return (
    <svg
      viewBox="0 0 600 150"
      height={height}
      className={`shrink-0 select-none overflow-visible w-auto ${className}`}
      style={{
        filter: glow
          ? 'drop-shadow(0 0 16px rgba(255, 85, 0, 0.45)) drop-shadow(0 2px 6px rgba(0, 0, 0, 0.6))'
          : 'drop-shadow(0 2px 4px rgba(0, 0, 0, 0.4))',
      }}
    >
      <defs>
        <radialGradient id="wmApexGlow" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#FFAA66" />
          <stop offset="60%" stopColor="#FF6B00" />
          <stop offset="100%" stopColor="#E05300" />
        </radialGradient>
        <linearGradient id="wmCenterGrad" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stopColor="#FF8833" />
          <stop offset="100%" stopColor="#FF5500" />
        </linearGradient>
        <linearGradient id="wmSilverLeft" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#FFFFFF" />
          <stop offset="60%" stopColor="#F1F5F9" />
          <stop offset="100%" stopColor="#CBD5E1" />
        </linearGradient>
        <linearGradient id="wmSilverRight" x1="100%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stopColor="#FFFFFF" />
          <stop offset="60%" stopColor="#F1F5F9" />
          <stop offset="100%" stopColor="#CBD5E1" />
        </linearGradient>
        <filter id="wmFacetShadow" x="-10%" y="-10%" width="120%" height="120%">
          <feDropShadow dx="0" dy="1.5" stdDeviation="1.5" floodColor="#000000" floodOpacity="0.45" />
        </filter>
      </defs>

      {/* LETTER 'I' (Clean straight origami column) */}
      <g id="wm-letter-i">
        <polygon points="100,20 145,20 145,40 110,40" fill="url(#wmSilverLeft)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="145,20 190,20 180,40 145,40" fill="url(#wmSilverRight)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="127,40 145,40 145,110 127,110" fill="url(#wmSilverLeft)" stroke="rgba(255,107,0,0.35)" strokeWidth="0.8" />
        <polygon points="145,40 163,40 163,110 145,110" fill="url(#wmSilverRight)" stroke="rgba(255,107,0,0.35)" strokeWidth="0.8" />
        <polygon points="110,110 145,110 145,130 100,130" fill="url(#wmSilverLeft)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="145,110 180,110 190,130 145,130" fill="url(#wmSilverRight)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
      </g>

      {/* HERO LETTER 'V' (Iconic glowing keystone apex) */}
      <g id="wm-letter-v">
        <polygon points="260,105 300,101 340,105 300,140" fill="url(#wmApexGlow)" stroke="#FFAA66" strokeWidth="1" />
        <polygon points="232,69 266,69 288,105 260,105" fill="url(#wmSilverLeft)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="340,105 312,105 334,69 368,69" fill="url(#wmSilverRight)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="245,20 276,57 300,86 324,57 355,20 300,40" fill="url(#wmCenterGrad)" stroke="rgba(255,140,50,0.7)" strokeWidth="0.8" />
        <polygon points="210,20 245,20 266,69 232,69" fill="url(#wmSilverLeft)" stroke="rgba(255,120,30,0.4)" strokeWidth="0.8" />
        <polygon points="334,69 368,69 390,20 355,20" fill="url(#wmSilverRight)" stroke="rgba(255,120,30,0.4)" strokeWidth="0.8" />
      </g>

      {/* LETTER 'Y' — shifted 22 units left of its original position so the
          I↔V and V↔Y gaps match (were 20 and 42 units respectively) */}
      <g id="wm-letter-y">
        <polygon points="448,79 478,62 508,79 478,91" fill="url(#wmCenterGrad)" stroke="#FFAA66" strokeWidth="0.8" />
        <polygon points="410,20 436,20 453,54 428,54" fill="url(#wmSilverLeft)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="428,54 453,54 465,79 448,79" fill="url(#wmSilverLeft)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="520,20 546,20 528,54 503,54" fill="url(#wmSilverRight)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="503,54 528,54 508,79 491,79" fill="url(#wmSilverRight)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="467,87 489,87 493,111 463,111" fill="url(#wmSilverLeft)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
        <polygon points="463,111 493,111 501,138 455,138" fill="url(#wmSilverRight)" stroke="rgba(255,107,0,0.4)" strokeWidth="0.8" />
      </g>
    </svg>
  );
});
