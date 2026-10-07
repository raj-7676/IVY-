import { DictationSession, UserStats } from './types';

export interface DictationStats {
  totalWords: number;
  wordsPerMinute: number;
  dayStreak: number;
  sessionCount: number;
  /** Words dictated per day, oldest → newest, for the last 14 days. */
  recentDaily: { day: number; words: number }[];
}

const DAY_MS = 86_400_000;

/** Local midnight for an epoch-ms instant, as epoch ms. */
function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function formatDateKey(ms: number): string {
  const d = new Date(ms);
  const year = d.getFullYear();
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

export function computeStatsFromUserStats(userStats: UserStats): DictationStats {
  const today = startOfDay(Date.now());
  const recentDaily: { day: number; words: number }[] = [];

  for (let i = 13; i >= 0; i--) {
    const dayMs = today - i * DAY_MS;
    const dateKey = formatDateKey(dayMs);
    const words = userStats.dailyWords ? (userStats.dailyWords[dateKey] || 0) : 0;
    recentDaily.push({ day: dayMs, words });
  }

  return {
    totalWords: userStats.totalWords,
    wordsPerMinute: userStats.wordsPerMinute,
    dayStreak: userStats.dayStreak,
    sessionCount: userStats.sessionCount,
    recentDaily,
  };
}

export function computeStats(statsOrSessions: UserStats | DictationSession[]): DictationStats {
  if (!Array.isArray(statsOrSessions)) {
    return computeStatsFromUserStats(statsOrSessions);
  }

  const sessions = statsOrSessions;
  const totalWords = sessions.reduce((sum, s) => sum + (s.wordsCount || 0), 0);
  const totalSeconds = sessions.reduce((sum, s) => sum + (s.durationSec || 0), 0);
  const wordsPerMinute = totalSeconds > 0 ? Math.round(totalWords / (totalSeconds / 60)) : 0;

  const dated = sessions.filter((s) => s.createdAt > 0);
  const daysWithDictation = new Set(dated.map((s) => startOfDay(s.createdAt)));

  let dayStreak = 0;
  const today = startOfDay(Date.now());
  if (daysWithDictation.size > 0) {
    let cursor = daysWithDictation.has(today) ? today : today - DAY_MS;
    while (daysWithDictation.has(cursor)) {
      dayStreak++;
      cursor -= DAY_MS;
    }
  }

  const recentDaily: { day: number; words: number }[] = [];
  for (let i = 13; i >= 0; i--) {
    const day = today - i * DAY_MS;
    const words = dated
      .filter((s) => startOfDay(s.createdAt) === day)
      .reduce((sum, s) => sum + (s.wordsCount || 0), 0);
    recentDaily.push({ day, words });
  }

  return {
    totalWords,
    wordsPerMinute,
    dayStreak,
    sessionCount: sessions.length,
    recentDaily,
  };
}

export function greeting(now = new Date()): string {
  const h = now.getHours();
  if (h >= 5 && h < 12) return 'Good morning';
  if (h >= 12 && h < 17) return 'Good afternoon';
  if (h >= 17 && h < 21) return 'Good evening';
  return 'Good night';
}
