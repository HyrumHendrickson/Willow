from math import prod, isqrt
from typing import List

def gcd(a: int, b: int) -> int:
    """Greatest common divisor using Euclid's algorithm."""
    a, b = abs(int(a)), abs(int(b))
    while b:
        a, b = b, a % b
    return a

def lcm(a: int, b: int) -> int:
    """Least common multiple."""
    if a == 0 or b == 0:
        return 0
    return abs(a*b) // gcd(a, b)

def is_prime(n: int) -> bool:
    """Primality test for non‑negative integers."""
    n = int(n)
    if n < 2: return False
    if n % 2 == 0: return n == 2
    if n % 3 == 0: return n == 3
    r = isqrt(n)
    f = 5
    while f <= r:
        if n % f == 0 or n % (f+2) == 0:
            return False
        f += 6
    return True

def prime_factors(n: int) -> List[int]:
    """Return prime factors of n in non‑decreasing order."""
    n = abs(int(n))
    factors = []
    # factor 2s
    while n % 2 == 0 and n > 0:
        factors.append(2)
        n //= 2
    # odd factors
    f = 3
    while f * f <= n:
        while n % f == 0:
            factors.append(f)
            n //= f
        f += 2
    if n > 1:
        factors.append(n)
    return factors