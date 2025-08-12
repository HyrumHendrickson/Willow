from typing import Iterable, List, Tuple
import math
from collections import Counter

def _to_list(values: Iterable[float]) -> List[float]:
    lst = [float(v) for v in values]
    if not lst:
        raise ValueError("values must not be empty")
    return lst

def mean(values: Iterable[float]) -> float:
    vals = _to_list(values)
    return sum(vals) / len(vals)

def median(values: Iterable[float]) -> float:
    vals = sorted(_to_list(values))
    n = len(vals)
    mid = n // 2
    if n % 2 == 1:
        return vals[mid]
    return (vals[mid-1] + vals[mid]) / 2.0

def mode(values: Iterable[float]) -> float:
    vals = _to_list(values)
    counts = Counter(vals).most_common()
    # In case of ties, return the smallest modal value for determinism
    top_count = counts[0][1]
    modes = [v for v,c in counts if c == top_count]
    return min(modes)

def data_range(values: Iterable[float]) -> float:
    vals = _to_list(values)
    return max(vals) - min(vals)

def variance(values: Iterable[float], sample: bool = False) -> float:
    vals = _to_list(values)
    mu = mean(vals)
    n = len(vals)
    if sample and n < 2:
        raise ValueError("sample variance requires at least two values")
    denom = n - 1 if sample else n
    return sum((x - mu) ** 2 for x in vals) / denom

def stdev(values: Iterable[float], sample: bool = False) -> float:
    return math.sqrt(variance(values, sample=sample))