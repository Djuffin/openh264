#!/usr/bin/env python3
"""Flag Rust functions whose body calls strictly fewer things than the C++ original.

Stubs under a correct name have been the dominant defect class in this port for
three phases running: `find_dup_types.sh` and every audit built on it match on
*names*, so a function that exists but does a fraction of the work passes
silently. Phase 4.5 lost most of its time to three such stubs, Phase 4.6 found
seven more (one missing an entire colour plane), and Phase 5.1 found the same
shape one level up — a faithful `GomRCInitForOneSlice` with no call site.

The check is crude and that is the point: extract the set of identifiers that
appear in call position in each body and diff them. If the Rust side calls
strictly fewer things, read it.

    rust/tools/find_stub_bodies.py                    # whole encoder + processing
    rust/tools/find_stub_bodies.py WelsMdInterMb ...  # only these functions
    rust/tools/find_stub_bodies.py --dups             # duplicated Rust definitions

False positives are cheap and expected — macros (`ST32`, `LD32`, `WELS_MAX`),
calls made through a local binding or a function pointer, and anything Rust
spells as an operator or method. False negatives are what matter: a name in the
C++ body that is absent from the Rust body is worth a look every time.

**Known limitation (F85).** The C++ map is keyed by bare function name and keeps the
*longest* body when a name is defined more than once, so two classes with a method of
the same name collapse into one row: `CWelsDecoder::GetOption` (53 statements) is
invisible behind `CWelsH264SVCEncoder::GetOption` (230). Anything the decoder's
version calls and the encoder's does not is therefore un-diffable by this tool. Read
`port_census.py --classify` and the reference beside it when the name is a method.

Exit status is always 0; this is a reading aid, not a gate.
"""

import importlib.util
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CPP_DIRS = [
    ROOT / "codec/encoder/core/src",
    ROOT / "codec/decoder/core/src",
    ROOT / "codec/common/src",
    ROOT / "codec/processing/src",
    # **T8b.A2.** The two `plus` directories were missing, so the whole public
    # entry-point layer — `CWelsDecoder::GetOption`/`SetOption`/`DecodeParser`,
    # `CWelsH264SVCEncoder::ForceIntraFrame` and their neighbours — was never
    # compared against anything. F80 and the 21 `GetOption` gtest rows were both
    # in this blind spot. The matching Rust files are `api/codec_api.rs` and
    # `encoder/wels_encoder_ext.rs`, already inside RUST_DIRS.
    ROOT / "codec/decoder/plus/src",
    ROOT / "codec/encoder/plus/src",
]
RUST_DIRS = [
    ROOT / "rust/crates/openh264-rs/src/encoder",
    ROOT / "rust/crates/openh264-rs/src/decoder",
    ROOT / "rust/crates/openh264-rs/src/processing",
    ROOT / "rust/crates/openh264-rs/src/common",
    ROOT / "rust/crates/openh264-rs/src/api",
]

IDENT = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(")

# Spelled differently on the two sides, or a language construct rather than a call.
IGNORE = {
    "if", "for", "while", "switch", "return", "sizeof", "match", "assert",
    "static_cast", "reinterpret_cast", "const_cast", "dynamic_cast",
    "int32_t", "int16_t", "int8_t", "uint32_t", "uint16_t", "uint8_t", "int64_t",
    "uint64_t", "bool", "int", "float", "double", "void", "unsigned", "long",
    "memset", "memcpy", "WelsMemset", "WelsMemcpy", "printf", "fprintf",
    "WelsLog", "WELS_VERIFY_RETURN_IF", "WELS_VERIFY_RETURN_IFNEQ",
    "eprintln", "println", "format", "write", "vec", "Some", "None", "Ok", "Err",
    "unsafe", "expect", "unwrap", "is_null", "as_ptr", "as_mut_ptr", "offset",
    "add", "sub", "wrapping_add", "wrapping_sub", "wrapping_mul", "abs", "min",
    "max", "into", "from", "default", "clone", "len", "iter", "unwrap_or",
    "debug_assert", "assert_eq", "panic", "todo", "unimplemented", "dump_enabled",
}


def read_all(dirs, suffixes):
    out = []
    for d in dirs:
        if not d.is_dir():
            continue
        for suffix in suffixes:
            for f in sorted(d.rglob(f"*{suffix}")):
                out.append((f, f.read_text(errors="replace")))
    return out


def brace_body(text, open_idx):
    """Return the text between the brace at open_idx and its match."""
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[open_idx + 1 : i]
    return ""


def cpp_bodies(files):
    """name -> body, for top-level and `Class::method` definitions."""
    bodies = {}
    pat = re.compile(
        r"^[A-Za-z_][\w:<>,\s\*&]*?\b(?:([A-Za-z_]\w*)::)?([A-Za-z_]\w*)\s*\([^;{}]*?\)\s*(?:const\s*)?\{",
        re.M | re.S,
    )
    for path, text in files:
        for m in pat.finditer(text):
            name = m.group(2)
            if name in IGNORE:
                continue
            body = brace_body(text, text.index("{", m.end() - 1))
            # Prefer the longest definition when a name appears more than once.
            # `name not in bodies` is not redundant: `len("") > len("")` is false,
            # so without it an **empty body is never stored at all** — the name
            # simply vanishes from the map. That is how `GetVclNalTemporalId`'s
            # `{}` stayed invisible to this tool even after the empty-body rule
            # was added (T8b.A2). The instrument had S46's own blind spot.
            if name not in bodies or len(body) > len(bodies[name][0]):
                bodies[name] = (body, path)
    return bodies


def rust_bodies(files):
    bodies = {}
    pat = re.compile(r"^\s*(?:pub\s+)?(?:unsafe\s+)?(?:extern\s+\"C\"\s+)?fn\s+([A-Za-z_]\w*)", re.M)
    for path, text in files:
        for m in pat.finditer(text):
            name = m.group(1)
            brace = text.find("{", m.end())
            if brace < 0:
                continue
            body = brace_body(text, brace)
            if name not in bodies or len(body) > len(bodies[name][0]):  # see cpp_bodies
                bodies[name] = (body, path)
    return bodies


# **T8b.A2: the plus layer is named differently on the two sides.** Upstream's
# entry points are C++ methods (`CWelsDecoder::DecodeParser`); the port's are
# `extern "C"` thunks named for the vtable slot (`decoder_decode_parser_c`). Without
# this, a name-keyed comparison either found nothing (no Rust `DecodeParser`) or —
# worse — matched a *different* Rust function that happened to share the name, which
# is what happened here: the C++ `DecodeParser` was compared against a vtable
# trampoline and came out clean while the real thunk returned `dsErrorFree` and
# wrote nothing. One table, shared with `port_census.py`, so the two tools cannot
# disagree about what corresponds to what.
_census = importlib.util.spec_from_file_location(
    "port_census", Path(__file__).resolve().parent / "port_census.py"
)
_mod = importlib.util.module_from_spec(_census)
_census.loader.exec_module(_mod)
ALIASES = _mod.ALIASES


def calls(body):
    return {i for i in IDENT.findall(body) if i not in IGNORE}


# --- T8b.A2: the empty case, which the call-set diff cannot see ---------------
#
# S46. This tool diffs *call sets*, so a C++ body that only assigns is a
# zero-call body and a Rust `{}` calls zero things — equal, and not flagged.
# `GetVclNalTemporalId` (`decoder.cpp:716`: three assignments, no calls) sat in
# that hole for four phases as `pub fn GetVclNalTemporalId(pCtx) {}`, and the 21
# decoder-option gtest rows sat behind it.
#
# The rule: a Rust body that is empty, or one bare literal/path with an optional
# `return`, opposite a C++ body of two or more statements. Both halves are crude
# on purpose — the statement count is a `;` count, which over-counts `for` heads
# and under-counts braces-only bodies, and both errors are safe here because the
# only claim made is "read this one".

COMMENT = re.compile(r"//[^\n]*|/\*.*?\*/", re.S)
TRIVIAL = re.compile(r"^(?:return\s+)?[A-Za-z0-9_:.\-]*\s*;?$")


def strip_comments(body):
    return COMMENT.sub(" ", body)


def trivial_rust_body(body):
    """Empty, or a single literal / path / `return <literal>`."""
    stripped = " ".join(strip_comments(body).split())
    return bool(TRIVIAL.match(stripped))


def cpp_statements(body):
    """Crude statement count: semicolons outside comments."""
    return strip_comments(body).count(";")


# --- T8b.A4: the *short* case, which is neither empty nor call-diffable ---------
#
# `ForceCodingIDR` (`encoder_ext.cpp:3046`) resets five fields per dependency layer,
# bumps a counter and clears a flag — about twenty statements and, apart from a
# `WelsLog`, **no calls**. The port's body was
#
#     if pCtx.is_null() { return 1; }
#     0
#
# which the call-set diff cannot see (both bodies call nothing this tool tracks) and
# the empty-body rule cannot see (it has an `if`). It reported success and did
# nothing, and `ISVCEncoder::ForceIntraFrame` was a no-op for seven phases.
#
# The rule: a Rust body under a quarter of the C++'s statement count, where the C++
# has at least six. Both counts are `;` counts and both are crude; the ratio is what
# carries, and the only claim is "read this one".
#
# It is the noisiest of the three sections — 90 rows at T8b.A4 against 8 from the
# empty rule — because a Rust rewrite that replaces a pointer loop with a slice
# iterator legitimately has a fraction of the statements. Read it as a worklist,
# not as a defect list.
SHORT_RATIO = 4
SHORT_MIN_CPP_STATEMENTS = 6


def suspiciously_short(rust_body, cpp_body):
    n_cpp = cpp_statements(cpp_body)
    if n_cpp < SHORT_MIN_CPP_STATEMENTS:
        return None
    n_rust = strip_comments(rust_body).count(";")
    if n_rust * SHORT_RATIO > n_cpp:
        return None
    return (n_rust, n_cpp)


def duplicate_fns():
    """Report Rust functions defined more than once, worst size disparity first.

    A local item beats a glob import, so a short body under a correct name
    silently wins over the real one. Phase 5.1 lost most of its time to exactly
    that: an empty `WelsMdUpdateBGDInfo` in `encoder_context.rs` shadowed the
    real one in `svc_mode_decision.rs`, so no background macroblock was ever
    converted to P_SKIP. `find_dup_types.sh` checks types only, so nothing saw it.
    """
    pat = re.compile(r"^\s*(?:pub\s+)?(?:unsafe\s+)?(?:extern\s+\"C\"\s+)?fn\s+([A-Za-z_]\w*)", re.M)
    seen = {}
    for path, text in read_all(RUST_DIRS, [".rs"]):
        for m in pat.finditer(text):
            name = m.group(1)
            brace = text.find("{", m.end())
            if brace < 0:
                continue
            body = brace_body(text, brace)
            seen.setdefault(name, []).append((len(body.strip()), path))
    rows = []
    for name, defs in seen.items():
        if len(defs) < 2 or name in ("default", "new", "drop", "fmt"):
            continue
        defs.sort()
        rows.append((defs[-1][0] - defs[0][0], name, defs))
    rows.sort(reverse=True)
    for gap, name, defs in rows:
        print(f"{name}  ({len(defs)} definitions, {gap} chars between smallest and largest)")
        for size, path in defs:
            print(f"    {size:6} chars  {path.relative_to(ROOT)}")
    print(f"\n{len(rows)} duplicated function names.")


def main():
    if sys.argv[1:2] == ["--dups"]:
        duplicate_fns()
        return
    wanted = set(sys.argv[1:])
    cpp = cpp_bodies(read_all(CPP_DIRS, [".cpp"]))
    rust = rust_bodies(read_all(RUST_DIRS, [".rs"]))

    def rust_for(name):
        """The Rust body that corresponds to this C++ name, or None.

        **The alias wins over a same-named Rust function (F95, T8b.C5).** This used
        to try `rust[name]` first and fall back to the alias, which put the table
        exactly where it could not help. Every one of the twenty C-ABI entry points
        has *two* Rust functions: the thunk that does the work
        (`decoder_decode_parser_c`) and an inline vtable trampoline named for the
        slot (`ISVCDecoder::DecodeParser`, one expression, no semicolons). The
        name-first lookup found the trampoline and reported

            DecodeParser   [0 Rust statements vs 37 C++]

        which is what it also said when the thunk really was a stub — so the
        instrument's signal on the port's whole outward surface was zero in both
        directions.

        F95 blamed `abi_guard!` for this and that reading does not survive
        measurement: the macro's body strips to five semicolons, counted correctly.
        The table was simply never consulted, which is the same shape as the defect
        it was added for (F85) one level up.
        """
        alias = ALIASES.get(name)
        if alias is not None and alias in rust:
            return rust[alias]
        return rust.get(name)

    flagged = 0
    for name in sorted(set(rust) | set(cpp)):
        if wanted and name not in wanted:
            continue
        if name not in cpp:
            continue
        r = rust_for(name)
        if r is None:
            continue
        missing = calls(cpp[name][0]) - calls(r[0])
        # Only report identifiers the port knows about somewhere; a name that
        # exists in neither tree is a macro or a C-library call.
        missing = {m for m in missing if m in rust or m in cpp}
        if missing:
            flagged += 1
            print(f"{name}" + ("" if name in rust else f"  (as {ALIASES[name]})"))
            print(f"    C++  {cpp[name][1].relative_to(ROOT)}")
            print(f"    Rust {r[1].relative_to(ROOT)}")
            print(f"    not called on the Rust side: {', '.join(sorted(missing))}")
    scope = f"{len(wanted)} named" if wanted else f"{len(rust)} Rust"
    print(f"\n{flagged} flagged out of {scope} functions "
          f"({len(set(rust) & set(cpp))} have a C++ counterpart).")

    # The empty case, in its own section — see the note above `trivial_rust_body`.
    print("\n=== empty or constant Rust bodies opposite a C++ body of >= 2 statements")
    empty = 0
    for name in sorted(cpp):
        if wanted and name not in wanted:
            continue
        r = rust_for(name)
        if r is None or not trivial_rust_body(r[0]):
            continue
        n = cpp_statements(cpp[name][0])
        if n < 2:
            continue
        empty += 1
        shown = " ".join(strip_comments(r[0]).split()) or "(empty)"
        print(f"{name}   [Rust body: {shown}]")
        print(f"    C++  {cpp[name][1].relative_to(ROOT)}  ({n} statements)")
        print(f"    Rust {r[1].relative_to(ROOT)}")
    print(f"\n{empty} empty/constant Rust bodies opposite a non-trivial C++ one.")

    # The short case — see the note above `suspiciously_short`.
    print(f"\n=== Rust bodies under 1/{SHORT_RATIO} the C++'s statement count")
    short = 0
    for name in sorted(cpp):
        if wanted and name not in wanted:
            continue
        r = rust_for(name)
        if r is None or trivial_rust_body(r[0]):
            continue          # already reported by the empty rule
        counts = suspiciously_short(r[0], cpp[name][0])
        if counts is None:
            continue
        short += 1
        print(f"{name}   [{counts[0]} Rust statements vs {counts[1]} C++]")
        print(f"    C++  {cpp[name][1].relative_to(ROOT)}")
        print(f"    Rust {r[1].relative_to(ROOT)}")
    print(f"\n{short} Rust bodies under 1/{SHORT_RATIO} the C++'s statement count.")


if __name__ == "__main__":
    main()
