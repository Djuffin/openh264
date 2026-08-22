#!/usr/bin/env python3
"""Every C++ function definition under `codec/`, and whether the port has one by name.

`find_stub_bodies.py` answers "does the Rust body do less than the C++ one" — it
can only ask that about functions the port already *has*. Nothing asked the other
question: **what is not there at all**. F80 (`IncreasePicBuff`/`DecreasePicBuff`,
never ported, `WelsRequestMem`'s third arm absent), the three parameter-set listing
strategies, the denoise and downsample plugins and the two decoder-option feeders
were each found by accident, one at a time, over four phases. This is the list they
should have come off.

    rust/tools/port_census.py                # the whole table, missing first
    rust/tools/port_census.py --present      # also list what is present
    rust/tools/port_census.py --by-file      # per C++ file: missing / present counts
    rust/tools/port_census.py --classify     # the missing list against
                                             # `port_census_classification.txt`:
                                             # dead / renamed / missing, and — the
                                             # point of it — what is UNCLASSIFIED

The match is **by name only** and deliberately dumb. A name that exists on both
sides is "present" and says nothing about the body — that is the other tool's
question, and the two are meant to be read together. `--aliases` prints the rename
table, which is the one place judgement enters.

False positives (a C++ name absent from the port) are the output; each one has to be
classified by *reading the C++*, which is what `docs/phase8b_port_census.md` records:
`dead` (no caller, SIMD-only, debug-only, threaded-decoder-only), `renamed`, or
`missing`. False negatives — a name present on both sides but meaning something
different — are the risk, and the alias table is where they get pinned.

Exit status is always 0; this is a reading aid, not a gate.
"""

import collections
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

CPP_ROOTS = [
    ROOT / "codec/decoder",
    ROOT / "codec/encoder",
    ROOT / "codec/processing",
    ROOT / "codec/common",
]
RUST_ROOT = ROOT / "rust/crates/openh264-rs/src"

# Hand-written assembly and its C wrappers: a different port question (the plan
# keeps the port scalar and the diffharness pins the scalar C++ against it), and
# ~600 names that would drown everything else.
ARCH_DIRS = {"x86", "arm", "arm64", "mips", "loongarch", "generic"}

# ...and the SIMD wrappers that live in an ordinary `src` file behind `#ifdef
# X86_ASM` / `HAVE_NEON`. `codec/common/src/mc.cpp` alone holds 168 of them, which
# is a third of everything this tool would otherwise report. The suffix is the
# reference's own convention and it is exact — no scalar function in the tree ends
# in one of these.
SIMD_SUFFIXES = (
    "_sse", "_sse2", "_sse3", "_ssse3", "_sse4", "_sse41", "_sse42", "_avx", "_avx2",
    "_mmx", "_mmi", "_neon", "_AArch64_neon", "_lsx", "_lasx", "_msa",
)

# Not functions, or spelled by the language on one side.
IGNORE = {
    "if", "for", "while", "switch", "return", "sizeof", "match", "assert",
    "static_cast", "reinterpret_cast", "const_cast", "dynamic_cast", "catch",
    "int32_t", "int16_t", "int8_t", "uint32_t", "uint16_t", "uint8_t", "int64_t",
    "uint64_t", "bool", "int", "float", "double", "void", "unsigned", "long",
    "do", "else", "operator", "typedef", "struct", "class", "union", "enum",
}

# --- the rename table -------------------------------------------------------
#
# The one place judgement enters this tool. Every row is a C++ name the port
# deliberately spells differently; the comment says why. A row here is a claim
# that the two are the same function, and it is checked by reading both.
ALIASES = {
    # The C-ABI thunk layer. Upstream's public entry points are C++ methods on
    # `CWelsDecoder` / `CWelsH264SVCEncoder`; the port's are `extern "C"` thunks
    # named for the vtable slot, with the work in a safe `Decoder`/`Encoder`
    # method underneath. `codec_api.rs`'s slot table is the correspondence.
    "Initialize": "decoder_init_c",
    "Uninitialize": "decoder_uninit_c",
    "DecodeFrame": "decoder_decode_frame_c",
    "DecodeFrameNoDelay": "decoder_decode_frame_nodelay_c",
    "DecodeFrame2": "decoder_decode_frame2_c",
    "DecodeFrameEx": "decoder_decode_frame_ex_c",
    "DecodeParser": "decoder_decode_parser_c",
    "FlushFrame": "decoder_flush_frame_c",
    "SetOption": "decoder_set_opt_c",
    "GetOption": "decoder_get_opt_c",
    "InitializeExt": "encoder_initialize_ext_c",
    "EncodeFrame": "encoder_encode_frame_c",
    "EncodeParameterSets": "encoder_encode_parameter_sets_c",
    "ForceIntraFrame": "encoder_force_intra_frame_c",
    "GetDefaultParams": "encoder_get_default_params_c",
    # Types and their methods, renamed when the port folded a C++ class into a
    # Rust struct with a different name.
    "SParserBsInfo": "ParseOnlyBsBuffers",
}

CPP_DEF = re.compile(
    r"^[A-Za-z_][\w:<>,\s\*&]*?\b(?:([A-Za-z_]\w*)::)?([A-Za-z_]\w*)\s*\([^;{}]*?\)\s*(?:const\s*)?\{",
    re.M | re.S,
)
RUST_DEF = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?"
    r"(?:extern\s+\"C\"\s+)?fn\s+([A-Za-z_]\w*)",
    re.M,
)


def arch_path(p):
    return any(part in ARCH_DIRS for part in p.parts)


def simd_name(name):
    return name.endswith(SIMD_SUFFIXES)


TRAILING_C = re.compile(r"_c$")
WELS_PREFIX = re.compile(r"^(?:CWels|SWels|Wels)")


def normalize(name):
    """The port's two mechanical renames — see `present()` for why these two."""
    return WELS_PREFIX.sub("", TRAILING_C.sub("", name)).lower().replace("_", "")


def brace_body(text, open_idx):
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[open_idx + 1 : i]
    return ""


def cpp_defs():
    """[(name, cls, file, line, statements)] for every definition under CPP_ROOTS/**/src."""
    out = []
    for root in CPP_ROOTS:
        for f in sorted(root.rglob("*.cpp")):
            if arch_path(f.relative_to(ROOT)) or "src" not in f.parts:
                continue
            text = f.read_text(errors="replace")
            for m in CPP_DEF.finditer(text):
                name = m.group(2)
                if name in IGNORE or simd_name(name):
                    continue
                body = brace_body(text, text.index("{", m.end() - 1))
                out.append(
                    (
                        name,
                        m.group(1) or "",
                        f.relative_to(ROOT),
                        text.count("\n", 0, m.start()) + 1,
                        body.count(";"),
                        len(body.splitlines()),
                    )
                )
    return out


def rust_names():
    names = set()
    for f in sorted(RUST_ROOT.rglob("*.rs")):
        names |= set(RUST_DEF.findall(f.read_text(errors="replace")))
    return names


CLASSIFICATION = Path(__file__).resolve().parent / "port_census_classification.txt"


def load_classification():
    """`name X` / `file P` -> (class, evidence). A name rule wins over a file rule."""
    by_name, by_file = {}, {}
    if not CLASSIFICATION.is_file():
        return by_name, by_file
    for raw in CLASSIFICATION.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key, _, rest = line.partition("|")
        cls, _, why = rest.partition("|")
        kind, _, what = key.strip().partition(" ")
        row = (cls.strip(), why.strip())
        if kind == "name":
            by_name[what.strip()] = row
        elif kind == "file":
            by_file[what.strip()] = row
    return by_name, by_file


def classify(missing):
    """[(row, class, evidence)] plus the unclassified, which is the whole point."""
    by_name, by_file = load_classification()
    out, unknown = [], []
    for row in missing:
        name, cls_, path = row[0], row[1], str(row[2])
        rule = by_name.get(name) or by_name.get(cls_) or by_file.get(path)
        if rule is None:
            unknown.append(row)
        else:
            out.append((row, rule[0], rule[1]))
    return out, unknown


def main():
    show_present = "--present" in sys.argv[1:]
    by_file = "--by-file" in sys.argv[1:]
    do_classify = "--classify" in sys.argv[1:]
    if "--aliases" in sys.argv[1:]:
        for k, v in sorted(ALIASES.items()):
            print(f"{k:28} -> {v}")
        return

    defs = cpp_defs()
    have = rust_names()

    # The port's two systematic renames, applied as a fallback after the exact
    # match fails. Neither is a judgement call — both are mechanical:
    #
    #   * the scalar-kernel `_c` suffix, which the port drops because it has no
    #     SIMD sibling to disambiguate from (`IdctResAddPred_c` -> `idct_res_add_pred`);
    #   * the `Wels`/`CWels` prefix and CamelCase, dropped where the function
    #     became an ordinary safe helper rather than a transliterated one
    #     (`WelsI8x8LumaPredDDR_c` -> `i8x8_luma_pred_ddr`).
    #
    # 29 of the 1351 definitions match only this way, and every one of them was
    # spot-read when the rule was added. The port's convention is still to keep
    # the reference's name; this is the tail.
    norm = {}
    for n in have:
        norm.setdefault(normalize(n), n)

    def present(name):
        return name in have or ALIASES.get(name, "\0") in have or normalize(name) in norm

    # One row per (name, file): the same name defined twice in one file is one
    # question, but the same name in two files is two places to read.
    seen, missing, ok = set(), [], []
    for name, cls, path, line, stmts, lines in defs:
        key = (name, path)
        if key in seen:
            continue
        seen.add(key)
        (ok if present(name) else missing).append((name, cls, path, line, stmts, lines))

    if by_file:
        per = {}
        for _, _, path, _, _, _ in defs:
            per.setdefault(path, [0, 0])
        for row in missing:
            per[row[2]][0] += 1
        for row in ok:
            per[row[2]][1] += 1
        print(f"{'file':58} {'missing':>8} {'present':>8}")
        for path in sorted(per, key=lambda p: (-per[p][0], str(p))):
            m, p_ = per[path]
            print(f"{str(path):58} {m:8} {p_:8}")
        print(f"\n{sum(v[0] for v in per.values())} missing, "
              f"{sum(v[1] for v in per.values())} present, {len(per)} files.")
        return

    if do_classify:
        rows, unknown = classify(missing)
        counts = collections.Counter(c for _, c, _ in rows)
        print("=== classified (rust/tools/port_census_classification.txt)")
        for cls_ in ("missing", "renamed", "dead"):
            print(f"  {cls_:8} {counts.get(cls_, 0)}")
        print("\n=== missing — the port's remaining gaps, largest first")
        mine = [(r, w) for r, c, w in rows if c == "missing"]
        for (name, cls_, path, line, stmts, lines), why in sorted(mine, key=lambda t: -t[0][4]):
            qual = f"{cls_}::{name}" if cls_ else name
            print(f"{qual:44} {stmts:4}st {lines:4}ln  {path}:{line}")
            print(f"    {why}")
        if unknown:
            print("\n=== UNCLASSIFIED — no rule covers these; read the C++ and add one")
            for name, cls_, path, line, stmts, lines in sorted(unknown, key=lambda r: -r[4]):
                qual = f"{cls_}::{name}" if cls_ else name
                print(f"{qual:44} {stmts:4}st {lines:4}ln  {path}:{line}")
        print(f"\n{len(missing)} missing by name: "
              f"{counts.get('missing', 0)} missing, {counts.get('renamed', 0)} renamed, "
              f"{counts.get('dead', 0)} dead, {len(unknown)} unclassified.")
        return

    print("=== C++ definitions with no same-name Rust definition (largest first)")
    print(f"{'name':40} {'stmts':>6} {'lines':>6}  where")
    for name, cls, path, line, stmts, lines in sorted(missing, key=lambda r: -r[4]):
        qual = f"{cls}::{name}" if cls else name
        print(f"{qual:40} {stmts:6} {lines:6}  {path}:{line}")
    print(f"\n{len(missing)} missing / {len(ok)} present "
          f"({len(missing) + len(ok)} C++ definitions, {len(have)} Rust fn names).")

    if show_present:
        print("\n=== present by name")
        for name, cls, path, line, _, _ in sorted(ok):
            qual = f"{cls}::{name}" if cls else name
            note = "" if name in have else f"  (as {ALIASES[name]})"
            print(f"{qual:40}  {path}:{line}{note}")


if __name__ == "__main__":
    main()
