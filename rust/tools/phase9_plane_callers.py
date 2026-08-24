#!/usr/bin/env python3
"""The Phase 9 plane family's **caller** census — T9.B1, session B's step 0.

`phase9_census.py` classifies each tagged `unsafe` site by its **signature**.
That is the wrong instrument for scoping the plane family, and F103 says why:
`pure` is a property of the signature, not of the call sites. A plane kernel can
have a single-family signature and still be unconvertible because of what its
callers *pass*. The coefficient family lost 15 of 19 sites that way.

So this tool reads the other end. For every raw-pointer plane entry point — the
shims and the `SWelsFuncPtrList` / `SSampleDealingFunc` slots — it finds the call
sites, extracts each pointer-valued argument, and classifies the argument by
**which surface it names**:

    src    the source (spatial) picture     read-only for the whole frame
    ref    a reference picture              read-only for the whole frame
    rec    the reconstruction picture       WRITTEN by every worker of the fork
    cache  an SMbCache-owned buffer         single-owner scratch
    local  a caller stack array             single-owner scratch
    ?      unclassified — read it by hand

and marks each call site `in-fork` (reachable from one of the three thread bodies
`EncodeOneSliceInJob` / `EncodeOnePartitionSizeLimited` /
`UpdateMbListNeighborParallel`) or `ST-only`.

`in-fork` is deliberately an **over-approximation**: reachability follows every
dispatch slot and every file-scope function table, so a preprocessing step that
shares a slot with the encode tree reads as `in-fork`
(`processing/scene_change_detection.rs` is one). A false `in-fork` costs caution;
a false `ST-only` would cost soundness, so the error is pushed that way on
purpose. Read the `safe-now`/`blocked` column as the primary answer and this one
as the qualifier.

A call site is **safe-now** when every operand is `src`/`ref`/`cache`/`local`:
those surfaces are either read-only while a frame encodes (so any number of
shared `PlaneCursor`s across threads is sound) or single-owner. A call site with a
`rec` operand is **blocked** on the reconstruction-write design (session B step 5,
session C's conversion) — writing that plane cannot be expressed as one
`&mut [u8]` per worker, because a slice may start mid-macroblock-row.

    rust/tools/phase9_plane_callers.py             # the caller table (markdown)
    rust/tools/phase9_plane_callers.py --sites     # every call site, one per line
    rust/tools/phase9_plane_callers.py --unknown   # only the unclassified operands
    rust/tools/phase9_plane_callers.py --internal  # the kernel-internal composition calls
    rust/tools/phase9_plane_callers.py --handoff   # slots passed/stored, not called

Exit status is 0 on a clean read and **1 when any operand is unclassified**.

That last part is the T9.B20 change and it is not cosmetic. The classifier keys
on how an operand is *spelled*, so a refactor that renames a buffer does not fail
the tool — it silently moves call sites into `?`, and `?` is the column nobody
reads. Session D renamed the ten `md::mem_pred_*` accessors to direct field
access; this census went from 0 unclassified to 40 and from 13 coefficient-gated
to 0, printed the new numbers without comment, and exited 0. Session B2 scoped
against those numbers. S46 and S55 are the same lesson twice (an empty scan and
an uncalibrated detector must be loud); this is the third.

If a survivor is genuinely unclassifiable, add it to `UNKNOWN_ALLOWLIST` with the
reason — an allowlisted unknown still prints, it just stops failing.
"""

import argparse
import os
import re
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.normpath(os.path.join(HERE, "..", "crates", "openh264-rs", "src"))

# --------------------------------------------------------------------------
# The plane entry points: the raw-signature kernels and the table slots that
# hold them. `args` names the argument positions that carry a plane pointer,
# so an entry point with a coefficient or scalar first parameter reports only
# the pixel operands.
# --------------------------------------------------------------------------
SHIMS = {
    # name                        -> (group, plane-carrying arg positions)
    "WelsSampleSad4x4_c":         ("sad", (0, 2)),
    "WelsSampleSad8x4_c":         ("sad", (0, 2)),
    "WelsSampleSad4x8_c":         ("sad", (0, 2)),
    "WelsSampleSad8x8_c":         ("sad", (0, 2)),
    "WelsSampleSad16x8_c":        ("sad", (0, 2)),
    "WelsSampleSad8x16_c":        ("sad", (0, 2)),
    "WelsSampleSad16x16_c":       ("sad", (0, 2)),
    "WelsSampleSadFour4x4_c":     ("sad4", (0, 2)),
    "WelsSampleSadFour8x4_c":     ("sad4", (0, 2)),
    "WelsSampleSadFour4x8_c":     ("sad4", (0, 2)),
    "WelsSampleSadFour8x8_c":     ("sad4", (0, 2)),
    "WelsSampleSadFour16x8_c":    ("sad4", (0, 2)),
    "WelsSampleSadFour8x16_c":    ("sad4", (0, 2)),
    "WelsSampleSadFour16x16_c":   ("sad4", (0, 2)),
    "WelsSampleSatd4x4_c":        ("satd", (0, 2)),
    "WelsSampleSatd8x4_c":        ("satd", (0, 2)),
    "WelsSampleSatd4x8_c":        ("satd", (0, 2)),
    "WelsSampleSatd8x8_c":        ("satd", (0, 2)),
    "WelsSampleSatd16x8_c":       ("satd", (0, 2)),
    "WelsSampleSatd8x16_c":       ("satd", (0, 2)),
    "WelsSampleSatd16x16_c":      ("satd", (0, 2)),
    "WelsCopy4x4_c":              ("copy", (0, 2)),
    "WelsCopy8x4_c":              ("copy", (0, 2)),
    "WelsCopy4x8_c":              ("copy", (0, 2)),
    "WelsCopy8x8_c":              ("copy", (0, 2)),
    "WelsCopy16x8NotAligned_c":   ("copy", (0, 2)),
    "WelsCopy8x16_c":             ("copy", (0, 2)),
    "WelsCopy16x16_c":            ("copy", (0, 2)),
    "WelsCopy16x16NotAligned_c":  ("copy", (0, 2)),
    "WelsDctT4_c":                ("dct", (0, 1, 3)),
    "WelsDctFourT4_c":            ("dct", (0, 1, 3)),
    "WelsDctMb":                  ("dct", (0, 1, 3)),
    "WelsIDctT4Rec_c":            ("idct", (0, 2, 4)),
    "WelsIDctFourT4Rec_c":        ("idct", (0, 2, 4)),
    "WelsIDctRecI16x16Dc_c":      ("idct", (0, 2, 4)),   # F137, added T9.C2
    "WelsIDctT4RecOnMb":          ("idct", (0, 2, 4)),
    "McLuma_c":                   ("mc", (0, 2)),
    "McChroma_c":                 ("mc", (0, 2)),
    # **T9.B24 (session B3).** The four `mc.rs` entry points the encoder's fractional
    # refinement calls directly (`md.rs`, `MeRefineFracPixel`/`MeRefineQuarPixel`):
    # the three half-pel filters and the two-source average. They were never in this
    # table, so the census read 20 MC sites where the tree has 30 — the charter's B3
    # row says "20" and session B3's brief says "30" for that reason (F120). Same
    # group as `McLuma_c`: a plane source and an arena destination, no table slot
    # (Phase 4a made MC direct), so they retire caller by caller.
    "McHorVer20_c":               ("mc", (0, 2)),
    "McHorVer02_c":               ("mc", (0, 2)),
    "McHorVer22_c":               ("mc", (0, 2)),
    # `(pDst, iDstStride, pSrcA, iSrcAStride, pSrcB, iSrcBStride, w, h)`.
    "PixelAvg_c":                 ("mc", (0, 2, 4)),
    "ExpandReferencingPicture":   ("expand", (0, 1, 2)),
}
# The intra predictors all share `(pPred, pRef, kiStride)`.
for _n in (
    "WelsI4x4LumaPredV_c WelsI4x4LumaPredH_c WelsI4x4LumaPredDc_c WelsI4x4LumaPredDcLeft_c "
    "WelsI4x4LumaPredDcTop_c WelsI4x4LumaPredDcNA_c WelsI4x4LumaPredDDL_c WelsI4x4LumaPredDDLTop_c "
    "WelsI4x4LumaPredDDR_c WelsI4x4LumaPredVL_c WelsI4x4LumaPredVLTop_c WelsI4x4LumaPredVR_c "
    "WelsI4x4LumaPredHU_c WelsI4x4LumaPredHD_c WelsIChromaPredV_c WelsIChromaPredH_c "
    "WelsIChromaPredPlane_c WelsIChromaPredDc_c WelsIChromaPredDcLeft_c WelsIChromaPredDcTop_c "
    "WelsIChromaPredDcNA_c WelsI16x16LumaPredV_c WelsI16x16LumaPredH_c WelsI16x16LumaPredDc_c "
    "WelsI16x16LumaPredPlane_c WelsI16x16LumaPredDcLeft_c WelsI16x16LumaPredDcTop_c "
    "WelsI16x16LumaPredDcNA_c"
).split():
    SHIMS[_n] = ("intrapred", (0, 1))

# Table slots, by the name that appears at the call site. `args` as above.
SLOTS = {
    # **T9.B25/B28/B29 — the cost tables' *raw* names only.** `pfSampleSad`,
    # `pfSampleSatd`, `pfSample4Sad`, `md_cost` and `me_cost` are the **safe** slots
    # now (`fn(&PlaneCursor, &PlaneCursor) -> i32`) and their call sites are
    # *converted* — counting them would report finished work as remaining. The raw
    # tables carry the transitional `*Raw` suffix and are what is left to convert;
    # they and these five entries are deleted together when the last raw reader goes.
    #
    # Listing only the raw names is also what keeps the tool honest through the
    # campaign: a site that converts *leaves* the census, and a site that is merely
    # re-spelled does not (S58, and F116's lesson arriving by a different route).
    "pfSampleSadRaw":     ("sad", (0, 2)),
    "pfSampleSatdRaw":    ("satd", (0, 2)),
    "pfSample4SadRaw":    ("sad4", (0, 2)),
    "md_cost_raw":        ("sad|satd", (0, 2)),
    "me_cost_raw":        ("sad|satd", (0, 2)),
    "pfGetLumaI4x4Pred":  ("intrapred", (0, 1)),
    "pfGetLumaI16x16Pred": ("intrapred", (0, 1)),
    "pfGetChromaPred":    ("intrapred", (0, 1)),
    "pfCopy16x16Aligned": ("copy", (0, 2)),
    "pfCopy16x16NotAligned": ("copy", (0, 2)),
    "pfCopy8x8Aligned":   ("copy", (0, 2)),
    "pfCopy16x8NotAligned": ("copy", (0, 2)),
    "pfCopy8x16Aligned":  ("copy", (0, 2)),
    "pfCopy4x4":          ("copy", (0, 2)),
    "pfCopy8x4":          ("copy", (0, 2)),
    "pfCopy4x8":          ("copy", (0, 2)),
    # `pfCopyBlockByMode` is `SMeRefinePointer`'s own slot, safe since T9.B29 — its
    # one call site is converted, so it leaves the census with the others.

    "pfIDctT4":           ("idct", (0, 2, 4)),
    "pfIDctFourT4":       ("idct", (0, 2, 4)),
    # **F137, added T9.C2.** Missing since the census was written, and it is a
    # *blocked* site: `WelsEncRecI16x16Y`'s DC-only branch
    # (`svc_encode_mb.rs:616`) writes the reconstruction plane through `pPred`
    # from the `pBestPred` arena, the same operand pair as the four
    # `pfIDctFourT4` calls twenty lines above it that the census always listed.
    # Same `PIDctFunc` signature, so the same operand positions.
    "pfIDctI16x16Dc":     ("idct", (0, 2, 4)),
    "pfDctT4":            ("dct", (0, 1, 3)),
    "pfDctFourT4":        ("dct", (0, 1, 3)),
}

# --------------------------------------------------------------------------
# Operand classification. First match wins, so the specific `SPicData` fields
# are tested before the bare names they contain.
# --------------------------------------------------------------------------
CLASSIFIERS = [
    # The per-macroblock carrier, which is unambiguous and therefore first.
    # The receiver is deliberately *not* pinned to the literal `SPicData`: the
    # bundle is also reached through a binding that already names it, and
    # `AcceptPskip` takes it as a `&SPicData` parameter called `kpPicData`
    # (`svc_base_layer_md.rs:1602`) — one `.pEncMb[0]` operand that a
    # receiver-pinned rule reports as unclassified. The four field names are
    # unique to this carrier and to `SWelsME`, whose `pEncMb`/`pRefMb` name the
    # same two surfaces, so widening the receiver cannot mis-class anything.
    ("rec",   r"\.\s*(pCsMb|pDecMb)\b"),
    ("src",   r"\.\s*pEncMb\b"),
    ("ref",   r"\.\s*(pRefMb|pColoRefMb)\b"),
    # SMbCache-owned scratch, reached through the `md::` accessors.
    # SMbCache-owned scratch. The `md::` accessor names are session D's *former*
    # spelling: T9.D7 deleted every one of them in favour of reaching the field
    # directly (`addr_of_mut!((*pMbCache).sSkipMb)`), so the field spellings are
    # what actually appears at HEAD. Both are kept — the accessor arm costs
    # nothing and the file's history still reads.
    ("cache", r"(?:encoder::)?md::(mem_pred_\w+|best_pred_\w+|skip_mb|buffer_inter_pred_me)"
              r"|\bsMemPred\w*|\bsBufferInterPredMe|\bsSkipMb\b|\bsBestPred\w*"),
    # Coefficient buffers — family 3's, not this family's (F103).
    # Coefficient buffers — family 3's, not this family's (F103). `sCoeffLevel`
    # is the same session-D respelling as `sSkipMb` above, and it is the single
    # biggest hole: 13 dct and 10 idct operands read as unclassified without it.
    ("coeff", r"(?:encoder::)?md::(coeff_level|dct)\b|\biLumaBlock\b|\biChromaBlock\b"
              r"|\bg_kiQuant\w*|\bget_quant_\w+|\bg_kuiDequant\w*|\bsCoeffLevel\b"),
    # **The reconstruction seam (T9.C2).** A site whose reconstruction operand is
    # a `RecCursor` is *converted*, not blocked and not unclassifiable: the plane
    # is reached through `RecPicView` and the write goes through `&self`. Tested
    # before the raw-root arm below so a converted site never reads as `rec`, and
    # given its own class so the census cannot confuse "done" with "unknown" —
    # which is exactly what the five intra-pred sites did at first measurement.
    ("seam",  r"\bview\s*\.\s*plane\s*\(|\blayer_rec_view\b|\bRecCursor\b"
              r"|&\s*pCurDec\b|&\s*pDest(?:Y|Cb|Cr)\b"),
    # The layer's stamped plane roots and views.
    ("rec",   r"\bpCsData\b|sDecPicView|\bpDecPic\b"),
    ("src",   r"\bpEncData\b|\bpEncPic\b"),
    ("ref",   r"sRefPicView|\bpRefPic\b"),
    # The VAA frame info's plane copies (`wels_preprocess.rs` stamps `pCurY` from
    # the source picture and `pRefY` from the reference).
    ("src",   r"pVaaInfo\s*\)\s*\.\s*pCur[YUV]|\bpCur[YUV]\b"),
    ("ref",   r"pVaaInfo\s*\)\s*\.\s*pRef[YUV]|\bpRef[YUV]\b"),
    # `WelsMdI4x4Fast`'s `score!`/`alt_buf!` macro pair: `$dst` is always
    # `md::mem_pred_blk4(pMbCache).offset(..)` at every expansion
    # (`svc_base_layer_md.rs:560-563`), which no textual walk of the macro body
    # can see.
    ("cache", r"^\$dst$"),
    # `MeRefineQuarPixel`'s four averages read `pParams.pSrcA[i]` / `pSrcB[i]`, stamped
    # by `MeRefineFracPixel` per half-pel arm (`md.rs:1539-1660`): `pSrcA` is a
    # `pBufMe` offset in every arm; `pSrcB` is the reference block in the no-best arm
    # and a `pBufMe`/reference mix in the four others. Classed by the surface that
    # *can* appear — the verdict (`safe-now`) is the same either way, and the
    # alias walk cannot follow a field of a parameter.
    ("cache", r"\bpParams\s*\.\s*pSrcA\b"),
    ("ref",   r"\bpParams\s*\.\s*pSrcB\b"),
    # `SWelsME`'s own plane cursors — the motion search's source block and the
    # reference block it walks with the candidate vector.
    ("src",   r"pMe\s*\)?\s*\.\s*pEncMb"),
    ("ref",   r"pMe\s*\)?\s*\.\s*(pRefMb|pColoRefMb)"),
    ("local", r"\bas_mut_ptr\s*\(\)|\bas_ptr\s*\(\)|\bnull_mut\s*\(\)"),
]

# Parameters an alias walk cannot follow, because the value comes from the
# caller. Each was read once, at the anchor named, and the class is the same at
# every call site of that function.
PARAM_CLASS = {
    # `md::buffer_inter_pred_me(pMbCache)` at both callers — and the parameter's
    # own doc comment says so (`md.rs:1485-1488`).
    ("MeRefineQuarPixel", "pBufMe"): "cache",
    ("MeRefineFracPixel", "pBufMe"): "cache",
    # `pDstLuma`/`pDstCb`/`pDstCr`, all `md::mem_pred_*` (`svc_base_layer_md.rs:1638-1640`).
    ("MeRefineFracPixel", "pMemPredInterMb"): "cache",
    # `md::skip_mb(pMbCache)` (`svc_base_layer_md.rs:1470`, `WelsMdPSkipEnc`).
    ("AcceptPskip", "pDstLuma"): "cache",
    # `SPicData.pEncMb[1..2]` and the *original* picture behind the reference
    # (`SDqLayer::pRefOri[0]` through `ctx_pic_ref_mut`) at all four call sites
    # (`svc_mode_decision.rs:1907`, `:1915`, `:1971`, `:1979`).
    ("CalUVSadCost", "pEncOri"): "src",
    ("CalUVSadCost", "pRefOri"): "ref",
}

# Slots handed on as *values* rather than called: `pfCalculateSatd` takes a
# `pfSampleSatd` slot as its first argument, `SFeatureSearchIn::pSad` stores one.
# Re-typing a slot re-types these signatures too, so they are counted separately
# rather than missed.
HANDOFF_RE = re.compile(
    r"(pfSampleSad|pfSampleSatd|pfSample4Sad)(?:Raw)?\s*\[[^\]]*\]\s*(?![.(\[])")

# Identifiers an alias walk must not mistake for a variable root.
STOPWORDS = {"if", "else", "match", "let", "mut", "as", "isize", "usize", "i32",
             "u8", "i16", "crate", "self", "std", "unsafe", "return", "Some", "None"}

FORK_ROOTS = ("EncodeOneSliceInJob", "EncodeOnePartitionSizeLimited",
              "UpdateMbListNeighborParallel")

FN_RE = re.compile(
    r'^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+"C"\s+)?fn\s+([A-Za-z0-9_]+)')


def rust_files():
    out = []
    for root, _dirs, files in os.walk(SRC):
        for f in sorted(files):
            if f.endswith(".rs"):
                out.append(os.path.join(root, f))
    return sorted(out)


def rel(path):
    return os.path.relpath(path, SRC)


def strip_comment(line):
    """Drop a trailing `//` comment. Crude but adequate: no `//` appears inside a
    string literal in the bodies this walks, and a false strip would only lose a
    call site, never invent one."""
    i = line.find("//")
    return line if i < 0 else line[:i]


def fn_spans(lines):
    """[(start, end, name)] for every `fn` in the file, 0-based, end inclusive.

    Nested fns (test helpers, closures written as fns) are included; a call site
    is attributed to the **innermost** span containing it."""
    spans = []
    for i, raw in enumerate(lines):
        m = FN_RE.match(raw)
        if not m:
            continue
        # Walk to the opening brace of the body, then brace-match.
        depth, j, seen = 0, i, False
        while j < len(lines):
            for ch in strip_comment(lines[j]):
                if ch == "{":
                    depth += 1
                    seen = True
                elif ch == "}":
                    depth -= 1
            if seen and depth == 0:
                break
            if not seen and strip_comment(lines[j]).rstrip().endswith(";"):
                break  # a trait/extern declaration, no body
            j += 1
        if seen:
            spans.append((i, j, m.group(1)))
    return spans


def owner_of(spans, line_idx):
    best = None
    for (s, e, name) in spans:
        if s <= line_idx <= e and (best is None or s > best[0]):
            best = (s, e, name)
    return best[2] if best else "<file scope>"


def balanced_args(text, open_idx):
    """The argument list of a call whose `(` is at `open_idx`, split at depth-1
    commas. Returns (args, index just past the matching `)`) or (None, -1)."""
    depth, i, start, args = 0, open_idx, open_idx + 1, []
    while i < len(text):
        c = text[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                args.append(text[start:i])
                return [a.strip() for a in args], i + 1
        elif c == "," and depth == 1:
            args.append(text[start:i])
            start = i + 1
        i += 1
    return None, -1


# `(file, line, operand)` triples that are known-unclassifiable, each with the
# reason. An entry here still prints in `--unknown`; it just does not fail the
# run. Empty is the correct state: every operand at HEAD classifies.
UNKNOWN_ALLOWLIST = {}


def classify(expr):
    for cls, pat in CLASSIFIERS:
        if re.search(pat, expr):
            return cls
    return "?"


def build_index():
    """(files -> text/lines/spans), the set of all fn names, and the call graph."""
    files = {}
    names = set()
    for p in rust_files():
        lines = open(p, encoding="utf-8").read().split("\n")
        spans = fn_spans(lines)
        files[p] = (lines, spans)
        names.update(n for (_s, _e, n) in spans)
    return files, names


def call_graph(files, names):
    """Direct-call edges, plus the two indirect routes the encoder dispatches through.

    * **`SWelsFuncPtrList` slots.** A slot install (`pfX = Some(F)` /
      `pfX[i] = Some(F)`) makes `F` a callee of every function whose body names
      `pfX`.
    * **File-scope function tables.** `g_pWelsSliceCoding` and
      `g_pWelsWriteSliceHeader` are `static` arrays of function items, and
      `WelsCodeOneSlice` indexes them — the whole per-macroblock encode tree hangs
      off exactly those two. A `static`/`const` item holding known function names
      makes them callees of every function whose body names the table.

    Both over-approximate: a function that only *assigns* a slot gets an edge. That
    is the right direction for a reachability question whose answer decides whether
    a call site may run on a worker thread — a false `in-fork` costs caution, a
    false `ST-only` would cost soundness."""
    edges = defaultdict(set)
    provides = defaultdict(set)   # container (slot or table) -> {fn it can dispatch}
    readers = defaultdict(set)    # container -> {fn whose body names it}
    # `Some(F)` / `Some(path::to::F)`, anywhere. The slot it belongs to is the
    # nearest `pf*` name at or above it within SLOT_LOOKBACK lines: the port
    # installs several slots as `pfX = if cond { Some(A) } else { Some(B) };`,
    # which a same-line regex misses entirely (`svc_encode_slice.rs:2618-2622`
    # is `pfInterMd`, and missing it hides the whole P-macroblock tree).
    some_re = re.compile(r"Some\s*\(\s*(?:[A-Za-z0-9_]+\s*::\s*)*([A-Za-z0-9_]+)\s*\)")
    slot_re = re.compile(r"\b(pf[A-Za-z0-9_]+)\b")
    SLOT_LOOKBACK = 6
    item_re = re.compile(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:static|const)\s+([A-Za-z0-9_]+)\s*:")

    for p, (lines, spans) in files.items():
        cur_item = None       # nearest preceding file-scope static/const
        for i, raw in enumerate(lines):
            line = strip_comment(raw)
            mi = item_re.match(raw)
            if mi:
                cur_item = mi.group(1)
            owner = owner_of(spans, i)
            in_fn = owner != "<file scope>"

            for m in some_re.finditer(line):
                if m.group(1) not in names:
                    continue
                slot = None
                for back in range(0, SLOT_LOOKBACK + 1):
                    if i - back < 0:
                        break
                    hits = slot_re.findall(strip_comment(lines[i - back]))
                    if hits:
                        slot = hits[-1] if back else hits[0]
                        break
                if slot:
                    provides[slot].add(m.group(1))
            for m in slot_re.finditer(line):
                readers[m.group(1)].add(owner)

            for m in re.finditer(r"\b([A-Za-z0-9_]+)\b\s*(\()?", line):
                ident = m.group(1)
                if ident not in names:
                    continue
                if m.group(2):
                    edges[owner].add(ident)          # a call
                elif not in_fn and cur_item:
                    provides[cur_item].add(ident)    # a file-scope function table
            if cur_item and in_fn and re.search(r"\b" + re.escape(cur_item) + r"\b", line):
                readers[cur_item].add(owner)

    # A table read anywhere in a body: second pass, now that every table is known.
    tables = {k for k in provides if not k.startswith("pf")}
    for p, (lines, spans) in files.items():
        for i, raw in enumerate(lines):
            line = strip_comment(raw)
            for t in tables:
                if re.search(r"\b" + re.escape(t) + r"\b", line):
                    readers[t].add(owner_of(spans, i))

    for container, rs in readers.items():
        for r in rs:
            edges[r] |= provides.get(container, set())
    return edges


def reachable(edges, roots):
    seen, stack = set(roots), list(roots)
    while stack:
        cur = stack.pop()
        for nxt in edges.get(cur, ()):
            if nxt not in seen:
                seen.add(nxt)
                stack.append(nxt)
    return seen


ASSIGN_RE = re.compile(
    r"^\s*(?:let\s+(?:mut\s+)?)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::\s*[^=]*?)?=\s*(.*?);?\s*$")


def _balanced(text):
    d = 0
    for c in text:
        if c in "([{":
            d += 1
        elif c in ")]}":
            d -= 1
    return d <= 0


def assignments(lines, spans):
    """[(line, name, rhs, owner)] for every `let x = ..` / `x = ..` in the file.

    A right-hand side that is empty or unbalanced on its own line takes the next
    three lines with it: the port wraps at 100 columns, and
    `let pPredIntraChma: [*mut u8; 2] =` puts the whole answer on the line below
    (`svc_base_layer_md.rs:711`)."""
    out = []
    for i, raw in enumerate(lines):
        m = ASSIGN_RE.match(strip_comment(raw))
        if not m:
            continue
        rhs = m.group(2) or ""
        j = i
        while (not rhs.strip() or not _balanced(rhs)) and j + 1 < len(lines) and j - i < 3:
            j += 1
            rhs += " " + strip_comment(lines[j]).strip()
        if rhs.strip():
            out.append((i, m.group(1), rhs, owner_of(spans, i)))
    return out


def root_ident(expr):
    """The variable an operand expression is built from: `pRefCb.offset(x)` ->
    `pRefCb`, `(*pMbCache).SPicData...` -> None (already classified),
    `pPredIntraChma[0]` -> `pPredIntraChma`."""
    # `[` has to be stripped as well as `&*( `, and for two different reasons.
    # Leading: an operand may be an array *literal* whose first element is the
    # root — `let pPredIntraChma = [pMemPredChroma, pMemPredChroma.add(128)]`,
    # the right-hand side the walk lands on from `pDstChma`. Without it the
    # regex fails on `[` and the walk dead-ends one hop short of the `sMemPredMb`
    # that classifies it, losing four `WelsMdIntraChroma` operands.
    e = expr.strip().lstrip("&*([ ")
    m = re.match(r"([A-Za-z_][A-Za-z0-9_]*)", e)
    if not m or m.group(1) in STOPWORDS:
        return None
    return m.group(1)


def resolve(expr, fn, line, assigns, depth=0):
    """`classify`, then follow local aliases up the function until something
    classifies. Bounded, and never leaves the function the call sits in."""
    cls = classify(expr)
    if cls != "?" or depth >= 6:
        return cls
    name = root_ident(expr)
    if name is None:
        return "?"
    best = None
    for (i, n, rhs, owner) in assigns:
        if n == name and owner == fn and i < line and (best is None or i > best[0]):
            best = (i, rhs)
    if best is None:
        return PARAM_CLASS.get((fn, name), "?")
    return resolve(best[1], fn, best[0], assigns, depth + 1)


def test_spans(lines):
    """Line ranges of `#[cfg(test)] mod tests { .. }` — excluded from the census.

    A shim's own unit test calls it with a stack array and would otherwise report
    as a `local`/`local` safe-now site, inflating exactly the number this table
    exists to scope a session against."""
    spans = []
    for i, raw in enumerate(lines):
        if "cfg(test)" not in raw:
            continue
        j, depth, seen = i, 0, False
        while j < len(lines):
            for ch in strip_comment(lines[j]):
                if ch == "{":
                    depth += 1
                    seen = True
                elif ch == "}":
                    depth -= 1
            if seen and depth == 0:
                break
            j += 1
        spans.append((i, j))
    return spans


def collect_sites(files, in_fork):
    """Every call site of a plane entry point, with its classified operands.

    Three routes reach a kernel and all three are walked:

      * `direct`   — the shim called by name;
      * `slot`     — `(*pFunc).pfSampleSad[BLOCK_16x16].unwrap()(..)` inline;
      * `local`    — the slot hoisted into a local first (`let pfSatd4x4 = ..`,
                     `if let Some(f) = ..`), then called. The binding is scoped to
                     its own function: `func` is the port's habitual name for a
                     hoisted slot and a file-wide binding table would attribute
                     every `func(` in the file to the first slot bound in it.

    Sites inside a `#[cfg(test)] mod tests` are dropped, and a site whose enclosing
    function is itself one of the raw kernels is marked `internal` — the composite
    shims build the big shapes out of the small ones (`WelsSampleSad16x16_c` sums
    four 8x8 quadrants), and those calls disappear with the shims rather than
    needing a caller conversion."""
    sites = []
    let_re = re.compile(r"\blet\s+(?:mut\s+)?([A-Za-z0-9_]+)\s*(?::[^=]*?)?=\s*(.*)$")
    some_bind_re = re.compile(r"\bSome\s*\(\s*(?:mut\s+)?([A-Za-z0-9_]+)\s*\)\s*=\s*(.*)$")

    for p, (lines, spans) in files.items():
        tspans = test_spans(lines)
        assigns = assignments(lines, spans)
        # Every rebinding of a name, whatever it is bound from: `func` is the
        # port's habitual name for a hoisted slot and `WelsEncRecI4x4Y` rebinds it
        # six times over six different slots (`svc_encode_mb.rs:668-698`), so a
        # shadow table built only from plane slots reads `pfIDctT4`'s operands as
        # `pfDctT4`'s.
        shadows = defaultdict(list)
        for (i, n, _rhs, owner) in assigns:
            shadows[owner].append((i, n))
        for i, raw in enumerate(lines):
            for m in re.finditer(r"\bSome\s*\(\s*(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*\)\s*=",
                                 strip_comment(raw)):
                shadows[owner_of(spans, i)].append((i, m.group(1)))

        def in_test(ln):
            return any(a <= ln <= b for a, b in tspans)

        # Slot-bound locals, scoped to the function they are bound in.
        # bindings[fn_name] = [(line, local_name, group, argpos, slot)]
        # A slot is often hoisted twice — `let pSad = ..pfSampleSad[b];` then
        # `if let Some(sad_fn) = pSad { sad_fn(..) }` — so the binding pass runs to
        # a fixpoint over the aliases it has already found
        # (`svc_motion_estimate.rs:701` -> `:718`, and the same shape at `:866`,
        # `:1004`). One pass sees `pSad` and none of the four call sites.
        bindings = defaultdict(list)
        for _round in range(4):
            grew = False
            for i, raw in enumerate(lines):
                line = strip_comment(raw)
                for rx in (let_re, some_bind_re):
                    mb = rx.search(line)
                    if not mb:
                        continue
                    rhs, owner, name = mb.group(2), owner_of(spans, i), mb.group(1)
                    hit = None
                    for slot, (grp, argpos) in SLOTS.items():
                        if re.search(r"\b" + slot + r"\b", rhs):
                            hit = (grp, argpos, slot)
                            break
                    if hit is None:
                        for (bl, bn, bg, ba, bs) in bindings.get(owner, []):
                            if bl < i and re.search(r"(?<![.\w])" + re.escape(bn) + r"(?![\w])", rhs):
                                hit = (bg, ba, bs)
                                break
                    if hit and not any(b[0] == i and b[1] == name
                                       for b in bindings.get(owner, [])):
                        bindings[owner].append((i, name, hit[0], hit[1], hit[2]))
                        grew = True
                    break
            if not grew:
                break

        text = "\n".join(lines)
        offs, acc = [], 0
        for l in lines:
            offs.append(acc)
            acc += len(l) + 1

        def line_of(idx):
            lo, hi = 0, len(offs) - 1
            while lo < hi:
                mid = (lo + hi + 1) // 2
                if offs[mid] <= idx:
                    lo = mid
                else:
                    hi = mid - 1
            return lo

        def add(idx, entry, grp, argpos, via):
            ln = line_of(idx)
            if in_test(ln):
                return
            owner = owner_of(spans, ln)
            open_idx = text.index("(", idx)
            args, _ = balanced_args(text, open_idx)
            if args is None or owner == "<file scope>" or owner == entry:
                return
            ops = []
            for pos in argpos:
                if pos >= len(args):
                    continue
                e = re.sub(r"\s+", " ", args[pos])
                ops.append((e, resolve(e, owner, ln, assigns)))
            sites.append({
                "file": rel(p), "line": ln + 1, "fn": owner, "entry": entry,
                "group": grp, "via": via, "ops": ops,
                "fork": owner in in_fork,
                "internal": owner in SHIMS,
            })

        for name, (grp, argpos) in SHIMS.items():
            for mm in re.finditer(r"(?<![.\w])" + name + r"\s*\(", text):
                if text[:mm.start()].rstrip().endswith("fn"):
                    continue
                add(mm.end() - 1, name, grp, argpos, "direct")

        for slot, (grp, argpos) in SLOTS.items():
            for mm in re.finditer(r"\b" + slot + r"\b", text):
                tail = text[mm.end():mm.end() + 400]
                # `md_cost(block)` / `me_cost(block)` *select* a kernel; the call
                # that takes the operands is the one after the `.unwrap()`, so for
                # those two the suffix is required rather than optional.
                suffix = "" if slot in ("md_cost_raw", "me_cost_raw") else "?"
                m2 = re.match(r"(\s*\[[^\]]*\]\s*|\s*\([^)]*\)\s*)?"
                              r"(\s*\.\s*(unwrap|expect)\s*\([^)]*\)\s*)" + suffix + r"\(",
                              tail, re.S)
                if not m2:
                    continue
                add(mm.end() + m2.end() - 1, slot, grp, argpos, "slot")

        for fn, binds in bindings.items():
            for (bline, local, grp, argpos, slot) in binds:
                for mm in re.finditer(r"(?<![.\w])" + re.escape(local) + r"\s*\(", text):
                    ln = line_of(mm.end() - 1)
                    if ln <= bline or owner_of(spans, ln) != fn:
                        continue
                    # The nearest binding of this name above the call wins.
                    # Any rebinding of the name between the binding and the call
                    # shadows it. `func` is the port's habitual name for a hoisted
                    # slot and `WelsEncRecI4x4Y` rebinds it six times over six
                    # different slots, so plane-slot rebindings alone are not enough.
                    if any(bline < i2 < ln and n2 == local
                           for (i2, n2) in shadows.get(fn, ())):
                        continue
                    add(mm.end() - 1, slot + " via " + local, grp, argpos, "local")

    seen, out = set(), []
    for s in sorted(sites, key=lambda s: (s["file"], s["line"], s["entry"])):
        key = (s["file"], s["line"], tuple(e for e, _ in s["ops"]))
        if key in seen:
            continue
        seen.add(key)
        out.append(s)
    return out


def verdict(site):
    """What stands between this call site and a safe-typed kernel call.

    `blocked` beats `coeff` beats `safe-now`: a site with both a reconstruction
    operand and a coefficient one waits on the later of the two families."""
    classes = {c for _e, c in site["ops"]}
    if not site["ops"]:
        return "n/a"
    if "?" in classes:
        return "?"
    if "rec" in classes:
        return "blocked"
    if "seam" in classes:
        return "seam"
    if "coeff" in classes:
        return "coeff"
    return "safe-now"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sites", action="store_true", help="one line per call site")
    ap.add_argument("--unknown", action="store_true", help="only unclassified operands")
    ap.add_argument("--internal", action="store_true",
                    help="the kernel-internal composition calls, excluded from the tables")
    ap.add_argument("--handoff", action="store_true",
                    help="places a cost slot is passed or stored as a value, not called")
    ap.add_argument("--group", help="restrict to one kernel group")
    ap.add_argument("--allow-unknown", action="store_true",
                    help="report unclassified operands but exit 0 anyway")
    args = ap.parse_args()

    files, names = build_index()
    edges = call_graph(files, names)
    in_fork = reachable(edges, FORK_ROOTS)
    sites = collect_sites(files, in_fork)
    internal = [s for s in sites if s["internal"]]
    sites = [s for s in sites if not s["internal"]]
    if args.group:
        sites = [s for s in sites if args.group in s["group"]]
        internal = [s for s in internal if args.group in s["group"]]

    unknown = [(s["file"], s["line"], e)
               for s in sites for e, c in s["ops"] if c == "?"]
    unknown = [u for u in unknown if u not in UNKNOWN_ALLOWLIST]

    def finish(rc=0):
        """Fail loudly on an unclassified operand — see the module docstring."""
        if not unknown or args.allow_unknown:
            return rc
        print(f"\n!! {len(unknown)} UNCLASSIFIED OPERAND(S) — this census is not "
              f"safe to scope a session against.", file=sys.stderr)
        print("   The classifier keys on how an operand is spelled, so a rename "
              "moves sites into `?`", file=sys.stderr)
        print("   rather than failing. Run --unknown, then either extend "
              "CLASSIFIERS or add the", file=sys.stderr)
        print("   survivor to UNKNOWN_ALLOWLIST with its reason.", file=sys.stderr)
        for f, ln, e in unknown[:10]:
            print(f"     {f}:{ln}  <<{e}>>", file=sys.stderr)
        if len(unknown) > 10:
            print(f"     ... and {len(unknown) - 10} more", file=sys.stderr)
        return 1

    if args.handoff:
        for path in rust_files():
            lines, spans = files[path]
            tsp = test_spans(lines)
            for i, raw in enumerate(lines):
                if any(a <= i <= b for a, b in tsp):
                    continue
                for m in HANDOFF_RE.finditer(strip_comment(raw)):
                    print(f"{rel(path)}:{i + 1:<5} {owner_of(spans, i):<32} "
                          f"{m.group(1)} passed/stored as a value")
        return finish()

    if args.internal:
        for s in internal:
            print(f"{s['file']}:{s['line']:<5} {s['fn']:<28} -> {s['entry']}")
        return finish()

    if args.unknown:
        for s in sites:
            for e, c in s["ops"]:
                if c == "?":
                    print(f"{s['file']}:{s['line']}  {s['fn']}  {s['entry']}  <<{e}>>")
        return finish()

    if args.sites:
        for s in sites:
            ops = " ".join(f"{c}:{e}" for e, c in s["ops"])
            print(f"{s['file']}:{s['line']:<5} {'fork' if s['fork'] else 'ST  '} "
                  f"{verdict(s):<8} {s['group']:<10} {s['entry']:<28} {s['fn']:<30} {ops}")
        return finish()

    # Group x verdict, and file x verdict.
    by_group = defaultdict(lambda: defaultdict(int))
    by_file = defaultdict(lambda: defaultdict(int))
    for s in sites:
        v = verdict(s)
        by_group[s["group"]][v] += 1
        by_group[s["group"]]["fork" if s["fork"] else "st"] += 1
        by_file[s["file"]][v] += 1
        by_file[s["file"]]["fork" if s["fork"] else "st"] += 1

    cols = ["safe-now", "seam", "coeff", "blocked", "?", "n/a", "fork", "st"]

    def table(title, d, keyname):
        rows = [[k] + [str(d[k].get(c, 0)) for c in cols] for k in sorted(d)]
        tot = [str(sum(d[k].get(c, 0) for k in d)) for c in cols]
        rows.append(["**total**"] + tot)
        head = [keyname] + cols
        w = [max(len(r[i]) for r in rows + [head]) for i in range(len(head))]
        print(f"\n### {title}\n")
        print("| " + " | ".join(h.ljust(w[i]) for i, h in enumerate(head)) + " |")
        print("|" + "|".join("-" * (w[i] + 2) for i in range(len(head))) + "|")
        for r in rows:
            print("| " + " | ".join(r[i].ljust(w[i]) for i in range(len(head))) + " |")

    print(f"{len(sites)} plane call sites ({len(internal)} kernel-internal "
          f"composition calls excluded — they die with their shims); "
          f"{sum(1 for s in sites if verdict(s) == 'safe-now')} safe-now, "
          f"{sum(1 for s in sites if verdict(s) == 'blocked')} blocked on the "
          f"reconstruction write, "
          f"{sum(1 for s in sites if verdict(s) == 'coeff')} gated on the coefficient "
          f"family (F103), {sum(1 for s in sites if verdict(s) == '?')} unclassified.")
    table("By kernel group", by_group, "group")
    table("By file", by_file, "file")
    return finish()


if __name__ == "__main__":
    sys.exit(main())
