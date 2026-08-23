#!/usr/bin/env python3
"""The F66 aliasing-hazard detector — run this BEFORE converting any
`*mut sWelsEncCtx` parameter to `&mut sWelsEncCtx`, not after.

    rust/tools/q1c.py                 # the summary: sites, callers, per file
    rust/tools/q1c.py --sites         # every hazardous site, one per line
    rust/tools/q1c.py --callee NAME   # only sites whose call is to NAME
    rust/tools/q1c.py --caller NAME   # only sites inside caller NAME

Exit status is 0 when clean, 1 when any hazard is found, so a future session can
gate the conversion on it. It is a **heuristic**: a green site is "no hazard
found", not "proved safe" (F66's own words — a false negative is silent UB with
no gate behind it).

## What it detects, and why

Phase 6 session J converted 109 context-only signatures from `*mut sWelsEncCtx`
to `&mut sWelsEncCtx`, root-down, four green commits — and then Miri refused it
and the conversion was reverted in full (T6.J5, F66 in `phase6_findings.md`).

The reason generalises and is the fact that orders all of Phase 9. A
`&mut sWelsEncCtx` **function-entry retag invalidates the entire context, at
every offset**. Miri prints the range:

    help: <17125342> was created by a SharedReadOnly retag at offsets [0x258..0x288]
        --> encoder_ext.rs:3033  let pSpatialIndexMap = (*pCtx).sSpatialIndexMap.as_ptr();
    help: <17125342> was later invalidated at offsets [0x0..0x17ee8] by a Unique retag
        --> encoder_ext.rs:3034  InitBitStream(&mut *pCtx);

`[0x0..0x17ee8]` is the whole `sWelsEncCtx`. A **raw** write through the parent
pops only the offsets it touches — which is why the port's dominant idiom (derive
a cursor from the context, call something, use the cursor) has been sound through
six phases. A **`&mut` retag** pops every cursor into the context regardless of
what the callee goes near.

There are two shapes, and F66's two Miri-confirmed sites are one of each.

**Shape A — a cursor held across the call.** Three parts inside one body:

    let cur = (*pCtx).some_field.as_mut_ptr();   // 1. derive a cursor from the ctx
    SomeCallee(pCtx, ...);                       // 2. call a ctx-taking callee
    *cur = ...;                                  // 3. use the cursor afterwards

Today (2) passes a raw pointer and nothing is invalidated. The moment `SomeCallee`
takes `&mut sWelsEncCtx`, step 2 invalidates `cur` and step 3 is UB. This is
F66's second confirmed site (`pSpatialIndexMap` before `InitBitStream`).

**Shape B — argument evaluation order.** One line, no binding at all:

    WelsRcInitModule(pCtx, (*ctx_param(pCtx)).iRCMode);

Arguments evaluate left to right. Once the first parameter is `&mut sWelsEncCtx`
the call site reads `WelsRcInitModule(&mut *pCtx, (*ctx_param(pCtx)).iRCMode)`:
argument 1 takes the Unique retag, argument 2 then reads through the raw, and the
raw is already dead. This is F66's first confirmed site. Session J fixed it by
hoisting argument 2 above the call — the fix is local and safe here precisely
because nothing reallocates in between, which is *not* true of shape A in general.

Shape B is reported separately because its remedy is different: hoist the
argument. Shape A's remedy is to retire the cursor's family first.

## How to use the result

Per F66 this is the **precondition** to family 6, not its post-mortem:

  * **green for a callee** — no caller holds a context-derived cursor across any
    call to it; its `*mut sWelsEncCtx` parameter can become `&mut sWelsEncCtx`.
  * **red for a callee** — the cursor named in the report has to retire first.
    Look up which family owns it (`rust/tools/phase9_census.py --family ...`) and
    convert that family before this callee.

Reordering the derivation past the call is **not** a fix in general: several of
these callees reallocate the container the cursor points into
(`ExtendLayerBuffer`, `ReallocSliceBuffer`, `RequestMemorySvc`), so moving a
derivation across one changes which allocation the cursor names — a behaviour
change, and hard rule 8's line.

## Provenance (T9.A2)

Session J's original `q1c.py` was **not** recoverable. F66 and the charter both
say it is "reproduced in the session J log"; it is not, in that log or any other,
and it was never committed (`git log --all -- '*q1c*'` is empty). This file is a
**reimplementation from F66's written specification**, so its numbers are not
expected to match session J's exactly — and session J's own numbers are not
self-consistent either: F66 reports "93 sites in 28 callers" above a per-file
table that sums to 165. Treat both as measurements of the same shape, not as a
figure to reproduce.
"""

import argparse
import collections
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "rust/crates/openh264-rs/src"

CTX_TYPE = "sWelsEncCtx"

# A parameter of type `*mut sWelsEncCtx` (not `*mut *mut`, which would become
# `&mut &mut` — a different type, and the four out-parameters F66 excluded).
CTX_PARAM_RE = re.compile(
    r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*\*\s*(?:mut|const)\s+" + CTX_TYPE + r"\b")
CTX_PARAM_PP_RE = re.compile(
    r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*\*\s*(?:mut|const)\s+\*\s*(?:mut|const)\s+" + CTX_TYPE + r"\b")

FN_HEAD_RE = re.compile(
    r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:default\s+)?(?:const\s+)?(?:unsafe\s+)?"
    r"(?:extern\s+\"C\"\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")

# `let x = ...` / `let mut x = ...` — the binding a cursor lands in.
LET_RE = re.compile(r"^\s*let\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=]+)?=\s*(.*)$")

# A derivation is context-derived if it reaches through the context root and
# produces something pointer-like: a raw pointer, a reference, or a slice.
DERIV_HINT = re.compile(
    r"as_mut_ptr\(\)|as_ptr\(\)|\.as_mut\(\)|&\s*mut\b|&\s*[a-zA-Z_(*]|"
    r"\.add\(|\.offset\(|_mut\(|_ptr\b|as\s+\*\s*(?:mut|const)\b")

# Lines that cannot use a binding even if the name appears (the derivation itself,
# comments).
COMMENT_RE = re.compile(r"^\s*(//|/\*|\*)")


def blank_parens(expr):
    """Empty every balanced parenthesised group, innermost first.

    The hint test has to look at the *shape* of the expression, not at what is
    nested inside its calls. Without this,
    `let iRet = (*(*pCtx).pVpp).BuildSpatialPicList(pCtx, p, &mut n)` reads as a
    derivation because `&mut n` appears somewhere in it — but `iRet` is an `i32`,
    not a cursor. Blanking the groups leaves `().BuildSpatialPicList()`, which
    matches nothing, while `(*pCtx).sSpatialIndexMap.as_ptr()` leaves
    `().sSpatialIndexMap.as_ptr()` and still matches.
    """
    out, depth = [], 0
    for ch in expr:
        if ch == "(":
            depth += 1
            if depth == 1:
                out.append("(")
        elif ch == ")":
            if depth == 1:
                out.append(")")
            depth = max(0, depth - 1)
        elif depth == 0:
            out.append(ch)
    return "".join(out)


def call_args(lines, j, end, callee, col):
    """The balanced argument text of the call to `callee` starting at line j, col."""
    buf, depth, started = "", 0, False
    for k in range(j, min(j + 12, end + 1)):
        seg = strip_comment(lines[k])
        seg = seg[col:] if k == j else seg
        for ch in seg:
            if ch == "(":
                depth += 1
                started = True
                if depth == 1:
                    continue
            elif ch == ")":
                depth -= 1
                if depth == 0:
                    return buf
            if started and depth >= 1:
                buf += ch
        buf += " "
        col = 0
    return buf


def strip_comment(line):
    i = line.find("//")
    return line[:i] if i >= 0 else line


def split_bodies(lines):
    """Yield (name, start_idx, end_idx, ctx_roots) for every fn with a body.

    `ctx_roots` are the parameter names bound to `*mut sWelsEncCtx` in this fn's
    own signature.
    """
    n = len(lines)
    i = 0
    while i < n:
        m = FN_HEAD_RE.match(lines[i])
        if not m:
            i += 1
            continue
        name = m.group(1)
        # accumulate the signature until the parameter list balances
        sig, depth, started, k = "", 0, False, i
        while k < n and k - i < 60:
            sig += " " + lines[k]
            for ch in lines[k]:
                if ch == "(":
                    depth += 1
                    started = True
                elif ch == ")":
                    depth -= 1
            if started and depth <= 0:
                break
            k += 1
        pp = set(CTX_PARAM_PP_RE.findall(sig))
        roots = set(CTX_PARAM_RE.findall(sig)) - pp
        # find the body's opening brace at or after k, then match it
        b = k
        while b < n and "{" not in strip_comment(lines[b]):
            if ";" in strip_comment(lines[b]):
                b = -1
                break
            b += 1
        if b < 0 or b >= n:
            i = k + 1
            continue
        depth, end = 0, None
        for j in range(b, n):
            s = strip_comment(lines[j])
            depth += s.count("{") - s.count("}")
            if depth <= 0 and j > b:
                end = j
                break
            if depth <= 0 and j == b and s.count("{") and s.count("}") >= s.count("{"):
                end = j
                break
        if end is None:
            end = n - 1
        yield name, b, end, roots, pp
        i = end + 1


# A local that holds a raw context pointer. `WelsInitEncoderExt` is the case that
# forces this: its own parameter is already owned (`&mut Option<Box<sWelsEncCtx>>`)
# and the raw context is a *local* — `let pCtx = Box::into_raw(...)` — so a
# parameter-only root scan sees no context in the function that holds F66's first
# Miri-confirmed site.
LOCAL_ROOT_RE = re.compile(
    r":\s*\*\s*mut\s+" + CTX_TYPE + r"\b|"          # let x: *mut sWelsEncCtx = ...
    r"as\s+\*\s*mut\s+" + CTX_TYPE + r"\b|"         # ... as *mut sWelsEncCtx
    r"Box\s*::\s*into_raw\s*\(")                     # Box::into_raw(Box::new(ctx))


def body_roots(lines, b, end, roots, pp):
    """`roots` grown with every local in the body that also holds a raw context.

    Three ways a local becomes one: an explicit `*mut sWelsEncCtx` type or cast,
    `Box::into_raw` of a context, a deref of a `*mut *mut sWelsEncCtx` parameter,
    or a plain alias of a root already known.
    """
    out = set(roots)
    for j in range(b, end + 1):
        m = LET_RE.match(strip_comment(lines[j]))
        if not m:
            continue
        nm, rhs = m.group(1), m.group(2)
        decl = strip_comment(lines[j])
        if LOCAL_ROOT_RE.search(decl) and CTX_TYPE in decl:
            out.add(nm)
        elif re.search(r"Box\s*::\s*into_raw\s*\(.*" + CTX_TYPE, decl):
            out.add(nm)
        elif any(re.fullmatch(r"\*\s*" + re.escape(q) + r"\s*;?", rhs.strip()) for q in pp):
            out.add(nm)
        elif rhs.strip().rstrip(";").strip() in out:
            out.add(nm)
    return out


def ctx_taking_callees():
    """Every function in the tree with a `*mut sWelsEncCtx` parameter.

    These are the calls that become a `&mut` retag when family 6 converts them.
    """
    out = {}
    for path in sorted(SRC.rglob("*.rs")):
        lines = path.read_text().splitlines()
        for name, b, end, roots, _pp in split_bodies(lines):
            if roots:
                out[name] = f"{path.relative_to(SRC)}:{b + 1}"
    return out


class Hazard:
    __slots__ = ("shape", "file", "caller", "binding", "derive_ln", "call_ln",
                 "use_ln", "callee", "text")

    def __init__(self, shape, file, caller, binding, derive_ln, call_ln, use_ln,
                 callee, text):
        self.shape, self.file, self.caller, self.binding = shape, file, caller, binding
        self.derive_ln, self.call_ln, self.use_ln = derive_ln, call_ln, use_ln
        self.callee, self.text = callee, text


def scan():
    callees = ctx_taking_callees()
    callee_re = re.compile(r"\b(" + "|".join(sorted(map(re.escape, callees))) + r")\s*\(")
    hazards = []
    for path in sorted(SRC.rglob("*.rs")):
        rel = str(path.relative_to(SRC))
        lines = path.read_text().splitlines()
        for caller, b, end, roots, pp in split_bodies(lines):
            roots = body_roots(lines, b, end, roots, pp)
            if not roots:
                continue
            body = range(b, end + 1)

            # --- shape B: argument evaluation order ---------------------------
            # The context passed bare as one argument (that argument becomes the
            # `&mut` and takes the Unique retag) while another argument of the
            # same call still reads *through* the raw. Left-to-right evaluation
            # makes the later argument read a pointer the earlier one killed.
            for j in body:
                s_ = strip_comment(lines[j])
                for cm in callee_re.finditer(s_):
                    nm = cm.group(1)
                    if nm == caller or nm not in callees:
                        continue
                    args = call_args(lines, j, end, nm, cm.end() - 1)
                    for r in roots:
                        bare = re.search(r"(^|[,(\s])(&\s*mut\s*\*\s*)?"
                                         + re.escape(r) + r"\s*(,|$)", args)
                        if not bare:
                            continue
                        rest = args[:bare.start()] + args[bare.end():]
                        thru = re.search(r"\*\s*" + re.escape(r) + r"\b|"
                                         r"[A-Za-z_][A-Za-z0-9_]*\s*\(\s*"
                                         + re.escape(r) + r"\s*[,)]", rest)
                        if thru:
                            hazards.append(Hazard("B", rel, caller, "-", j + 1,
                                                  j + 1, j + 1, nm, s_.strip()))
                            break

            # --- shape A: a cursor derived, then held across the call ---------
            # 1. context-derived bindings: line -> name
            derived = {}
            for j in body:
                s = strip_comment(lines[j])
                m = LET_RE.match(s)
                if not m:
                    continue
                nm, rhs = m.group(1), m.group(2)
                if nm == "_":
                    continue
                if not any(re.search(r"\b" + re.escape(r) + r"\b", rhs) for r in roots):
                    continue
                if not DERIV_HINT.search(blank_parens(rhs)):
                    continue
                derived[nm] = j
            if not derived:
                continue
            # 2. calls to a ctx-taking callee that pass the context
            calls = []
            for j in body:
                s = strip_comment(lines[j])
                for cm in callee_re.finditer(s):
                    nm = cm.group(1)
                    if nm == caller:
                        continue
                    # the context has to actually reach the call for the retag to
                    # happen; look at the call's argument text (this line and, for
                    # a wrapped call, the next few)
                    args = " ".join(strip_comment(x) for x in lines[j:min(j + 8, end + 1)])
                    if not any(re.search(r"\b" + re.escape(r) + r"\b", args) for r in roots):
                        continue
                    calls.append((j, nm))
            if not calls:
                continue
            # 3. a use of the binding strictly after the call
            for nm, dj in derived.items():
                use_re = re.compile(r"\b" + re.escape(nm) + r"\b")
                for cj, cname in calls:
                    if cj <= dj:
                        continue
                    uj = None
                    for j in range(cj + 1, end + 1):
                        s = strip_comment(lines[j])
                        if COMMENT_RE.match(lines[j]):
                            continue
                        if LET_RE.match(s) and LET_RE.match(s).group(1) == nm:
                            break          # rebound; the old cursor is dead
                        if use_re.search(s):
                            uj = j
                            break
                    if uj is None:
                        continue
                    hazards.append(Hazard("A", rel, caller, nm, dj + 1, cj + 1,
                                          uj + 1, cname, lines[dj].strip()))
    return hazards, callees


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sites", action="store_true", help="every hazardous site")
    ap.add_argument("--callee", help="only sites whose call is to this callee")
    ap.add_argument("--caller", help="only sites inside this caller")
    a = ap.parse_args()

    hazards, callees = scan()
    if a.callee:
        hazards = [h for h in hazards if h.callee == a.callee]
    if a.caller:
        hazards = [h for h in hazards if h.caller == a.caller]

    print(f"q1c — F66 aliasing-hazard detector (heuristic; see the module docstring)")
    print(f"{len(callees)} functions take a `*mut {CTX_TYPE}` parameter and would retag on conversion.\n")

    A = [h for h in hazards if h.shape == "A"]
    B = [h for h in hazards if h.shape == "B"]

    if a.sites:
        for h in sorted(hazards, key=lambda x: (x.file, x.derive_ln)):
            if h.shape == "A":
                print(f"A  {h.file}:{h.derive_ln}  in {h.caller}()")
                print(f"     derive {h.derive_ln:5d}  {h.text[:96]}")
                print(f"     call   {h.call_ln:5d}  {h.callee}(...)  <-- retags the whole context")
                print(f"     use    {h.use_ln:5d}  `{h.binding}` read after the call")
            else:
                print(f"B  {h.file}:{h.call_ln}  in {h.caller}()")
                print(f"     call   {h.call_ln:5d}  {h.text[:96]}")
                print(f"            argument order: `{h.callee}` takes the retag, a later "
                      f"argument still reads through the raw")
        print()

    byfile = collections.Counter(h.file for h in hazards)
    bycaller = collections.Counter(h.caller for h in hazards)
    bycallee = collections.Counter(h.callee for h in hazards)

    w = max([len(f) for f in byfile] + [12])
    print(f"{'file':<{w}}  sites   (A=held cursor, B=argument order)")
    for f, n in byfile.most_common():
        na = sum(1 for h in A if h.file == f)
        print(f"{f:<{w}}  {n:5d}   A={na} B={n - na}")
    bindings = {(h.file, h.caller, h.binding) for h in A}
    print(f"\n{len(hazards)} hazardous sites in {len(bycaller)} callers, "
          f"across {len(bycallee)} distinct ctx-taking callees.")
    print(f"  shape A (a cursor held across the call): {len(A)} sites, "
          f"{len(bindings)} distinct cursors at risk")
    print(f"  shape B (argument evaluation order):     {len(B)} sites")
    if bycaller:
        print("\nworst callers:")
        for c, n in bycaller.most_common(8):
            print(f"  {n:4d}  {c}")
        print("\nmost-implicated callees (these are the conversions that stay blocked):")
        for c, n in bycallee.most_common(8):
            print(f"  {n:4d}  {c}   ({callees.get(c, '?')})")

    clean = sorted(set(callees) - set(bycallee))
    print(f"\n{len(clean)} of the {len(callees)} ctx-taking callees have no detected hazard "
          f"at any call site.")
    print("A clean callee is 'no hazard found', not 'proved safe' — F66 rejected "
          "converting\nthe clean subset for exactly that reason.")
    return 1 if hazards else 0


if __name__ == "__main__":
    sys.exit(main())
