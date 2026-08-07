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

Exit status is always 0; this is a reading aid, not a gate.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CPP_DIRS = [
    ROOT / "codec/encoder/core/src",
    ROOT / "codec/common/src",
    ROOT / "codec/processing/src",
]
RUST_DIRS = [
    ROOT / "rust/crates/openh264-rs/src/encoder",
    ROOT / "rust/crates/openh264-rs/src/processing",
    ROOT / "rust/crates/openh264-rs/src/common",
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
            if len(body) > len(bodies.get(name, ("", ""))[0]):
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
            if len(body) > len(bodies.get(name, ("", ""))[0]):
                bodies[name] = (body, path)
    return bodies


def calls(body):
    return {i for i in IDENT.findall(body) if i not in IGNORE}


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

    flagged = 0
    for name in sorted(rust):
        if wanted and name not in wanted:
            continue
        if name not in cpp:
            continue
        missing = calls(cpp[name][0]) - calls(rust[name][0])
        # Only report identifiers the port knows about somewhere; a name that
        # exists in neither tree is a macro or a C-library call.
        missing = {m for m in missing if m in rust or m in cpp}
        if missing:
            flagged += 1
            print(f"{name}")
            print(f"    C++  {cpp[name][1].relative_to(ROOT)}")
            print(f"    Rust {rust[name][1].relative_to(ROOT)}")
            print(f"    not called on the Rust side: {', '.join(sorted(missing))}")
    scope = f"{len(wanted)} named" if wanted else f"{len(rust)} Rust"
    print(f"\n{flagged} flagged out of {scope} functions "
          f"({len(set(rust) & set(cpp))} have a C++ counterpart).")


if __name__ == "__main__":
    main()
