#!/usr/bin/env python3
"""S1 prohibition #1: no `&mut self` accessor may be called from a body whose
context parameter is `*mut sWelsEncCtx` (the forksplit's universe = in-fork +
the two not-yet-flipped ST bodies).  Usage: prohibit.py writer1 writer2 ..."""
import re, subprocess, sys, os

writers = sys.argv[1:]
out = subprocess.run(["python3", "../../tools/phase9_forksplit.py", "--list"],
                     capture_output=True, text=True).stdout
bodies, sec = [], None
for l in out.splitlines():
    if l.startswith("== IN-FORK"): sec = "in-fork"; continue
    if l.startswith("== ST"):      sec = "ST-flippable"; continue
    m = re.match(r"\s+(\S+):(\d+)\s+(\S+)\s*$", l)
    if m and sec: bodies.append((m.group(1), int(m.group(2)), m.group(3), sec))

bad = []
for f, ln, name, sec in bodies:
    src = open("src/" + f).read().split("\n")
    d, seen, end = 0, False, ln - 1
    for i in range(ln - 1, len(src)):
        d += src[i].count("{") - src[i].count("}")
        if "{" in src[i]: seen = True
        if seen and d == 0: end = i; break
    body = "\n".join(src[ln - 1:end + 1])
    for w in writers:
        if re.search(r"\b%s\s*\(" % re.escape(w), body):
            bad.append("%s:%d %s (%s) calls %s" % (f, ln, name, sec, w))
print("prohibition 1: %d *mut-ctx bodies scanned against %d writers"
      % (len(bodies), len(writers)))
print("  " + ("\n  ".join(bad) if bad else "no violations"))
sys.exit(1 if bad else 0)
