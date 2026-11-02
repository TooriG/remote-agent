"""Writes the short interface sounds placed on the film's beats.

    python3 scripts/sfx.py public/sfx
Standard library only: tap (finger on glass), pop (a pill appears), whoosh (camera move).
"""

import math
import pathlib
import random
import struct
import sys
import wave

RATE = 44100
random.seed(3)


def write(path, samples):
    peak = max(abs(x) for x in samples) or 1
    with wave.open(str(path), 'wb') as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(RATE)
        out.writeframes(b''.join(struct.pack('<h', int(x / peak * 0.9 * 32767)) for x in samples))


def tap():
    n = int(0.05 * RATE)
    return [math.sin(2 * math.pi * 2300 * i / RATE) * math.exp(-i / RATE * 140) + 0.3 * (random.random() * 2 - 1) * math.exp(-i / RATE * 600) for i in range(n)]


def pop():
    n = int(0.16 * RATE)
    phase = 0.0
    out = []
    for i in range(n):
        t = i / RATE
        phase += 2 * math.pi * (520 + 900 * min(1, t / 0.05)) / RATE
        out.append(math.sin(phase) * min(1, t / 0.004) * math.exp(-t * 30))
    return out


def whoosh():
    n = int(0.6 * RATE)
    state = 0.0
    out = []
    for i in range(n):
        p = i / n
        a = 1 - math.exp(-2 * math.pi * (300 + 3500 * math.sin(math.pi * p)) / RATE)
        state += a * ((random.random() * 2 - 1) - state)
        out.append(state * math.sin(math.pi * p) ** 2)
    return out


directory = pathlib.Path(sys.argv[1])
directory.mkdir(parents=True, exist_ok=True)
for name, make in (('tap', tap), ('pop', pop), ('whoosh', whoosh)):
    write(directory / f'{name}.wav', make())
print(f'wrote tap, pop and whoosh to {directory}')
