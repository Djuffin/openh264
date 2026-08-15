#!/usr/bin/env python3
"""find_elem_byte_confusion — the F40-class sweep (plan §7.2, Phase 5 W7).

F40: `ExpandBsLenBuffer` copied four times what it should, because the C++ is

    memcpy (dst, src, n * sizeof (int32_t));          // bytes

and the transliteration kept the `* sizeof` while changing the primitive to one
whose count is in **elements**:

    std::ptr::copy_nonoverlapping(src, dst, n * size_of::<i32>());   // elements

That is a transliteration hazard, not a logic error: C's `memcpy`/`memset` count
bytes, Rust's `copy_nonoverlapping`/`copy`/`write_bytes` count elements of `T`,
and a faithful-looking `* sizeof(T)` is exactly the bug. F40's own sweep covered
`src/decoder/` and said so; W7 asks for it crate-wide, which is what this is.

What it flags
-------------
A call to an element-counting primitive whose **count argument** mentions
`size_of`. That is the F40 shape. It is not automatically a defect: when the
pointee is byte-sized the element count and the byte count are the same number,
which is why those are reported separately rather than hidden — `write_bytes` on
a `*mut u8` with `size_of::<SFoo>()` is correct and common in this port.

Benign has to be **proved**, and everything else is a SUSPECT. The pointee is
read from an explicit turbofish (`copy_nonoverlapping::<u8>`) or from the last
cast on a pointer argument (`as *mut u8`); if neither says byte-sized, the hit is
a SUSPECT to be read by hand.

That default is not fastidiousness — it is what this tool's own first run taught
it. Written with a third "no visible pointee" bucket, it was pointed at the tree
as it stood before T5.O5 and filed **F40 itself** into that bucket, exiting 0:
F40's pointers are plain `*mut i32` struct fields, so there is no turbofish and
no cast to classify. The shape worth finding is exactly the shape carrying no
annotation. This tool narrows the reading list; it does not adjudicate, and S24's
clause applies to it as to any other instrument — the hand count arbitrates.

    usage: rust/tools/find_elem_byte_confusion.py [--all] [path ...]
             --all   also list the byte-sized (benign) hits, not just the summary

Exit status is 1 when any SUSPECT survives, so this can gate.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_ROOTS = [ROOT / "rust" / "crates" / "openh264-rs" / "src"]

# Element-counting primitives: the last argument is a count of `T`, not of bytes.
ELEMENT_COUNTING = [
    "copy_nonoverlapping",
    "copy",
    "write_bytes",
    "swap_nonoverlapping",
    "slice_from_raw_parts",
    "slice_from_raw_parts_mut",
    "from_raw_parts",
    "from_raw_parts_mut",
]

BYTE_SIZED = {"u8", "i8", "c_char", "std::ffi::c_char"}


def split_args(s: str) -> list[str]:
    """Top-level comma split — the count is the last argument, and the earlier
    ones routinely contain commas inside generics and nested calls."""
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "([{<":
            depth += 1
        elif ch in ")]}>":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur)
    return [a.strip() for a in out]


def match_call(text: str, start: int) -> tuple[str, int] | None:
    """Given the index of the '(' after a call name, return (args, end)."""
    depth, i = 0, start
    while i < len(text):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return text[start + 1 : i], i
        i += 1
    return None


def scan(path: Path):
    text = path.read_text(errors="replace")
    hits = []
    for name in ELEMENT_COUNTING:
        # `ptr::copy_nonoverlapping::<T>(` / `.copy_to(` are both spelled with the
        # bare identifier before the turbofish, so anchor on the identifier and
        # take an optional turbofish after it.
        for m in re.finditer(r"\b" + name + r"\s*(::<([^>]*)>)?\s*\(", text):
            turbofish = (m.group(2) or "").strip()
            open_paren = m.end() - 1
            got = match_call(text, open_paren)
            if not got:
                continue
            args, _ = got
            parts = split_args(args)
            if not parts:
                continue
            count = parts[-1]
            if "size_of" not in count:
                continue
            line = text[: m.start()].count("\n") + 1
            # Pointee: the turbofish if present, else a trailing `as *mut T` cast
            # on any pointer argument.
            pointee = turbofish
            if not pointee:
                # The *last* cast is the one that fixes the pointer's type, and
                # `as *mut _ as *mut u8` is a common two-step in this port — so
                # take the last and drop the inferred `_`, rather than giving up
                # because two casts disagree. (This tool called that shape
                # UNKNOWN on its first run; the hand count arbitrated, S24.)
                casts = [c for c in
                         re.findall(r"as \*(?:mut|const)\s+([A-Za-z_][A-Za-z0-9_:]*)", args)
                         if c != "_"]
                if casts:
                    pointee = casts[-1]
            # **Two buckets, and the default is SUSPECT.** The first cut of this
            # tool had three, with "no visible pointee" as its own middle
            # category — and then it was run against the tree as it stood before
            # T5.O5 and filed **F40 itself** as UNKNOWN, exiting 0. F40's
            # pointers are plain `*mut i32` struct fields: no turbofish, no cast,
            # nothing for a classifier to read. So the shape this tool exists to
            # find is precisely the shape that carries no annotation, and a
            # middle bucket is where it goes to be ignored.
            #
            # Benign therefore has to be *proved* (a byte-sized pointee, where
            # the element count and the byte count are the same number) and
            # everything else is a SUSPECT to be read by hand. S33's corollary:
            # an instrument that disagrees with a hand count is wrong until
            # proven otherwise, and the hand count arbitrates.
            verdict = "byte-sized" if pointee in BYTE_SIZED else "SUSPECT"
            hits.append((line, name, pointee or "?", verdict, " ".join(count.split())))
    return hits


def main() -> int:
    argv = [a for a in sys.argv[1:] if not a.startswith("--")]
    show_all = "--all" in sys.argv[1:]
    roots = [Path(a) for a in argv] or DEFAULT_ROOTS

    files = sorted(f for r in roots for f in r.rglob("*.rs"))
    tally = {"SUSPECT": [], "byte-sized": []}
    for f in files:
        for line, name, pointee, verdict, count in scan(f):
            try:
                shown = f.relative_to(ROOT)
            except ValueError:
                shown = f  # a path outside the repo (a historical checkout, say)
            tally[verdict].append((shown, line, name, pointee, count))

    for verdict in ("SUSPECT", "byte-sized"):
        rows = tally[verdict]
        if verdict == "byte-sized" and not show_all:
            print(f"\n{verdict}: {len(rows)} (element size 1 — the count is the same "
                  f"number either way; --all to list)")
            continue
        print(f"\n{verdict}: {len(rows)}")
        for path, line, name, pointee, count in rows:
            print(f"  {path}:{line}\n      {name}::<{pointee}> count = {count}")

    print(f"\nscanned {len(files)} files under {', '.join(str(r) for r in roots)}")
    # A SUSPECT is an element-counting primitive with a `size_of` in the count
    # whose pointee is not provably byte-sized: F40's exact shape. Read each.
    return 1 if tally["SUSPECT"] else 0


if __name__ == "__main__":
    sys.exit(main())
