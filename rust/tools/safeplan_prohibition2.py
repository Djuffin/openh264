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

# Two spellings of the same retag, counted apart because they read differently:
#   (a) explicit — `&mut *pCtx`, `&mut **ppCtx`; NOT a field projection
#       (`&mut *pCtx.field` is a different, benign shape).
#   (b) auto-ref — `(*pCtx).something_mut()`, where the method's `&mut self`
#       receiver is a whole-context retag the source never spells. A6 added the
#       category to this tool: the flip makes accessor calls the normal way to
#       reach the context, so the implicit form is now the one to watch.
pats = [("explicit", re.compile(r"&mut\s*\*(\*ppCtx|pCtx|pEncCtx)($|[^.\w])")),
        ("auto-ref", re.compile(r"\(\*+p\w*[Cc]tx\)\.\w+_mut\("))]
rc = 0
for kind, pat in pats:
    hits = []
    for f in sorted(glob.glob("src/**/*.rs", recursive=True)):
        for i, l in enumerate(open(f).read().split("\n")):
            if l.lstrip().startswith("//"):
                continue                   # F178: prose is not a retag
            for _ in pat.finditer(l):
                hits.append((f, i + 1, l.strip()))
    per = {}
    for f, _, _ in hits:
        per[f] = per.get(f, 0) + 1
    print("prohibition 2 (%s): %d whole-context `&mut` retags through a raw root"
          % (kind, len(hits)))
    for f in sorted(per, key=lambda k: -per[k]):
        print("  %-45s %d" % (f, per[f]))
    if len(sys.argv) > 1:
        for f, n, l in hits:
            print("    %s:%d  %s" % (f, n, l[:100]))
