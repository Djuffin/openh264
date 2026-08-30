#!/usr/bin/env python3
"""The de-unsafe cascade, in its converging form.

After a conversion retires raw pointers from a family of signatures, some
`unsafe fn` declarations no longer need to be unsafe — and each one that goes
safe can free its callers in turn, which is why this iterates to a fixpoint
rather than running once.

**The converging form, and why the obvious form does not terminate** (F230):
strip `unsafe` only from *declarations* whose signature carries no raw pointer.
Stripping declarations and bodies together diverges — removing a body's `unsafe`
block makes its own calls illegal, which the next round "fixes" by re-adding, and
the pass oscillates. Declarations only, bodies never.

Rounds:
  1. strip every eligible declaration at once;
  2. compile; every error attributable to a stripped declaration reverts it —
     either because the body needs `unsafe` (E0133 inside it) or because the
     item is used where an `unsafe fn` type is required (fn-pointer tables);
  3. repeat until a round reverts nothing.

Then `--retire` sweeps the allows the cascade made dead: a
`#[allow(unsafe_code)]` whose item now contains no `unsafe` at all retires with
its `// unsafe-cat:` tag. **`#[unsafe(no_mangle)]` counts as unsafe** — F241
recorded a detector that looked only for the bare token, reported
`WelsGetCodecVersion` as dead, and broke the build when its allow came off.

Usage (from the crate root, rust/crates/openh264-rs):
    python3 ../../tools/deunsafe_cascade.py            # cascade, then report
    python3 ../../tools/deunsafe_cascade.py --retire   # ...and retire dead allows
    python3 ../../tools/deunsafe_cascade.py --dry-run  # list candidates only
"""
import re, glob, sys, json, subprocess, collections

RAW = re.compile(r"\*\s*(?:mut|const)\s")
DECL = re.compile(r"^(\s*)((?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?)"
                  r"unsafe\s+((?:extern\s+\"[^\"]*\"\s+)?fn\s+(\w+))")

def sources():
    return sorted(glob.glob("src/**/*.rs", recursive=True))

def signature_span(src, i):
    """From the `fn` line to the `{` that opens the body (or `;` for a trait decl).
    Multi-line signatures are the norm in this tree, so this must be balanced."""
    depth, j = 0, i
    while j < len(src):
        code = re.sub(r"//.*$", "", src[j])
        depth += code.count("(") - code.count(")")
        if depth <= 0 and ("{" in code or code.rstrip().endswith(";")):
            return j
        j += 1
    return i

def body_end(src, i):
    d, seen = 0, False
    for j in range(i, len(src)):
        code = re.sub(r"//.*$", "", src[j])
        d += code.count("{") - code.count("}")
        if "{" in code: seen = True
        if seen and d <= 0: return j
    return len(src) - 1

def candidates():
    """Every `unsafe fn` whose *signature* carries no raw pointer."""
    out = []
    for f in sources():
        src = open(f).read().split("\n")
        for i, l in enumerate(src):
            m = DECL.match(l)
            if not m: continue
            end = signature_span(src, i)
            sig = "\n".join(re.sub(r"//.*$", "", x) for x in src[i:end + 1])
            if RAW.search(sig): continue
            out.append({"file": f, "line": i, "name": m.group(4),
                        "end": body_end(src, i)})
    return out

SNAP = {}

def snapshot():
    """Hold every source file's original text in memory. Reverting a round restores
    from this snapshot and never from version control — the cascade routinely runs
    on top of an uncommitted conversion, and a checkout here would discard it."""
    SNAP.clear()
    for f in sources(): SNAP[f] = open(f).read()

def restore():
    for f, t in SNAP.items(): open(f, "w").write(t)

def apply(strips):
    """Rewrite each file once, removing `unsafe ` from the chosen declarations.
    Always from the snapshot, so a round's strip set is exactly `strips`."""
    restore()
    by = collections.defaultdict(list)
    for c in strips: by[c["file"]].append(c)
    for f, cs in by.items():
        src = open(f).read().split("\n")
        for c in cs:
            l = src[c["line"]]
            m = DECL.match(l)
            if m: src[c["line"]] = m.group(1) + m.group(2) + m.group(3) + l[m.end(3):]
        open(f, "w").write("\n".join(src))

def check():
    """cargo check --all-targets, as JSON. Returns [(file, line, code, msg)]."""
    p = subprocess.run(["cargo", "check", "--all-targets", "--message-format=json"],
                       capture_output=True, text=True)
    errs = []
    for line in p.stdout.split("\n"):
        if not line.strip(): continue
        try: m = json.loads(line)
        except Exception: continue
        if m.get("reason") != "compiler-message": continue
        d = m["message"]
        if d.get("level") != "error": continue
        code = (d.get("code") or {}).get("code") or ""
        for s in d.get("spans", []):
            if s.get("file_name", "").startswith("src/"):
                errs.append((s["file_name"], s["line_start"], code, d["message"]))
    return errs

def cascade():
    cands = candidates()
    print("cascade: %d unsafe fn declarations carry no raw pointer in their signature"
          % len(cands))
    if "--dry-run" in sys.argv:
        for c in cands: print("  %-46s %s:%d" % (c["name"], c["file"], c["line"] + 1))
        return cands, []
    snapshot()
    live = {(c["file"], c["line"]): c for c in cands}
    apply(list(live.values()))
    reverted, rnd = [], 0
    while True:
        rnd += 1
        errs = check()
        if not errs:
            print("round %d: clean" % rnd); break
        drop = set()
        names = {c["name"]: k for k, c in live.items()}
        for (f, l, code, msg) in errs:
            for k, c in live.items():
                if c["file"] == f and c["line"] <= l - 1 <= c["end"]:
                    drop.add(k)
            for n, k in names.items():          # fn-pointer tables: `unsafe fn` is a type
                if re.search(r"\b%s\b" % re.escape(n), msg): drop.add(k)
        if not drop:
            print("round %d: %d error(s) not attributable to any stripped declaration:"
                  % (rnd, len(errs)))
            for e in errs[:8]: print("   %s:%d %s %s" % e)
            print("   -> reverting everything; the tree is restored, nothing kept.")
            for k in list(live): reverted.append(live.pop(k))
            restore()
            return cands, reverted
        print("round %d: %d error(s) -> reverting %d declaration(s)" % (rnd, len(errs), len(drop)))
        for k in drop: reverted.append(live.pop(k))
        restore()
        apply(list(live.values()))
    print("cascade kept %d of %d; reverted %d" % (len(live), len(cands), len(reverted)))
    for c in sorted(live.values(), key=lambda c: (c["file"], c["line"])):
        print("  safe now  %-44s %s:%d" % (c["name"], c["file"], c["line"] + 1))
    return cands, reverted

ALLOW = re.compile(r"^\s*#\[allow\(unsafe_code\)\]\s*$")
CAT = re.compile(r"^\s*//\s*unsafe-cat:")

def dead_allows(f):
    """Allows whose item no longer contains `unsafe` in any form.

    **The item starts above the allow, not at it** (F241): `#[unsafe(no_mangle)]`
    is written two lines up, past the `// unsafe-cat:` tag, and a detector that
    reads only downward calls `WelsGetCodecVersion` dead — removing its allow
    breaks the build. Comments are stripped before the test so the tag line's own
    `unsafe-cat` cannot keep an item alive; `#[allow(unsafe_code)]` cannot either,
    because `unsafe_code` has no word boundary after `unsafe`."""
    src = open(f).read().split("\n")
    out = []
    for i, l in enumerate(src):
        if not ALLOW.match(l): continue
        top = i
        while top > 0 and (src[top - 1].lstrip().startswith("#[")
                           or src[top - 1].lstrip().startswith("//")):
            top -= 1
        j = i + 1
        while j < len(src) and (src[j].lstrip().startswith("#[")
                                or src[j].lstrip().startswith("//")
                                or not src[j].strip()):
            j += 1
        if j >= len(src): continue
        item = "\n".join(src[top:body_end(src, j) + 1])
        if re.search(r"\bunsafe\b", re.sub(r"//.*$", "", item, flags=re.M)): continue
        # the tag block: contiguous comment lines above the allow, from the
        # `// unsafe-cat:` line down. A tag comes off only with its unsafe.
        lo, k = i, i - 1
        while k >= 0 and src[k].lstrip().startswith("//"):
            if CAT.match(src[k]): lo = k
            k -= 1
        name = re.search(r"fn\s+(\w+)", src[j])
        out.append((lo, i, name.group(1) if name else src[j].strip()[:44]))
    return out, src

def retire(dry=False):
    total = 0
    for f in sources():
        if f.startswith("src/api/"): continue     # the C-ABI island keeps its allows
        found, src = dead_allows(f)
        if not found: continue
        for (lo, i, name) in found:
            print("  %-8s %-44s %s:%d" % ("would" if dry else "retire", name, f, i + 1))
        total += len(found)
        if dry: continue
        kill = set()
        for (lo, i, _) in found: kill |= set(range(lo, i + 1))
        open(f, "w").write("\n".join(l for n, l in enumerate(src) if n not in kill))
    print("retire: %d allow(s) %s" % (total, "would be removed" if dry else "removed"))
    return total

if __name__ == "__main__":
    if not glob.glob("src/**/*.rs", recursive=True):
        print("no sources — run from the crate root (rust/crates/openh264-rs)"); sys.exit(2)
    cascade()
    if "--retire" in sys.argv or "--retire-dry" in sys.argv:
        n = retire(dry="--retire-dry" in sys.argv)
        if "--retire-dry" in sys.argv: sys.exit(0)
        if n:
            e = check()
            print("post-retire check: %d error(s)" % len(e))
            for x in e[:8]: print("   %s:%d %s %s" % x)
            if e: sys.exit(1)
