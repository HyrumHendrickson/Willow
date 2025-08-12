from typing import Tuple, Optional

def solve_linear(a: float, b: float) -> float:
    """Solve a*x + b = 0 for x.

    Raises:
        ValueError: if a == 0 and b != 0 (no solution) or both 0 (infinitely many solutions).
    """
    a = float(a)
    b = float(b)
    if a == 0.0 and b == 0.0:
        raise ValueError("Infinitely many solutions (0*x + 0 = 0).")
    if a == 0.0:
        raise ValueError("No solution (0*x + b = 0 with b != 0).")
    return -b / a

def solve_quadratic(a: float, b: float, c: float) -> Tuple[float, float]:
    """Solve a*x^2 + b*x + c = 0; returns real roots if discriminant >= 0.

    Raises:
        ValueError: if a == 0 (not quadratic) or discriminant < 0 (no real roots).
    """
    a = float(a); b = float(b); c = float(c)
    if a == 0.0:
        raise ValueError("Not a quadratic (a must be nonzero).")
    disc = b*b - 4*a*c
    if disc < 0:
        raise ValueError("No real roots (discriminant < 0).")
    import math
    sqrt_disc = math.sqrt(disc)
    return ((-b - sqrt_disc) / (2*a), (-b + sqrt_disc) / (2*a))

def simplify(expr: str) -> str:
    """Optionally simplify an algebraic expression using sympy if available.
    Falls back to returning the original string when sympy is not installed.
    """
    try:
        import sympy as sp
        return str(sp.simplify(expr))
    except Exception:
        return expr