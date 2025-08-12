import math

def area_rectangle(w: float, h: float) -> float:
    return float(w) * float(h)

def perimeter_rectangle(w: float, h: float) -> float:
    return 2.0 * (float(w) + float(h))

def area_circle(r: float) -> float:
    return math.pi * float(r) ** 2

def circumference(r: float) -> float:
    return 2.0 * math.pi * float(r)

def area_triangle(base: float, height: float) -> float:
    return 0.5 * float(base) * float(height)