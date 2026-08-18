#!/usr/bin/env python3
"""Trivial function bodies that shadow real implementations — the F43 sweep.

Phase 5 session S. `decoder_core.rs` defined five stubs (`NeedErrorCon` returning
`false`, `FmoNextMb` returning `iMbIdx + 1`, …) in the same module that called
them. A local item beats every other path, so **every production call site resolved
to the stub** and two whole subsystems — error concealment and FMO — never ran. No
instrument could see it: `find_stub_bodies.py` matches names against the C++ and
finds the *real* body in the other module, and the duplicate census compares text,
which a stub and its implementation differ in maximally.

This is the check that would have caught it. Three filters, and the order matters —
each one removes a class of false positive that swamped the previous draft:

  1. **name defined in more than one module** — 194 names. Unusable alone.
  2. **one body trivial (<= 2 statements), another substantial (>= 6)** — 35. Still
     swamped, because a delegating wrapper looks exactly like a stub from here, and
     `decoder_core.rs` is full of deliberate ones (`pub unsafe fn WelsResetRefPic(..)
     { manage_dec_ref::WelsResetRefPic(..) }`).
  3. **the trivial body does not mention the function's own name** — delegation is
     the whole difference between a re-export and a stub. 15 left.

What survives is trait-impl methods: twenty types implementing `default` or `new`
share a name and share nothing else. They are reported rather than filtered because
recognising an `impl` block needs a parser, and a reader dismisses them in a glance
— but that is why this prints candidates and not verdicts.

Result at `cf31bf2f`: **no remaining instance of F43's shape.** Every decoder
`Wels*` one-liner delegates; every other candidate is a trait method.

Result at Phase 6 session B (F52's close): **22 candidate names before the
declaration filter, 18 after**, and F52's six are adjudicated in
`phase6_findings.md` — four were trait-method *declarations* (no body at all,
which this sweep had scored as an empty body), `WelsRcPostFrameSkipping` is a
faithful `return false` (`ratectl.cpp:1015`) beside its `RCMode` dispatcher, and
`push_back` is two methods on two types. The last two still print, by design.

    usage: rust/tools/find_shadowing_stubs.py [--self-test]

`--self-test` runs the sweep over a two-file fixture that holds the *original* F52
stub shape (a five-line signature whose whole body is `true`, beside the real
implementation) and a trait-method declaration beside its real impl, and exits
non-zero unless the stub prints and the declaration does not. It is the guard on
the declaration filter — the same guard F52 put on the brace fix, so that a filter
added to remove one false positive cannot quietly remove the finding's own shape.
"""

import collections
import os
import re
import sys
import tempfile

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "openh264-rs", "src")
FN = re.compile(r'\s*(?:pub(?:\([^)]*\))? )?(?:unsafe )?(?:extern "C" )?fn (\w+)')
TRIVIAL, SUBSTANTIAL = 2, 6


def functions(text):
    """(name, statement_count, body_text, line_no) for each fn in one file.

    **The body starts at the opening brace, not at the `fn` line** (F52, Phase 5
    session X). Counting from `i + 1` charges a multi-line signature's parameter
    lines to the body: a four-parameter stub whose whole body is `true` scored 6
    statements and was classified SUBSTANTIAL, so it never appeared opposite its own
    real implementation. That is exactly how a sixth F43-class stub —
    `decoder_core.rs`'s `CheckAccessUnitBoundaryExt`, shadowing `nalu.rs`'s 80-line
    one — survived this sweep's clean run at session U. `body_start` is the line the
    brace that opens the body is on; everything before it is signature.

    **A signature that ends in `;` before any `{` is a declaration and has no body**
    (F52's Phase 6 close, session B). A trait method declaration —
    `unsafe fn Uninit(&mut self);` — used to be scanned forward to the *next* item's
    opening brace and the nothing in between scored as a trivial body, which is how
    four of F52's six candidates (`Uninit`, `InitFrame`, `ExecuteTasks`,
    `OnTaskStop`) came to be printed opposite their own impls. There is no body to
    shadow with, so a declaration is skipped. The test is on the trimmed line with a
    trailing `//` comment removed, and it is *ends with*, not *contains*: an array
    type in a signature (`x: [u8; 4],`) has a `;` and is not a declaration.
    """
    out, lines = [], text.split("\n")
    for i, line in enumerate(lines):
        m = FN.match(line)
        if not m:
            continue
        depth, j, started, body_start = 0, i, False, i
        declaration = False
        while j < len(lines) and j < i + 400:
            if not started:
                sig = lines[j].split("//")[0].rstrip()
                if "{" not in sig and sig.endswith(";"):
                    declaration = True
                    break
            depth += lines[j].count("{") - lines[j].count("}")
            if "{" in lines[j] and not started:
                started = True
                body_start = j
            if started and depth <= 0:
                break
            j += 1
        if declaration:
            continue
        inner = [
            b.strip()
            for b in lines[body_start + 1:j]
            if b.strip() and not b.strip().startswith("//")
        ]
        out.append((m.group(1), len(inner), "\n".join(inner), i + 1))
    return out


def sweep(root, out=sys.stdout):
    """Print the candidates under `root`; return the candidate names."""
    defs = collections.defaultdict(list)
    for dirpath, _, files in os.walk(root):
        for f in sorted(files):
            if not f.endswith(".rs"):
                continue
            path = os.path.join(dirpath, f)
            rel = os.path.relpath(path, root)
            with open(path) as fh:
                for name, n, body, ln in functions(fh.read()):
                    defs[name].append((rel, n, body, ln))

    candidates = []
    for name, ds in sorted(defs.items()):
        if len(ds) < 2:
            continue
        stubs = [d for d in ds if d[1] <= TRIVIAL and name not in d[2]]
        real = [d for d in ds if d[1] >= SUBSTANTIAL]
        if not (stubs and real):
            continue
        candidates.append(name)
        print(f"  {name}", file=out)
        for rel, n, body, ln in stubs:
            print(f"      trivial, non-delegating  {rel}:{ln}  body: {body[:64]!r}", file=out)
        for rel, n, _, ln in real:
            print(f"      substantial ({n:3d})        {rel}:{ln}", file=out)

    print(f"\n{len(candidates)} candidate name(s).", file=out)
    print("Trait-impl methods (`default`, `new`, `eq`, `get`) are expected here and are", file=out)
    print("not shadowing. A *free function* in this list is F43 until read and disproved.", file=out)
    return candidates


# The fixture for `--self-test`. `stub.rs` carries F52's own shape verbatim in
# spirit — the multi-line signature is the part the brace fix exists for — plus a
# trait declaration; `real.rs` carries the two real bodies. The stub must print;
# the declaration must not; the real bodies must be classified substantial.
SELF_TEST_STUB = '''\
pub trait ITaskManage {
    unsafe fn Uninit(&mut self);
    fn OnTaskStop(
        &mut self,
        pThread: *mut CWelsTaskThread,
        pTask: Option<TaskPtr>,
    ) -> WELS_THREAD_ERROR_CODE;
    fn sized(&self, x: [u8; 4]) -> bool {
        // an array type's `;` inside a signature is not a declaration: this
        // trivial body must still print beside real.rs's substantial `sized`
        x[0] == 0
    }
}

#[inline]
pub unsafe fn CheckAccessUnitBoundaryExt(
    pLastNalHdr: *mut SNalUnitHeaderExt,
    pCurNalHdr:  *mut SNalUnitHeaderExt,
    pLastSh:     *mut SSliceHeader,
    pCurSh:      *mut SSliceHeader,
) -> bool {
    true
}
'''

SELF_TEST_REAL = '''\
pub unsafe fn CheckAccessUnitBoundaryExt(
    pLastNalHdr: *mut SNalUnitHeaderExt,
    pCurNalHdr:  *mut SNalUnitHeaderExt,
    pLastSh:     *mut SSliceHeader,
    pCurSh:      *mut SSliceHeader,
) -> bool {
    let a = (*pLastNalHdr).uiTemporalId;
    let b = (*pCurNalHdr).uiTemporalId;
    if a != b {
        return true;
    }
    let c = (*pLastSh).iFrameNum;
    let d = (*pCurSh).iFrameNum;
    c != d
}

impl ITaskManage for CTaskManage {
    unsafe fn Uninit(&mut self) {
        self.a = 0;
        self.b = 0;
        self.c = 0;
        self.d = 0;
        self.e = 0;
        self.f = 0;
        self.g = 0;
    }
    fn OnTaskStop(
        &mut self,
        pThread: *mut CWelsTaskThread,
        pTask: Option<TaskPtr>,
    ) -> WELS_THREAD_ERROR_CODE {
        let a = 1;
        let b = 2;
        let c = 3;
        let d = 4;
        let e = 5;
        let f = 6;
        a + b + c + d + e + f
    }
    fn sized(&self, x: [u8; 4]) -> bool {
        let a = x[0] as u32;
        let b = x[1] as u32;
        let c = x[2] as u32;
        let d = x[3] as u32;
        let s = a + b + c + d;
        s == 0
    }
}
'''


def self_test():
    with tempfile.TemporaryDirectory() as d:
        with open(os.path.join(d, "stub.rs"), "w") as fh:
            fh.write(SELF_TEST_STUB)
        with open(os.path.join(d, "real.rs"), "w") as fh:
            fh.write(SELF_TEST_REAL)
        print("--- self-test sweep over the fixture:")
        names = sweep(d)
    ok = True
    if "CheckAccessUnitBoundaryExt" not in names:
        print("SELF-TEST FAIL: F52's stub shape (multi-line signature, body `true`) did not print")
        ok = False
    for decl in ("Uninit", "OnTaskStop"):
        if decl in names:
            print(f"SELF-TEST FAIL: the trait declaration `{decl}` printed as a trivial body")
            ok = False
    if "sized" not in names:
        print("SELF-TEST FAIL: a `;` inside an array type in a signature was read as a declaration")
        ok = False
    print("SELF-TEST " + ("PASS" if ok else "FAIL")
          + ": the stub prints, the declarations do not, the array-type `;` is not a declaration")
    return 0 if ok else 1


def main(argv):
    if "--self-test" in argv:
        return self_test()
    sweep(ROOT)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
