"""
Shared statistical helpers for benchmark analysis modules.

All functions are safe for empty inputs and zero denominators.
"""

from __future__ import annotations

import math


def safe_div(n: float, d: float) -> float:
    """Return n/d, or 0.0 when d is zero."""
    if d == 0:
        return 0.0
    return n / d


def percentile(values: list[float], pct: float) -> float:
    """Return the *pct* percentile (0-100) via linear interpolation.

    Returns 0.0 for an empty list.
    """
    if not values:
        return 0.0
    sorted_v = sorted(values)
    n = len(sorted_v)
    if n == 1:
        return float(sorted_v[0])
    k = (pct / 100.0) * (n - 1)
    f = math.floor(k)
    c = math.ceil(k)
    if f == c:
        return float(sorted_v[int(k)])
    return float(sorted_v[f] + (sorted_v[c] - sorted_v[f]) * (k - f))


def mean(values: list[float]) -> float:
    """Arithmetic mean. Returns 0.0 for an empty list."""
    if not values:
        return 0.0
    return sum(values) / len(values)


def point_biserial(binary: list[bool], continuous: list[float]) -> float:
    """Point-biserial correlation between a binary and a continuous variable.

    Uses the formula:
        r_pb = (M1 - M0) / S_n * sqrt(n1 * n0 / N^2)

    where M1/M0 are means of the continuous variable for True/False groups,
    S_n is the population std-dev, and n1/n0 are the group sizes.

    Returns 0.0 when computation is impossible (empty input, zero variance,
    or only one group present).
    """
    if len(binary) != len(continuous) or len(binary) < 2:
        return 0.0

    group1 = [c for b, c in zip(binary, continuous) if b]
    group0 = [c for b, c in zip(binary, continuous) if not b]

    n1 = len(group1)
    n0 = len(group0)
    if n1 == 0 or n0 == 0:
        return 0.0

    m1 = sum(group1) / n1
    m0 = sum(group0) / n0
    n = len(continuous)

    # Population standard deviation
    overall_mean = sum(continuous) / n
    variance = sum((x - overall_mean) ** 2 for x in continuous) / n
    if variance == 0:
        return 0.0
    sd = math.sqrt(variance)

    r = (m1 - m0) / sd * math.sqrt(n1 * n0 / (n * n))
    # Clamp to [-1, 1] for floating-point safety
    return max(-1.0, min(1.0, r))
