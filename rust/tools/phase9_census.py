#!/usr/bin/env python3
"""The Phase 9 family census — which tagged `unsafe` site belongs to which family,
and what each family waits on.

Phase 9 retires the encoder's tagged `#[allow(unsafe_code)]` sites by making the
code safe. The tags are the work queue; this tool is the map. It reads every
`// unsafe-cat: port-raw(Phase 9)` and `// unsafe-cat: cursor` tag, finds the item
the tag sits above, and buckets it by the raw-pointer types in that item's
**parameter list**.

    rust/tools/phase9_census.py                # the whole census to stdout
    rust/tools/phase9_census.py --write        # (re)write rust/docs/phase9_census.md
    rust/tools/phase9_census.py --family coeff # list the sites in one family
    rust/tools/phase9_census.py --types        # the raw pointee-type inventory

Exit status is always 0; this is a reading aid, not a gate.

## Why the signature has to be read whole (T9.A1 / F101)

The charter's family table (`prompts/phase9.md` §2: 419 body-only / 120 ctx / 51
plane / 33 layer / 31 other / 25 coeff / 15 SMbCache) was produced by looking at
only the **first line** of each tagged item. The port wraps any signature longer
than 100 columns, so for every multi-line signature the first line is just

    pub unsafe fn WelsDctMb(

— no raw pointer visible — and the site was filed as "body-only, converts for
free". Restricting this tool to the first line reproduces the charter's table
almost exactly (420/121/51/36/39/19/9), which is what identifies it as an
artefact rather than a measurement.

Reading the whole parameter list moves **272** sites out of "body-only": the real
count is **147**, not 419. That is the number that matters, because "419 fall out
when their callees convert" is the load-bearing assumption behind the charter's
10-14 session estimate.

## How a site is assigned to a family

A signature can carry several raw types at once — `WelsDctT4_c(pDct: *mut i16,
pPixel1: *mut u8, ...)` is both coeff and plane. Such a signature can only convert
when **every** one of its raw parameters can, so its blocking family is the
**latest** of them in dependency order:

    coeff -> plane -> mbcache -> layer/pic/slice -> dispatch -> ctx -> body-tail

So each site gets two numbers, and they answer different questions:

  * `pure`    — the site's raw parameters are all of this one family. This is the
                set that family's own session can convert unaided.
  * `blocked` — the site's latest family is this one, but it carries earlier
                families too. It converts in this family's session, after the
                earlier ones have landed.

`pure + blocked` is the family's total workload; `pure` is what it can start with.

## What `pure` does and does not promise (T9.A3)

`pure` is a property of the **signature**, not of the call sites. A kernel can have
a coefficient-only signature and still be unconvertible, because what its callers
*pass* comes from somewhere else. The coefficient family is the worked example:
19 sites are coeff-pure, but only 4 could be converted in session A.

  * **4 are called directly** with an owned `[i16; N]` on the caller's stack, so
    the raw pointer is a pure `as_mut_ptr()` round-trip that deletes cleanly.
  * **15 are installed in an `SWelsFuncPtrList` slot** and have no direct caller
    at all outside their own unit tests. Production reaches them through the
    table, and the 59 call-through sites pass **SMbCache-derived walking cursors**
    — `pRes = md::coeff_level(pMbCache)` then `pRes.add(64)` once per quadrant, and
    `pBlock` likewise (`svc_encode_mb.rs:511-517`, whose S28 comment says the
    cursor is deliberately derived from the whole array because it walks every 4x4
    block). Converting the slot types therefore waits on family 3, not on family 5.

So a family's real start-set is `pure` intersected with "the callers already hold
the safe type". Sessions B-E should expect the same gap and check their call sites
before scoping, which `--family <name>` and a grep of the call sites will show.

A tag that sits above a *statement* rather than an item (an `#[allow]` on a `let`
or a `match` inside a body) has no signature to read and is reported separately as
`stmt` — those retire with the code around them, not by a signature conversion.
"""

import argparse
import collections
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "rust/crates/openh264-rs/src"
OUT = ROOT / "rust/docs/phase9_census.md"

TAG_RE = re.compile(r"//\s*unsafe-cat:\s*(.+?)\s*$")
PHASE9_TAGS = ("port-raw(Phase 9)", "cursor")

# A raw pointer's pointee, allowing `*mut *mut T` and `*mut a::b::T`; the family
# is decided by the last path segment.
RAW_RE = re.compile(
    r"\*\s*(?:mut|const)\s+(?:\*\s*(?:mut|const)\s+)?"
    r"((?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*[A-Za-z_][A-Za-z0-9_]*)"
)

# --- the families, in dependency order --------------------------------------
#
# The order is the conversion order and it is forced, not chosen (F66): a
# `&mut sWelsEncCtx` function-entry retag invalidates the whole 65-field context
# under Stacked Borrows, so every raw cursor derived from the context has to
# retire before the context parameter itself can become a borrow. `body` is not a
# family you convert; it is the tail that goes safe when its callees do.
ORDER = ["coeff", "plane", "mbcache", "layer", "dispatch", "other", "ctx", "body"]

FAMILY_OF_TYPE = {
    # 1. coefficient buffers — DCT / Hadamard / quant / scan / NZC blocks.
    "i16": "coeff",
    # 2. plane and prediction byte cursors.
    "u8": "plane",
    # 3. per-macroblock metadata.
    "SMbCache": "mbcache",
    "SMB": "mbcache",
    "SMVComponentUnit": "mbcache",
    "SMotionTextureUnit": "mbcache",
    # 4. layer / picture / slice storage.
    "SDqLayer": "layer",
    "SSlice": "layer",
    "SPicture": "layer",
    "SSourcePicture": "layer",
    "SSliceCtx": "layer",
    "SSliceHeader": "layer",
    "SSliceArgument": "layer",
    "SDynamicSlicingStack": "layer",
    "Scaled_Picture": "layer",
    "SPixMap": "layer",
    # 5. dispatch tables and the fn-pointer slots.
    "SWelsFuncPtrList": "dispatch",
    "DeblockingFunc": "dispatch",
    "Option": "dispatch",
    # 6. the context itself.
    "sWelsEncCtx": "ctx",
}

# Everything else is `other`, but `other` is 100+ sites across unrelated
# subsystems, so it is sub-grouped for the per-session scoping. These are
# reporting labels only — they do not affect the dependency order.
OTHER_GROUP = {
    "BsWriter": "bitstream", "SLayerBSInfo": "bitstream", "SFrameBSInfo": "bitstream",
    "SWelsEncoderOutput": "bitstream",
    "SWelsSPS": "paramset", "SWelsPPS": "paramset", "SSubsetSps": "paramset",
    "SExistingParasetList": "paramset", "SParaSetOffsetVariable": "paramset",
    "SNalUnitHeaderExt": "paramset", "SWelsSvcCodingParam": "paramset",
    "SSpatialLayerConfig": "paramset", "SSpatialLayerInternal": "paramset",
    "SWelsSvcRc": "rc",
    "SLTRState": "ltr", "SLTRConfig": "ltr", "SLTRRecoverRequest": "ltr",
    "SLTRMarkingFeedback": "ltr", "SRefInfoParam": "ltr", "SRefJudgement": "ltr",
    "SVAAFrameInfo": "vaa", "SVAAFrameInfoExt_t": "vaa", "CWelsPreProcess": "vaa",
    "SScreenBlockFeatureStorage": "vaa",
    "SDeblockingFilter": "deblock",
    "SLogContext": "trace",
    "i32": "scalar", "u32": "scalar", "u16": "scalar", "i8": "scalar",
    "bool": "scalar", "c_void": "scalar", "f32": "scalar", "i64": "scalar",
}

FAMILY_DOC = {
    "coeff":    ("Coefficient buffers", "`*mut i16` / `*const i16` DCT, Hadamard, quant, scan and NZC blocks. Kernels are already safe; the `unsafe` is a shim wrapping each for a raw caller."),
    "plane":    ("Plane / byte cursors", "`*mut u8` plane and prediction cursors, and with them `common/`'s mc / sad / deblocking / intra-pred shims."),
    "mbcache":  ("MB metadata", "`*mut SMbCache` / `*mut SMB` per-macroblock metadata — the encoder's `MbGrid` analogue."),
    "layer":    ("Layer / picture / slice", "`*mut SDqLayer` / `SSlice` / `SPicture` and the slice-argument and slicing-stack pointers reached as owned storage."),
    "dispatch": ("Dispatch tables", "`SWelsFuncPtrList` slots and the fn-pointer types — de-virtualized the way Phase 4a did `pfMdCost`."),
    "other":    ("Other raw signatures", "Bitstream writers, parameter sets, the rate controller, LTR, VAA and scalar out-parameters. Not one family; see the sub-group table."),
    "ctx":      ("Context parameter", "`*mut sWelsEncCtx`. **Blocked** until every cursor above retires (F66)."),
    "body":     ("Body-only", "No raw pointer in the signature; unsafe only because the body derefs a cursor or calls an unsafe fn. Not converted directly — goes safe when its callees do."),
}


def item_after(lines, i):
    """The first non-attribute, non-comment, non-blank line after the tag at `i`."""
    j = i + 1
    while j < len(lines):
        s = lines[j].strip()
        if s.startswith("#[") or s.startswith("//") or s == "":
            j += 1
            continue
        return j
    return None


def signature(lines, j):
    """The full signature at line `j`, joined, up to and including the return type.

    Returns None if the item is not a function — a tag above a statement inside a
    body has no signature and is counted as `stmt`.
    """
    head = lines[j].strip()
    if not re.match(r"^(pub(\([a-z]+\))?\s+)?(default\s+)?(const\s+)?(unsafe\s+)?(extern\s+\"C\"\s+)?fn\s", head):
        return None
    buf, depth, started = "", 0, False
    k = j
    while k < len(lines) and k - j < 60:
        seg = lines[k]
        buf += " " + seg.strip()
        for ch in seg:
            if ch == "(":
                depth += 1
                started = True
            elif ch == ")":
                depth -= 1
        if started and depth <= 0:
            break
        k += 1
    # The return type and any `where` clause sit after the closing paren.
    if k < len(lines):
        rest = lines[k]
        idx = rest.rfind(")")
        if idx >= 0:
            tail = rest[idx + 1:]
            buf += " " + tail.split("{")[0]
    return re.sub(r"\s+", " ", buf).strip()


def fn_name(sig):
    m = re.search(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)", sig)
    return m.group(1) if m else "?"


def param_list(sig):
    i, j = sig.find("("), sig.rfind(")")
    return sig[i:j + 1] if 0 <= i < j else ""


def pointees(text):
    out = []
    for t in RAW_RE.findall(text):
        out.append(t.split("::")[-1].strip())
    return out


def family_of(pointee, siblings=()):
    """The family a raw pointee belongs to.

    One contextual rule: `*const u16` is normally a motion-estimation cost table
    (`pMvdCost`, `pFeatureOfBlock`, ...) and so `other`/scalar — but alongside a
    `*mut i16` it is the dequantisation multiplier table, which makes
    `WelsDequant4x4_c(pRes: *mut i16, kpMF: *const u16)` a coefficient kernel and
    not a mixed-family one. Only these two dequant kernels match.
    """
    if pointee == "u16" and "i16" in siblings:
        return "coeff"
    return FAMILY_OF_TYPE.get(pointee, "other")


class Site:
    __slots__ = ("file", "line", "tag", "kind", "name", "sig", "types", "families", "blocking")

    def __init__(self, file, line, tag, kind, name, sig, types):
        self.file, self.line, self.tag = file, line, tag
        self.kind, self.name, self.sig = kind, name, sig
        self.types = types
        self.families = sorted({family_of(t, types) for t in types}, key=ORDER.index)
        if kind == "stmt":
            self.blocking = "stmt"
        elif not self.families:
            self.blocking = "body"
        else:
            self.blocking = max(self.families, key=ORDER.index)

    @property
    def pure(self):
        return len(self.families) == 1


def tag_family(tag):
    """The tag's category, with any ` — reason` annotation stripped.

    **F200**: sessions E3/G/H annotated tags in place (`port-raw(Phase 9) — the
    in-fork *mut SDqLayer (S63)`), and this tool's exact match silently dropped
    them — 16 tags were invisible when session J's disposition census recounted.
    The family is the prefix; the annotation is the site's own business.
    """
    return tag.split(" — ")[0].strip()


def collect():
    sites = []
    for path in sorted(SRC.rglob("*.rs")):
        rel = str(path.relative_to(SRC))
        lines = path.read_text().splitlines()
        for i, ln in enumerate(lines):
            m = TAG_RE.search(ln)
            if not m or tag_family(m.group(1)) not in PHASE9_TAGS:
                continue
            j = item_after(lines, i)
            sig = signature(lines, j) if j is not None else None
            if sig is None:
                sites.append(Site(rel, i + 1, tag_family(m.group(1)), "stmt", "-", "", []))
            else:
                sites.append(Site(rel, i + 1, tag_family(m.group(1)), "fn", fn_name(sig), sig,
                                  pointees(param_list(sig))))
    return sites


def collect_all_tags():
    """Every `unsafe-cat` tag by family — the lawful categories beside the queue
    (D-exit-3's table)."""
    from collections import Counter
    c = Counter()
    for path in sorted(SRC.rglob("*.rs")):
        for ln in path.read_text().splitlines():
            m = TAG_RE.search(ln)
            if m:
                c[tag_family(m.group(1))] += 1
    return c


# --- reporting ---------------------------------------------------------------

def tbl(rows, head):
    w = [len(h) for h in head]
    for r in rows:
        for n, cell in enumerate(r):
            w[n] = max(w[n], len(str(cell)))
    out = ["| " + " | ".join(h.ljust(w[n]) for n, h in enumerate(head)) + " |"]
    out.append("|" + "|".join("-" * (w[n] + 2) for n in range(len(head))) + "|")
    for r in rows:
        out.append("| " + " | ".join(str(c).ljust(w[n]) for n, c in enumerate(r)) + " |")
    return "\n".join(out)


def report(sites):
    L = []
    add = L.append
    pr = [s for s in sites if s.tag == "port-raw(Phase 9)"]
    cu = [s for s in sites if s.tag == "cursor"]

    add("# Phase 9 census — the family map\n")
    add("*Generated by `rust/tools/phase9_census.py`. Regenerate with "
        "`python3 rust/tools/phase9_census.py --write`; do not hand-edit.*\n")
    add(f"**{len(sites)} queue tags**: {len(pr)} `port-raw(Phase 9)` + {len(cu)} `cursor` — "
        "the remaining conversion queue after D-exit-3 (session J), which recategorized "
        "the lawful survivors. The full tag table:\n")
    cats = collect_all_tags()
    add(tbl(sorted(((k, v) for k, v in cats.items()), key=lambda kv: -kv[1]),
            ["category", "tags"]))
    add("")
    add("`fork-shared(S63)` is the in-fork closure (soundness argument: "
        "`phase9_disposition.md` §2); `instrument(test)` are tags on `#[test]` items; "
        "`lawful-single(...)` carry their reason at the site. The queue below is owned "
        "by `phase9_disposition.md` §4.\n")
    add("**This census reads signatures.** For the plane family a signature census is")
    add("not enough to scope against (F103, and F104 which measures it), so the")
    add("companion `phase9_plane_census.md` — `rust/tools/phase9_plane_callers.py` —")
    add("reads the **call sites** instead and classifies each operand by the surface it")
    add("names (source / reference / reconstruction / SMbCache / coefficient). Read both.\n")

    add("## 1. The correction this census makes\n")
    add("The charter (`prompts/phase9.md` §2) reports **419 body-only** sites that")
    add("\"fall out as their callees go safe — not converted directly\". That number is an")
    add("artefact of reading only the **first line** of each tagged item: the port wraps")
    add("signatures past 100 columns, so a multi-line signature's first line is bare")
    add("(`pub unsafe fn WelsDctMb(`) and the site was filed as having no raw parameter.\n")
    body = [s for s in pr if s.blocking == "body"]
    add("Measured at the charter's own commit `aabd0da5` (695 tags), reading the whole")
    add("parameter list gives **147** body-only, not 419 — **272 sites** carry raw")
    add("parameters and need a signature conversion in some family's session. That is")
    add("F101, and it is the number the phase estimate rests on. For comparison, forcing")
    add("this tool to look at the first line only reproduces the charter's table almost")
    add("exactly: 420/121/51/36/39/19/9 against its 419/120/51/33/31/25/15.\n")
    add(f"At **this** commit the body-only set is **{len(body)}** of **{len(pr)}** "
        f"`port-raw(Phase 9)` tags; the two figures differ as conversions land.\n")

    add("## 2. Families, in dependency order\n")
    add("`pure` = every raw parameter is of this family, so this family's session can")
    add("convert the site unaided. `blocked` = the site's *latest* family is this one but")
    add("it also carries earlier families, so it converts here only after those land.")
    add("`pure + blocked` is the family's real workload.\n")
    add("**`pure` is a property of the signature, not of the call sites.** A kernel can")
    add("have a single-family signature and still be unconvertible, because what its")
    add("callers *pass* comes from elsewhere. The coefficient family is the worked")
    add("example (F103): at `aabd0da5` 19 sites were coeff-pure, but only 5 had a direct")
    add("caller passing an owned `[i16; N]` — those 5 converted in T9.A3/A4 and are gone")
    add("from the table above. The other 14 sit in `SWelsFuncPtrList` slots with no direct")
    add("caller outside their own unit tests, and the 59 call-through sites pass")
    add("**SMbCache-derived walking cursors** (`pRes = md::coeff_level(pMbCache)`, then")
    add("`pRes.add(64)` per quadrant). Those slots wait on family 3, not family 5.")
    add("Sessions B-E should expect the same gap and read their call sites before")
    add("scoping — `--family <name>` lists the signatures, but only a grep of the callers")
    add("says what is actually reachable.\n")
    rows = []
    for fam in ORDER:
        inf = [s for s in pr if s.blocking == fam]
        if fam == "body":
            rows.append(("—", "body", FAMILY_DOC[fam][0], len(inf), 0, len(inf)))
            continue
        p = sum(1 for s in inf if s.pure)
        rows.append((ORDER.index(fam) + 1, fam, FAMILY_DOC[fam][0], p, len(inf) - p, len(inf)))
    stmt = [s for s in pr if s.blocking == "stmt"]
    rows.append(("—", "stmt", "Statement-level allows (no signature)", len(stmt), 0, len(stmt)))
    add(tbl(rows, ["#", "family", "what it is", "pure", "blocked", "total"]))
    add(f"\nTotal `port-raw(Phase 9)`: **{len(pr)}**.\n")
    for fam in ORDER:
        add(f"- **{fam}** — {FAMILY_DOC[fam][1]}")
    add("")

    add("## 3. Family x file\n")
    fams = [f for f in ORDER if any(s.blocking == f for s in pr)] + ["stmt"]
    byfile = collections.defaultdict(collections.Counter)
    for s in pr:
        byfile[s.file][s.blocking] += 1
    rows = []
    for f in sorted(byfile, key=lambda x: -sum(byfile[x].values())):
        rows.append([f] + [byfile[f].get(k, "") or "" for k in fams] + [sum(byfile[f].values())])
    rows.append(["**total**"] + [sum(1 for s in pr if s.blocking == k) for k in fams] + [len(pr)])
    add(tbl(rows, ["file"] + fams + ["all"]))
    add("")

    add("## 4. What each family waits on\n")
    add("```")
    add("  coeff ---+")
    add("  plane ---+")
    add("  mbcache -+--> ctx (F66: a &mut ctx retag invalidates all 65 fields,")
    add("  layer ---+          so every context-derived cursor must retire first)")
    add("             |")
    add("  dispatch --+ (independent of the cursor spine; can run any time)")
    add("             |")
    add("             v")
    add("           body (147 sites: no signature change, they go safe when callees do)")
    add("```\n")
    add("`other` is not one family — it is bitstream, parameter-set, rate-control, LTR,")
    add("VAA, deblock, trace and scalar out-parameters, each independent of the others")
    add("and of the cursor spine except where it also carries a context parameter.\n")
    og = collections.Counter()
    for s in pr:
        if s.blocking != "other":
            continue
        for t in s.types:
            if family_of(t, s.types) == "other":
                og[OTHER_GROUP.get(t, "misc")] += 1
    add(tbl(sorted(og.items(), key=lambda kv: (-kv[1], kv[0])),
            ["`other` sub-group", "type occurrences"]))
    add("")

    add("## 5. The `cursor` tags\n")
    add(f"{len(cu)} tags: the owned-storage cursor accessors (S28/S40) and the dispatch")
    add("survivors. Classified the same way, by the signature each sits above.\n")
    rows = []
    cbf = collections.defaultdict(collections.Counter)
    for s in cu:
        cbf[s.file][s.blocking] += 1
    cfams = [f for f in ORDER + ["stmt"] if any(s.blocking == f for s in cu)]
    for f in sorted(cbf, key=lambda x: -sum(cbf[x].values())):
        rows.append([f] + [cbf[f].get(k, "") or "" for k in cfams] + [sum(cbf[f].values())])
    rows.append(["**total**"] + [sum(1 for s in cu if s.blocking == k) for k in cfams] + [len(cu)])
    add(tbl(rows, ["file"] + cfams + ["all"]))
    add("")

    add("## 6. Scoping the remaining sessions\n")
    add("Sizes are `pure` (convertible by that session alone) and `total` (its full")
    add("workload once the earlier families have landed). The charter's §8 table is")
    add("scoped off the first-line numbers and is superseded by this one.\n")
    rows = []
    for fam, sess, note in [
        ("coeff", "A -> D", "session A converted the 5 with a direct caller; the 14 left are table slots that follow their data into family 3 (F103)"),
        ("plane", "B-C", "largest cursor family; pulls `common/`'s mc/sad/intra-pred shims with it"),
        ("mbcache", "D", "45 pure — 3x the charter's 15 — **plus the 14 coefficient slot types and their 59 call-throughs**; `svc_mode_decision.rs` 19, `svc_set_mb_syn_cabac.rs` 11, `md.rs` 11"),
        ("layer", "E", "47 pure — `svc_encode_slice.rs` 21, `svc_enc_slice_segment.rs` 11"),
        ("dispatch", "F", "the 5 signature sites plus the 22 `cursor` survivors"),
        ("other", "?", "**unscoped in the charter** — 123 sites, 7 unrelated sub-groups; needs its own split"),
        ("ctx", "G-H", "F66's conversion; 111 pure is the set `q1c.py` gates"),
        ("body", "-", "no conversion; audited at the phase close once the callees are safe"),
    ]:
        inf = [x for x in pr if x.blocking == fam]
        # a body-only site has no families at all, so `pure` is meaningless there
        p_ = "—" if fam == "body" else sum(1 for x in inf if x.pure)
        rows.append((fam, sess, p_, len(inf), note))
    add(tbl(rows, ["family", "session", "pure", "total", "note"]))
    add("")
    add("Two things the charter does not budget for:\n")
    add("1. **`other` is 123 sites and has no owner.** It is not one family — bitstream,")
    add("   parameter sets, RC, LTR, VAA, deblock, trace and scalar out-parameters. 80 of")
    add("   them are pure and independent of the cursor spine, so they can run in parallel")
    add("   with B-E, but somebody has to be given them.")
    add("2. **`ctx` is 234, not 120.** 111 are ctx-only (F66's 109 convertible plus the two")
    add("   `*mut *mut sWelsEncCtx` out-parameters); the other 123 carry a cursor *as well*")
    add("   as the context, so they convert only after that cursor's family lands. The")
    add("   ctx session is therefore last in fact as well as in principle.\n")

    add("## 7. Raw pointee-type inventory\n")
    add("Every distinct pointee behind a `*mut`/`*const` in a tagged parameter list, with")
    add("the number of tagged signatures it appears in. This is the evidence behind the")
    add("family assignment in `FAMILY_OF_TYPE`.\n")
    tc = collections.Counter()
    for s in pr + cu:
        for t in set(s.types):
            tc[t] += 1
    rows = [(t, n, family_of(t), OTHER_GROUP.get(t, "") if family_of(t) == "other" else "")
            for t, n in sorted(tc.items(), key=lambda kv: (-kv[1], kv[0]))]
    # `u16`'s row shows its default family; the contextual rule above moves the two
    # dequant kernels to `coeff` and is noted there, not here.
    add(tbl(rows, ["pointee", "signatures", "family", "sub-group"]))
    add("")
    return "\n".join(L)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--write", action="store_true", help="(re)write rust/docs/phase9_census.md")
    ap.add_argument("--family", help="list the sites in one family")
    ap.add_argument("--types", action="store_true", help="raw pointee-type inventory only")
    a = ap.parse_args()
    sites = collect()

    if a.family:
        for s in sites:
            if s.blocking == a.family:
                mark = "pure " if s.pure else "mixed"
                print(f"{mark} {s.file}:{s.line}  {s.name}  [{','.join(s.families) or '-'}]")
                print(f"        {s.sig[:160]}")
        n = sum(1 for s in sites if s.blocking == a.family)
        print(f"\n{n} sites in family {a.family}")
        return
    if a.types:
        tc = collections.Counter()
        for s in sites:
            for t in set(s.types):
                tc[t] += 1
        for t, n in sorted(tc.items(), key=lambda kv: (-kv[1], kv[0])):
            print(f"{n:5d}  {t:32s} {family_of(t)}")
        return

    text = report(sites)
    if a.write:
        OUT.write_text(text + "\n")
        print(f"wrote {OUT.relative_to(ROOT)} ({len(sites)} tags)")
    else:
        print(text)


if __name__ == "__main__":
    main()
