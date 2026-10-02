import React, { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Search,
  Trash2,
  RotateCcw,
  Copy,
  Check,
  AlertCircle,
  RefreshCw,
  MoreVertical,
  Download,
  Sparkles,
} from 'lucide-react';
import { DictationSession } from '../types';

interface HistoryViewProps {
  sessions: DictationSession[];
  onDeleteSession: (id: string) => void;
  onUpdateSession?: (session: DictationSession) => void;
}

interface RetryResult {
  success: boolean;
  pasted: boolean;
  transcript: string;
  wordsCount: number;
}

const ACCENT_RGB = '255, 107, 0';

export const HistoryView: React.FC<HistoryViewProps> = ({ sessions, onDeleteSession, onUpdateSession }) => {
  const [searchQuery, setSearchQuery] = useState('');
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [pastedId, setPastedId] = useState<string | null>(null);
  const [failedId, setFailedId] = useState<string | null>(null);
  const [retryingId, setRetryingId] = useState<string | null>(null);
  const [retryFailedId, setRetryFailedId] = useState<string | null>(null);
  const [menuId, setMenuId] = useState<string | null>(null);
  const [summarizingId, setSummarizingId] = useState<string | null>(null);
  const [isClearingAll, setIsClearingAll] = useState(false);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 2600);
    return () => clearTimeout(t);
  }, [toast]);

  const handleCopy = (session: DictationSession) => {
    navigator.clipboard?.writeText(session.fullTranscript);
    setCopiedId(session.id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  // Real paste through the Rust backend — same guarded clipboard + Ctrl+V
  // path a fresh dictation uses. Reports the truth instead of always
  // claiming success: a clipboard-only fallback (no matching text box) is
  // not the same thing as a real paste.
  const handleRetryPaste = async (session: DictationSession) => {
    try {
      const didPaste = await invoke<boolean>('repaste_transcript', { text: session.fullTranscript });
      if (didPaste) {
        setPastedId(session.id);
        setTimeout(() => setPastedId(null), 1800);
      } else {
        setToast('Copied — no text box found');
      }
    } catch {
      setFailedId(session.id);
      setTimeout(() => setFailedId(null), 2400);
    }
  };

  // Re-runs real transcription against the audio already saved for this
  // session — no re-recording. Updates both local session immediately
  // and the persistent history store.
  const handleRetryTranscription = async (session: DictationSession) => {
    setRetryingId(session.id);
    try {
      const res = await invoke<RetryResult>('retry_transcription', { id: session.id });
      if (res && res.transcript) {
        // Spread into an array of code points first — `.length`/`.slice`
        // count UTF-16 code units, which can split a surrogate pair
        // (an emoji, for instance) right down the middle and render a
        // broken/replacement character in the truncated preview.
        const codePoints = Array.from(res.transcript);
        const updated: DictationSession = {
          ...session,
          fullTranscript: res.transcript,
          preview: codePoints.length > 80 ? `${codePoints.slice(0, 80).join('')}...` : res.transcript,
          wordsCount: res.wordsCount,
        };
        onUpdateSession?.(updated);
        setToast(res.pasted ? 'Transcribed and pasted!' : 'Transcribed & copied to clipboard!');
      } else {
        setToast('Transcription completed');
      }
    } catch (e) {
      setRetryFailedId(session.id);
      setTimeout(() => setRetryFailedId(null), 2400);
      setToast(typeof e === 'string' ? e : 'Could not transcribe');
    } finally {
      setRetryingId(null);
    }
  };

  const handleExtractAudio = async (session: DictationSession) => {
    try {
      const dest = await invoke<string>('extract_audio', { id: session.id });
      setToast(`Saved to ${dest}`);
    } catch (e) {
      setToast(typeof e === 'string' ? e : 'Could not extract audio');
    }
  };

  // Real, explicit, opt-in — never runs automatically, and never happens as
  // part of a live dictation (that must stay instant). Runs the actual
  // Qwen 2.5 3B model against the full transcript; on a real failure this
  // shows the real error, never a raw-transcript-relabeled-as-summary fake.
  const handleSummarize = async (session: DictationSession) => {
    setSummarizingId(session.id);
    try {
      const summary = await invoke<string>('summarize_transcript', { id: session.id });
      onUpdateSession?.({ ...session, summary });
    } catch (e) {
      setToast(typeof e === 'string' ? e : 'Could not summarize this one');
    } finally {
      setSummarizingId(null);
    }
  };

  // Manual, explicit, and separate from the automatic 2-day purge — full
  // user control to clear everything right now instead of waiting it out.
  // Real confirm since this is irreversible: audio files are deleted too.
  // A styled in-app modal, not window.confirm() — that renders as a bare
  // OS dialog ("tauri.localhost says"), completely outside the app's UI.
  const handleClearAll = async () => {
    setShowClearConfirm(false);
    setIsClearingAll(true);
    try {
      await invoke('clear_all_history');
    } catch (e) {
      setToast(typeof e === 'string' ? e : 'Could not clear history');
    } finally {
      setIsClearingAll(false);
    }
  };

  const filtered = useMemo(() => {
    const q = searchQuery.toLowerCase();
    if (!q) return sessions;
    return sessions.filter(
      (s) =>
        s.fullTranscript.toLowerCase().includes(q) ||
        s.preview.toLowerCase().includes(q) ||
        s.appTarget.toLowerCase().includes(q)
    );
  }, [sessions, searchQuery]);

  // Real duplicate detection: re-recording the same thing right after a
  // bad take is common (retry didn't sound right, so you just redo it) —
  // group those into one thread instead of cluttering History with near-
  // identical rows. Pure word-overlap + a tight time window, no AI: two
  // dictations only ever count as duplicates if they're both close in time
  // AND share most of their words.
  const DUPLICATE_WINDOW_MS = 10 * 60 * 1000;
  const DUPLICATE_SIMILARITY = 0.6;
  const wordSet = (text: string) =>
    new Set(text.toLowerCase().split(/\s+/).map((w) => w.replace(/[^a-z0-9']/g, '')).filter(Boolean));
  const jaccard = (a: Set<string>, b: Set<string>) => {
    if (a.size === 0 && b.size === 0) return 1;
    let inter = 0;
    a.forEach((w) => { if (b.has(w)) inter++; });
    const union = a.size + b.size - inter;
    return union === 0 ? 0 : inter / union;
  };

  const { hiddenIds, dupEntriesByPrimaryId } = useMemo(() => {
    const hidden = new Set<string>();
    const dupMap = new Map<string, DictationSession[]>();
    let primary: DictationSession | null = null;
    let primaryWords: Set<string> | null = null;
    // `filtered` is newest-first, matching how the backend inserts entries.
    for (const s of filtered) {
      if (!s.fullTranscript) {
        primary = null;
        primaryWords = null;
        continue;
      }
      if (primary && primaryWords) {
        const closeInTime = Math.abs(primary.createdAt - s.createdAt) <= DUPLICATE_WINDOW_MS;
        if (closeInTime && jaccard(primaryWords, wordSet(s.fullTranscript)) >= DUPLICATE_SIMILARITY) {
          hidden.add(s.id);
          const arr = dupMap.get(primary.id) ?? [];
          arr.push(s);
          dupMap.set(primary.id, arr);
          continue;
        }
      }
      primary = s;
      primaryWords = wordSet(s.fullTranscript);
    }
    return { hiddenIds: hidden, dupEntriesByPrimaryId: dupMap };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filtered]);

  const [expandedDupId, setExpandedDupId] = useState<string | null>(null);

  return (
    <div id="screen-history" className="flex-1 flex flex-col h-full overflow-hidden relative">
      <header className="px-8 pt-7 pb-5 shrink-0 space-y-5">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-[22px] font-semibold tracking-tight text-white/95">History</h1>
            <p className="text-[12.5px] text-white/40 mt-1">
              Every dictation, kept on this machine only. Both the transcript and the recording are deleted automatically after 24 hours — nothing is kept longer than that.
            </p>
          </div>
          {sessions.length > 0 && (
            <button
              id="clear-all-history-btn"
              onClick={() => setShowClearConfirm(true)}
              disabled={isClearingAll}
              className="shrink-0 px-4 py-2 rounded-xl text-[12.5px] font-semibold text-white disabled:opacity-50 transition-opacity duration-150"
              style={{ backgroundColor: `rgb(${ACCENT_RGB})` }}
            >
              {isClearingAll ? 'Clearing…' : 'Clear all transcriptions'}
            </button>
          )}
        </div>

        <div className="relative">
          <Search className="w-4 h-4 text-white/30 absolute left-3.5 top-1/2 -translate-y-1/2 pointer-events-none" />
          <input
            id="history-search-input"
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search transcripts"
            className="w-full bg-white/[0.04] border border-white/[0.08] rounded-2xl pl-10 pr-3 py-2.5 text-[13px] text-white/90 focus:outline-none focus:border-white/[0.18] focus:bg-white/[0.06] transition-colors duration-150"
          />
        </div>
      </header>

      <div className="flex-1 overflow-y-auto px-8 pb-8">
        {filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-56 text-center">
            <p className="text-[13px] text-white/60">
              {sessions.length === 0 ? 'No dictations yet' : 'Nothing matches that search'}
            </p>
            <p className="text-[12px] text-white/30 mt-1.5">
              {sessions.length === 0 ? (
                <>
                  Hold{' '}
                  <kbd className="px-1.5 py-0.5 rounded bg-white/[0.07] text-white/70 text-[10.5px]">
                    Alt+Space
                  </kbd>{' '}
                  anywhere and speak.
                </>
              ) : (
                'Try a different word.'
              )}
            </p>
          </div>
        ) : (
          <div className="divide-y divide-white/[0.06]">
            {filtered.map((session) => {
              // Rendered nested under its primary instead — see the
              // duplicate-grouping useMemo above.
              if (hiddenIds.has(session.id)) return null;
              const isExpanded = expandedId === session.id;
              const isCopied = copiedId === session.id;
              const isPasted = pastedId === session.id;
              const isFailed = failedId === session.id;
              const isRetrying = retryingId === session.id;
              const retryFailed = retryFailedId === session.id;
              const menuOpen = menuId === session.id;
              const isSummarizing = summarizingId === session.id;
              const transcriptionFailed = session.fullTranscript.length === 0;
              // Matches the backend's own definition of "a long dictation"
              // (cleanup.rs's fast-path threshold) — summarizing a short
              // one has nothing to compress.
              const isLongEnoughToSummarize = session.wordsCount > 120;
              const duplicates = dupEntriesByPrimaryId.get(session.id);

              return (
                <React.Fragment key={session.id}>
                <div
                  id={`session-${session.id}`}
                  onClick={() => setExpandedId(isExpanded ? null : session.id)}
                  className="group relative flex gap-5 py-4 cursor-pointer"
                >
                  {/* Timestamp rail */}
                  <div className="w-24 shrink-0 pt-0.5">
                    <div className="font-mono text-[11px] text-white/45 tabular leading-snug">
                      {session.timestamp}
                    </div>
                    <div className="font-mono text-[10.5px] text-white/25 mt-1 tabular">
                      {session.duration}
                    </div>
                  </div>

                  {/* Transcript */}
                  <div className="flex-1 min-w-0">
                    {!transcriptionFailed && session.title && (
                      <div className="text-[13.5px] font-semibold text-white/95 mb-0.5 select-text">
                        {session.title}
                      </div>
                    )}
                    {transcriptionFailed ? (
                      <div className="flex flex-wrap items-center justify-between gap-3 py-1">
                        <p className="text-[13.5px] leading-relaxed text-amber-400/90 flex items-center gap-2">
                          <AlertCircle className="w-4 h-4 shrink-0 text-amber-400" />
                          <span>
                            Couldn't transcribe — audio was saved safely.
                            {retryFailed && <span className="text-white/40 ml-1.5">(retry failed)</span>}
                          </span>
                        </p>
                        {session.audioPath && (
                          <button
                            id={`retry-btn-${session.id}`}
                            onClick={(e) => {
                              e.stopPropagation();
                              void handleRetryTranscription(session);
                            }}
                            disabled={isRetrying}
                            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-amber-400/20 hover:bg-amber-400/30 border border-amber-400/40 text-amber-300 text-xs font-semibold transition-colors disabled:opacity-40 shadow-sm"
                          >
                            <RefreshCw className={`w-3.5 h-3.5 ${isRetrying ? 'animate-spin text-amber-300' : ''}`} />
                            {isRetrying ? 'Transcribing…' : 'Retry Transcription'}
                          </button>
                        )}
                      </div>
                    ) : (
                      <p
                        className={`text-[13.5px] leading-relaxed text-white/85 select-text ${
                          isExpanded ? '' : 'line-clamp-2'
                        }`}
                      >
                        {isExpanded ? session.fullTranscript : session.preview}
                      </p>
                    )}

                    {session.summary && (
                      <div
                        className="mt-2.5 rounded-xl px-3 py-2.5 text-[12.5px] leading-relaxed text-white/75 whitespace-pre-line select-text"
                        style={{
                          backgroundColor: `rgba(${ACCENT_RGB}, 0.06)`,
                          border: `1px solid rgba(${ACCENT_RGB}, 0.18)`,
                        }}
                      >
                        <div
                          className="text-[10px] uppercase tracking-wider font-semibold mb-1"
                          style={{ color: `rgb(${ACCENT_RGB})` }}
                        >
                          Summary
                        </div>
                        {session.summary}
                      </div>
                    )}

                    {isSummarizing && (
                      <div className="mt-2 flex items-center gap-1.5 text-[11px] text-white/40">
                        <RefreshCw className="w-3 h-3 animate-spin" />
                        Summarizing…
                      </div>
                    )}

                    <div className="flex items-center gap-2 mt-2">
                      <span
                        className="px-2 py-0.5 rounded-full text-[10.5px] font-medium"
                        style={{
                          backgroundColor: `rgba(${ACCENT_RGB}, 0.12)`,
                          border: `1px solid rgba(${ACCENT_RGB}, 0.26)`,
                          color: `rgb(${ACCENT_RGB})`,
                        }}
                      >
                        {session.tonePreset}
                      </span>
                      <span className="text-[11px] text-white/30">{session.appTarget}</span>
                      {!transcriptionFailed && (
                        <>
                          <span className="text-[11px] text-white/20">·</span>
                          <span className="font-mono text-[10.5px] text-white/30 tabular">
                            {session.wordsCount}w
                          </span>
                        </>
                      )}
                    </div>
                  </div>

                  {/* Actions — quiet until hover. Download/Retry/Copy stay inline */}
                  <div
                    className="flex items-start gap-1 shrink-0 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity duration-150"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {session.audioPath && (
                      <button
                        id={`play-${session.id}`}
                        onClick={() => void handleExtractAudio(session)}
                        title="Download recording"
                        className="p-2 rounded-xl text-white/40 hover:text-white hover:bg-white/[0.08] transition-colors duration-150"
                      >
                        <Download className="w-3.5 h-3.5" />
                      </button>
                    )}

                    {session.audioPath && (
                      <button
                        id={`retry-${session.id}`}
                        onClick={() => void handleRetryTranscription(session)}
                        disabled={isRetrying}
                        title={isRetrying ? 'Retrying transcription...' : 'Retry transcription'}
                        className="p-2 rounded-xl text-white/40 hover:text-[#E59530] hover:bg-white/[0.08] transition-colors duration-150 disabled:opacity-40"
                      >
                        <RefreshCw className={`w-3.5 h-3.5 ${isRetrying ? 'animate-spin text-[#E59530]' : ''}`} />
                      </button>
                    )}

                    {!transcriptionFailed && (
                      <button
                        id={`copy-${session.id}`}
                        onClick={() => handleCopy(session)}
                        title="Copy transcript"
                        className="p-2 rounded-xl text-white/40 hover:text-white hover:bg-white/[0.08] transition-colors duration-150"
                      >
                        {isCopied ? (
                          <Check className="w-3.5 h-3.5 text-emerald-400" />
                        ) : (
                          <Copy className="w-3.5 h-3.5" />
                        )}
                      </button>
                    )}

                    <div className="relative">
                      <button
                        id={`more-${session.id}`}
                        onClick={() => setMenuId(menuOpen ? null : session.id)}
                        title="More options"
                        className="p-2 rounded-xl text-white/40 hover:text-white hover:bg-white/[0.08] transition-colors duration-150"
                      >
                        <MoreVertical className="w-3.5 h-3.5" />
                      </button>

                      {menuOpen && (
                        <>
                          <div className="fixed inset-0 z-20" onClick={() => setMenuId(null)} />
                          <div
                            className="absolute z-30 top-full right-0 mt-1.5 min-w-[190px] rounded-2xl overflow-hidden py-1"
                            style={{
                              backgroundColor: 'rgba(16, 13, 20, 0.97)',
                              border: '1px solid rgba(255,255,255,0.1)',
                              backdropFilter: 'blur(20px)',
                            }}
                          >
                            {!transcriptionFailed && (
                              <button
                                onClick={() => {
                                  setMenuId(null);
                                  void handleRetryPaste(session);
                                }}
                                className="w-full flex items-center gap-2.5 px-3.5 py-2.5 text-[12.5px] text-left text-white/80 hover:bg-white/[0.07] transition-colors duration-150"
                              >
                                {isPasted ? (
                                  <Check className="w-3.5 h-3.5 text-emerald-400" />
                                ) : isFailed ? (
                                  <AlertCircle className="w-3.5 h-3.5 text-amber-400" />
                                ) : (
                                  <RotateCcw className="w-3.5 h-3.5" />
                                )}
                                {isFailed ? "Couldn't paste" : 'Paste again'}
                              </button>
                            )}
                            {session.audioPath && (
                              <button
                                onClick={() => {
                                  setMenuId(null);
                                  void handleRetryTranscription(session);
                                }}
                                disabled={isRetrying}
                                className="w-full flex items-center gap-2.5 px-3.5 py-2.5 text-[12.5px] text-left text-white/80 hover:bg-white/[0.07] transition-colors duration-150 disabled:opacity-40"
                              >
                                <RefreshCw className={`w-3.5 h-3.5 ${isRetrying ? 'animate-spin' : ''}`} />
                                Retry transcription
                              </button>
                            )}
                            {session.audioPath && (
                              <button
                                onClick={() => {
                                  setMenuId(null);
                                  void handleExtractAudio(session);
                                }}
                                className="w-full flex items-center gap-2.5 px-3.5 py-2.5 text-[12.5px] text-left text-white/80 hover:bg-white/[0.07] transition-colors duration-150"
                              >
                                <Download className="w-3.5 h-3.5" />
                                Extract audio
                              </button>
                            )}
                            {!transcriptionFailed && isLongEnoughToSummarize && (
                              <button
                                onClick={() => {
                                  setMenuId(null);
                                  void handleSummarize(session);
                                }}
                                disabled={isSummarizing}
                                className="w-full flex items-center gap-2.5 px-3.5 py-2.5 text-[12.5px] text-left text-white/80 hover:bg-white/[0.07] transition-colors duration-150 disabled:opacity-40"
                              >
                                <Sparkles className={`w-3.5 h-3.5 ${isSummarizing ? 'animate-pulse' : ''}`} />
                                {session.summary ? 'Re-summarize' : 'Summarize'}
                              </button>
                            )}
                            <button
                              onClick={() => {
                                setMenuId(null);
                                onDeleteSession(session.id);
                              }}
                              className="w-full flex items-center gap-2.5 px-3.5 py-2.5 text-[12.5px] text-left text-red-400/90 hover:bg-red-500/10 transition-colors duration-150"
                            >
                              <Trash2 className="w-3.5 h-3.5" />
                              Delete transcript
                            </button>
                          </div>
                        </>
                      )}
                    </div>
                  </div>
                </div>

                {duplicates && duplicates.length > 0 && (
                  <div className="pl-[116px] pb-3 -mt-1">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        setExpandedDupId(expandedDupId === session.id ? null : session.id);
                      }}
                      className="text-[11px] text-white/35 hover:text-white/60 transition-colors duration-150"
                    >
                      {expandedDupId === session.id
                        ? 'Hide similar attempts'
                        : `+${duplicates.length} similar attempt${duplicates.length === 1 ? '' : 's'} · Show`}
                    </button>
                    {expandedDupId === session.id && (
                      <div className="mt-2 space-y-2 border-l border-white/[0.08] pl-3">
                        {duplicates.map((dup) => (
                          <div key={dup.id} className="flex items-center justify-between gap-3 text-[11.5px] text-white/45">
                            <span className="font-mono text-[10px] text-white/30 shrink-0">{dup.timestamp}</span>
                            <span className="truncate flex-1 select-text">{dup.preview}</span>
                            <button
                              onClick={(e) => {
                                e.stopPropagation();
                                onDeleteSession(dup.id);
                              }}
                              title="Delete this attempt"
                              className="text-white/25 hover:text-red-400 transition-colors duration-150 shrink-0"
                            >
                              <Trash2 className="w-3 h-3" />
                            </button>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
                </React.Fragment>
              );
            })}
          </div>
        )}
      </div>

      {toast && (
        <div
          className="absolute bottom-5 left-1/2 -translate-x-1/2 px-4 py-2.5 rounded-xl text-[12.5px] text-white/90 shadow-lg"
          style={{ backgroundColor: 'rgba(16, 13, 20, 0.97)', border: '1px solid rgba(255,255,255,0.12)' }}
        >
          {toast}
        </div>
      )}

      {/* Clear-all confirmation — a real in-app modal matching the rest of
          Ivy's UI, not window.confirm()'s bare OS dialog */}
      {showClearConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/75 backdrop-blur-md animate-in fade-in duration-200">
          <div
            className="w-full max-w-md p-6 rounded-3xl border border-white/[0.14] shadow-[0_30px_70px_-15px_rgba(0,0,0,0.95)]"
            style={{ backgroundColor: 'rgba(16, 12, 20, 0.98)' }}
          >
            <div className="flex items-center gap-3.5 mb-2">
              <div className="w-11 h-11 rounded-2xl flex items-center justify-center shrink-0 bg-red-500/15 border border-red-500/30 text-red-400">
                <Trash2 className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-[15.5px] font-semibold text-white/95 tracking-tight">
                  Clear all transcriptions?
                </h3>
                <p className="text-[11.5px] text-white/40 mt-0.5">
                  {sessions.length} transcript{sessions.length === 1 ? '' : 's'} and their recordings
                </p>
              </div>
            </div>

            <p className="text-[12.5px] text-white/70 leading-relaxed mt-3">
              This deletes every transcript and every saved recording right now. This can't be undone.
            </p>

            <div className="flex items-center justify-end gap-3 mt-6 pt-1">
              <button
                disabled={isClearingAll}
                onClick={() => setShowClearConfirm(false)}
                className="px-4 py-2 rounded-xl text-[12px] font-medium text-white/70 hover:text-white bg-white/[0.06] hover:bg-white/[0.1] border border-white/[0.08] transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                disabled={isClearingAll}
                onClick={() => void handleClearAll()}
                className="px-4 py-2 rounded-xl text-[12px] font-semibold text-white bg-red-500 hover:bg-red-600 transition-colors disabled:opacity-50"
              >
                {isClearingAll ? 'Clearing…' : 'Delete everything'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
