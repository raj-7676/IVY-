// Plays the launch intro's sound: one file synthesized note-for-note on IvyLaunchIntro's beats
// (V pieces land 0.43-0.98 s, I/Y pieces 1.60-1.97 s, landing 2.05 s, exit fade 2.65 s).
// Made by scripts/intro-sound.py; change the animation's timing and that script must change with it.
import INTRO_OGG from '../assets/sounds/intro.ogg';
// The same sound as AAC for a Mac: WebKit before Safari 18.4 can't decode Ogg Opus.
import INTRO_M4A from '../assets/sounds/intro.m4a';
import { IS_MAC } from './platform';

const INTRO_URL = IS_MAC ? INTRO_M4A : INTRO_OGG;

class SoundEngine {
  private ctx: AudioContext | null = null;
  private introBuffer: Promise<AudioBuffer | null> | null = null;
  private introGain: GainNode | null = null;
  private introSource: AudioBufferSourceNode | null = null;
  private introRun = 0;

  private getContext(): AudioContext | null {
    if (typeof window === 'undefined' || !window.AudioContext) return null;
    // 'interactive' asks for the smallest output buffer, so the sound isn't late on Windows/WASAPI.
    this.ctx ??= new AudioContext({ latencyHint: 'interactive' });
    if (this.ctx.state === 'suspended') this.ctx.resume().catch(() => {});
    return this.ctx;
  }

  private loadIntro(ctx: AudioContext): Promise<AudioBuffer | null> {
    this.introBuffer ??= fetch(INTRO_URL)
      .then((r) => r.arrayBuffer())
      .then((b) => ctx.decodeAudioData(b))
      .catch(() => null);
    return this.introBuffer;
  }

  // Creates the AudioContext and decodes the intro sound before the intro mounts.
  public warmUp() {
    const ctx = this.getContext();
    if (ctx) this.loadIntro(ctx);
  }

  // Fades out the intro sound (Esc/Space/click dismiss, or the intro unmounting).
  public stopIntro() {
    this.introRun++;
    const ctx = this.ctx;
    const gain = this.introGain;
    const src = this.introSource;
    this.introGain = null;
    this.introSource = null;
    if (!ctx || !gain || !src) return;
    const now = ctx.currentTime;
    gain.gain.setValueAtTime(gain.gain.value, now);
    gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.12);
    try {
      src.stop(now + 0.14);
    } catch {}
  }

  public playIntroSequence() {
    const ctx = this.getContext();
    if (!ctx) return;
    this.stopIntro();
    const run = this.introRun;
    const startedAt = performance.now();
    Promise.all([this.loadIntro(ctx), ctx.resume().catch(() => {})]).then(([buf]) => {
      if (!buf || run !== this.introRun) return;
      // Skip ahead by however long decoding/resuming took since the animation started, plus the device's
      // output delay (large on Bluetooth), so what you hear lands on the beats you see.
      const offset =
        (performance.now() - startedAt) / 1000 + (ctx.outputLatency || 0) + (ctx.baseLatency || 0);
      if (offset >= buf.duration) return;
      const gain = ctx.createGain();
      gain.connect(ctx.destination);
      const src = ctx.createBufferSource();
      src.buffer = buf;
      src.connect(gain);
      src.start(0, offset);
      this.introGain = gain;
      this.introSource = src;
    });
  }
}

export const soundEngine = new SoundEngine();
