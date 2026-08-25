#!/usr/bin/env python3
"""The join of Phase 9's two ctx instruments — the work list G's step 2 actually has.

    rust/tools/phase9_ctx_join.py            # the summary
    rust/tools/phase9_ctx_join.py --live     # every site that really blocks a flip
    rust/tools/phase9_ctx_join.py --moot     # every site that models a retag S63 forbids

## Why a join exists at all

`q1c.py --kind raw` answers one question, and its own docstring says which:
**"it models one conversion — the parameter becomes `&mut T`."** It is the
pre-conversion question, asked of every `*mut sWelsEncCtx`-taking body in the
tree.

`phase9_forksplit.py` answers a different one: which of those bodies can ever be
asked. **S63 (F146): the in-fork half cannot take `&mut` by any amount of hazard
work** — the blocker is not a held cursor but N workers retagging one allocation,
so its end states are interior mutability or lawful raw. A body in that column is
never converted, so the retag q1c models on it is never taken.

Neither instrument knows about the other, so a hazard reported against an in-fork
callee reads exactly like a hazard reported against a flippable one. **At the
commit this tool landed, 135 of the 266 reported hazards were of the first kind**
— including all 55 against `ctx_param`, the single most-implicated callee in the
report and the centrepiece of session G's brief. Clearing them buys nothing: they
are not UB today (a raw retag pops only the offsets it touches — F66's own
finding, and the tree is green under Miri with all 266 present), and they are not
UB after G, because their callee still takes `*mut`.

This is F146's error in the mirror. E2's brief ordered a `&mut` flip on bodies
the fork shares; G's brief ordered hazard-clearing *for* bodies the fork shares.
Both applied the split to the family and not to the detector's output.

## What "live" and "moot" mean here

**live** — the hazard's *callee* is in the ST column, so the flip really does put
a `&mut sWelsEncCtx` at its entry and really does invalidate what the caller
holds. These are step 2's work list, and they are the only sites whose clearing
is load-bearing.

**moot** — the hazard's callee is in the in-fork column. Reported, real as a
*reading* of today's code, and not work: no retag is ever taken there.

A cursor can appear in both classes (it crosses an ST callee and an in-fork one).
Fixing it for the live crossing retires the moot ones for free, which is why the
live cursor count is the number that scopes the campaign.

## What this tool is not

It inherits both parents' limits. q1c is a heuristic on both sides and a text
scan, not a resolver (two duplicate names); forksplit is a deliberate
over-approximation (a body reachable only under a configuration the fork never
takes still lands in-fork). Both err toward *more* in-fork and *fewer* reported
hazards, so `live` here is a lower bound on the work and `moot` an upper bound on
the relief. The proof of a conversion is still the borrow checker, and the proof
of fork-disjointness is still Miri.
"""
import argparse, collections, re, subprocess, sys, pathlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def run(script, *args):
    r = subprocess.run([sys.executable, str(HERE / script), *args],
                       cwd=ROOT, capture_output=True, text=True)
    if r.returncode not in (0, 1):
        sys.exit(f"{script} failed (rc={r.returncode}):\n{r.stderr}")
    return r.stdout


def fork_columns(typename):
    """name -> {'F'} | {'S'} | {'F','S'} (the last only for duplicate names)."""
    out = run('phase9_forksplit.py', '--type', typename, '--list')
    col, body = None, {}
    for ln in out.split('\n'):
        if ln.startswith('== IN-FORK'):
            col = 'F'; continue
        if ln.startswith('== ST-FLIPPABLE'):
            col = 'S'; continue
        m = re.match(r'\s+(\S+):(\d+)\s+(\S+)\s*$', ln)
        if m and col:
            body.setdefault(m.group(3), set()).add(col)
    if not body:
        sys.exit("phase9_forksplit.py --list produced no bodies — the parse is stale")
    return body


def hazards(typename):
    out = run('q1c.py', '--type', typename, '--kind', 'raw', '--sites')
    sites, cur = [], None
    for ln in out.split('\n'):
        m = re.match(r'^([ABCDE])\s+(\S+):(\d+)\s+in (\S+)\(\)', ln)
        if m:
            cur = dict(shape=m.group(1), file=m.group(2), line=int(m.group(3)),
                       caller=m.group(4), cursor=None, callee=None, call_line=None)
            sites.append(cur)
            continue
        if cur is None:
            continue
        m = re.match(r'\s+derive\s+(\d+)\s+(.*)$', ln)
        if m:
            mm = re.match(r'let (?:mut )?([A-Za-z_][A-Za-z0-9_]*)', m.group(2))
            cur['cursor'] = mm.group(1) if mm else m.group(2)[:48]
        m = re.match(r'\s+call\s+(\d+)\s+([A-Za-z_][A-Za-z0-9_]*)\(\.\.\.\)', ln)
        if m:
            cur['call_line'], cur['callee'] = int(m.group(1)), m.group(2)
        m = re.match(r'\s+argument order: `([A-Za-z_][A-Za-z0-9_]*)`', ln)
        if m:
            cur['callee'] = m.group(1)
    if not sites:
        sys.exit("q1c.py --sites produced no hazards — the parse is stale, or the campaign is done")
    unresolved = [s for s in sites if not s['callee']]
    if unresolved:
        # S58: an instrument's unknown column is louder than its answer.
        print(f"WARNING: {len(unresolved)} hazard sites with no callee parsed — "
              f"the join under-reports by that much", file=sys.stderr)
    return sites


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--type', default='sWelsEncCtx')
    ap.add_argument('--live', action='store_true', help='every blocking site')
    ap.add_argument('--moot', action='store_true', help='every site the split exempts')
    a = ap.parse_args()

    body = fork_columns(a.type)
    sites = hazards(a.type)

    def col(n):
        c = body.get(n)
        return '?' if not c else ('F' if c == {'F'} else 'S' if c == {'S'} else 'FS')

    live = [s for s in sites if col(s['callee']) in ('S', '?', 'FS')]
    moot = [s for s in sites if col(s['callee']) == 'F']

    print(f"join of q1c (hazards) x forksplit (fork-reachability), type {a.type}\n")
    print(f"  {len(sites):4d}  hazards reported by q1c --kind raw")
    print(f"  {len(live):4d}  LIVE  — callee is ST-flippable, so the &mut retag really happens")
    print(f"  {len(moot):4d}  moot  — callee is in-fork, so S63 forbids the retag q1c models")
    amb = [s for s in sites if col(s['callee']) in ('?', 'FS')]
    if amb:
        print(f"        (of the live, {len(amb)} have a callee that is not a single "
              f"ctx-taking body — counted live, which is the safe side)")

    for label, rows in (('LIVE', live), ('moot', moot)):
        c = collections.Counter(s['shape'] for s in rows)
        keys = {(s['file'], s['caller'], s['cursor']) for s in rows if s['shape'] == 'A'}
        print(f"\n{label}: A={c['A']} B={c['B']}, over {len(keys)} distinct held cursors "
              f"(file, caller, binding)")
        for f, n in collections.Counter(s['file'] for s in rows).most_common():
            cc = collections.Counter(s['shape'] for s in rows if s['file'] == f)
            print(f"  {n:4d}  {f}   A={cc['A']} B={cc['B']}")

    print("\nLIVE, by held cursor — this is what step 2 retires:")
    byc = collections.Counter((s['file'], s['caller'], s['cursor'])
                              for s in live if s['shape'] == 'A')
    for (f, caller, cur), n in byc.most_common():
        print(f"  {n:4d}  {cur:<20s} in {caller}()   {f}")
    nb = [s for s in live if s['shape'] == 'B']
    print(f"\nLIVE shape B ({len(nb)} argument-order hoists), by caller:")
    for caller, n in collections.Counter(s['caller'] for s in nb).most_common():
        print(f"  {n:4d}  {caller}()")

    if a.live or a.moot:
        rows = live if a.live else moot
        print(f"\n---- every {'LIVE' if a.live else 'moot'} site ----")
        for s in sorted(rows, key=lambda s: (s['file'], s['line'], s['call_line'] or 0)):
            print(f"  {s['shape']}  {s['file']}:{s['line']} in {s['caller']}()"
                  f"  cursor={s['cursor']}  call:{s['call_line']} {s['callee']}"
                  f"  [{col(s['callee'])}]")

    return 1 if live else 0


if __name__ == '__main__':
    sys.exit(main())
