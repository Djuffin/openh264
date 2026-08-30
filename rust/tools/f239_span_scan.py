#!/usr/bin/env python3
"""F239: a whole-struct reborrow pops a field-precise *exclusive* derivation.

The shape, from F239's own write-up (S6.A1, `encoder_ext.rs:2190/2236/2243`):

  1. `let X = &mut (*R).field;` or `let X = addr_of_mut!((*R).field);`
     — a Unique / SharedReadWrite claim on *part* of the struct behind raw root `R`;
  2. a later `R.as_ref()` / `&*R` / `R.as_mut()` / `&mut *R`
     — a claim on *all* of it, which pops X;
  3. a later *use* of X, read or write — now through a dead tag.

Miri reports this only on the path it actually walks, and it aborts at the first
UB, so a green re-run after one fix proves exactly one fix. This scan is what
covers the rest.

Two calibration decisions, both load-bearing, both inherited from earlier findings:

* **`addr_of!` / `&(*R).field` are NOT flagged.** F123: a shared reborrow of a
  field is a read through the parent, and a read does not remove `SharedReadWrite`
  items above the tag it reaches through — the sibling survives. Only the
  `&mut`-shaped (Unique) derivation is popped. `addr_of_mut!` *is* flagged even
  though F208's second-order note argues it inherits the parent tag rather than
  minting a child: F239's five recorded spans include two `addr_of_mut!` bodies,
  the session narrowed them, and this instrument reports the shape rather than
  adjudicating it. A cleared `addr_of_mut!` span earns a CLEARED line, not a
  silent exemption.
* **A re-binding ends a derivation's live range.** `let X = ...` again below the
  reborrow is not a use of the popped tag — it is F239's *fix* (derive at the use).
  A scanner that missed this would flag every site the last session repaired.

Usage:  python3 rust/tools/f239_span_scan.py            # scan, from the crate root
        python3 rust/tools/f239_span_scan.py --selftest # fixtures, both directions
Exits non-zero when any span is reported.
"""
import re, glob, sys

# ---- step 1: a field-precise EXCLUSIVE derivation, bound to a name -------------
# `let X = &mut (*R).f...;`  `let X = addr_of_mut!((*R).f...);`  `&raw mut (*R).f...`
# Anchored on `let` because an unbound `&mut (*R).f` in argument position is a
# temporary whose live range ends at the statement — not a span.
DERIVE = re.compile(
    r"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?=\s*"
    r"(?:(?:std::ptr::|core::ptr::)?addr_of_mut!\s*\(\s*|&\s*raw\s+mut\s+|&\s*mut\s+)"
    r"\(\s*\*\s*(\w+)\s*\)\s*\.",
    re.S)

# ---- step 2: a WHOLE-struct reborrow of that same root ------------------------
# `R.as_ref()` / `R.as_mut()` / `&*R` / `&mut *R`, same identifier.
# `(*R).field` is field-precise and is deliberately not this.
def reborrow_re(root):
    r = re.escape(root)
    return re.compile(r"\b%s\s*\.\s*as_(?:ref|mut)\s*\(\s*\)|&\s*(?:mut\s+)?\*\s*%s\b(?!\s*\)\s*\.)" % (r, r))

def rebind_re(name):
    return re.compile(r"\blet\s+(?:mut\s+)?%s\b" % re.escape(name))

def use_re(name):
    return re.compile(r"\b%s\b" % re.escape(name))

FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?"
                r"(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\s+(\w+)")

# Bodies read by hand and cleared, with the reason. A body earns a line here only
# when the span provably cannot be the F239 defect. Keyed (file, fn, binding).
CLEARED = {}

def statements(body, base):
    """Yield (start_line, text, depth) per logical statement, so a `let` whose value
    sits on the next line is still one match — F208's scanner is line-based and
    would miss `let pParamInternal =` / `addr_of_mut!(...)`, a real shape in this
    tree. `depth` is the brace nesting at the statement's start, which is what lets
    the caller tell a re-binding in a nested block (scoped, ends nothing) from one
    at the derivation's own level, and an early-returning branch from a fallthrough."""
    out, buf, start, depth, brace = [], [], None, 0, 0
    for k, l in enumerate(body):
        code = re.sub(r"//.*$", "", l)
        if start is None and code.strip():
            start, sbrace = base + k + 1, brace
        if start is not None:
            buf.append(code)
        depth += code.count("(") - code.count(")")
        depth += code.count("[") - code.count("]")
        brace += code.count("{") - code.count("}")
        if start is not None and depth <= 0 and re.search(r"[;{}]\s*$", code.rstrip()):
            out.append((start, "\n".join(buf), min(sbrace, brace)))
            buf, start, depth = [], None, 0
    if buf: out.append((start, "\n".join(buf), brace))
    return out

EXIT = re.compile(r"^\s*(?:return\b|break\b|continue\b|\?\s*;)")

def unreachable_after_block(stmts, k):
    """If the reborrow at index k sits in a block that unconditionally exits
    (`return`/`break`/`continue` at the reborrow's own nesting level, before the
    block closes), then nothing after that block runs on this reborrow's path.
    Returns the index at which the block closes, else None. This is a *sound*
    discard — control flow provably cannot reach — and it is what separates
    `WelsEncoderEncodeExt`'s frame-skipping branch from a live span."""
    d = stmts[k][2]
    if d == 0: return None
    for j in range(k + 1, len(stmts)):
        if stmts[j][2] < d:
            return None
        if stmts[j][2] == d and EXIT.match(stmts[j][1]):
            for m in range(j + 1, len(stmts)):
                if stmts[m][2] < d: return m
            return len(stmts)
    return None

def scan_file(path):
    src = open(path).read().split("\n")
    fns = [(i, m.group(1)) for i, l in enumerate(src) if (m := FN.match(l))]
    spans, cand = [], 0
    for i, name in fns:
        d, seen, end = 0, False, i
        for j in range(i, len(src)):
            code = re.sub(r"//.*$", "", src[j])
            d += code.count("{") - code.count("}")
            if "{" in code: seen = True
            if seen and d <= 0: end = j; break
        stmts = statements(src[i:end + 1], i)
        for si, (dline, text, ddepth) in enumerate(stmts):
            m = DERIVE.search(text)
            if not m: continue
            X, R = m.group(1), m.group(2)
            cand += 1
            rb, us, ur = reborrow_re(R), use_re(X), rebind_re(X)
            # Live range ends at a re-binding of X *at the derivation's own nesting
            # level*. A `let X` inside a nested block is scoped to it and ends
            # nothing — treating it as a stop would silently drop real spans.
            stop = len(stmts)
            # A binding goes out of scope when its own block closes; a `use` of the
            # same *name* past that point is a different binding, not this tag.
            for k in range(si + 1, len(stmts)):
                if stmts[k][2] < ddepth: stop = k; break
            for k in range(si + 1, stop):
                if stmts[k][2] <= ddepth and ur.search(stmts[k][1]): stop = k; break
            hit_r = hit_u = None
            for k in range(si + 1, stop):
                if not rb.search(stmts[k][1]): continue
                dead_from = unreachable_after_block(stmts, k)
                lim = min(stop, dead_from) if dead_from is not None else stop
                for n in range(k + 1, lim):
                    if us.search(stmts[n][1]):
                        hit_r, hit_u = stmts[k][0], stmts[n][0]; break
                if hit_u is not None: break
            if hit_u is None: continue
            spans.append((path, name, X, R, dline, hit_r, hit_u))
    return spans, cand, len(fns)

def run(files):
    spans, cand, bodies = [], 0, 0
    for f in files:
        s, c, b = scan_file(f); spans += s; cand += c; bodies += b
    live = 0
    for (f, fn, X, R, d, r, u) in spans:
        key = (f.split("/")[-1], fn, X)
        if key in CLEARED:
            print("cleared  %-28s %-34s %-16s %s" % (f.split("/")[-1], fn, X, CLEARED[key]))
            continue
        live += 1
        print("SPAN     %s:%d  fn %s  `%s` from `*%s`   derive@%d  reborrow@%d  use@%d"
              % (f, d, fn, X, R, d, r, u))
    print("f239 span scan: %d span(s) / %d exclusive field derivations / %d bodies / %d files"
          % (live, cand, bodies, len(files)))
    return live

def selftest():
    import tempfile, os
    F = [
      ("defect: F239's own shape (derive, whole-struct reborrow, use)", """
unsafe fn f(p: *mut S) {
    let x = &mut (*p).field;
    let s = slice_in(p.as_ref(), 0);
    x.a = 1;
}
""", 1),
      ("fix: derive-at-use (the binding moves below the reborrow)", """
unsafe fn f(p: *mut S) {
    let s = slice_in(p.as_ref(), 0);
    let x = &mut (*p).field;
    x.a = 1;
}
""", 0),
      ("fix: re-binding ends the live range (F239's repair spelling)", """
unsafe fn f(p: *mut S) {
    let x = &mut (*p).field;
    x.a = 0;
    let s = slice_in(p.as_ref(), 0);
    let x = &mut (*p).field;
    x.a = 1;
}
""", 0),
      ("immune: shared derivation (F123 - a read does not pop SharedReadWrite)", """
unsafe fn f(p: *mut S) {
    let x = &(*p).field;
    let s = slice_in(p.as_ref(), 0);
    let _ = x.a;
}
""", 0),
      ("immune: addr_of! is read-only", """
unsafe fn f(p: *mut S) {
    let x = std::ptr::addr_of!((*p).field);
    let s = slice_in(p.as_ref(), 0);
    let _ = (*x).a;
}
""", 0),
      ("flagged: addr_of_mut! (F239 recorded two such spans)", """
unsafe fn f(p: *mut S) {
    let x = std::ptr::addr_of_mut!((*p).field);
    let s = slice_in(p.as_ref(), 0);
    (*x).a = 1;
}
""", 1),
      ("not a span: no use after the reborrow", """
unsafe fn f(p: *mut S) {
    let x = &mut (*p).field;
    x.a = 1;
    let s = slice_in(p.as_ref(), 0);
}
""", 0),
      ("not a span: field-precise access is not a whole-struct reborrow", """
unsafe fn f(p: *mut S) {
    let x = &mut (*p).field;
    let y = (*p).other;
    x.a = 1;
}
""", 0),
      ("not a span: reborrow of a DIFFERENT root", """
unsafe fn f(p: *mut S, q: *mut S) {
    let x = &mut (*p).field;
    let s = slice_in(q.as_ref(), 0);
    x.a = 1;
}
""", 0),
      ("defect: multi-line binding (F208's line-based scan misses this)", """
unsafe fn f(p: *mut S) {
    let x =
        std::ptr::addr_of_mut!((*p).field[i as usize]);
    let s = slice_in(p.as_ref(), 0);
    (*x).a = 1;
}
""", 1),
      ("defect: `&*R` spelling of the whole-struct reborrow", """
unsafe fn f(p: *mut S) {
    let x = &mut (*p).field;
    take(&*p);
    x.a = 1;
}
""", 1),
      ("not a span: reborrow's branch returns before the use (encoder_ext.rs:4068)", """
unsafe fn f(p: *mut S) -> i32 {
    let x = std::ptr::addr_of_mut!((*p).field);
    if cond() {
        clear(&mut *p);
        return 0;
    }
    (*x).a = 1;
    1
}
""", 0),
      ("defect: exiting branch does not shield a use INSIDE the same branch", """
unsafe fn f(p: *mut S) -> i32 {
    let x = std::ptr::addr_of_mut!((*p).field);
    if cond() {
        clear(&mut *p);
        (*x).a = 1;
        return 0;
    }
    1
}
""", 1),
      ("defect: a re-binding in a NESTED block does not end the live range", """
unsafe fn f(p: *mut S) {
    let x = &mut (*p).field;
    if cond() {
        let x = &mut (*p).other;
        x.b = 2;
    }
    let s = slice_in(p.as_ref(), 0);
    x.a = 1;
}
""", 1),
      ("not a span: temporary in argument position, not a binding", """
unsafe fn f(p: *mut S) {
    take(&mut (*p).field);
    let s = slice_in(p.as_ref(), 0);
}
""", 0),
    ]
    d = tempfile.mkdtemp(); bad = 0
    for n, (label, src, want) in enumerate(F):
        path = os.path.join(d, "t%d.rs" % n)
        open(path, "w").write(src)
        got = len(scan_file(path)[0])
        ok = got == want
        bad += 0 if ok else 1
        print("%-4s %-62s expected %d, got %d" % ("ok" if ok else "FAIL", label, want, got))
    print("selftest: %d/%d fixtures pass" % (len(F) - bad, len(F)))
    return bad

if __name__ == "__main__":
    if "--selftest" in sys.argv:
        sys.exit(1 if selftest() else 0)
    files = sorted(glob.glob("src/**/*.rs", recursive=True))
    if not files:
        print("f239 span scan: NO FILES MATCHED - run this from the crate root "
              "(rust/crates/openh264-rs). A zero from the wrong directory is not a zero.")
        sys.exit(2)
    sys.exit(1 if run(files) else 0)
