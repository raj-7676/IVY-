# Generates src/assets/sounds/intro.ogg, the launch intro's sound. Usage: python scripts/intro-sound.py
# (needs numpy and ffmpeg on PATH). Every sound is synthesized here, no samples, so it's ours to ship.
#
# Timed to IvyLaunchIntro.tsx (each piece "lands" at its delay + 0.33 x its duration, where the ease curve
# reaches 90%): V pieces 0.43-0.98 s, I/Y pieces 1.60-1.96 s every 60 ms, logo complete + gleam 2.05 s,
# exit fade 2.65-3.55 s. Change those delays/durations and the times below must change with them.
#   - a soft rounded "fold" tap per piece (origami): V centred, I pieces left, Y pieces right
#   - the logo settles with one deeper fold and a warm low F chord
import os, subprocess
import numpy as np

SR, TOTAL = 48000, 3.6
N = int(TOTAL * SR)
T = np.arange(N) / SR
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "assets", "sounds", "intro.ogg")
rng = np.random.default_rng(11)

def hz(n):
    names = {"C": 0, "C#": 1, "D": 2, "D#": 3, "E": 4, "F": 5, "F#": 6, "G": 7, "G#": 8, "A": 9, "A#": 10, "B": 11}
    return 440 * 2 ** ((names[n[:-1]] + 12 * (int(n[-1]) + 1) - 69) / 12)

def fold(f0, dur=0.11):
    """Soft rounded tap: a low sine 'thock' that drops in pitch, plus a faint octave - no noise, no metal."""
    t = np.arange(int(dur * SR)) / SR
    f = f0 * (0.45 + 0.55 * np.exp(-t / 0.012))
    ph = 2 * np.pi * np.cumsum(f) / SR
    env = np.clip(t / 0.0015, 0, 1) * np.exp(-t / 0.028)
    return (np.sin(ph) + 0.18 * np.sin(2 * ph)) * env

class Mix:
    def __init__(self):
        self.buf = np.zeros((N, 2))
        self.send = np.zeros((N, 2))

    def add(self, x, at, gain, pan=0.0, verb=0.5):
        a = int(at * SR)
        x = x[:N - a] * gain
        l, r = np.cos((pan + 1) * np.pi / 4) * 1.414, np.sin((pan + 1) * np.pi / 4) * 1.414
        st = np.stack([x * l, x * r], 1)
        self.buf[a:a + len(x)] += st
        self.send[a:a + len(x)] += st * verb

def room(x, rt=1.3):
    """Warm small hall: dark decorrelated tail with a soft early-reflection cluster."""
    n = int(rt * SR)
    t = np.arange(n) / SR
    out = np.zeros_like(x)
    for ch in range(2):
        ir = rng.standard_normal(n) * np.exp(-6.9 * t / rt)
        for _ in range(3):
            ir = np.convolve(ir, np.ones(16) / 16, "same")  # dark, no fizz
        for d, g in ((0.011, 0.5), (0.017, 0.35), (0.023 + 0.004 * ch, 0.3)):
            ir[int(d * SR)] += g * np.abs(ir).max() * 6
        ir[: int(0.008 * SR)] = 0
        ir /= np.sqrt(np.sum(ir ** 2))
        size = len(x) + n
        out[:, ch] = np.fft.irfft(np.fft.rfft(x[:, ch], size) * np.fft.rfft(ir, size), size)[: len(x)]
    return out

mx = Mix()
LAND = 2.05
# V pieces: fold taps rising gently, centred with the wings slightly apart
for at, f, p in zip([0.43, 0.61, 0.68, 0.81, 0.91, 0.98], [520, 560, 580, 620, 650, 680],
                    [0.0, -0.25, 0.25, 0.0, -0.3, 0.3]):
    mx.add(fold(f), at, 0.32, pan=p, verb=0.35)
# I/Y pieces: quicker, brighter taps; I on the left, Y on the right
for i, at in enumerate([1.60, 1.66, 1.72, 1.78, 1.84, 1.90, 1.96]):
    mx.add(fold(720 + 25 * i), at, 0.26, pan=-0.7, verb=0.35)
    if i < 6:
        mx.add(fold(760 + 25 * i), at + 0.006, 0.26, pan=0.7, verb=0.35)
# Landing: one deeper fold as the logo settles, then a soft warm low chord
t = np.arange(int(1.6 * SR)) / SR
mx.add(fold(330, 0.2), LAND, 0.6, verb=0.4)
chord = sum(a * np.sin(2 * np.pi * hz(n) * t) for n, a in (("F2", 1.0), ("C3", 0.5), ("F3", 0.35), ("A3", 0.18)))
mx.add(chord * np.clip(t / 0.06, 0, 1) * np.exp(-t / 0.7), LAND, 0.3, verb=0.5)

out = mx.buf + room(mx.send) * 0.45
out *= np.clip((TOTAL - T) / 0.75, 0, 1)[:, None]  # follows the exit fade, silent by 3.6 s
pcm = (out / np.abs(out).max() * 0.9).astype(np.float32).tobytes()
subprocess.run(["ffmpeg", "-y", "-v", "error", "-f", "f32le", "-ar", str(SR), "-ac", "2", "-i", "-",
                "-af", "loudnorm=I=-19:TP=-1.5:LRA=11", "-ar", str(SR), "-c:a", "libopus", "-b:a", "160k", OUT],
               input=pcm, check=True)
print(OUT)
