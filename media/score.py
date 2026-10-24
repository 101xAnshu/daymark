#!/usr/bin/env python3
"""Generate the original ambient score for the Daymark tour.

A slow, warm pad that changes chord every four seconds, with a quiet sub and a
little air. No samples, no downloads; regenerate with `python3 score.py`.
"""
import math
import wave

import numpy as np

SR = 48_000
DUR = 26.0
N = int(SR * DUR)

CHORDS = [
    # Fmaj9
    [87.31, 110.00, 130.81, 164.81, 196.00],
    # Cmaj9
    [65.41, 82.41, 98.00, 123.47, 146.83],
    # Am9
    [110.00, 130.81, 164.81, 196.00, 246.94],
    # G6/9
    [98.00, 123.47, 146.83, 164.81, 220.00],
]


def smooth(x):
    return x * x * (3.0 - 2.0 * x)


def chord_segment(freqs, seconds=4.9):
    n = int(SR * seconds)
    t = np.arange(n) / SR
    out = np.zeros((n, 2))
    root = freqs[0]
    for i, f in enumerate(freqs):
        vib = 1.0 + 0.0006 * np.sin(2 * math.pi * 0.11 * t + i)
        tone = np.zeros(n)
        for k, amp in ((1, 1.0), (2, 0.32), (3, 0.10), (4, 0.035)):
            tone += amp * np.sin(2 * math.pi * f * k * vib * t + i * 0.7)
        # a slightly detuned twin gives the pad width without chorus plugins
        tone += 0.5 * np.sin(2 * math.pi * f * 1.0031 * t + i)
        pan = 0.5 + 0.34 * math.sin(i * 2.1 + 0.4)
        out[:, 0] += tone * (1.0 - pan)
        out[:, 1] += tone * pan

    sub = np.sin(2 * math.pi * (root / 2) * t) * 0.55
    out[:, 0] += sub * 0.9
    out[:, 1] += sub * 0.9

    attack = int(SR * 1.35)
    release = int(SR * 1.35)
    env = np.ones(n)
    env[:attack] = smooth(np.linspace(0, 1, attack))
    env[-release:] = smooth(np.linspace(1, 0, release))
    out *= env[:, None]
    return out


def main():
    mix = np.zeros((N, 2))
    segments = CHORDS * 2
    for ci, chord in enumerate(segments):
        start = int(ci * 4.0 * SR)
        if start >= N:
            break
        seg = chord_segment(chord, 4.9)
        end = min(N, start + len(seg))
        mix[start:end] += seg[: end - start]

    # A whisper of air so the pad is not perfectly sterile.
    rng = np.random.default_rng(7)
    air = rng.normal(0, 1, N)
    # cheap one-pole low-pass
    for _ in range(2):
        air = np.convolve(air, [0.02, 0.96, 0.02], mode="same")
    mix[:, 0] += air * 0.006
    mix[:, 1] += np.roll(air, 977) * 0.006

    # Global fades and a soft top so it never clips.
    fade = int(SR * 1.2)
    mix[:fade] *= smooth(np.linspace(0, 1, fade))[:, None]
    mix[-int(SR * 1.8) :] *= smooth(np.linspace(1, 0, int(SR * 1.8)))[:, None]
    mix *= 0.86 / max(1e-9, np.max(np.abs(mix)))

    pcm = (np.clip(mix, -1, 1) * 32767).astype("<i2")
    with wave.open("public/score.wav", "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())
    print("wrote public/score.wav")


if __name__ == "__main__":
    main()
