import argparse, math, sys
from . import (
    gcd, lcm, prime_factors, is_prime,
    mean, median, mode, data_range, variance, stdev,
    solve_linear, solve_quadratic, simplify
)

def main(argv=None):
    parser = argparse.ArgumentParser(prog="willow", description="Student‑friendly math helper CLI.")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_calc = sub.add_parser("calc", help="Evaluate a simple Python arithmetic expression safely.")
    p_calc.add_argument("expr", help="Expression, e.g., '2 + 3*4 - 5'")

    p_gcd = sub.add_parser("gcd", help="Greatest common divisor of two integers.")
    p_gcd.add_argument("a", type=int); p_gcd.add_argument("b", type=int)

    p_lcm = sub.add_parser("lcm", help="Least common multiple of two integers.")
    p_lcm.add_argument("a", type=int); p_lcm.add_argument("b", type=int)

    p_pf = sub.add_parser("pf", help="Prime factors of an integer.")
    p_pf.add_argument("n", type=int)

    p_isprime = sub.add_parser("is-prime", help="Primality test.")
    p_isprime.add_argument("n", type=int)

    p_sl = sub.add_parser("solve-linear", help="Solve a*x + b = 0")
    p_sl.add_argument("a", type=float); p_sl.add_argument("b", type=float)

    p_sq = sub.add_parser("solve-quadratic", help="Solve a*x^2 + b*x + c = 0 (real roots only)")
    p_sq.add_argument("a", type=float); p_sq.add_argument("b", type=float); p_sq.add_argument("c", type=float)

    p_stats = sub.add_parser("stats", help="Compute mean, median, mode, range, variance, stdev for numbers.")
    p_stats.add_argument("values", nargs="+", type=float)

    p_simpl = sub.add_parser("simplify", help="Algebraic simplification (requires sympy).")
    p_simpl.add_argument("expr", help="Expression like '2*x + 3*x - 5'")

    args = parser.parse_args(argv)

    if args.cmd == "calc":
        # Very restricted eval for arithmetic only
        allowed_names = {k: getattr(math, k) for k in dir(math) if not k.startswith("_")}
        allowed_names.update({"__builtins__": {}})
        try:
            val = eval(args.expr, allowed_names, {})
        except Exception as e:
            print(f"Error: {e}", file=sys.stderr); return 1
        print(val); return 0

    if args.cmd == "gcd":
        print(gcd(args.a, args.b)); return 0

    if args.cmd == "lcm":
        print(lcm(args.a, args.b)); return 0

    if args.cmd == "pf":
        print(" ".join(map(str, prime_factors(args.n)))); return 0

    if args.cmd == "is-prime":
        print("true" if is_prime(args.n) else "false"); return 0

    if args.cmd == "solve-linear":
        try:
            print(solve_linear(args.a, args.b))
        except Exception as e:
            print(f"Error: {e}", file=sys.stderr); return 1
        return 0

    if args.cmd == "solve-quadratic":
        try:
            r1, r2 = solve_quadratic(args.a, args.b, args.c)
            print(f"{r1} {r2}")
        except Exception as e:
            print(f"Error: {e}", file=sys.stderr); return 1
        return 0

    if args.cmd == "stats":
        vals = args.values
        print(f"mean={mean(vals)} median={median(vals)} mode={mode(vals)} "
              f"range={data_range(vals)} var={variance(vals)} stdev={stdev(vals)}")
        return 0

    if args.cmd == "simplify":
        print(simplify(args.expr)); return 0

    parser.print_help(); return 0

if __name__ == "__main__":
    raise SystemExit(main())