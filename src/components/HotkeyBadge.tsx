import React from 'react';
import { motion } from 'motion/react';

interface HotkeyBadgeProps {
  hotkey: string;
  isPressed: boolean;
  size?: 'normal' | 'large';
  onClick?: () => void;
  id?: string;
}

export const HotkeyBadge: React.FC<HotkeyBadgeProps> = React.memo(({
  hotkey,
  isPressed,
  size = 'normal',
  onClick,
  id = 'hotkey-badge',
}) => {
  // Parse hotkey tokens, e.g. "Alt + Space" -> ["Alt", "Space"]
  const keys = hotkey.split('+').map((k) => k.trim());

  return (
    <motion.div
      id={id}
      onClick={onClick}
      animate={{
        scale: isPressed ? 1.04 : 1,
      }}
      transition={{ duration: 0.12 }}
      className={`inline-flex items-center gap-2 select-none ${
        onClick ? 'cursor-pointer' : ''
      }`}
    >
      {keys.map((keyLabel, idx) => (
        <span key={keyLabel} className="inline-flex items-center gap-2">
          <motion.kbd
            animate={
              isPressed
                ? {
                    backgroundColor: '#FF6B00',
                    color: '#FFFFFF',
                    borderColor: '#FFA133',
                    boxShadow:
                      'inset 0 1.5px 2px rgba(255, 255, 255, 0.6), inset 0 -2px 4px rgba(0,0,0,0.5)',
                  }
                : {
                    backgroundColor: '#14101D',
                    color: '#E2E8F0',
                    borderColor: 'rgba(255, 255, 255, 0.12)',
                    boxShadow:
                      '0 4px 14px rgba(0, 0, 0, 0.6), inset 0 1px 1.5px rgba(255,255,255,0.18), inset 0 -2px 4px rgba(0,0,0,0.4)',
                  }
            }
            transition={{ duration: 0.12 }}
            className={`font-mono font-bold tracking-wide rounded-xl border flex items-center justify-center transition-all ${
              size === 'large'
                ? 'px-5 py-2.5 text-base sm:text-lg min-w-[70px]'
                : 'px-3.5 py-1.5 text-xs sm:text-sm min-w-[48px]'
            }`}
          >
            {keyLabel}
          </motion.kbd>

          {idx < keys.length - 1 && (
            <span
              className={`font-semibold ${
                isPressed ? 'text-[#FFA133]' : 'text-white/40'
              } ${size === 'large' ? 'text-lg' : 'text-xs'}`}
            >
              +
            </span>
          )}
        </span>
      ))}
    </motion.div>
  );
});
