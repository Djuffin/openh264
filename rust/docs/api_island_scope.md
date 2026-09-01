# The C-ABI island, measured — what is actually boundary and what is decoder logic

*Scoping report, S12 (2026-08-31). Written in answer to one question the plan never
asks: **which of the port's remaining `unsafe` and raw pointers are about the
external C API?** Every number below was measured against the tree at S12.6; the
commands are given so they can be re-run.*

> ## STATUS: **DONE**, S12.7–S12.11 (2026-08-31)
>
> The user directed this be executed immediately rather than after S13, overriding
> §6's recommendation. It landed in **five** gated checkpoints, not six — A.2 and
> A.3 merged, for a reason §5 had not seen (below). Every `family` gate PASSed;
> `abi_sizes.sh --check` stayed current at 170 lines and `abi_exports.sh` at 7/7
> throughout, which is the constraint on any work inside this file.
>
> | | estimate | actual |
> |---|---|---|
> | checkpoints | 6 | **5** |
> | crate `unsafe_fn` | 59 → 53 | **59 → 53** ✔ exact |
> | `codec_api.rs` `unsafe fn` | 56 → 50 | **56 → 50** ✔ exact |
> | `codec_api.rs` allows | 44 → 40 | **44 → 39** — one better |
> | crate `unsafe_block` | — | 147 → **138** |
> | crate `raw_ptr` | — | 418 → **399** |
>
> **Three things the estimate got wrong, all in the useful direction.**
>
> *A.2 and A.3 could not be separated.* `pool_for` returning `Option<&mut SPicBuff>`
> needs the context to be a reference for its lifetime, and once it is, the *caller*
> would hold two `&mut` of one object — the pool borrow and the context it came
> from. Resolving the pool inside `EmitBufferedPicture` and passing `bUsePool` down
> fixes both at once, and is behaviour-identical (checked: nothing between the old
> resolution point and the call writes `pCtx.pPicBuff`).
>
> *A.4's blocker was not one.* `self.initialize(&sPrevParam)` compiled unchanged.
> NLL ends a borrow at its last use **per path**, and on the branch that calls
> `initialize` the next two statements return. §5 predicted the conflict from the
> borrow *set*; liveness is what decides it. The checkpoint that was flagged
> "take it alone, do not batch" was the cheapest of the five.
>
> *Three deletions the scoping did not anticipate*: `ctx_ptr` (S42's decoder-side
> root, dead with its last caller), `PicPool::slot_at_mut`'s pointer return, and
> `slot_ptr_mut` beneath it — the last two outside the island entirely.
>
> **A.6 landed as specified and is the part that outlasts the rest**: the census
> now exempts the *ABI surface* (`extern "C"` items) rather than the *directory*,
> pinning **27** `island-nonextern(...)` rows that were invisible before. Most are
> the nineteen vtable dispatch helpers, which are irreducibly boundary and stay —
> pinning them is the difference between an exception argued and one inherited.
> The tracking number is unchanged at **29**, so the series stays comparable.
>
> What §5 called the end state holds: `codec_api.rs` now carries only ABI-shaped
> unsafe — the `extern "C"` thunks, the vtable dispatches, and the deliberate C-ABI
> test drivers.

---

## 1. Why this document exists

`unsafe_census.sh` — E2's instrument, the one that pins the exit floor — opens with:

> `src/api/` is out of scope here, as it is for the tracking number: it is the
> frozen C-ABI layer, watched by `abi_exports.sh` and `abi_sizes.sh` instead.

That exemption was written when `src/api/` was assumed to be boundary throughout.
**It is not.** Of the 218 raw-pointer dereferences in `api/codec_api.rs`, **25 are
in `extern "C"` thunk bodies and 193 are not**; of its 56 `unsafe fn`, **25 are
`extern "C"` and 31 are not**. The two referees that do watch the island watch its
*surface* — `abi_exports.sh` asserts the exported symbol set is exactly upstream's
seven names, `abi_sizes.sh` asserts 170 type-size lines — and neither looks inside
a body.

So the largest single concentration of real `unsafe` work in the crate has never
been measured by anything, and is exempt **by file path**.

---

## 2. The measurement

Outside the island the campaign is finished, and every item is named:

| metric | `src/api/` | outside | total |
|---|---:|---:|---:|
| `unsafe_fn` | 57 | 2 | 59 |
| `unsafe_block` | 90 | 57 | 147 |
| `raw_ptr` | 316 | 102 | 418 |
| `unsafe_impl` | 0 | 1 | 1 |

The 57 blocks outside are 51 test instruments plus 6 production, all pinned by the
census: 2 `fork-shared(S63)`, 2 `recon-seam`, 1 `lawful-single(F162)`, 1 `C-ABI`.

Inside, counting **dereferences** rather than tokens — `unsafe {}` blocks and
`*mut` in a signature say where the unsafe is *declared*; `(*p)` says where the
work happens:

```
raw dereferences in src/api/codec_api.rs   218
  in `extern "C"` thunk bodies              25   <- the boundary
  in non-`extern "C"` bodies               193
```

**The 193 decompose exactly, and the accounting closes:**

| group | derefs | what it is | convertible? |
|---|---:|---|---|
| **A** | **160** | nine functions of decoder logic that happen to live here | **yes** |
| B | 19 | nineteen vtable dispatch helpers, one deref each: `((*(*this).lpVtbl).X)(this, …)` | no — `this` is the application's pointer |
| C | 14 | `drive_decoder_over` / `drive_encoder_over`, `pub(crate)` drivers that call the C ABI exactly as a C caller does | no — raw on purpose |
| | **193** | | |

### Group A in full

| fn | derefs | `unsafe fn`? | `extern "C"`? | exported? |
|---|---:|---|---|---|
| `decode` | 46 | no | no | no |
| `ReleaseBufferedReadyPictureReorder` | 25 | yes | no | no |
| `decode_parser` | 24 | no | no | no |
| `BufferingReadyPicture` | 21 | yes | no | no |
| `ReorderPicturesInDisplay` | 16 | yes | no | no |
| `EmitBufferedPicture` | 12 | yes | no | no |
| `ReleaseBufferedReadyPictureNoReorder` | 12 | yes | no | no |
| `flush` | 3 | no | no | no |
| `pool_for` | 1 | yes | no | no |
| **total** | **160** | 6 of 9 | 0 of 9 | 0 of 9 |

None is `extern "C"`, none is `#[no_mangle]`, and none is exported —
`abi_exports.sh` proves the cdylib exports exactly seven symbols and none of these
is among them. They are the decoder's **picture-reordering algorithm and decode
loop**, private to the crate, living in `codec_api.rs` because that is where
`welsDecoderExt.cpp` maps. Four of `codec_api.rs`'s 44 allows attach to them.

Re-run:

```bash
python3 - <<'PY'
import re, collections
src = open('src/api/codec_api.rs').read().split('\n')
fnre = re.compile(r'^(\s*)(pub(\([a-z]+\))?\s+)?(unsafe\s+)?(extern "C"\s+)?fn\s+(\w+)')
fns = []
for i, l in enumerate(src):
    m = fnre.match(l)
    if not m: continue
    d = 0; st = False; k = i
    while k < len(src):
        d += src[k].count('{') - src[k].count('}')
        if '{' in src[k]: st = True
        if st and d == 0: break
        k += 1
    fns.append({'n': m.group(6), 'ext': bool(m.group(5)), 'a': i, 'b': k})
c = collections.Counter(); ext = {}
for i, l in enumerate(src):
    n = len(re.findall(r'\(\*\w+\)', re.sub(r'//.*$', '', l)))
    if not n: continue
    o = [f for f in fns if f['a'] <= i <= f['b']]
    if not o: continue
    o = o[-1]; c[o['n']] += n; ext[o['n']] = o['ext']
print('extern "C" bodies:', sum(v for k, v in c.items() if ext[k]))
print('non-extern bodies:', sum(v for k, v in c.items() if not ext[k]))
for k, v in c.most_common(10): print(f'  {v:4d}  {k}{"  [extern C]" if ext[k] else ""}')
PY
```

---

## 3. Three findings that make the work smaller than it looks

**3.1 The out-parameters are a round trip, not a boundary.** `decode` receives
`ppDst: &mut [*mut u8; 3]` and `pDstInfo: &mut SBufferInfo` — **safe references
already** — and then calls

```rust
ReorderPicturesInDisplay(p_ctx, ppDst.as_mut_ptr(), ptr::from_mut(pDstInfo));
```

purely so the helper can dereference them back. **Three** places do the conversion
— `decode` twice (`:2830`, `:2849`) and `flush` once, in a `let (ppDst, pDstInfo) =
(ppDst.as_mut_ptr(), ptr::from_mut(pDstInfo));` that then feeds two calls — and the
remaining six call sites are helper-to-helper, already raw because their *caller*
was. Delete the three conversions and all nine sites pass references. It is the
exact shape S12.3 deleted when `expose_provenance`/`with_exposed_provenance_mut`
lost their subject: a safe value made raw to cross a boundary that is not there.
(`ppDst`'s *contents* are the application's plane pointers and stay raw — that
part is real. The `&mut [_; 3]` around them is not.)

**3.2 `pool_for` throws away a safe accessor that already exists.** Its whole body:

```rust
crate::decoder::decoder_context::pic_pool_ptr(&mut (*pCtx).pPicBuff)
    .map_or(ptr::null_mut(), |pool| pool)
```

and `pic_pool_ptr` is `pub fn(&mut Option<Box<SPicBuff>>) -> Option<&mut SPicBuff>`
— safe, already the decoder's own idiom (`PicRefs`, the bracket-top pattern at
`decoder_context.rs:1053`). `pool_for` is an adapter that converts
`Option<&mut>` into a nullable raw for one caller. Delete the adapter and the
`Option` reaches `EmitBufferedPicture` intact. This was the item I expected to be
hard; it is the easiest one.

**3.3 The blockers are two lines.** Scanning both entry points for uses of `self.`
while the context borrow is live:

* `decode` (46 derefs) — **two** lines touch `self.`: the `ctx_ptr` call itself,
  and `self.initialize(&sPrevParam)` at `:2725`.
* `decode_parser` (24) — **five**: `ctx_ptr`, `self.initialize(…)` at `:2958`, and
  three `self.trace.m_sLogCtx` reads, which are *sibling field* reads and fall to
  the field-scoped borrow the campaign has applied since F282.
* `flush` (3) — already half-converted; it calls `self.ctx.as_mut()` beside the raw.

So one genuine borrow conflict, appearing twice: a `&mut self` method called while
a `&mut` into `self.ctx` is live. Standard, and the campaign has two settled
answers (scope the borrow, or hand `initialize` the fields it needs).

The pointer itself is F71 verbatim — `ctx_ptr` is
`fn(&mut Option<Box<SWelsDecoderContext>>) -> *mut SWelsDecoderContext`, returning
`addr_of_mut!(**pCtx)` from an owned `Box`. That accessor family was converted
hundreds of times outside the island.

---

## 4. The referee — measured, because the first guess was wrong

**The 583/583 byte sweep does not execute one line of Group A.** The diffharness is
**encoder-only**: `cxx_enc` against `rust_enc`. Its single decoder line runs the
*reference C* decoder over Rust-encoded output as a sanity check, never the Rust
decoder. Any plan resting on "the sweep will catch it" is wrong here.

The integration suite is a different story. Probes in all seven live candidates,
one `cargo test` run (569 tests, 0 failed):

| fn | entries in one test run |
|---|---:|
| `decode` | 24,800 |
| `ReorderPicturesInDisplay` | 24,800 |
| `BufferingReadyPicture` | 2,807 |
| `EmitBufferedPicture` | 2,803 |
| `ReleaseBufferedReadyPictureNoReorder` | 2,017 |
| `ReleaseBufferedReadyPictureReorder` | 831 |
| `decode_parser` | 159 |

And the referees behind those numbers are byte-exact, not smoke tests:
`e2e_conformance_test` compares the decoder's output against reference Y4M **frame
by frame** over JVT conformance streams; `loopback_sha1_test`,
`decoder_conformance_test`, `decoder_nodelay_parity_test`,
`decoder_resolution_change_test` and `multithread_decoder_test` sit beside it;
`ecref` referees 2,707 malformed-stream rows against the C++ decoder's error
codes; the gtest suite drives the real C ABI at `exit` level.

**This is the best-refereed unconverted code in the crate** — the exact opposite of
S12.3's dark arm, where a unit test had to be built before anything could be
trusted. Nothing new needs building here.

---

## 5. What it would take

Six checkpoints, one session, on S11's family cadence — comparable to
`encoder_ext.rs`, which fell in five. Ordered so each is independently gated and
byte-refereed, and so the cheap structural wins land before the borrow work.

| # | checkpoint | derefs | notes |
|---|---|---:|---|
| **A.1** | the out-parameters stop round-tripping — `ppDst`/`pDstInfo` become `&mut [*mut u8; 3]` / `&mut SBufferInfo` on all five helpers | — | no context change yet; deletes the 3 conversion sites and 10 raw parameters |
| **A.2** | `pool_for` dissolves; `EmitBufferedPicture` takes `Option<&mut SPicBuff>` | 1 | `pic_pool_ptr` already returns it |
| **A.3** | the reordering family takes `&mut SWelsDecoderContext`; the five stop being `unsafe fn` | 86 | the bulk, and mechanical once A.1/A.2 land |
| **A.4** | `decode`'s context borrow — the `initialize` conflict | 46 | the one real design decision |
| **A.5** | `decode_parser` and `flush` | 27 | same shape; sibling-field log reads |
| **A.6** | **re-pin, and fix the exemption that hid all of this** | — | see below |

**A.6 is the deliverable that outlasts the conversion.** `unsafe_census.sh` should
stop exempting a *directory* and start exempting the *ABI surface*: `extern "C"`
items, `#[repr(C)]` struct fields, and the vtable function-pointer types. Everything
else in `src/api/` gets pinned by file and category like the rest of the tree. Had
that rule existed, Group A would have been on the floor list since E2 and this
document would not have been needed.

**End state.** `codec_api.rs` holds only ABI-shaped unsafe: the 25 `extern "C"`
thunk dereferences, the 19 vtable dispatches, the 14 in the deliberate C-ABI test
drivers. Its `unsafe fn` count falls 56 → 50 and its allows 44 → 40. The crate's
`unsafe_fn` total falls 59 → 53.

**Risk: low–medium.** The referee is byte-exact and fires 24,800 times per run, so
a mistake is loud. The one place to slow down is A.4: restructuring `decode`'s
borrow around `initialize` is the only step where a wrong answer is a *design*
error rather than a compile error. Suggest taking A.4 alone, with the Miri decoder
probes, and not batching it.

**What this does not buy.** The C ABI stays exactly as raw as it is: 25 thunk
dereferences, 19 vtable dispatches, seven exported symbols, 170 pinned type sizes.
That is the boundary, it is supposed to be raw, and none of it moves.

---

## 6. Recommendation

Do it, and do it **after S13**, not before. S13 is the exit battery — full Miri,
the fork probes, both benches against 85 checkpoints of debt — and it should run
against the tree the campaign actually produced rather than a tree being edited
underneath it. But S13's report should say plainly that the posture it certifies
has a 193-dereference exemption in it, so the number it publishes is not read as
"the port is done".

The alternative — leaving Group A permanently — is defensible only if the exemption
is made honest: the census would have to pin these nine functions by name as
*named permanent exceptions*, the way `fork-shared(S63)` and `recon-seam` are, with
the argument written at each site. Nobody has made that argument, and the
measurement above suggests it could not be made: the code is ordinary decoder logic
with a context pointer, and the port converted hundreds of those.
