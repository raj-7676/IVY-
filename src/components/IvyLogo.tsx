import React from 'react';

interface IvyLogoProps {
  size?: number | string;
  className?: string;
  glow?: boolean;
  animated?: boolean;
}

/**
 * Official Ivy Logo: The iconic geometric origami faceted 'V'
 * Normalized coordinates: 180px wide by 150px high (6:5 aspect ratio)
 */
export const IvyLogo: React.FC<IvyLogoProps> = ({
  size = 32,
  className = '',
  glow = true,
  animated = false,
}) => {
  const width = typeof size === 'number' ? size : size;
  const height = typeof size === 'number' ? Math.round((size * 150) / 180) : 'auto';

  return (
    <svg
      viewBox="0 0 180 150"
      width={width}
      height={height}
      className={`shrink-0 select-none overflow-visible ${className} ${
        animated ? 'transition-transform duration-300 hover:scale-105 active:scale-95' : ''
      }`}
      style={{
        filter: glow
          ? 'drop-shadow(0 0 14px rgba(255, 85, 0, 0.45)) drop-shadow(0 2px 4px rgba(0, 0, 0, 0.5))'
          : 'drop-shadow(0 2px 4px rgba(0, 0, 0, 0.4))',
      }}
    >
      <defs>
        {/* Radial glow gradient for keystone apex */}
        <radialGradient id="ivyLogoApexGlow" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#FFAA66" />
          <stop offset="60%" stopColor="#FF6B00" />
          <stop offset="100%" stopColor="#E05300" />
        </radialGradient>

        {/* Center facet gradient */}
        <linearGradient id="ivyLogoCenterGrad" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stopColor="#FF8833" />
          <stop offset="100%" stopColor="#FF5500" />
        </linearGradient>

        {/* Soft shadow for depth between origami facets */}
        <filter id="ivyLogoFacetShadow" x="-10%" y="-10%" width="120%" height="120%">
          <feDropShadow dx="0" dy="1.5" stdDeviation="1.5" floodColor="#000000" floodOpacity="0.45" />
        </filter>
      </defs>

      {/* Center Facet of V */}
      <polygon
        points="35,0 66,46 90,82 114,46 145,0 90,25"
        fill="url(#ivyLogoCenterGrad)"
        stroke="rgba(255, 120, 40, 0.6)"
        strokeWidth="1"
        strokeLinejoin="round"
        filter="url(#ivyLogoFacetShadow)"
      />

      {/* Upper Left Wing */}
      <polygon
        points="0,0 35,0 56,62 22,62"
        fill="#FFFFFF"
        stroke="rgba(255, 107, 0, 0.3)"
        strokeWidth="0.8"
        strokeLinejoin="round"
        filter="url(#ivyLogoFacetShadow)"
      />

      {/* Upper Right Wing */}
      <polygon
        points="124,62 158,62 180,0 145,0"
        fill="#FFFFFF"
        stroke="rgba(255, 107, 0, 0.3)"
        strokeWidth="0.8"
        strokeLinejoin="round"
        filter="url(#ivyLogoFacetShadow)"
      />

      {/* Lower Left Fold */}
      <polygon
        points="22,62 56,62 78,106 50,106"
        fill="#F1F5F9"
        stroke="rgba(255, 107, 0, 0.4)"
        strokeWidth="0.8"
        strokeLinejoin="round"
        filter="url(#ivyLogoFacetShadow)"
      />

      {/* Lower Right Fold */}
      <polygon
        points="130,106 102,106 124,62 158,62"
        fill="#F1F5F9"
        stroke="rgba(255, 107, 0, 0.4)"
        strokeWidth="0.8"
        strokeLinejoin="round"
        filter="url(#ivyLogoFacetShadow)"
      />

      {/* The Glowing Keystone Apex (The Heart of Ivy) */}
      <polygon
        points="50,106 90,102 130,106 90,150"
        fill="url(#ivyLogoApexGlow)"
        stroke="#FFAA66"
        strokeWidth="1.2"
        strokeLinejoin="round"
        filter="url(#ivyLogoFacetShadow)"
      />
    </svg>
  );
};
