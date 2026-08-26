#!/usr/bin/env python3
"""The F66 aliasing-hazard detector — run this BEFORE converting any
`*mut sWelsEncCtx` parameter to `&mut sWelsEncCtx`, not after.

    rust/tools/q1c.py                 # the summary: sites, callers, per file
    rust/tools/q1c.py --sites         # every hazardous site, one per line
    rust/tools/q1c.py --callee NAME   # only sites whose call is to NAME
    rust/tools/q1c.py --caller NAME   # only sites inside caller NAME
    rust/tools/q1c.py --type SMbCache # aim it at a different arena (T9.D1)

**Run it from `rust/tools/`.** The source tree is resolved from this file's own
path, so a copy elsewhere scans nothing; both empty cases (no sources, no
function taking the named type) now exit 2 with a message rather than printing a
clean run.

## What it is not

Two limits, both of which cost a reader time if they are not stated up front.

1. **It is a heuristic on both sides.** A *derivation* is recognised by the shape
   of its right-hand side (`DERIV_HINT`), so `let p = accessor(pArena);` — a bare
   call returning a raw pointer, with no `.add()`, `as_mut_ptr()` or `&mut` on it
   — is **not** seen as a cursor. Session D found three such bindings alongside
   the one the tool did flag, in the same four lines of the same function
   (`WelsMdInterMbRefinement`: `pDstCb`, `pDstLuma`, `pBufMe` beside `pDstCr`).
   Widening the hint to "any call taking a root" was not done, because on the
   context struct it matches `let pCurLayer = current_layer(pCtx);` and every
   sibling accessor, and would swamp the signal it exists to raise. **Zero
   reported is a scoping result, not a proof.** The proof of a conversion is the
   borrow checker: once the arena is behind `&mut`, a cursor held across a
   whole-arena call does not compile.
2. **It models one conversion — "the parameter becomes `&mut T`".** Where a
   callee is instead *narrowed* to the one field it touches, the retag covers
   that field only and the report's hazard was never real. Ten of session D's
   twenty-one `SMbCache` sites were of that kind: the "callee" was one of the
   arena's own accessors, which became a field projection rather than a
   whole-arena borrow.

Exit status is 0 when clean, 1 when any hazard is found, so a future session can
gate the conversion on it. It is a **heuristic**: a green site is "no hazard
found", not "proved safe" (F66's own words — a false negative is silent UB with
no gate behind it).

3. **It over-reports in two ways it cannot see, and the count is not a work list**
   (Phase 9 session G — F161, F163, S65). **(a) Reachability.** It models the
   conversion for every body in the family; S63 forbids the fork-reachable half from
   ever taking `&mut`, so a hazard whose *callee* is in that column models a retag
   that will never be taken. At G's step 1 that was **135 of 266**, including all 55
   against `ctx_param`. Join it with `phase9_forksplit.py` before quoting a number —
   `phase9_ctx_join.py` does exactly that and is the tool to run. **(b) Allocation.**
   The retag it models covers the arena's **own allocation** — F66's trace prints the
   range, `[0x0..0x17ee8]`, and the cursor it killed was an *inline array field*. A
   cursor into a `Vec` buffer or a `Box` the arena merely points at lives in a
   different allocation and cannot be reached; this port's accessors launder
   provenance out of the container on purpose (F71, and `ctx_param`'s own comment
   says "the pointer still carries the heap block's own provenance"). This scan
   classifies by type, not by allocation, so those are reported and are not hazards.

   A consequence worth stating because a brief got it wrong: **"drive this tool to
   zero" is not a reachable exit criterion.** Its remedy for shape B — hoist the
   argument into a binding — produces shape A whenever the hoisted value is a
   pointer, which for this context is most of them. Read the join's live count per
   site instead.

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

There are **five** shapes. F66's two Miri-confirmed sites are one each of A and
B; F114's two are one each of C and D, and those two were added in T9.B20 after
session D shipped one of each past ten green byte gates *and* a clean run of this
tool. A and B are hazards a conversion **exposes** — they are latent in the tree
before it. C and D are hazards a conversion **creates**: both are sound while the
parent is a raw pointer, so no amount of pre-conversion Miri finds them, and the
only instrument that does is this scan plus the post-conversion Miri step.

**The negative calibration (S66, recorded by session H).** Every calibration
below is a *positive* one — plant a known fault, watch the scanner fire — and
S66 exists because a detector that fires on everything passes those perfectly.
Two runs, both at H's step-0 commit:

    $ python3 rust/tools/q1c.py --type SSlice --kind raw ; echo $?
    q1c: FATAL — scanned 83 files under .../src and found no function taking a
         `*mut SSlice` parameter.
    q1c: that is 'nothing to find', not 'nothing found'. Check the spelling of
         --type against the tree: grep -rn 'struct SSlice' .../src
    2

The slice family was flipped whole by session E2, so `*mut SSlice` is genuinely
gone and the right answer is to refuse rather than to report zero. That is the
**guard's** negative, and it is worth having — but note what it does *not* test:
the scanner never ran. So a second one, over a family that exists:

    $ python3 rust/tools/q1c.py --type SSlice --kind ref
    80 function bodies (79 distinct names) take a `&mut SSlice` parameter
    ...
    1 hazardous sites in 1 callers, across 1 distinct ctx-taking callees.
    80 of the 80 SSlice-taking bodies have no detected hazard at any call site.

**80 bodies scanned, 1 site reported, 79 silent.** That is the discriminating
negative: a scanner that fired on everything would report ~80 here, and this one
reports one. Re-run both beside the positives below when either scanner changes.

Both C and D are calibrated against the tree at `0bfc7687^` — the tree that
carried F114's two live defects. Shape C reports exactly the four bodies F114a
named (`WelsEncRecI4x4Y`, `WelsEncRecI16x16Y`, `WelsEncInterY`, `WelsEncRecUV`),
with the Miri-reported one line-for-line: derive `svc_encode_mb.rs:650`, borrow
`:684`. Shape D reports `AddSliceBoundary` and `DynSlcJudgeSliceBoundaryStepBack`,
the two functions T9.D9 converted. Both are clean at HEAD, where T9.D11 fixed
them. **Re-run that calibration before trusting a change to either scanner** — a
detector that has never been shown to fire is not a detector (S55).

**Shape A — a cursor held across the call.** Three parts inside one body:

    let cur = (*pCtx).some_field.as_mut_ptr();   // 1. derive a cursor from the ctx
    SomeCallee(pCtx, ...);                       // 2. call a ctx-taking callee
    *cur = ...;                                  // 3. use the cursor afterwards

Today (2) passes a raw pointer and nothing is invalidated. The moment `SomeCallee`
takes `&mut sWelsEncCtx`, step 2 invalidates `cur` and step 3 is UB. This is
F66's second confirmed site (`pSpatialIndexMap` before `InitBitStream`) — **which
T9.G2 has since retired**, along with the shape-B example below (`WelsRcInitModule`,
hoisted in T9.G6). Both are kept here as the canonical illustrations; neither is
still in the tree, so do not grep for them expecting a hit (S58).

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

**Shape E — a foreign READ pops the protector, and no `&mut` is minted anywhere**
(F148.3, added in T9.E3):

    fn WelsUpdateSliceHeaderSyntax(pCtx: *mut sWelsEncCtx, pLayer: &mut SDqLayer) {
        let n = (*current_layer(pCtx)).iMaxSliceNum;   // the SAME layer, other path

`pLayer` is strongly protected for the whole body; the accessor hands back an
independent same-tag copy of the object it names, and a plain **read** through
that copy pops the protected `Unique`. Shape D's closure looks for reborrow
*mints* and therefore cannot see this — E2's close hit it after a `--kind ref`
scan came back clean, which is exactly the gap this shape closes. Its remedy is
to read through the parameter (T9.D11's move), or to hoist the read above the
call that creates the protector.

Calibrated per S55 in **both** directions at T9.E3: at `020dfb0a^` — the tree
that carried the live defect — `--type SDqLayer --kind ref` reports **exactly
one** site, `WelsUpdateSliceHeaderSyntax` reaching `current_layer`, line for
line with what Miri named; at HEAD it reports 0, T9.D11 having fixed it. And
adding it changed nothing about the older shapes: at `0bfc7687^`, C still
reports F114a's four bodies (`WelsEncRecI16x16Y`, `WelsEncRecI4x4Y`,
`WelsEncInterY`, `WelsEncRecUV`) and D still reports its two
(`AddSliceBoundary`, `DynSlcJudgeSliceBoundaryStepBack`), identical before and
after the change.

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

DEFAULT_CTX_TYPE = "sWelsEncCtx"

# The type currently aimed at, and the three regexes derived from it. `aim()`
# rebuilds all four; everything below reads the module globals, so a caller that
# only ever wants the default gets exactly the behaviour this file always had.
CTX_TYPE = DEFAULT_CTX_TYPE
CTX_PARAM_RE = CTX_PARAM_PP_RE = LOCAL_ROOT_RE = None


# Which parameter spelling counts as "this function retags the type on entry".
# `raw` is the pre-conversion question — *which* `*mut T` parameters would retag if
# they became references — and is the default, because that is what the detector is
# for. `ref` is the **post**-conversion audit: once the parameters *are* `&mut T`,
# every one of them retags on every call, and the same scan answers "is a cursor held
# across one of those calls today". T9.D7 needed the second, and without it the tool
# reports a converted family as "nothing to find" and exits 2.
PARAM_KIND = "raw"


def aim(ctx_type, kind="raw"):
    """Point the detector at `ctx_type` — **T9.D1**.

    The tool was written around one hard-coded constant, and session D needed it
    aimed at `SMbCache`. Re-aiming by `sed`ing the constant into a copy works
    exactly once and then bites: the copy has to sit in `rust/tools/` or `ROOT`
    resolves somewhere with no source tree under it, and the scan silently finds
    nothing (see `main`, which now refuses to report that as a clean run).
    """
    global CTX_TYPE, CTX_PARAM_RE, CTX_PARAM_PP_RE, LOCAL_ROOT_RE, PARAM_KIND
    global REF_PARAM_RE, MUT_REF_PARAM_RE, RAW_LOCAL_RE
    CTX_TYPE, PARAM_KIND = ctx_type, kind
    # Shape D needs **both** reference spellings whatever `--kind` is: F114b's
    # protector was a *shared* `&SMB`. A shared reference retags harmlessly and
    # still protects, so `&T` and `&mut T` are equally the subject there.
    REF_PARAM_RE = re.compile(
        r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*&\s*(?:'[a-z_]+\s+)?(mut\s+)?"
        + ctx_type + r"\b")
    MUT_REF_PARAM_RE = re.compile(
        r":\s*&\s*(?:'[a-z_]+\s+)?mut\s+" + ctx_type + r"\b")
    # A local or parameter holding a `*mut T` — shape D's re-borrow is spelled
    # `&mut *that`.
    RAW_LOCAL_RE = re.compile(
        r"([A-Za-z_][A-Za-z0-9_]*)\s*(?::\s*\*\s*(?:mut|const)\s+" + ctx_type
        + r"\b|\s*=\s*[^;]*as\s+\*\s*mut\s+" + ctx_type + r"\b)")
    if kind == "ref":
        CTX_PARAM_RE = re.compile(
            r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*&\s*(?:'[a-z_]+\s+)?mut\s+" + ctx_type + r"\b")
        CTX_PARAM_PP_RE = re.compile(r"(?!)")          # never matches
        LOCAL_ROOT_RE = re.compile(
            r":\s*&\s*mut\s+" + ctx_type + r"\b|Box\s*::\s*into_raw\s*\(")
        return
    # A parameter of type `*mut T` (not `*mut *mut`, which would become
    # `&mut &mut` — a different type, and the four out-parameters F66 excluded).
    CTX_PARAM_RE = re.compile(
        r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*\*\s*(?:mut|const)\s+" + ctx_type + r"\b")
    CTX_PARAM_PP_RE = re.compile(
        r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*\*\s*(?:mut|const)\s+\*\s*(?:mut|const)\s+"
        + ctx_type + r"\b")
    # A local that holds a raw root. `WelsInitEncoderExt` is the case that forces
    # this: its own parameter is already owned (`&mut Option<Box<sWelsEncCtx>>`)
    # and the raw context is a *local* — `let pCtx = Box::into_raw(...)` — so a
    # parameter-only root scan sees no context in the function that holds F66's
    # first Miri-confirmed site.
    LOCAL_ROOT_RE = re.compile(
        r":\s*\*\s*mut\s+" + ctx_type + r"\b|"          # let x: *mut T = ...
        r"as\s+\*\s*mut\s+" + ctx_type + r"\b|"         # ... as *mut T
        r"Box\s*::\s*into_raw\s*\(")                     # Box::into_raw(Box::new(ctx))

aim(DEFAULT_CTX_TYPE)

FN_HEAD_RE = re.compile(
    r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:default\s+)?(?:const\s+)?(?:unsafe\s+)?"
    r"(?:extern\s+\"C\"\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")

# `let x = ...` / `let mut x = ...` — the binding a cursor lands in.
LET_RE = re.compile(r"^\s*let\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=]+)?=\s*(.*)$")

# A derivation is context-derived if it reaches through the context root and
# produces something pointer-like: a raw pointer, a reference, or a slice.
#
# **T9.E2a (F144.1): `&&` is not `&`.** The two reference alternatives are guarded
# with `(?<!&)`/`(?!&)` because a boolean chain reads as a borrow without them:
# in `let bLeft = (*pMb).iMbX > 0 && uiSliceIdc == WelsMbToSliceIdc(pLayer, iLeftXY)`
# the second `&` of `&&` sits before ` uiSliceIdc` and matched `&\s*[a-zA-Z_(*]`,
# so a held *bool* was reported as a held cursor — 11 of the layer family's 14
# "hazards" were this one shape (a value cannot be invalidated by a retag).
# Calibrated per S55 against `0bfc7687^`: shape C still reports F114a's four
# bodies with the Miri-reported one line-for-line, shape D still reports its two
# functions, and the only shape-A sites the narrowing removes are `&&`-chain
# bool bindings.
DERIV_HINT = re.compile(
    r"as_mut_ptr\(\)|as_ptr\(\)|\.as_mut\(\)|(?<!&)&(?!&)\s*mut\b|"
    r"(?<!&)&(?!&)\s*[a-zA-Z_(*]|"
    r"addr_of(?:_mut)?!\(|"
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


def owning_fields():
    """Struct fields whose declared type **is** `CTX_TYPE`, owned inline.

    **T9.D5.** A root does not have to be a parameter. `SMbCache` lives as one owned
    field of `SSlice`, and 32 bodies mint their root with

        let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);

    which `LOCAL_ROOT_RE` cannot see: the type is nowhere in the line. Reading the
    field's *declaration* out of the tree supplies it, so those 32 bodies stop being
    invisible as callers. The context struct has no such field and is unaffected.
    """
    out = set()
    pat = re.compile(r"^\s*(?:pub\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*" + CTX_TYPE + r"\s*,")
    for path in sorted(SRC.rglob("*.rs")):
        for line in path.read_text().splitlines():
            m = pat.match(strip_comment(line))
            if m:
                out.add(m.group(1))
    return out


def body_roots(lines, b, end, roots, pp, fields=()):
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
        elif any(re.search(r"(addr_of(?:_mut)?!|&\s*mut)\s*\(?[^;]*\.\s*"
                           + re.escape(f) + r"\b", rhs) for f in fields):
            out.add(nm)          # `let p = addr_of_mut!((*pSlice).sMbCacheInfo);`
    return out


def ctx_taking_callees():
    """Every function **body** in the tree with a `*mut CTX_TYPE` parameter.

    These are the calls that become a `&mut` retag when the family converts them.

    **T9.D1 — keyed by `(name, file)`, not by name.** This table used to be
    `name -> "file:line"`, so a second definition of the same name overwrote the
    first and the tool reported one body where there were two. `WelsRecPskip` is
    defined at both `svc_encode_mb.rs:928` and `svc_mode_decision.rs:484`, and
    that is F85's shape in a second tool: `find_stub_bodies.py` hid the decoder's
    `GetOption` behind the encoder's same-named one the same way. A call site
    still resolves by bare name (this is a text scan, not a resolver), so a
    *hazard* can still only be attributed to a name — but the report now prints
    **every** definition that name has, so an ambiguous attribution is visible
    instead of silently picking the last one scanned.
    """
    out = {}
    for path in sorted(SRC.rglob("*.rs")):
        lines = path.read_text().splitlines()
        for name, b, end, roots, _pp in split_bodies(lines):
            if roots:
                out.setdefault(name, []).append(f"{path.relative_to(SRC)}:{b + 1}")
    return out


class Hazard:
    __slots__ = ("shape", "file", "caller", "binding", "derive_ln", "call_ln",
                 "use_ln", "callee", "text")

    def __init__(self, shape, file, caller, binding, derive_ln, call_ln, use_ln,
                 callee, text):
        self.shape, self.file, self.caller, self.binding = shape, file, caller, binding
        self.derive_ln, self.call_ln, self.use_ln = derive_ln, call_ln, use_ln
        self.callee, self.text = callee, text


def defs_of(callees, name):
    """Every definition site of `name`, rendered for the report."""
    where = callees.get(name) or ["?"]
    return ", ".join(where)


def n_bodies(callees):
    """Definitions, not names — the two differ wherever a name is defined twice."""
    return sum(len(v) for v in callees.values())



# --------------------------------------------------------------------------
# Shape C and shape D — F114's two mechanisms, neither of which shapes A and B
# model. Added in T9.B20 after session D shipped one of each past ten green byte
# gates and a clean `q1c` run.
# --------------------------------------------------------------------------

# `let p = addr_of_mut!((*root).field)...` — a raw cursor into one named field.
# The `.cast::<i16>()` / `.add(n)` tail is deliberately not required.
ADDR_OF_FIELD_RE = re.compile(
    r"addr_of(?:_mut)?\s*!\s*\(\s*\(\s*\*\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)"
    r"\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)")


def safe_borrows_of_field(line):
    """Every **`&mut`** `(*root).field` / `root.field` in one line.

    Shape C's invalidating half. The borrow is usually not a statement of its
    own — F114a's was an argument three calls deep,
    `func(blk4x4_mut(&mut (*pMbCache).sCoeffLevel, 0), pFF, pMF)` — so this
    matches anywhere in the line rather than only at a binding.

    **`&mut` only, since T9.B30 — this used to match `&` as well and over-reported.**
    The invalidation shape C models is a `Unique` retag popping a sibling
    `SharedReadWrite` raw. A **shared** reborrow of the same field is a *read*
    through the parent, and a read access does not remove `SharedReadWrite` items
    above the tag it reaches through: the raw survives, the `SharedReadOnly` is
    pushed on top of it, and the next write through the raw pops that in turn. So
    `&(*p).field` beside a raw into the same field is sound, and reporting it as a
    hazard teaches sessions to ignore the scanner — the failure mode S55 is about,
    arriving from the over-report side rather than the zero side.

    Calibrated the same way it was introduced: with this narrowing, `q1c.py --type
    SMbCache --kind ref` at `0bfc7687^` still reports F114a's four bodies —
    `WelsEncRecI4x4Y`, `WelsEncRecI16x16Y`, `WelsEncInterY`, `WelsEncRecUV` — with
    the Miri-reported one line-for-line (derive `svc_encode_mb.rs:650`, borrow
    `:684`), because every one of those borrows is a `&mut`.
    """
    out = set()
    for m in re.finditer(
            r"&\s*mut\s+\(\s*\*\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)"
            r"\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)", line):
        out.add((m.group(1), m.group(2)))
    for m in re.finditer(
            r"&\s*mut\s+([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)",
            line):
        out.add((m.group(1), m.group(2)))
    return out


def signature(lines, name, b):
    """The whole parameter list of the body `split_bodies` reports at line `b`.

    **`b` is the opening brace, not the `fn` head.** For a signature that wraps
    across lines — which every one of the port's longer ones does — the
    parameters sit *above* `b`, so a window that reads forward from `b` sees the
    body and none of the types. That is not a hypothetical: written the naive
    way, shape D matched eleven `&mut SMB` parameters that happened to fit on one
    line and silently missed `AddSliceBoundary(pCurMb: &SMB)` at
    `svc_encode_slice.rs:3056` — F114b itself, the site the shape exists to
    catch. Walk back to the `fn` head and take everything between.
    """
    start = b
    for k in range(b, max(-1, b - 40), -1):
        m = FN_HEAD_RE.match(lines[k])
        if m and m.group(1) == name:
            start = k
            break
    return " ".join(strip_comment(x) for x in lines[start:b + 1]), start


def scan_shape_c(rel, lines, caller, b, end, roots):
    """A raw cursor into `root.field`, held across a **safe borrow of that same
    field**, and used after it — F114a.

    While `root` is `*mut T` this is sound and has been for six phases: the
    `addr_of_mut!` reuses the parent's `SharedReadWrite` item, and a sibling
    `&mut` pushes above it without popping it (T5.O8's rule, F70's site). The
    moment `root` becomes `&mut T` the same spelling gets its **own** item above
    the parent's `Unique`, and the sibling borrow pops it. So this is not a
    hazard the conversion *exposes* — it is one the conversion *creates*, which
    is why no amount of pre-conversion Miri would have found it.

    The borrow covers the **whole field**, not the sub-range the cursor uses:
    `blk4x4_mut` takes `&mut [i16]`, so the caller borrows all 384 coefficients
    before indexing. Narrowing the callee's parameter does not help.
    """
    out = []
    derived = {}                     # name -> (line, root, field)
    for j in range(b, end + 1):
        m = LET_RE.match(strip_comment(lines[j]))
        if not m or m.group(1) == "_":
            continue
        am = ADDR_OF_FIELD_RE.search(m.group(2))
        if am and am.group(1) in roots:
            derived[m.group(1)] = (j, am.group(1), am.group(2))
    if not derived:
        return out
    for nm, (dj, root, field) in derived.items():
        use_re = re.compile(r"\b" + re.escape(nm) + r"\b")
        for j in range(dj + 1, end + 1):
            s_ = strip_comment(lines[j])
            if COMMENT_RE.match(lines[j]):
                continue
            if (root, field) not in safe_borrows_of_field(s_):
                continue
            # the derivation's own line re-appearing is not a use
            for k in range(j + 1, end + 1):
                s2 = strip_comment(lines[k])
                if COMMENT_RE.match(lines[k]):
                    continue
                if LET_RE.match(s2) and LET_RE.match(s2).group(1) == nm:
                    break                      # rebound; the old cursor is dead
                if use_re.search(s2):
                    out.append(Hazard("C", rel, caller, nm, dj + 1, j + 1, k + 1,
                                      f"&mut (*{root}).{field}",
                                      lines[dj].strip()))
                    break
            break
    return out


def split_top(text):
    """Split on commas that are not inside (), [], <> or a string."""
    out, depth, cur = [], 0, ""
    for ch in text:
        if ch in "([<":
            depth += 1
        elif ch in ")]>":
            depth -= 1
        if ch == "," and depth <= 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    out.append(cur)
    return [x.strip() for x in out]


def mut_ref_positions(head):
    """Indices of the parameters typed `&mut CTX_TYPE` in a signature.

    Needed because the argument has to be matched to the **parameter position**,
    not merely to the call. Without it, `ParseInterBInfo` reads as a minter of
    `&mut SPicture` on the strength of `... (pCtx, &mut *pCurDqLayer, ..)` — the
    callee does take a `&mut SPicture` somewhere in its list, and a positionless
    scan happily types the `SDqLayer` argument as one. That single confusion was
    the whole of `SPicture`'s 12 reported sites, all of them in decoder code that
    is not even in this phase's scope.
    """
    i = head.find("(")
    if i < 0:
        return set()
    depth, j = 0, i
    for j in range(i, len(head)):
        if head[j] == "(":
            depth += 1
        elif head[j] == ")":
            depth -= 1
            if depth == 0:
                break
    return {k for k, ptxt in enumerate(split_top(head[i + 1:j]))
            if MUT_REF_PARAM_RE.search(ptxt)}


def reborrow_closure(files):
    """Functions that can mint a `&mut CTX_TYPE` — directly or through a callee.

    Shape D's other half, and the reason it has to be a closure rather than a
    one-level look: F114b's protector was on `AddSliceBoundary`'s frame and the
    conflicting `&mut` was minted by `UpdateMbNeighbourInfoForNextSlice`, which
    `AddSliceBoundary` calls. A one-level scan of the annotated function's own
    body sees nothing.

    **Minting is not the same as taking.** A body that re-borrows *its own*
    `&(mut) T` parameter is sound — that is one borrow path, not two. The
    conflict needs a **second, independent path** to the same object. So a
    signature taking `&mut T` does not make a body hazardous (that rule is
    self-referential and turned `SMB`'s 2 real sites into 69); forming a `&mut T`
    from something that is *not* one of the body's own reference parameters does.

    The port never spells the raw's type at the mint — `UpdateMbNeighbourInfo\
    ForNextSlice` writes `let pMb = ...` off the layer's macroblock list with no
    annotation — so the type is taken from the **callee's** signature instead:
    `UpdateMbNeighbor(pCurDq, &mut *pMb, ..)` mints a `&mut SMB` because
    `UpdateMbNeighbor`'s second parameter is one. That is the only inference in
    this tool and it is a cheap one.

    Still an over-approximation, the same trade the plane census makes with
    `in-fork`: a false positive costs a read, a false negative is silent UB with
    no gate behind it.
    """
    names, mut_takers, own_refs, mut_pos = set(), set(), {}, {}
    for rel, lines in files:
        for name, b, end, _roots, _pp in split_bodies(lines):
            names.add(name)
            head, _ = signature(lines, name, b)
            pos = mut_ref_positions(head)
            if pos:
                mut_takers.add(name)
                mut_pos[name] = mut_pos.get(name, set()) | pos
            own_refs[name] = {m.group(1)
                              for m in REF_PARAM_RE.finditer(head)}
    direct, calls = set(), {}
    for rel, lines in files:
        for name, b, end, _roots, _pp in split_bodies(lines):
            mine = own_refs.get(name, set())
            raws = set()
            for j in range(b, end + 1):
                mm = RAW_LOCAL_RE.search(strip_comment(lines[j]))
                if mm:
                    raws.add(mm.group(1))
            hit, out_calls = False, set()
            for j in range(b, end + 1):
                s_ = strip_comment(lines[j])
                for mm in re.finditer(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(", s_):
                    g = mm.group(1)
                    if g == name or g not in names:
                        continue
                    out_calls.add(g)
                    if g not in mut_takers:
                        continue
                    # the callee's signature types the argument: a `&mut *x`
                    # handed to a `&mut T` parameter is a `&mut T`.
                    args = split_top(call_args(lines, j, end, g, mm.end() - 1))
                    for k in mut_pos.get(g, ()):
                        if k >= len(args):
                            continue
                        am = re.search(
                            r"&\s*mut\s*\(?\s*\*\s*([A-Za-z_][A-Za-z0-9_]*)", args[k])
                        if am and am.group(1) not in mine:
                            hit = True
                # a mint with no call around it, where the raw *is* annotated
                for mm in re.finditer(
                        r"&\s*mut\s*\(?\s*\*\s*([A-Za-z_][A-Za-z0-9_]*)", s_):
                    if mm.group(1) in raws and mm.group(1) not in mine:
                        hit = True
            calls[name] = calls.get(name, set()) | out_calls
            if hit:
                direct.add(name)
    closure = set(direct)
    changed = True
    while changed:
        changed = False
        for f, gs in calls.items():
            if f not in closure and (gs & closure):
                closure.add(f)
                changed = True
    return closure, direct


def scan_shape_d(files, closure):
    """A `&(mut) CTX_TYPE` **argument** is strongly protected for the whole call;
    if anything the callee reaches re-borrows the same object, that is UB — F114b.

    The raw parameter never had a protector and neither does the C++, so, like
    shape C, the conversion *creates* this rather than exposing it. `&T` counts:
    a shared reference retags harmlessly and still protects, which is precisely
    what made `AddSliceBoundary(pCurMb: &SMB)` — "it only reads" — unsound.
    """
    out = []
    for rel, lines in files:
        for name, b, end, _roots, _pp in split_bodies(lines):
            head, start = signature(lines, name, b)
            m = REF_PARAM_RE.search(head)
            if not m:
                continue
            if name not in closure:
                continue
            out.append(Hazard("D", rel, name, m.group(1), start + 1, start + 1,
                              start + 1,
                              ("&mut " if m.group(2) else "&") + CTX_TYPE,
                              lines[start].strip()))
    return out

def accessor_names(files):
    """Functions that resolve a `CTX_TYPE` **from somewhere else** — the
    independent path shape E needs.

    Two spellings, both of which the port uses for every root type this tool is
    ever aimed at: a function returning `*mut CTX_TYPE` (`current_layer`,
    `slice_in_layer`, `ctx_dq_layer`) and one returning
    `Option<&(mut) CTX_TYPE>` (`layer_ref_pic`'s shape). A body that calls one
    of these has a second, independent handle on the object — which is the whole
    precondition of the shape.
    """
    ret = re.compile(
        r"->\s*(?:Option\s*<\s*)?(?:&\s*(?:'[a-z_]+\s+)?(?:mut\s+)?|\*\s*(?:mut|const)\s+)"
        + CTX_TYPE + r"\b")
    out = set()
    for _rel, lines in files:
        for name, b, _end, _roots, _pp in split_bodies(lines):
            head, _ = signature(lines, name, b)
            if ret.search(head):
                out.add(name)
    return out


def _accessor_reads(line, accessors):
    """The accessor calls on `line` that actually **access** the object.

    A nullness test does not: `current_layer(pCtx).is_null()` calls the
    accessor, looks at the returned pointer, and never touches a byte of the
    layer, so it pops nothing. The deref two lines below it —
    `(*current_layer(pCtx)).iMaxSliceNum` — is the access F148.3's Miri run
    named, and reporting the null check instead would point the reader at the
    wrong line. Anything else counts, including handing the resolved pointer to
    a callee, which reads it on the caller's behalf.
    """
    out = []
    for m in re.finditer(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(", line):
        if m.group(1) not in accessors:
            continue
        depth, k = 0, m.end() - 1
        for k in range(m.end() - 1, len(line)):
            if line[k] == "(":
                depth += 1
            elif line[k] == ")":
                depth -= 1
                if depth == 0:
                    break
        tail = line[k + 1:].lstrip()
        if tail.startswith("."):
            meth = re.match(r"\.\s*([A-Za-z_][A-Za-z0-9_]*)", tail)
            if meth and meth.group(1) in ("is_null", "is_none", "is_some"):
                continue
        out.append(m.group(1))
    return out


def scan_shape_e(files, accessors):
    """**A protector is violated by a foreign READ, and it needs no mint** —
    F148.3, the class E2's close caught and this tool could not see.

    Shape D models *mints*: it asks whether anything the callee reaches forms a
    second `&mut`. The site that aborted both encode probes formed none.
    `WelsUpdateSliceHeaderSyntax`, freshly `&mut SDqLayer`, simply **read**
    `(*current_layer(pCtx)).iMaxSliceNum` — the same object its own parameter
    protects, reached by an independent accessor path. A plain read through an
    independent same-tag copy pops a strongly protected `Unique`, so the whole
    hazard is: *this body has a `&mut CTX_TYPE` parameter, and it also resolves
    and accesses a `CTX_TYPE` through an accessor rather than through that
    parameter.*

    Deliberately narrower than shape D in one way and wider in another. Narrower:
    only `&mut` parameters qualify — a shared `&T` parameter is not a `Unique`
    and a foreign read cannot pop it (the protector class where a *read* is
    harmless is shape D's). Wider: no closure walk, because the offending access
    is in the body itself, in the accessor's own call. Nullness tests are not
    accesses (see `_accessor_reads`).

    The false positives it accepts, both cheap to read past and both seen in
    calibration: a body whose accessor call names a **different instance** of the
    same type (`GetRefMb`'s `mb_at(base_layer, ..)` beside its own `&mut SMB`
    — a different macroblock in a different layer, reported at `0bfc7687^`); and
    a resolution whose result is only written *through the parameter* afterwards.
    The report prints the accessor and the line so the reader can decide, which
    is the same trade shapes C and D make.
    """
    out = []
    if not accessors:
        return out
    for rel, lines in files:
        for name, b, end, _roots, _pp in split_bodies(lines):
            head, start = signature(lines, name, b)
            pm = None
            for m in REF_PARAM_RE.finditer(head):
                if m.group(2):                      # `mut ` — a Unique protector
                    pm = m
                    break
            if pm is None:
                continue
            for j in range(b, end + 1):
                if COMMENT_RE.match(lines[j]):
                    continue
                s_ = strip_comment(lines[j])
                reads = [r for r in _accessor_reads(s_, accessors) if r != name]
                if not reads:
                    continue
                out.append(Hazard("E", rel, name, pm.group(1), start + 1, j + 1,
                                  j + 1, reads[0], s_.strip()))
                break
    return out


def scan():
    callees = ctx_taking_callees()
    if not callees:
        return [], callees
    fields = owning_fields()
    callee_re = re.compile(r"\b(" + "|".join(sorted(map(re.escape, callees))) + r")\s*\(")
    hazards = []
    files = [(str(path.relative_to(SRC)), path.read_text().splitlines())
             for path in sorted(SRC.rglob("*.rs"))]
    closure, _direct = reborrow_closure(files)
    hazards += scan_shape_d(files, closure)
    hazards += scan_shape_e(files, accessor_names(files))
    for rel, lines in files:
        for caller, b, end, roots, pp in split_bodies(lines):
            roots = body_roots(lines, b, end, roots, pp, fields)
            if not roots:
                continue
            hazards += scan_shape_c(rel, lines, caller, b, end, roots)
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
    ap.add_argument("--type", default=DEFAULT_CTX_TYPE, metavar="NAME",
                    help=f"the struct to aim at (default: {DEFAULT_CTX_TYPE})")
    ap.add_argument("--kind", choices=("raw", "ref"), default="raw",
                    help="parameter spelling that retags: `*mut T` (default, the "
                         "pre-conversion question) or `&mut T` (the post-conversion audit)")
    ap.add_argument("--sites", action="store_true", help="every hazardous site")
    ap.add_argument("--callee", help="only sites whose call is to this callee")
    ap.add_argument("--caller", help="only sites inside this caller")
    a = ap.parse_args()

    aim(a.type, a.kind)

    # ------------------------------------------------------------------
    # T9.D1 — "nothing to find" and "nothing found" must not print the same.
    #
    # `ROOT` is resolved from this file's own path, so a copy of the tool run
    # from anywhere but `rust/tools/` points `SRC` at a directory with no Rust
    # in it, scans zero files, finds zero hazards and exits 0 — a confident,
    # wrong all-clear. That cost session D a wrong reading while scoping. Both
    # empty cases are now a loud exit 2.
    # ------------------------------------------------------------------
    n_files = sum(1 for _ in SRC.rglob("*.rs")) if SRC.is_dir() else 0
    if n_files == 0:
        print(f"q1c: FATAL — no Rust sources under {SRC}", file=sys.stderr)
        print("q1c: the source tree is resolved from this file's own path "
              "(ROOT = parents[2]),\n     so a copy of the tool outside "
              "`rust/tools/` scans nothing. Run it in place.", file=sys.stderr)
        return 2

    hazards, callees = scan()
    if not callees:
        print(f"q1c: FATAL — scanned {n_files} files under {SRC} and found no function "
              f"taking a\n     `*mut {CTX_TYPE}` parameter.", file=sys.stderr)
        print(f"q1c: that is 'nothing to find', not 'nothing found'. Check the spelling "
              f"of --type\n     against the tree: "
              f"grep -rn 'struct {CTX_TYPE}' {SRC}", file=sys.stderr)
        return 2
    if a.callee:
        hazards = [h for h in hazards if h.callee == a.callee]
    if a.caller:
        hazards = [h for h in hazards if h.caller == a.caller]

    print(f"q1c — F66 aliasing-hazard detector (heuristic; see the module docstring)")
    print(f"{n_bodies(callees)} function bodies ({len(callees)} distinct names) take a "
          f"`{'&mut' if PARAM_KIND == 'ref' else '*mut'} {CTX_TYPE}` parameter\n"
          f"and {'retag on every call' if PARAM_KIND == 'ref' else 'would retag on conversion'}.\n")
    dupes = {k: v for k, v in callees.items() if len(v) > 1}
    if dupes:
        print(f"{len(dupes)} of those names is defined more than once — a hazard reported "
              f"against one\ncould belong to either body (this is a text scan, not a "
              f"resolver):")
        for k, v in sorted(dupes.items()):
            print(f"  {k}   {', '.join(v)}")
        print()

    A = [h for h in hazards if h.shape == "A"]
    B = [h for h in hazards if h.shape == "B"]
    C = [h for h in hazards if h.shape == "C"]
    D = [h for h in hazards if h.shape == "D"]
    E = [h for h in hazards if h.shape == "E"]

    if a.sites:
        for h in sorted(hazards, key=lambda x: (x.file, x.derive_ln)):
            if h.shape == "A":
                print(f"A  {h.file}:{h.derive_ln}  in {h.caller}()")
                print(f"     derive {h.derive_ln:5d}  {h.text[:96]}")
                print(f"     call   {h.call_ln:5d}  {h.callee}(...)  <-- retags the whole context")
                print(f"     use    {h.use_ln:5d}  `{h.binding}` read after the call")
            elif h.shape == "B":
                print(f"B  {h.file}:{h.call_ln}  in {h.caller}()")
                print(f"     call   {h.call_ln:5d}  {h.text[:96]}")
                print(f"            argument order: `{h.callee}` takes the retag, a later "
                      f"argument still reads through the raw")
            elif h.shape == "C":
                print(f"C  {h.file}:{h.derive_ln}  in {h.caller}()")
                print(f"     derive {h.derive_ln:5d}  {h.text[:96]}")
                print(f"     borrow {h.call_ln:5d}  {h.callee}  <-- safe borrow of the "
                      f"same field pops the raw")
                print(f"     use    {h.use_ln:5d}  `{h.binding}` read after the borrow")
            elif h.shape == "D":
                print(f"D  {h.file}:{h.derive_ln}  in {h.caller}()")
                print(f"     param  {h.derive_ln:5d}  {h.binding}: {h.callee}  "
                      f"<-- strongly protected for the whole call")
                print(f"            and {h.caller} can reach a `&mut {CTX_TYPE}` "
                      f"re-borrow of what it names")
            else:
                print(f"E  {h.file}:{h.call_ln}  in {h.caller}()")
                print(f"     param  {h.derive_ln:5d}  {h.binding}  "
                      f"<-- strongly protected for the whole body")
                print(f"     read   {h.call_ln:5d}  {h.text[:96]}")
                print(f"            reaches a {CTX_TYPE} through `{h.callee}` — an "
                      f"independent path; a foreign READ pops a protected Unique")
        print()

    byfile = collections.Counter(h.file for h in hazards)
    bycaller = collections.Counter(h.caller for h in hazards)
    bycallee = collections.Counter(h.callee for h in hazards)

    w = max([len(f) for f in byfile] + [12])
    print(f"{'file':<{w}}  sites   (A=held cursor, B=arg order, "
          f"C=safe-borrow pop, D=protector, E=foreign read)")
    for f, n in byfile.most_common():
        c = collections.Counter(h.shape for h in hazards if h.file == f)
        print(f"{f:<{w}}  {n:5d}   A={c['A']} B={c['B']} C={c['C']} D={c['D']} E={c['E']}")
    bindings = {(h.file, h.caller, h.binding) for h in A}
    print(f"\n{len(hazards)} hazardous sites in {len(bycaller)} callers, "
          f"across {len(bycallee)} distinct ctx-taking callees.")
    print(f"  shape A (a cursor held across the call): {len(A)} sites, "
          f"{len(bindings)} distinct cursors at risk")
    print(f"  shape B (argument evaluation order):     {len(B)} sites")
    print(f"  shape C (raw popped by a safe borrow):   {len(C)} sites   [F114a]")
    print(f"  shape D (a reference argument protects): {len(D)} sites   [F114b]")
    print(f"  shape E (a foreign READ pops the protector): {len(E)} sites   [F148.3]")
    if C or D:
        print("  C and D are hazards the conversion *creates*, not ones it exposes:"
              "\n  both are sound while the parent is raw. Pre-conversion Miri cannot "
              "find them.")
    if bycaller:
        print("\nworst callers:")
        for c, n in bycaller.most_common(8):
            print(f"  {n:4d}  {c}")
        print("\nmost-implicated callees (these are the conversions that stay blocked):")
        for c, n in bycallee.most_common(8):
            print(f"  {n:4d}  {c}   ({defs_of(callees, c)})")

    clean = sorted(set(callees) - set(bycallee))
    n_clean = sum(len(callees[c]) for c in clean)
    print(f"\n{n_clean} of the {n_bodies(callees)} {CTX_TYPE}-taking bodies have no detected "
          f"hazard at any call site.")
    print("A clean callee is 'no hazard found', not 'proved safe' — F66 rejected "
          "converting\nthe clean subset for exactly that reason.")
    return 1 if hazards else 0


if __name__ == "__main__":
    sys.exit(main())
