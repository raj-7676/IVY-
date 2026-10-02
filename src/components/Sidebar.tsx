import React from 'react';
import { Clock, Settings, ShieldCheck, Home, BookMarked, Type } from 'lucide-react';
import { ScreenState } from '../types';
import { IvyWordmark } from './IvyWordmark';

interface SidebarProps {
  currentScreen: ScreenState;
  onSelectScreen: (screen: ScreenState) => void;
  sessionCount: number;
}

const ACCENT_RGB = '255, 107, 0';

type NavId = Extract<ScreenState, 'home' | 'history' | 'dictionary' | 'tone' | 'settings'>;

const NAV: { id: NavId; label: string; icon: typeof Clock }[] = [
  { id: 'home', label: 'Home', icon: Home },
  { id: 'history', label: 'History', icon: Clock },
  { id: 'dictionary', label: 'Dictionary', icon: BookMarked },
  { id: 'tone', label: 'Tone', icon: Type },
  { id: 'settings', label: 'Settings', icon: Settings },
];

export const Sidebar: React.FC<SidebarProps> = ({
  currentScreen,
  onSelectScreen,
  sessionCount,
}) => {
  return (
    <aside
      id="app-sidebar"
      className="w-60 shrink-0 h-full flex flex-col justify-between border-r border-white/[0.07] px-3 py-4"
    >
      <div className="space-y-7">
        {/* Main Title Wordmark — Click to open Voice Test Wizard */}
        <button
          id="sidebar-top-left-ivy-title"
          type="button"
          onClick={() => onSelectScreen('first-run')}
          className="w-full text-left rounded-2xl p-2 select-none hover:bg-white/[0.04] active:scale-[0.98] transition-all duration-200 cursor-pointer group block"
          title="Click IVY to open Voice Test Wizard"
        >
          <div className="transition-transform duration-200 group-hover:scale-[1.02] origin-left">
            <IvyWordmark height={34} glow={true} />
          </div>
          <div className="flex items-center gap-1.5 mt-2 opacity-0 group-hover:opacity-100 transition-opacity duration-200">
            <span
              className="w-1.5 h-1.5 rounded-full"
              style={{
                backgroundColor: `rgb(${ACCENT_RGB})`,
                boxShadow: `0 0 8px rgba(${ACCENT_RGB}, 0.9)`,
              }}
            />
            <span className="text-[10.5px] text-[#FFA133] font-medium tracking-wide">
              Voice Test Wizard
            </span>
          </div>
        </button>

        {/* Nav */}
        <nav className="space-y-1">
        {NAV.map(({ id, label, icon: Icon }) => {
          const active = currentScreen === id;
          return (
            <button
              key={id}
              id={`nav-${id}`}
              onClick={() => onSelectScreen(id)}
              style={
                active
                  ? {
                      backgroundColor: `rgba(${ACCENT_RGB}, 0.12)`,
                      border: `1px solid rgba(${ACCENT_RGB}, 0.3)`,
                      boxShadow: `0 0 16px rgba(${ACCENT_RGB}, 0.14)`,
                    }
                  : { border: '1px solid transparent' }
              }
              className={`w-full flex items-center justify-between px-3 py-2.5 rounded-2xl text-[13px] text-left transition-colors duration-150 ${
                active ? 'text-white' : 'text-white/50 hover:text-white/85 hover:bg-white/[0.05]'
              }`}
            >
              <span className="flex items-center gap-2.5">
                <Icon
                  className="w-4 h-4 stroke-[1.75]"
                  style={active ? { color: `rgb(${ACCENT_RGB})` } : undefined}
                />
                <span className={active ? 'font-medium' : ''}>{label}</span>
              </span>
              {id === 'history' && sessionCount > 0 && (
                <span className="font-mono text-[10.5px] text-white/40 tabular">
                  {sessionCount}
                </span>
              )}
            </button>
          );
        })}
      </nav>
    </div>

      {/* Offline assurance */}
      <div className="px-3 py-2.5 rounded-2xl bg-white/[0.04] border border-white/[0.07] flex items-center gap-2.5">
        <ShieldCheck className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
        <div className="min-w-0">
          <div className="text-[11.5px] text-white/80 leading-none">100% offline</div>
          <div className="text-[10px] text-white/35 mt-1 leading-none">Nothing leaves this machine</div>
        </div>
      </div>
    </aside>
  );
};
