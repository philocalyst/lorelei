#!/usr/bin/env python3
"""Model the scalar grayscale tent step and compare with a discrete Gaussian."""
import math

WIDTH = 321
Q15 = 32768


def clamp_sample(row, index):
    return row[max(0, min(len(row) - 1, index))]


def downsample(row):
    taps = (1, 3, 3, 1)
    return [
        (sum(w * clamp_sample(row, 2 * x + i - 1) for i, w in enumerate(taps)) + 4) // 8
        for x in range((len(row) + 1) // 2)
    ]


def q15(gray):
    return 128 * gray + gray // 2 + gray // 128


def reconstruct(row, level, x):
    denominator = 2 << level
    index, rem = divmod(2 * x + 1 - (1 << level), denominator)
    left = q15(clamp_sample(row, index))
    right = q15(clamp_sample(row, index + 1))
    return (left * (denominator - rem) + right * rem) // denominator


def levels(sigma):
    base = 330
    q = round(sigma * 256)
    if q < base:
        return 0, q * Q15 // base
    level = 1
    while level < 5 and q >= 2 * base:
        level += 1
        base *= 2
    return level, 64 * ((q - base) * 512 // base) if level < 5 else 0


def measure(sigma):
    level, fraction = levels(sigma)
    radius = math.ceil(3 * sigma)
    support = math.ceil(8 * sigma)
    weights = [(d, math.exp(-0.5 * (d / sigma)**2)) for d in range(-support, support + 1)]
    mass = sum(w for _, w in weights)
    errors = []
    for offset in (0, 1, 3, 7, 15, 23, 31):
        edge = 144 + offset
        pyramid = [[255 * (x >= edge) for x in range(WIDTH)]]
        for _ in range(level + bool(fraction)):
            pyramid.append(downsample(pyramid[-1]))
        for x in range(edge - radius, edge + radius):
            sample = reconstruct(pyramid[level], level, x)
            if fraction:
                higher = reconstruct(pyramid[level + 1], level + 1, x)
                sample = (sample * (Q15 - fraction) + Q15 // 2) // Q15 + (higher * fraction + Q15 // 2) // Q15
            gray = (sample * 255 + Q15 // 2) // Q15
            gaussian = 255 * sum(w for dx, w in weights if x + dx >= edge) / mass
            errors.append(gray - gaussian)
    return len(errors), max(map(abs, errors)), math.sqrt(sum(e * e for e in errors) / len(errors))


if __name__ == "__main__":
    print("# Source-derived scalar model for an opaque vertical step. Renderer comparison pending.")
    print("sigma_px\tsamples\tmax_byte_error\trms_byte_error")
    for s in (0.5, 1.2890625, 3.0, 6.0, 12.0, 24.0, 40.0):
        n, maximum, rms = measure(s)
        print(f"{s:.6f}\t{n}\t{maximum:.4f}\t{rms:.4f}")
