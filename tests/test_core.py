import math
from willow import gcd, lcm, is_prime, prime_factors, mean, median, mode, solve_linear, solve_quadratic

def test_gcd_lcm():
    assert gcd(84, 36) == 12
    assert lcm(12, 18) == 36

def test_primes():
    assert is_prime(2)
    assert is_prime(97)
    assert not is_prime(1)
    assert prime_factors(84) == [2,2,3,7]

def test_stats():
    assert mean([1,2,3,4]) == 2.5
    assert median([1,3,2,4]) == 2.5
    assert mode([1,2,2,3,3]) in (2,3)

def test_algebra():
    assert solve_linear(2, -8) == 4.0
    r1, r2 = solve_quadratic(1, 0, -9)
    assert math.isclose(min(r1, r2), -3.0) and math.isclose(max(r1, r2), 3.0)

