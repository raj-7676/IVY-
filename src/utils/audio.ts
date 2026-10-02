// Web Audio API synthesizer for cinematic sound effects and ambient immersion

class SoundEngine {
  private ctx: AudioContext | null = null;
  private introMasterGain: GainNode | null = null;
  private activeIntroSources: (AudioNode & { stop?: (when?: number) => void })[] = [];

  private getContext(): AudioContext | null {
    if (typeof window === 'undefined') return null;
    if (!this.ctx) {
      const AudioCtx =
        window.AudioContext ||
        (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      if (AudioCtx) {
        // 'interactive' requests the smallest audio buffer the device
        // allows — the default hint picks a larger, power-saving buffer
        // that adds real output latency (tens of ms on Windows/WASAPI).
        this.ctx = new AudioCtx({ latencyHint: 'interactive' });
      }
    }
    if (this.ctx && this.ctx.state === 'suspended') {
      this.ctx.resume().catch(() => {});
    }
    return this.ctx;
  }

  // Creates and resumes the AudioContext ahead of time
  public warmUp() {
    this.getContext();
  }

  // Stops any running intro sounds smoothly (e.g. user presses Esc/Space or clicks to dismiss)
  public stopIntro() {
    if (!this.ctx || !this.introMasterGain) return;
    try {
      const now = this.ctx.currentTime;
      this.introMasterGain.gain.cancelScheduledValues(now);
      this.introMasterGain.gain.setValueAtTime(this.introMasterGain.gain.value, now);
      this.introMasterGain.gain.exponentialRampToValueAtTime(0.0001, now + 0.12);

      const toStop = [...this.activeIntroSources];
      setTimeout(() => {
        toStop.forEach((src) => {
          try {
            if ('stop' in src && typeof src.stop === 'function') {
              src.stop();
            }
            src.disconnect();
          } catch (_) {}
        });
      }, 140);
      this.activeIntroSources = [];
    } catch (_) {}
  }

  // Synthesizes the complete intro audio sequence, frame-synchronized to IvyLaunchIntro:
  // - 0.15s: Central 'V' Awakening Swell
  // - 1.15s: 'V' Assembly Complete & Acoustic Pulse Ring
  // - 1.35s - 2.05s: Lateral Wings 'I' & 'Y' Bloom + Seam Gleam
  // - 2.08s: The Landing Lock-in (Crisp Tactile Snap + Warm Resonant Bloom + Triad Chime)
  public playIntroSequence() {
    try {
      const ctx = this.getContext();
      if (!ctx) return;

      // Stop any prior intro sound
      this.stopIntro();

      const now = ctx.currentTime;
      const master = ctx.createGain();
      master.gain.setValueAtTime(0.88, now);
      master.connect(ctx.destination);
      this.introMasterGain = master;
      this.activeIntroSources = [];

      // Helper to register node for clean disposal
      const track = <T extends AudioNode>(node: T): T => {
        this.activeIntroSources.push(node as unknown as AudioNode & { stop?: (when?: number) => void });
        return node;
      };

      // ─── 1. T = 0.15s: Central 'V' Awakening Swell (0.15s - 1.15s) ───
      // Soft warm sine wave rising in pitch (110Hz A2 -> 165Hz E3) through a lowpass filter
      const swellOsc = track(ctx.createOscillator());
      const swellFilter = track(ctx.createBiquadFilter());
      const swellGain = track(ctx.createGain());

      swellOsc.type = 'sine';
      swellOsc.frequency.setValueAtTime(110, now + 0.15);
      swellOsc.frequency.exponentialRampToValueAtTime(165, now + 1.10);

      swellFilter.type = 'lowpass';
      swellFilter.frequency.setValueAtTime(180, now + 0.15);
      swellFilter.frequency.exponentialRampToValueAtTime(480, now + 1.10);

      swellGain.gain.setValueAtTime(0.0001, now + 0.15);
      swellGain.gain.linearRampToValueAtTime(0.09, now + 0.85);
      swellGain.gain.exponentialRampToValueAtTime(0.0001, now + 1.18);

      swellOsc.connect(swellFilter);
      swellFilter.connect(swellGain);
      swellGain.connect(master);

      swellOsc.start(now + 0.15);
      swellOsc.stop(now + 1.20);

      // ─── 2. T = 1.15s: 'V' Assembly Complete & Acoustic Pulse Ring Emission ───
      // A soft, glassy tactile pulse confirming 'V' has locked
      const pulseOsc = track(ctx.createOscillator());
      const pulseGain = track(ctx.createGain());

      pulseOsc.type = 'sine';
      pulseOsc.frequency.setValueAtTime(660, now + 1.15);
      pulseOsc.frequency.exponentialRampToValueAtTime(440, now + 1.27);

      pulseGain.gain.setValueAtTime(0.0001, now + 1.15);
      pulseGain.gain.linearRampToValueAtTime(0.12, now + 1.155);
      pulseGain.gain.exponentialRampToValueAtTime(0.0001, now + 1.28);

      pulseOsc.connect(pulseGain);
      pulseGain.connect(master);

      pulseOsc.start(now + 1.15);
      pulseOsc.stop(now + 1.30);

      // Soft noise pop accompanying the pulse ring
      const popCount = Math.floor(ctx.sampleRate * 0.08);
      const popBuf = ctx.createBuffer(1, popCount, ctx.sampleRate);
      const popData = popBuf.getChannelData(0);
      for (let i = 0; i < popCount; i++) {
        popData[i] = (Math.random() * 2 - 1) * Math.exp(-i / (ctx.sampleRate * 0.018));
      }
      const popSource = track(ctx.createBufferSource());
      popSource.buffer = popBuf;
      const popFilter = track(ctx.createBiquadFilter());
      popFilter.type = 'bandpass';
      popFilter.frequency.setValueAtTime(1200, now + 1.15);
      popFilter.Q.setValueAtTime(1.8, now + 1.15);
      const popGain = track(ctx.createGain());
      popGain.gain.setValueAtTime(0.045, now + 1.15);
      popGain.gain.exponentialRampToValueAtTime(0.0001, now + 1.23);

      popSource.connect(popFilter);
      popFilter.connect(popGain);
      popGain.connect(master);

      popSource.start(now + 1.15);
      popSource.stop(now + 1.25);

      // ─── 3. T = 1.35s - 2.05s: Lateral Wings 'I' & 'Y' Bloom + Seam Gleam ───
      // Dual harmonic sine rising subtly to build momentum directly into the landing
      const bloomOsc1 = track(ctx.createOscillator());
      const bloomOsc2 = track(ctx.createOscillator());
      const bloomGain = track(ctx.createGain());

      bloomOsc1.type = 'sine';
      bloomOsc1.frequency.setValueAtTime(330, now + 1.35); // E4 -> C5
      bloomOsc1.frequency.exponentialRampToValueAtTime(523.25, now + 2.05);

      bloomOsc2.type = 'sine';
      bloomOsc2.frequency.setValueAtTime(440, now + 1.45); // A4 -> E5
      bloomOsc2.frequency.exponentialRampToValueAtTime(659.25, now + 2.05);

      bloomGain.gain.setValueAtTime(0.0001, now + 1.35);
      bloomGain.gain.linearRampToValueAtTime(0.07, now + 1.95);
      bloomGain.gain.exponentialRampToValueAtTime(0.0001, now + 2.07);

      bloomOsc1.connect(bloomGain);
      bloomOsc2.connect(bloomGain);
      bloomGain.connect(master);

      bloomOsc1.start(now + 1.35);
      bloomOsc2.start(now + 1.45);
      bloomOsc1.stop(now + 2.08);
      bloomOsc2.stop(now + 2.08);

      // ─── 4. T = 2.08s: THE FINAL LOCK-IN IMPACT (The Landing) ───
      const tImpact = now + 2.08;

      // Layer 4A: Crisp Tactile Origami Snap (triangle pitch drop)
      const snapOsc = track(ctx.createOscillator());
      const snapGain = track(ctx.createGain());
      snapOsc.type = 'triangle';
      snapOsc.frequency.setValueAtTime(1450, tImpact);
      snapOsc.frequency.exponentialRampToValueAtTime(240, tImpact + 0.038);

      snapGain.gain.setValueAtTime(0.0001, tImpact);
      snapGain.gain.linearRampToValueAtTime(0.38, tImpact + 0.003);
      snapGain.gain.exponentialRampToValueAtTime(0.0001, tImpact + 0.09);

      snapOsc.connect(snapGain);
      snapGain.connect(master);
      snapOsc.start(tImpact);
      snapOsc.stop(tImpact + 0.10);

      // Tactile physical transient impulse
      const impulseCount = Math.floor(ctx.sampleRate * 0.06);
      const impulseBuf = ctx.createBuffer(1, impulseCount, ctx.sampleRate);
      const impulseData = impulseBuf.getChannelData(0);
      for (let i = 0; i < impulseCount; i++) {
        impulseData[i] = (Math.random() * 2 - 1) * Math.exp(-i / (ctx.sampleRate * 0.009));
      }
      const impulseSource = track(ctx.createBufferSource());
      impulseSource.buffer = impulseBuf;
      const impulseFilter = track(ctx.createBiquadFilter());
      impulseFilter.type = 'bandpass';
      impulseFilter.frequency.setValueAtTime(2600, tImpact);
      impulseFilter.Q.setValueAtTime(2.2, tImpact);
      const impulseGain = track(ctx.createGain());
      impulseGain.gain.setValueAtTime(0.18, tImpact);
      impulseGain.gain.exponentialRampToValueAtTime(0.0001, tImpact + 0.045);

      impulseSource.connect(impulseFilter);
      impulseFilter.connect(impulseGain);
      impulseGain.connect(master);
      impulseSource.start(tImpact);
      impulseSource.stop(tImpact + 0.06);

      // Layer 4B: Warm Resonant Bloom (Ivy's signature body)
      const bodyOsc = track(ctx.createOscillator());
      const bodyGain = track(ctx.createGain());
      bodyOsc.type = 'sine';
      bodyOsc.frequency.setValueAtTime(98, tImpact);
      bodyOsc.frequency.exponentialRampToValueAtTime(65, tImpact + 0.35);
      bodyOsc.frequency.exponentialRampToValueAtTime(45, tImpact + 1.25);

      bodyGain.gain.setValueAtTime(0.0001, tImpact);
      bodyGain.gain.linearRampToValueAtTime(0.48, tImpact + 0.012);
      bodyGain.gain.exponentialRampToValueAtTime(0.0001, tImpact + 1.35);

      bodyOsc.connect(bodyGain);
      bodyGain.connect(master);
      bodyOsc.start(tImpact);
      bodyOsc.stop(tImpact + 1.40);

      // Sub-harmonic warmth
      const subOsc = track(ctx.createOscillator());
      const subGain = track(ctx.createGain());
      subOsc.type = 'sine';
      subOsc.frequency.setValueAtTime(196, tImpact);
      subOsc.frequency.exponentialRampToValueAtTime(130, tImpact + 0.40);

      subGain.gain.setValueAtTime(0.0001, tImpact);
      subGain.gain.linearRampToValueAtTime(0.20, tImpact + 0.015);
      subGain.gain.exponentialRampToValueAtTime(0.0001, tImpact + 0.85);

      subOsc.connect(subGain);
      subGain.connect(master);
      subOsc.start(tImpact);
      subOsc.stop(tImpact + 0.90);

      // Layer 4C: Crystalline Gleam Harmonic Chime (C major triad shimmer)
      const chord = [
        { freq: 523.25, gain: 0.09, decay: 1.20 }, // C5
        { freq: 783.99, gain: 0.07, decay: 1.05 }, // G5
        { freq: 1046.5, gain: 0.05, decay: 0.90 }, // C6
      ];

      chord.forEach(({ freq, gain, decay }) => {
        const chimeOsc = track(ctx.createOscillator());
        const chimeGain = track(ctx.createGain());
        chimeOsc.type = 'sine';
        chimeOsc.frequency.setValueAtTime(freq, tImpact);

        chimeGain.gain.setValueAtTime(0.0001, tImpact);
        chimeGain.gain.linearRampToValueAtTime(gain, tImpact + 0.005);
        chimeGain.gain.exponentialRampToValueAtTime(0.0001, tImpact + decay);

        chimeOsc.connect(chimeGain);
        chimeGain.connect(master);
        chimeOsc.start(tImpact);
        chimeOsc.stop(tImpact + decay + 0.05);
      });
    } catch (_) {}
  }

  // Backwards compatibility alias
  public playCinematicThud() {
    this.playIntroSequence();
  }
}

export const soundEngine = new SoundEngine();
