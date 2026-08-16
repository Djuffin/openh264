#!/usr/bin/env python3
"""Trivial function bodies that shadow real implementations — the F43 sweep.

Phase 5 session S. `decoder_core.rs` defined five stubs (`NeedErrorCon` returning
`false`, `FmoNextMb` returning `iMbIdx + 1`, …) in the same module that called
them. A local item beats every other path, so **every production call site resolved
to the stub** and two whole subsystems — error concealment and FMO — never ran. No
instrument could see it: `find_stub_bodies.py` matches names against the C++ and
finds the *real* body in the other module, and the duplicate census compares text,
which a stub and its implementation differ in maximally.

This is the check that would have caught it. Three filters, and the order matters —
each one removes a class of false positive that swamped the previous draft:

  1. **name defined in more than one module** — 194 names. Unusable alone.
  2. **one body trivial (<= 2 statements), another substantial (>= 6)** — 35. Still
     swamped, because a delegating wrapper looks exactly like a stub from here, and
     `decoder_core.rs` is full of deliberate ones (`pub unsafe fn WelsResetRefPic(..)
     { manage_dec_ref::WelsResetRefPic(..) }`).
  3. **the trivial body does not mention the function's own name** — delegation is
     the whole difference between a re-export and a stub. 15 left.

What survives is trait-impl methods: twenty types implementing `default` or `new`
share a name and share nothing else. They are reported rather than filtered because
recognising an `impl` block needs a parser, and a reader dismisses them in a glance
— but that is why this prints candidates and not verdicts.

Result at `cf31bf2f`: **no remaining instance of F43's shape.** Every decoder
`Wels*` one-liner delegates; every other candidate is a trait method.

    usage: rust/tools/find_shadowing_stubs.py
"""

import collections
import os
import re
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "openh264-rs", "src")
FN = re.compile(r'\s*(?:pub(?:\([^)]*\))? )?(?:unsafe )?(?:extern "C" )?fn (\w+)')
TRIVIAL, SUBSTANTIAL = 2, 6


def functions(text):
    """(name, statement_count, body_text, line_no) for each fn in one file.

    **The body starts at the opening brace, not at the `fn` line** (F52, Phase 5
    session X). Counting from `i + 1` charges a multi-line signature's parameter
    lines to the body: a four-parameter stub whose whole body is `true` scored 6
    statements and was classified SUBSTANTIAL, so it never appeared opposite its own
    real implementation. That is exactly how a sixth F43-class stub —
    `decoder_core.rs`'s `CheckAccessUnitBoundaryExt`, shadowing `nalu.rs`'s 80-line
    one — survived this sweep's clean run at session U. `body_start` is the line the
    brace that opens the body is on; everything before it is signature.
    """
    out, lines = [], text.split("\n")
    for i, line in enumerate(lines):
        m = FN.match(line)
        if not m:
            continue
        depth, j, started, body_start = 0, i, False, i
        while j < len(lines) and j < i + 400:
            depth += lines[j].count("{") - lines[j].count("}")
            if "{" in lines[j] and not started:
                started = True
                body_start = j
            if started and depth <= 0:
                break
            j += 1
        inner = [
            b.strip()
            for b in lines[body_start + 1:j]
            if b.strip() and not b.strip().startswith("//")
        ]
        out.append((m.group(1), len(inner), "\n".join(inner), i + 1))
    return out


def main():
    defs = collections.defaultdict(list)
    for dirpath, _, files in os.walk(ROOT):
        for f in files:
            if not f.endswith(".rs"):
                continue
            path = os.path.join(dirpath, f)
            rel = os.path.relpath(path, ROOT)
            with open(path) as fh:
                for name, n, body, ln in functions(fh.read()):
                    defs[name].append((rel, n, body, ln))

    candidates = 0
    for name, ds in sorted(defs.items()):
        if len(ds) < 2:
            continue
        stubs = [d for d in ds if d[1] <= TRIVIAL and name not in d[2]]
        real = [d for d in ds if d[1] >= SUBSTANTIAL]
        if not (stubs and real):
            continue
        candidates += 1
        print(f"  {name}")
        for rel, n, body, ln in stubs:
            print(f"      trivial, non-delegating  {rel}:{ln}  body: {body[:64]!r}")
        for rel, n, _, ln in real:
            print(f"      substantial ({n:3d})        {rel}:{ln}")

    print(f"\n{candidates} candidate name(s).")
    print("Trait-impl methods (`default`, `new`, `eq`, `get`) are expected here and are")
    print("not shadowing. A *free function* in this list is F43 until read and disproved.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
