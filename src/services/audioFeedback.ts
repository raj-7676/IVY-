// Web Audio synthesizer for tactile acoustic feedback in Ivy
class AudioFeedbackService {
  private audioCtx: AudioContext | null = null;

  private initCtx() {
    if (!this.audioCtx && typeof window !== 'undefined') {
      const AudioContextClass = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      if (AudioContextClass) {
        this.audioCtx = new AudioContextClass();
      }
    }
    if (this.audioCtx && this.audioCtx.state === 'suspended') {
      this.audioCtx.resume();
    }
  }

  // Picked by Yash from 15 auditioned candidates ("01-soft-pop") — capsule
  // overlay open / Alt+Space hotkey-down cue.
  playStartBeep() {
    try {
      this.initCtx();
      if (!this.audioCtx) return;
      const now = this.audioCtx.currentTime;
      const osc = this.audioCtx.createOscillator();
      const gain = this.audioCtx.createGain();
      osc.type = 'sine';
      osc.frequency.setValueAtTime(460, now);
      osc.frequency.exponentialRampToValueAtTime(5660, now + 0.09);

      gain.gain.setValueAtTime(0.0001, now);
      gain.gain.linearRampToValueAtTime(0.22, now + 0.004);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.13);

      osc.connect(gain);
      gain.connect(this.audioCtx.destination);
      osc.start(now);
      osc.stop(now + 0.14);
    } catch (_) {}
  }

  playStopBeep() {
    try {
      this.initCtx();
      if (!this.audioCtx) return;
      const osc = this.audioCtx.createOscillator();
      const gain = this.audioCtx.createGain();
      osc.type = 'sine';
      osc.frequency.setValueAtTime(720, this.audioCtx.currentTime);
      osc.frequency.exponentialRampToValueAtTime(420, this.audioCtx.currentTime + 0.09);

      gain.gain.setValueAtTime(0.05, this.audioCtx.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.001, this.audioCtx.currentTime + 0.1);

      osc.connect(gain);
      gain.connect(this.audioCtx.destination);
      osc.start();
      osc.stop(this.audioCtx.currentTime + 0.1);
    } catch (_) {}
  }

  // Picked by Yash from 15 auditioned candidates ("04-marimba-pluck") —
  // fires once a dictation actually pastes or copies (never on failure).
  playSuccessChime() {
    try {
      this.initCtx();
      if (!this.audioCtx) return;
      const now = this.audioCtx.currentTime;

      const body = this.audioCtx.createOscillator();
      const bodyGain = this.audioCtx.createGain();
      body.type = 'sine';
      body.frequency.setValueAtTime(660, now);
      bodyGain.gain.setValueAtTime(0.0001, now);
      bodyGain.gain.linearRampToValueAtTime(0.24, now + 0.002);
      bodyGain.gain.exponentialRampToValueAtTime(0.0001, now + 0.24);
      body.connect(bodyGain);
      bodyGain.connect(this.audioCtx.destination);
      body.start(now);
      body.stop(now + 0.28);

      const shimmer = this.audioCtx.createOscillator();
      const shimmerGain = this.audioCtx.createGain();
      shimmer.type = 'sine';
      shimmer.frequency.setValueAtTime(1320, now);
      shimmerGain.gain.setValueAtTime(0.0001, now);
      shimmerGain.gain.linearRampToValueAtTime(0.08, now + 0.002);
      shimmerGain.gain.exponentialRampToValueAtTime(0.0001, now + 0.1);
      shimmer.connect(shimmerGain);
      shimmerGain.connect(this.audioCtx.destination);
      shimmer.start(now);
      shimmer.stop(now + 0.12);
    } catch (_) {}
  }
}

export const audioFeedback = new AudioFeedbackService();

// Real-time microphone audio visualizer analyzer
export class MicrophoneAnalyzer {
  private audioCtx: AudioContext | null = null;
  private analyser: AnalyserNode | null = null;
  private mediaStream: MediaStream | null = null;
  private dataArray: Uint8Array<ArrayBuffer> | null = null;
  private animationFrameId: number | null = null;
  private onLevelUpdate: ((level: number, frequencyBars: number[]) => void) | null = null;
  private listeners: Set<(level: number, frequencyBars: number[]) => void> = new Set();
  // `stopListening()` can't cancel the in-flight `getUserMedia` promise
  // below (no AbortController on this browser API), so it sets this
  // instead — checked right after the await resolves. Without it, a quick
  // tap/release faster than the permission prompt/device-open latency
  // found `stopListening` a real no-op (nothing to stop yet), and the
  // stream that showed up moments later was never torn down: the mic
  // stayed open (Windows' "in use" indicator stuck lit) with no reference
  // left to stop it, and an orphaned rAF loop kept animating a UI that had
  // already moved on.
  private stopped = false;

  subscribe(callback: (level: number, frequencyBars: number[]) => void): () => void {
    this.listeners.add(callback);
    return () => {
      this.listeners.delete(callback);
    };
  }

  async startListening(onLevelUpdate?: (level: number, frequencyBars: number[]) => void): Promise<boolean> {
    // A re-entrant start (caller didn't await/await-then-discard the
    // previous call) must not orphan whatever the first call already
    // opened.
    this.stopListening();
    this.stopped = false;
    this.onLevelUpdate = onLevelUpdate || null;
    try {
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        return false;
      }
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
        },
      });
      if (this.stopped) {
        stream.getTracks().forEach((track) => track.stop());
        return false;
      }
      this.mediaStream = stream;

      const AudioContextClass = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      this.audioCtx = new AudioContextClass();
      const source = this.audioCtx.createMediaStreamSource(this.mediaStream);
      this.analyser = this.audioCtx.createAnalyser();
      this.analyser.fftSize = 64;
      source.connect(this.analyser);

      const bufferLength = this.analyser.frequencyBinCount;
      this.dataArray = new Uint8Array(bufferLength);

      const updateLoop = () => {
        if (!this.analyser || !this.dataArray) return;
        this.analyser.getByteFrequencyData(this.dataArray);

        let sum = 0;
        const bars: number[] = [];
        const step = Math.max(1, Math.floor(this.dataArray.length / 9));

        for (let i = 0; i < 9; i++) {
          const val = this.dataArray[i * step] || 0;
          bars.push(Math.min(1, Math.max(0.15, val / 255)));
          sum += val;
        }

        const avg = sum / (this.dataArray.length * 255);
        if (this.onLevelUpdate) {
          this.onLevelUpdate(avg, bars);
        }
        for (const listener of this.listeners) {
          listener(avg, bars);
        }

        this.animationFrameId = requestAnimationFrame(updateLoop);
      };

      updateLoop();
      return true;
    } catch (err) {
      console.warn('Microphone permission not granted or mic unavailable:', err);
      return false;
    }
  }

  stopListening() {
    this.stopped = true;
    this.listeners.clear();
    if (this.animationFrameId) {
      cancelAnimationFrame(this.animationFrameId);
      this.animationFrameId = null;
    }
    if (this.mediaStream) {
      this.mediaStream.getTracks().forEach((track) => track.stop());
      this.mediaStream = null;
    }
    if (this.audioCtx) {
      this.audioCtx.close().catch(() => {});
      this.audioCtx = null;
    }
    this.analyser = null;
    this.dataArray = null;
  }
}
