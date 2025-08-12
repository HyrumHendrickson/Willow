"""Willow: student‑friendly math helpers."""

from .number_theory import gcd, lcm, prime_factors, is_prime
from .stats import mean, median, mode, data_range, variance, stdev
from .algebra import solve_linear, solve_quadratic, simplify
from .geometry import area_rectangle, perimeter_rectangle, area_circle, circumference, area_triangle

__all__ = [
    "gcd","lcm","prime_factors","is_prime",
    "mean","median","mode","data_range","variance","stdev",
    "solve_linear","solve_quadratic","simplify",
    "area_rectangle","perimeter_rectangle","area_circle","circumference","area_triangle",
]