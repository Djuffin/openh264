#!/usr/bin/env python3
"""F208: a `&self` reader on the context is a SharedReadOnly retag over the WHOLE
context, so it invalidates any `Unique` derived from the same context — including
one into a disjoint field.  Where the context is reached through a raw pointer,
borrowck never referees that, so scan for it: a body that binds `&mut (*pCtx).x`
(or addr_of_mut!) and also calls one of the new readers."""
import re, glob, sys

READERS = ["rc", "rc_at", "mvd_cost_table", "mvd_cost_origin", "frame_bs",
           "frame_bs_cur", "ref_list", "sps_array", "subset_array", "pps_array",
           "paraset_arrays", "ref_list_and_ltr_mut",
           # A5 (`ctx_vaa`): the three `&self` shapes the video-analysis block
           # answers with — the reference, the raw root the fn-pointer alias still
           # needs, and the dormant screen-content downcast.
           "vaa", "vaa_ptr", "vaa_ext",
           # …and A5's two combined `&mut self` accessors, which retag the whole
           # context the same way the readers do.
           "vaa_ref_list_and_ltr_mut", "vaa_and_rc_at_mut",
           # A6 (`ctx_func_list`): the dispatch table's reader, and its writer.
           "func_list", "func_list_mut",
           # A7 (`ctx_param`): the coding parameters — the reader, the writer, the
           # guard's question, and the two combined accessors A7 minted.
           "param", "param_mut", "param_opt",
           "param_and_rc_at_mut", "param_and_paraset_arrays_mut",
           # …and the two raw roots that still take the context by reference, so
           # they retag it exactly as the readers above do.
           "ctx_param_raw", "ctx_ref_list_raw"]
rd = re.compile(r"\.(%s)\s*\(" % "|".join(READERS))
# a Unique retag into the context through a raw
uq = re.compile(r"&mut\s*\(\*(p\w*Ctx\w*)\)\.|addr_of_mut!\(\(\*(p\w*Ctx\w*)\)\.")
# Bodies read by hand and cleared, with the reason. A body earns a line here only
# when its `&mut`-shaped derivation provably cannot be live across the reader —
# the scanner is a line matcher and cannot see argument-position temporaries.
CLEARED = {
    # A7: the two uniques are `&mut (*pCtx).sLogCtx` in the argument lists of
    # `ParamValidationExt` and `GetMultipleThreadIdc`. Each dies at the end of its
    # own statement, long before the `param_mut` at :766; and `pOldParam` itself is
    # unused on the branch that reaches the reader at :921 (the reset branch, which
    # destroys the context and re-derives `pCtx`), which is why borrowck accepts it.
    ("wels_encoder_ext.rs", "WelsEncoderParamAdjust"),
}
hits = []
for f in sorted(glob.glob("src/**/*.rs", recursive=True)):
    src = open(f).read().split("\n")
    fns = []
    for i, l in enumerate(src):
        m = re.match(r"\s*(pub )?(unsafe )?(extern \"C\" )?fn (\w+)", l)
        if m: fns.append((i, m.group(4)))
    for i, name in fns:
        d, seen, end = 0, False, i
        for j in range(i, len(src)):
            d += src[j].count("{") - src[j].count("}")
            if "{" in src[j]: seen = True
            if seen and d == 0: end = j; break
        body = src[i:end + 1]
        txt = "\n".join(body)
        if not uq.search(txt) or not rd.search(txt): continue
        uqs = [i + k + 1 for k, l in enumerate(body) if uq.search(l)]
        rds = [i + k + 1 for k, l in enumerate(body) if rd.search(l)]
        hits.append((f, name, uqs, rds))
bad = 0
for f, name, u, r in hits:
    if (f.split("/")[-1], name) in CLEARED:
        print("cleared  %-24s %-38s see CLEARED in this file" % (f.split("/")[-1], name))
        continue
    # A candidate is only a HAZARD if some reader call precedes a `&mut`-shaped
    # derivation that is still live afterwards. The cheap, sound-enough screen is
    # "a reader line sits at or after the first unique line" — anything else is
    # already ordered read-then-retag, which is the safe shape.
    risky = [x for x in r if x >= min(u)]
    tag = "HAZARD?" if risky else "ordered"
    if risky: bad += 1
    print("%-8s %-24s %-38s unique@%s reader@%s"
          % (tag, f.split('/')[-1], name, u[:4], r[:6]))
print("candidate bodies: %d   needing a hand read: %d" % (len(hits), bad))
sys.exit(1 if bad else 0)
