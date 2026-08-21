#!/usr/bin/env python3
"""perfpair — the interleaved-pair measurement protocol (plan §7.6 S1/S2), as a tool.

Every prior session rebuilt this by hand. Phase 4a is the checkpoint phase — it
re-measures eight ledger rows, two parked families, and a pair per swap — so the
protocol is worth having as an instrument rather than as a habit.

What it enforces, because these are the rules sessions kept re-learning:

  S1  Never compare sequential runs of the same binary; they drift ~3%. Both
      binaries stay on disk and alternate inside ONE loop, medians over N pairs.
  S2  A null run (same binary in both slots) is how you learn this session's
      floor. `null` is a first-class subcommand, not something to remember.
  S17 FFMPEG must be set or the encoder bench measures the frame-skip path
      instead of encoding. Missing ffmpeg is a loud refusal, never a quiet skip.

Usage
-----
    perfpair.py build <label> [--ref <git-ref>]   build both benches, stash them
    perfpair.py run   <A> <B> [--pairs N]         interleave A vs B
    perfpair.py null  <label> [--pairs N]         same binary both slots: the floor
    perfpair.py list                              show stashed labels

A "row" is one bench stream (decode) or one stream x thread-count (encode).
Deltas are B relative to A: positive means B is slower.

`Spatial Ramps` is tagged EXCLUDED and kept out of every summary statistic (S2):
it has moved ±38%, -11% and +226% between runs of one binary. It is still
printed, because gradient content is the per-block-scaffolding path at its
purest and Phase 4a's checkpoint wants the datapoint.

The bench binaries are run directly rather than through `cargo bench`. Whether
the C++ dylib is found only changes which columns the bench prints; the parser
handles both, and the comparison here is Rust-A vs Rust-B either way.
Byte-exactness is gates.sh's job, not this script's.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CRATE = ROOT / "rust" / "crates" / "openh264-rs"
STASH = ROOT / ".perfpair"
BENCHES = ("decode_1080p_bench", "c_vs_rust_bench")
EXCLUDED = "Spatial Ramps"


def die(msg: str) -> "typing.NoReturn":  # noqa: F821
    print(f"perfpair: {msg}", file=sys.stderr)
    raise SystemExit(1)


def git(*args: str, check: bool = True) -> str:
    r = subprocess.run(
        ["git", "-C", str(ROOT), *args], capture_output=True, text=True
    )
    if check and r.returncode != 0:
        die(f"git {' '.join(args)} failed: {r.stderr.strip()}")
    return r.stdout.strip()


# --------------------------------------------------------------------- parsing
#
# Both parsers work by regex over the whole line rather than by field offset.
# That is deliberate: these benches pad numbers to a fixed column width, so
# "( 6.202 ms)" loses its space and becomes "(10.658 ms)" as soon as a value
# needs an extra digit. Offset-based parsing then drops precisely the slowest
# rows while still printing a plausible table — which is exactly the way S3 says
# a microbenchmark lies to you.

_DECODE_NAME = re.compile(r"^ ([A-Z].*?)\s*$")
_DECODE_MS = re.compile(r"Rust\s*:.*?([0-9]+\.[0-9]+)\s*ms/frame")
_ENCODE_NAME = re.compile(r"^ (\d+x\d+ \(.*?\))")
# **F74, T7.C9.** This was `\[(\d+) thread\]` — a literal `]` straight after the
# count — and `BENCH_SLICE_MODE` (T7.B0, F68's knob) makes the bench print
# `[1 thread sm=1 n=4]`. So from the moment the slice-mode axis existed, this parser
# matched **nothing** on any multi-slice run and `report()` printed an empty encode
# table without a word. The axis label is captured now and travels into the row key,
# which is also what makes `sm=1 n=4` and `sm=3` separate rows rather than one.
_ENCODE_MS = re.compile(
    r"\[(\d+) thread([^\]]*)\].*?Rust:\s*[0-9.]+\s*fps\s*\(\s*([0-9]+\.[0-9]+)\s*ms\)"
)


def parse_decode(text: str) -> dict[str, float]:
    rows: dict[str, float] = {}
    name = None
    for line in text.splitlines():
        if "source" in line or line.startswith(" C++ library"):
            continue
        m = _DECODE_MS.search(line)
        if m and name:
            rows[name] = float(m.group(1))
            continue
        m = _DECODE_NAME.match(line)
        if m:
            name = m.group(1)
    return rows


def parse_encode(text: str) -> dict[str, float]:
    rows: dict[str, float] = {}
    name = None
    for line in text.splitlines():
        m = _ENCODE_NAME.match(line)
        if m:
            name = m.group(1)
            continue
        m = _ENCODE_MS.search(line)
        if m and name:
            axis = m.group(2).strip()
            label = f"{m.group(1)}t {axis}" if axis else f"{m.group(1)}t"
            rows[f"{name} [{label}]"] = float(m.group(3))
    if not rows:
        # A parser that matches nothing must be loud — the same rule S17 applies to a
        # missing ffmpeg and gates.sh applies to a Miri step that runs zero tests. An
        # empty encode table is indistinguishable from a green one in the report, and
        # F74 is what that cost: every span since the slice-mode axis landed would
        # have measured the decoder only.
        die(
            "the encoder bench produced no parseable rows — the bench ran but "
            "`_ENCODE_MS` matched nothing. The encoder is UNMEASURED; fix the parser "
            "rather than reading the decode table alone (F74)."
        )
    return rows


# ---------------------------------------------------------------------- build


def target_dir() -> Path:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=CRATE, capture_output=True, text=True,
    )
    if out.returncode != 0:
        die("cargo metadata failed")
    return Path(json.loads(out.stdout)["target_directory"])


def cmd_build(label: str, ref: str | None) -> None:
    dest = STASH / label
    restore = None

    if ref:
        if git("rev-parse", "--verify", f"{ref}^{{commit}}", check=False) == "":
            die(f"not a commit: {ref}")
        if git("status", "--porcelain"):
            die("working tree is dirty; commit or stash before building from a ref")
        restore = git("rev-parse", "--abbrev-ref", "HEAD")
        if restore == "HEAD":
            restore = git("rev-parse", "HEAD")
        print(f"perfpair: checking out {ref} (will return to {restore})")
        git("checkout", "-q", ref)

    try:
        print(f"perfpair: building bench binaries for {label!r}...")
        r = subprocess.run(
            ["cargo", "build", "--release", "--benches"], cwd=CRATE,
            capture_output=True, text=True,
        )
        if r.returncode != 0:
            print(r.stderr[-3000:], file=sys.stderr)
            die(f"build for {label!r} failed")

        dest.mkdir(parents=True, exist_ok=True)
        deps = target_dir() / "release" / "deps"
        for bench in BENCHES:
            cands = [
                p for p in deps.glob(f"{bench}-*")
                if p.is_file() and p.suffix != ".d" and os.access(p, os.X_OK)
            ]
            if not cands:
                die(f"could not locate a built binary for {bench}")
            newest = max(cands, key=lambda p: p.stat().st_mtime)
            shutil.copy2(newest, dest / bench)
        (dest / "commit.txt").write_text(git("rev-parse", "--short", "HEAD") + "\n")
    finally:
        if restore:
            git("checkout", "-q", restore)

    print(f"perfpair: stashed {label!r} ({(dest / 'commit.txt').read_text().strip()}) in {dest}")


def cmd_list() -> None:
    if not STASH.exists():
        print("  (nothing stashed)")
        return
    for d in sorted(STASH.iterdir()):
        if d.is_dir():
            commit = (d / "commit.txt")
            print(f"  {d.name:<24} {commit.read_text().strip() if commit.exists() else '?'}")


# ------------------------------------------------------------------ execution


def resolve_ffmpeg() -> str | None:
    ff = os.environ.get("FFMPEG")
    if ff:
        return ff
    found = shutil.which("ffmpeg")
    if found:
        print(f"perfpair: FFMPEG was unset; using {found} from PATH")
        return found
    return None


def run_bench(binary: Path, which: str, ffmpeg: str | None) -> dict[str, float]:
    env = dict(os.environ)
    if which == "encode":
        env["FFMPEG"] = ffmpeg or ""
        env["BENCH_REQUIRE_FFMPEG"] = "1"
    r = subprocess.run(
        [str(binary), "--bench"], capture_output=True, text=True, env=env, cwd=ROOT
    )
    if r.returncode != 0:
        die(f"{binary.name} exited {r.returncode}\n{r.stderr[-2000:]}")
    return parse_decode(r.stdout) if which == "decode" else parse_encode(r.stdout)


def report(which: str, a_runs: list[dict[str, float]], b_runs: list[dict[str, float]]) -> None:
    keys = list(a_runs[0])
    for run in a_runs[1:] + b_runs:
        for k in run:
            if k not in keys:
                keys.append(k)

    print()
    print(f"{'row':<52}{'A (ms)':>10}{'B (ms)':>10}{'delta':>10}")
    print(f"{'-' * 52}{'-' * 10:>10}{'-' * 10:>10}{'-' * 10:>10}")

    deltas: list[float] = []
    missing: list[str] = []
    for key in keys:
        av = [r[key] for r in a_runs if key in r]
        bv = [r[key] for r in b_runs if key in r]
        if len(av) != len(a_runs) or len(bv) != len(b_runs):
            missing.append(key)
        if not av or not bv:
            continue
        ma, mb = statistics.median(av), statistics.median(bv)
        if ma <= 0:
            continue
        d = (mb - ma) / ma * 100.0
        excl = EXCLUDED in key
        print(f"{key:<52}{ma:>10.4f}{mb:>10.4f}{d:>+9.2f}%" + ("  EXCLUDED" if excl else ""))
        if not excl:
            deltas.append(d)

    if deltas:
        deltas.sort()
        print(
            f"\n  rows {len(deltas)}   median {statistics.median(deltas):+.2f}%"
            f"   min {deltas[0]:+.2f}%   max {deltas[-1]:+.2f}%   ({EXCLUDED} excluded)"
        )
        print(f"  rows over +5%: {sum(1 for d in deltas if d > 5.0)}")
    if missing:
        # Never let a row disappear quietly: a table that silently lost its
        # slowest streams still looks like a complete table.
        print(f"  *** {len(missing)} row(s) missing from some passes: {', '.join(missing[:6])}")
    print(f"  ^ {which}")


def cmd_run(a: str, b: str, pairs: int) -> None:
    dir_a, dir_b = STASH / a, STASH / b
    for label, d in ((a, dir_a), (b, dir_b)):
        if not d.is_dir():
            die(f"no stashed build {label!r} (perfpair.py list)")

    ffmpeg = resolve_ffmpeg()
    which_list = ["decode"]
    if ffmpeg:
        which_list.append("encode")
    else:
        print(
            "perfpair: *** FFMPEG unset and no ffmpeg on PATH — ENCODER BENCH SKIPPED (S17).\n"
            "perfpair: *** The encoder is UNMEASURED this run. Set FFMPEG=/path/to/ffmpeg.",
            file=sys.stderr,
        )

    results: dict[str, tuple[list[dict], list[dict]]] = {w: ([], []) for w in which_list}
    for p in range(1, pairs + 1):
        for which in which_list:
            for slot, (label, d) in enumerate(((a, dir_a), (b, dir_b))):
                print(f"perfpair: pair {p}/{pairs}  {which}  {'AB'[slot]}={label}", file=sys.stderr)
                binary = d / (BENCHES[0] if which == "decode" else BENCHES[1])
                results[which][slot].append(run_bench(binary, which, ffmpeg))

    ca = (dir_a / "commit.txt").read_text().strip() if (dir_a / "commit.txt").exists() else "?"
    cb = (dir_b / "commit.txt").read_text().strip() if (dir_b / "commit.txt").exists() else "?"
    print("\n" + "=" * 82)
    print(f"interleaved pairs: {pairs}   A={a} ({ca})   B={b} ({cb})")
    print(f"delta = B vs A; positive means B is slower.  medians over {pairs} pairs.")
    print("=" * 82)
    for which in which_list:
        report(which, *results[which])


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("build", help="build both bench binaries and stash them")
    p.add_argument("label")
    p.add_argument("--ref", default=None, help="git ref to build from (tree must be clean)")

    p = sub.add_parser("run", help="interleave two stashed builds")
    p.add_argument("a")
    p.add_argument("b")
    p.add_argument("--pairs", type=int, default=3)

    p = sub.add_parser("null", help="same binary in both slots — this session's floor")
    p.add_argument("label")
    p.add_argument("--pairs", type=int, default=3)

    sub.add_parser("list", help="show stashed labels")

    args = ap.parse_args()
    STASH.mkdir(exist_ok=True)

    if args.cmd == "build":
        cmd_build(args.label, args.ref)
    elif args.cmd == "run":
        cmd_run(args.a, args.b, args.pairs)
    elif args.cmd == "null":
        print("perfpair: NULL run (S2) — same binary in both slots. This is the session's floor.")
        cmd_run(args.label, args.label, args.pairs)
    elif args.cmd == "list":
        cmd_list()


if __name__ == "__main__":
    main()
