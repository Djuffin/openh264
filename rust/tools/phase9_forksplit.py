#!/usr/bin/env python3
"""S63's instrument: split a parameter family by **fork-reachability**.

    rust/tools/phase9_forksplit.py                  # the two-column summary
    rust/tools/phase9_forksplit.py --list           # every body, both columns
    rust/tools/phase9_forksplit.py --type SDqLayer  # aim it at another root type
    rust/tools/phase9_forksplit.py --no-slots       # calibration: drop the slot arm
    rust/tools/phase9_forksplit.py --why NAME       # the call path that reaches NAME

**Run it from `rust/tools/`** — the source tree is resolved from this file's own
path (q1c's rule, and its `SRC`).

## Why this exists

S63, from F146: *fork-reachability splits a parameter family before any flip is
planned.* A shared object's in-fork half cannot take `&mut` by any amount of
hazard work — the blocker is not a held cursor but N workers performing
concurrent entry retags of one allocation — so its end states are interior
mutability (the seam's shape) or lawful raw. E2 learned this on `SDqLayer` by
hand: 42 of 71 layer-param bodies turned out to be fork-reachable, and the
brief that ordered "flip them all the same staged way" contradicted its own
edge rule. The charter then said the same split — **not** the hazard count — is
the first number G's brief must carry for `sWelsEncCtx`.

Doing it by hand once is a session; doing it by hand every time a brief needs it
is how a number goes stale between sessions (S24). This is the walker.

## The method

1. **Seeds** — the bodies of the closures `std::thread::scope` spawns. The
   encoder has three such forks, all in `slice_multi_threading.rs`
   (`WelsMdInterMbLoopOverDynamicSlice`'s neighbour update, the fixed-slice
   coding tasks, and the size-limited ones — F132's "third fork the brief did
   not list"). A spawn whose closure only calls one function contributes that
   function; the walk does the rest.
2. **Dispatch slots** — a fork-reachable body that calls through a slot
   (`something.pfName`) pulls in **every function installed into that slot
   anywhere in the tree** (`… .pfName = Some(F)`, both arms of an `if`). This is
   the arm F146 names and the one a pure name-based walk misses: the port's MD,
   ME, deblocking and RC surfaces are all reached this way, and `pfInterMd`'s
   installee is chosen by spatial layer at run time.
2b. **Function tables** — a reachable body that mentions a static/local **array
   of function items** pulls in everything the array holds. `WelsCodeOneSlice`
   reaches all four mode-decision drivers this way (`g_pWelsSliceCoding`), and a
   walker without this arm reports the entire MD surface as single-threaded.
3. **Closure** — transitive over named calls, until it stops growing.
4. **Report** — every body carrying a `*mut CTX_TYPE` parameter, in one of two
   columns: **in-fork** (its end state is interior mutability or lawful raw) and
   **ST-flippable** (the half a root-down campaign can convert).

## What it is not

An **over-approximation, on purpose** — the same trade the plane census makes.
A body reached only under a configuration the fork never takes still lands in
the in-fork column; the cost is a conversion deferred, where a false negative is
a data race with no gate behind it. Two known over-counts: a slot's *other*
installee (only one `pfInterMd` runs per layer, both are pulled in), and helpers
shared between a fork path and an init path.

The seeds are structural, so a fork that stops being spawned from
`slice_multi_threading.rs` silently shrinks the in-fork column. `--why` prints
the path that reached a body, which is how a suspicious classification is read
rather than believed; and the seed count is printed on every run, with a
**non-zero exit** if it is zero (S46/S58: an instrument's empty case is a case,
and loudness lives in the exit code).
"""

import argparse
import collections
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import q1c  # the parser: split_bodies, signature, strip_comment, aim

SRC = q1c.SRC
FORK_FILE = "encoder/slice_multi_threading.rs"

SLOT_CALL_RE = re.compile(r"\.\s*(pf[A-Za-z0-9_]+)\b")
SLOT_INSTALL_RE = re.compile(r"\.\s*(pf[A-Za-z0-9_]+)\s*=\s*(.*)$")
SOME_RE = re.compile(r"Some\s*\(\s*([A-Za-z_][A-Za-z0-9_:]*)")


def load():
    return [(str(p.relative_to(SRC)), p.read_text().splitlines())
            for p in sorted(SRC.rglob("*.rs"))]


def bodies(files):
    """name -> list of (file, start_line, body_start, body_end)."""
    out = collections.defaultdict(list)
    for rel, lines in files:
        for name, b, end, _roots, _pp in q1c.split_bodies(lines):
            _head, start = q1c.signature(lines, name, b)
            out[name].append((rel, start, b, end))
    return out


def calls_in(lines, b, end, names):
    out = set()
    for j in range(b, end + 1):
        if q1c.COMMENT_RE.match(lines[j]):
            continue
        s = q1c.strip_comment(lines[j])
        for m in re.finditer(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(", s):
            if m.group(1) in names:
                out.add(m.group(1))
    return out


def slots_called_in(lines, b, end):
    out = set()
    for j in range(b, end + 1):
        if q1c.COMMENT_RE.match(lines[j]):
            continue
        for m in SLOT_CALL_RE.finditer(q1c.strip_comment(lines[j])):
            out.add(m.group(1))
    return out


def fn_tables(files, names):
    """`binding -> {functions}` for every **array of function items**.

    The third dispatch arm, and the one that decides whether the whole
    mode-decision surface is in-fork: `WelsCodeOneSlice` does not call a `pf`
    slot to reach MD, it indexes a static table —

        pub static g_pWelsSliceCoding: [[PWelsCodingSliceFunc; 2]; 2] = [
            [WelsISliceMdEnc, WelsISliceMdEncDynamic], ...

    — so a walker that knows only named calls and `pf` slots reports MD as
    single-threaded, which is exactly wrong (F150's shape one level up: read
    what the *table* holds, not what the call spells). Locals count too
    (`let pJudgeSkip: [pJudgeSkipFun; 2] = [JudgeStaticSkip, JudgeScrollSkip]`).
    """
    out = collections.defaultdict(set)
    decl = re.compile(r"\b(?:static|const|let)\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)"
                      r"\s*(?::[^=]*)?=\s*\[")
    for _rel, lines in files:
        for j, line in enumerate(lines):
            if q1c.COMMENT_RE.match(line):
                continue
            m = decl.search(q1c.strip_comment(line))
            if not m:
                continue
            depth, buf = 0, ""
            for k in range(j, min(j + 40, len(lines))):
                s_ = q1c.strip_comment(lines[k])
                buf += " " + s_
                depth += s_.count("[") - s_.count("]")
                if depth <= 0 and k > j or (depth <= 0 and "]" in s_):
                    break
            for fm in re.finditer(r"([.]?)\s*\b([A-Za-z_][A-Za-z0-9_]*)\b\s*([(]?)", buf):
                fn = fm.group(2)
                # A function **item**, not a call and not a method: `[A, B]`
                # holds items; `[mv.get(i), ..]` and `[x.cur(), ..]` are data
                # whose method names collide with body names (`get`, `cur`,
                # `iter` all matched before this filter, and each pulled a
                # different type's inherent method into the fork set).
                if fm.group(1) == "." or fm.group(3) == "(":
                    continue
                if fn in names and fn != m.group(1):
                    out[m.group(1)].add(fn)
    return out


def slot_installees(files, names):
    """slot name -> every function assigned into it, anywhere."""
    out = collections.defaultdict(set)
    for _rel, lines in files:
        for j, line in enumerate(lines):
            if q1c.COMMENT_RE.match(line):
                continue
            m = SLOT_INSTALL_RE.search(q1c.strip_comment(line))
            if not m:
                continue
            rhs = m.group(2)
            for k in range(j + 1, min(j + 6, len(lines))):   # `if` arms wrap
                if "=" in q1c.strip_comment(lines[k]) and "==" not in q1c.strip_comment(lines[k]):
                    break
                rhs += " " + q1c.strip_comment(lines[k])
            for sm in SOME_RE.finditer(rhs):
                fn = sm.group(1).split("::")[-1]
                if fn in names:
                    out[m.group(1)].add(fn)
    return out


def fork_seeds(files, names):
    """Functions called inside a `std::thread::scope` spawn, in the fork file."""
    seeds, blocks = set(), 0
    for rel, lines in files:
        if rel != FORK_FILE:
            continue
        for j, line in enumerate(lines):
            if "thread::scope" not in q1c.strip_comment(line):
                continue
            blocks += 1
            depth, k = 0, j
            for k in range(j, len(lines)):
                s = q1c.strip_comment(lines[k])
                depth += s.count("{") - s.count("}")
                if depth <= 0 and k > j:
                    break
            for n in calls_in(lines, j, k, names):
                seeds.add(n)
    return seeds, blocks


def walk(files, all_bodies, seeds, use_slots=True):
    """Transitive closure from `seeds`, optionally through dispatch slots."""
    names = set(all_bodies)
    installees = slot_installees(files, names) if use_slots else {}
    tables = fn_tables(files, names) if use_slots else {}
    lines_of = {}
    for rel, lines in files:
        lines_of[rel] = lines
    reach = set(seeds)
    why = {s: "fork seed (thread::scope spawn)" for s in seeds}
    frontier = list(seeds)
    while frontier:
        cur = frontier.pop()
        for rel, _start, b, end in all_bodies.get(cur, ()):
            lines = lines_of[rel]
            nxt = calls_in(lines, b, end, names)
            if use_slots:
                for slot in slots_called_in(lines, b, end):
                    nxt |= installees.get(slot, set())
                body_text = " ".join(q1c.strip_comment(x) for x in lines[b:end + 1])
                for tbl, fns in tables.items():
                    if re.search(r"\b" + re.escape(tbl) + r"\b", body_text):
                        nxt |= fns
            for n in nxt:
                if n not in reach:
                    reach.add(n)
                    why[n] = cur
                    frontier.append(n)
    return reach, why


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--type", default="sWelsEncCtx", metavar="NAME")
    ap.add_argument("--list", action="store_true", help="every body, both columns")
    ap.add_argument("--no-slots", action="store_true",
                    help="calibration: drop the dispatch-slot arm")
    ap.add_argument("--why", metavar="NAME", help="the path that reached NAME")
    a = ap.parse_args()

    q1c.aim(a.type, "raw")
    files = load()
    if not files:
        print(f"no sources under {SRC} — nothing scanned", file=sys.stderr)
        return 2
    all_bodies = bodies(files)
    seeds, blocks = fork_seeds(files, set(all_bodies))
    if not seeds:
        print(f"no fork seeds found in {FORK_FILE} — the walker scanned nothing "
              f"(has the fork moved?)", file=sys.stderr)
        return 2
    reach, why = walk(files, all_bodies, seeds, use_slots=not a.no_slots)

    infork, st = [], []
    for rel, lines in files:
        for name, b, end, _roots, _pp in q1c.split_bodies(lines):
            head, start = q1c.signature(lines, name, b)
            if not q1c.CTX_PARAM_RE.search(head):
                continue
            (infork if name in reach else st).append((rel, start + 1, name))

    if a.why:
        cur, path = a.why, [a.why]
        while cur in why and why[cur] not in ("fork seed (thread::scope spawn)",):
            cur = why[cur]
            path.append(cur)
            if len(path) > 40:
                break
        if a.why not in reach:
            print(f"{a.why} is NOT fork-reachable")
        else:
            print(" <- ".join(path) + " <- fork seed (thread::scope spawn)")
        return 0

    print(f"S63 fork-split of `*mut {a.type}` parameter bodies"
          + ("  [--no-slots: dispatch arm OFF]" if a.no_slots else ""))
    print(f"{blocks} thread::scope blocks in {FORK_FILE}, {len(seeds)} seed functions, "
          f"{len(reach)} bodies fork-reachable in total\n")

    if a.list:
        for title, rows in (("IN-FORK (interior mutability or lawful raw — S63)", infork),
                            ("ST-FLIPPABLE (a root-down campaign can convert)", st)):
            print(f"== {title}: {len(rows)}")
            for rel, ln, name in sorted(rows):
                print(f"   {rel}:{ln}  {name}")
            print()

    byfile = collections.defaultdict(lambda: [0, 0])
    for rel, _ln, _n in infork:
        byfile[rel][0] += 1
    for rel, _ln, _n in st:
        byfile[rel][1] += 1
    w = max([len(f) for f in byfile] + [12])
    print(f"{'file':<{w}}  in-fork  ST-flippable")
    for rel in sorted(byfile, key=lambda r: -(byfile[r][0] + byfile[r][1])):
        i, s = byfile[rel]
        print(f"{rel:<{w}}  {i:7d}  {s:12d}")
    print(f"{'**total**':<{w}}  {len(infork):7d}  {len(st):12d}")
    print(f"\n{len(infork) + len(st)} bodies carry a `*mut {a.type}` parameter.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
