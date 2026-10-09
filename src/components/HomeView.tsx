import React, { useEffect, useMemo, useState } from 'react';
import { ArrowUpRight, Sparkles, ArrowRight } from 'lucide-react';
import { DictationSession, ScreenState, UserStats } from '../types';
import { computeStats, greeting } from '../stats';
import { IS_MAC, keyLabel } from '../utils/platform';

interface HomeViewProps {
  sessions: DictationSession[];
  userStats?: UserStats;
  hotkey: string;
  onSelectScreen: (screen: ScreenState) => void;
}

const ACCENT_RGB = '255, 107, 0';

// A real "haven't loaded real stats yet" state, not a guess — `sessions` is
// pruned to the last 1-7 days (History's "Keep for") by the backend's retention purge, so deriving
// totals/streak from it whenever `userStats` hasn't arrived (including
// forever, if that fetch ever fails) can never reproduce a real long-time
// user's actual lifetime numbers. Zero and honest beats plausible and wrong.
const EMPTY_STATS: UserStats = {
  totalWords: 0,
  wordsPerMinute: 0,
  dayStreak: 0,
  lastActiveDate: '',
  totalDurationSec: 0,
  sessionCount: 0,
  dailyWords: {},
};

const Stat: React.FC<{ value: string; label: string; hint?: string }> = ({
  value,
  label,
  hint,
}) => (
  <div>
    <div className="flex items-baseline gap-1.5">
      <span className="text-[30px] font-semibold tracking-tight text-white/95 tabular leading-none">
        {value}
      </span>
      {hint && <span className="text-[12px] text-white/35">{hint}</span>}
    </div>
    <div className="text-[11.5px] text-white/40 mt-2">{label}</div>
  </div>
);

export const HomeView: React.FC<HomeViewProps> = ({ sessions, userStats, hotkey, onSelectScreen }) => {
  const stats = useMemo(() => computeStats(userStats ?? EMPTY_STATS), [userStats]);
  const recent = sessions.slice(0, 4);
  const peakDay = Math.max(1, ...stats.recentDaily.map((d) => d.words));
  // Ivy stays open in the tray all day, so the greeting must follow the clock, not the first render.
  const [hello, setHello] = useState(() => greeting());
  useEffect(() => {
    const id = setInterval(() => setHello(greeting()), 60_000);
    return () => clearInterval(id);
  }, []);

  return (
    <div id="screen-home" className="flex-1 flex flex-col h-full overflow-y-auto">
      <div className="px-8 pt-7 pb-10 space-y-8">
        <div>
          <h1 className="text-[24px] font-semibold tracking-tight text-white/95">
            {hello}
          </h1>
          <p className="text-[12.5px] text-white/40 mt-1.5">
            Hold{' '}
            <kbd className="px-1.5 py-0.5 rounded-md bg-white/[0.07] border border-white/[0.1] text-white/80 text-[10.5px]">
              {keyLabel(hotkey)}
            </kbd>{' '}
            anywhere and talk. Ivy types it where your cursor is.
          </p>
        </div>

        {/* Stats */}
        <section
          className="rounded-3xl px-7 py-6"
          style={{
            backgroundColor: `rgba(${ACCENT_RGB}, 0.07)`,
            border: `1px solid rgba(${ACCENT_RGB}, 0.22)`,
            boxShadow: `0 0 34px rgba(${ACCENT_RGB}, 0.1)`,
          }}
        >
          <div className="grid grid-cols-3 gap-6">
            <Stat value={stats.totalWords.toLocaleString()} label="Words dictated" />
            <Stat value={String(stats.wordsPerMinute)} label="Words per minute" />
            <Stat
              value={String(stats.dayStreak)}
              label="Day streak"
              hint={stats.dayStreak > 0 ? 'running' : undefined}
            />
          </div>

          {/* Last 14 days */}
          <div className="mt-7 pt-5 border-t border-white/[0.07]">
            <div className="flex items-end justify-between gap-[3px] h-10">
              {stats.recentDaily.map(({ day, words }) => (
                <div
                  key={day}
                  title={`${words} words`}
                  className="flex-1 rounded-sm transition-all duration-200"
                  style={{
                    height: `${Math.max(6, (words / peakDay) * 100)}%`,
                    backgroundColor: words
                      ? `rgba(${ACCENT_RGB}, ${0.35 + (words / peakDay) * 0.55})`
                      : 'rgba(255,255,255,0.06)',
                  }}
                />
              ))}
            </div>
            <div className="flex items-center justify-between mt-2.5 text-[10.5px] text-white/30">
              <span>14 days ago</span>
              <span>Today</span>
            </div>
          </div>
        </section>

        {/* Setup guide: one slim line (Yash, 2026-10-08: shorter, not gone) */}
        <button
          id="btn-test-wizard-home"
          onClick={() => onSelectScreen('first-run')}
          className="w-full flex items-center gap-3 px-4 py-2.5 rounded-2xl text-left transition-colors duration-150 hover:bg-white/[0.04]"
          style={{ border: '1px solid rgba(255, 107, 0, 0.22)', backgroundColor: 'rgba(18, 13, 26, 0.6)' }}
        >
          <Sparkles className="w-4 h-4 text-[#FF6B00] shrink-0" />
          <span className="text-[12.5px] text-white/75">
            <span className="font-semibold text-white/90">Setup guide</span> · your key, {IS_MAC ? 'permissions' : 'GPU or CPU'}, mic test and privacy
          </span>
          <ArrowRight className="w-3.5 h-3.5 text-white/40 ml-auto shrink-0" />
        </button>

        {/* Recent */}
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-[13.5px] font-medium text-white/85">Recent</h2>
            <button
              onClick={() => onSelectScreen('history')}
              className="flex items-center gap-1 text-[12px] text-white/40 hover:text-white/80 transition-colors duration-150"
            >
              All history
              <ArrowUpRight className="w-3.5 h-3.5" />
            </button>
          </div>

          {recent.length === 0 ? (
            <p className="text-[12.5px] text-white/35 py-6">
              Nothing yet — your first dictation shows up here.
            </p>
          ) : (
            <div className="divide-y divide-white/[0.06]">
              {recent.map((session) => (
                <div key={session.id} className="flex gap-5 py-3.5">
                  <div className="w-24 shrink-0 font-mono text-[11px] text-white/40 tabular whitespace-nowrap overflow-hidden text-ellipsis">
                    {session.timestamp}
                  </div>
                  <p className="flex-1 min-w-0 text-[13px] text-white/80 truncate">
                    {session.preview}
                  </p>
                  <span className="shrink-0 text-[11px] text-white/30">{session.appTarget}</span>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
};
