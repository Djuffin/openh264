#!/usr/bin/env python3
"""S1 prohibition #2, pinned to a command (the S1 commits quote the number 22 but
not the grep that produced it, and no obvious spelling reproduces it — see S2's
report).  The invariant: a **whole-context `&mut` retag taken through a raw
root** — `&mut *pCtx`, `&mut **ppCtx` — must not grow.  Those are the retags
`phase9_ctx_join.py` models and S63 forbids in-fork; the safe-conversion plan
retires them, it never adds them.

Run from the crate root:  python3 ../../tools/safeplan_prohibition2.py
"""
import re, glob, sys

# `&mut *root` where root is a raw context pointer, and NOT a field projection
# (`&mut *pCtx.field` is a different, benign shape).
pat = re.compile(r"&mut\s*\*(\*ppCtx|pCtx|pEncCtx)($|[^.\w])")
hits = []
for f in sorted(glob.glob("src/**/*.rs", recursive=True)):
    for i, l in enumerate(open(f).read().split("\n")):
        if l.lstrip().startswith("//"):
            continue                       # F178: prose is not a retag
        for _ in pat.finditer(l):
            hits.append((f, i + 1, l.strip()))
per = {}
for f, _, _ in hits:
    per[f] = per.get(f, 0) + 1
print("prohibition 2: %d `&mut *pCtx`-class retags through a raw root" % len(hits))
for f in sorted(per, key=lambda k: -per[k]):
    print("  %-45s %d" % (f, per[f]))
if len(sys.argv) > 1:
    for f, n, l in hits:
        print("    %s:%d  %s" % (f, n, l[:100]))
