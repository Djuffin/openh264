#!/usr/bin/env python3
"""Context fields the port reads but never writes — the F45 sweep.

Phase 5 session S. F43, F44 and F45 are three instances of one shape: **the C++
performs a write the port never performs.** F43 was a call that resolved to a stub,
F44 a call site that was absent, F45 a field assignment that was absent. None is
visible to an instrument that matches by name, because in every case the name
exists in the port and only the write is missing.

This is the mechanical form for the third kind. For every **scalar** field of a
struct, does anything in `src/` assign it? A scalar is the whole test: an aggregate
can legitimately be written through a subfield (`ctx.sRefPic.pRefList[i] = …`), so
including aggregates is all false positives — that was the first draft, and 15 of
its 15 hits were wrong.

A field that is read and never assigned is either dead or a missing write, and one
grep of the C++ tells you which:

    grep -rn 'FieldName' codec/decoder/

Validated: run against the tree at `47ef685c` (this session's start) it reports
`bInstantDecFlag` — F45, an emitted frame the C++ does not emit on every truncated
stream — and nothing else, out of 53 scalar fields.

    usage: rust/tools/find_unwritten_fields.py [struct-name ...]
           (default: SWelsDecoderContext)
"""

import os
import re
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "openh264-rs", "src")
SCALAR = r"(bool|i8|i16|i32|i64|u8|u16|u32|u64|usize|isize)"


def source_blob(root):
    blob = []
    for dirpath, _, files in os.walk(root):
        for f in files:
            if f.endswith(".rs"):
                with open(os.path.join(dirpath, f)) as fh:
                    blob.append(fh.read())
    return "\n".join(blob)


def scalar_fields(blob, struct):
    m = re.search(r"pub struct " + re.escape(struct) + r" \{(.*?)\n\}", blob, re.S)
    if not m:
        return None
    return re.findall(r"^\s*pub (\w+):\s*" + SCALAR + r"\s*,", m.group(1), re.M)


def main():
    structs = sys.argv[1:] or ["SWelsDecoderContext"]
    blob = source_blob(ROOT)
    total_hits = 0

    for struct in structs:
        fields = scalar_fields(blob, struct)
        if fields is None:
            print(f"{struct}: not found")
            continue
        print(f"{struct}: {len(fields)} scalar fields")
        for name, ty in fields:
            written = re.search(r"\." + name + r"\s*(=[^=]|\+=|-=|\|=|&=|\^=)", blob) or \
                      re.search(r"addr_of_mut!\([^)]*\." + name + r"\b", blob)
            reads = len(re.findall(r"\." + name + r"\b", blob))
            if not written and reads:
                total_hits += 1
                print(f"  NEVER ASSIGNED  {name:34s} {ty:6s} {reads} reads")
        print()

    if total_hits:
        print(f"{total_hits} field(s) read but never written — check each against the C++.")
    else:
        print("clean: every scalar field that is read is also written somewhere.")
    return 1 if total_hits else 0


if __name__ == "__main__":
    sys.exit(main())
