# Willow

Willow is a beginner‑friendly Python package that helps students with math concepts.
It offers clean, well‑documented functions for arithmetic, number theory, algebra,
geometry, and basic statistics—plus a small CLI for quick calculations.

## Installation

```bash
pip install -e .
# or build a wheel:
# pipx install build
# python -m build
```

Optional symbolic features require `sympy`:

```bash
pip install "willow[sym]"
```

## Usage (Python)

```python
from willow import mean, solve_linear, solve_quadratic

print(mean([1,2,3,4]))              # 2.5
print(solve_linear(2, -8))          # x = 4.0
print(solve_quadratic(1, 0, -9))    # roots: (-3.0, 3.0)
```

## CLI

The package installs a command `willow` with subcommands:

```bash
willow calc "2 + 3*4 - 5"            # 9
willow gcd 84 36                      # 12
willow lcm 12 18                      # 36
willow solve-linear 2 -8              # 4.0
willow solve-quadratic 1 0 -9         # -3.0 3.0
willow stats 1 2 2 3 4                # mean/median/mode
```

Run `willow --help` or `willow <subcommand> --help` for details.

## Features

- Safe integer/fraction arithmetic helpers
- Number theory: prime checks, prime factors, gcd/lcm
- Algebra: linear and quadratic solvers
- Geometry: common areas/perimeters (triangles, rectangles, circles)
- Statistics: mean, median, mode, range, variance, stdev
- Optional symbolic simplification when `sympy` is installed
- Friendly CLI for quick problems

## License

MIT