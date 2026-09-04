//! SIMD acceleration kernels and CPU feature detection for openh264-rs.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(feature = "wide")]
pub mod wide;

/// The scalar forwards, compiled only where they are what [`kernels`] names.
#[cfg(not(any(target_arch = "x86_64", feature = "wide")))]
pub mod scalar;

/// **The kernel set the dispatch sites call.** Every `has_simd()` arm and every
/// `WELS_CPU_SSE2` table install names its kernel `kernels::<family>::<kernel>`, and
/// this alias decides what that resolves to. Each dispatch file imports it once at
/// module level rather than spelling the path per call, because a kernel shares its
/// name with the scalar it was ported from — `pixel_avg` is both — and the module
/// qualifier is what tells them apart.
///
/// **The alias is total, and that is the point.** Three arms, and every target lands
/// on exactly one:
///
/// | build | resolves to | what runs |
/// |---|---|---|
/// | x86_64, default | [`x86_64`] | `core::arch` intrinsics |
/// | any target, `--features wide` | [`wide`] | portable `wide` lanes |
/// | neither | [`scalar`] | forwards to the scalar body |
///
/// The third arm exists so the sites need no `#[cfg]` of their own. It is never
/// *executed* — [`has_simd`] is false wherever it is selected, so each site takes its
/// scalar arm before reaching the alias — but it has to *resolve*, and 34 attributes
/// saying so at the call sites is the alternative.
///
/// The three modules export the same entry points with the same signatures, which
/// `tests::the_kernel_sets_expose_the_same_entry_points` holds; the sites therefore do
/// not change when the selection does, only this block does. [`x86_64`] and [`wide`]
/// are both compiled whenever they can be, which is what lets
/// `benches/kernel_bench.rs` time the implementations of one kernel in one process.
#[cfg(all(target_arch = "x86_64", not(feature = "wide")))]
pub use x86_64 as kernels;
#[cfg(feature = "wide")]
pub use wide as kernels;
#[cfg(not(any(target_arch = "x86_64", feature = "wide")))]
pub use scalar as kernels;

use crate::common::cpu_core::*;

/// Detects available CPU SIMD features, once per process.
///
/// Respects `OPENH264_NO_SIMD=1`, which forces scalar fallbacks for differential
/// verification. Latching makes it a process-start switch: every dispatch site reads
/// this one word, so the switch is all-or-nothing rather than half-applied.
///
/// Keep the body this small. [`has_simd`] is `#[inline(always)]` onto it from
/// twenty-four per-call dispatch sites, and it only folds into them because the
/// one-time initialiser lives out of line in [`latch_cpu_features`].
#[inline]
pub fn detect_cpu_features() -> u32 {
    // `Acquire` here pairs with the `Release` in `latch_cpu_features`, so a thread that
    // sees the ready flag also sees the word stored before it was set.
    if CPU_FEATURES_READY.load(Ordering::Acquire) {
        return CPU_FEATURES.load(Ordering::Relaxed);
    }
    latch_cpu_features()
}

/// Runs once per process; see [`detect_cpu_features`] for why it is out of line.
#[cold]
#[inline(never)]
fn latch_cpu_features() -> u32 {
    let flags = if std::env::var_os("OPENH264_NO_SIMD").is_some() {
        0
    } else {
        arch_cpu_features()
    };
    // Racing callers compute the same word from the same inputs, so both stores are
    // idempotent and neither needs a compare-exchange. The word goes first and the flag
    // second, under `Release`, so no reader can see the flag without the word.
    CPU_FEATURES.store(flags, Ordering::Relaxed);
    CPU_FEATURES_READY.store(true, Ordering::Release);
    flags
}

/// The x86_64 feature probe. MMX, SSE and SSE2 are part of the baseline x86_64
/// instruction set, so those bits are unconditional.
#[cfg(target_arch = "x86_64")]
fn arch_cpu_features() -> u32 {
    let mut flags = WELS_CPU_MMX | WELS_CPU_MMXEXT | WELS_CPU_SSE | WELS_CPU_SSE2;

    if std::is_x86_feature_detected!("sse3") {
        flags |= WELS_CPU_SSE3;
    }
    if std::is_x86_feature_detected!("ssse3") {
        flags |= WELS_CPU_SSSE3;
    }
    if std::is_x86_feature_detected!("sse4.1") {
        flags |= WELS_CPU_SSE41;
    }
    if std::is_x86_feature_detected!("sse4.2") {
        flags |= WELS_CPU_SSE42;
    }
    if std::is_x86_feature_detected!("avx") {
        flags |= WELS_CPU_AVX;
    }
    if std::is_x86_feature_detected!("avx2") {
        flags |= WELS_CPU_AVX2;
    }
    if std::is_x86_feature_detected!("fma") {
        flags |= WELS_CPU_FMA;
    }

    flags
}

/// Off x86_64 the answer is a question about the build, not about the CPU.
///
/// **`WELS_CPU_SSE2` names a slot here, not an instruction set.** It is upstream's
/// flag word (`codec/common/src/cpu.cpp`), which the port keeps because the `pfXxx`
/// tables are built from it exactly as the C++ builds them — but what the bit *means*
/// to this port is "there is a vector kernel for this slot". [`wide`] is written in
/// the `wide` crate's lane types throughout — no `core::arch`, every file
/// `#![forbid(unsafe_code)]` — so it compiles and runs wherever the crate does, and on
/// aarch64 LLVM lowers those lanes to NEON. Under `--features wide` the slots are
/// filled on every target, so the bit is set on every target.
///
/// The kernels themselves say nothing about an instruction set: `deblock_luma_lt4` is
/// that name in both modules, and the module path is the whole of the difference.
///
/// **`WELS_CPU_AVX2` is deliberately left clear.** It is the one place a second
/// runtime test picks a second kernel for a slot already filled, and off x86 there is
/// no wider register file to pick it for: `wide` has no runtime dispatch, so its
/// `_avx2` entry points are 128-bit bodies that step two rows. Installing them here
/// would swap a kernel for a differently-shaped copy of itself, so the AVX2 slots keep
/// the baseline kernel.
///
/// Without the feature there is no kernel set to point at — `simd::kernels` does not
/// exist on this target — so every bit stays clear and every dispatch site takes its
/// scalar fallback, as it always has.
///
/// Split out per arch rather than `#[cfg]`-ing a block inside one function, so that
/// `flags` is only `mut` where it is actually mutated (`lib.rs` denies `unused_mut`).
#[cfg(not(target_arch = "x86_64"))]
fn arch_cpu_features() -> u32 {
    if cfg!(feature = "wide") { WELS_CPU_SSE2 } else { 0 }
}

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The process-wide feature word, and whether it has been computed yet.
///
/// **Two cells rather than a sentinel bit inside the word.** Every bit of the `u32` is
/// spoken for — `WELS_CPU_CACHELINE_128` is `0x8000_0000` (`common/cpu_core.rs:49`) and
/// upstream sets it for real (`codec/common/src/cpu.cpp:207`) — so a marker inside the
/// word would be masked out of a live flag as soon as `arch_cpu_features` grows to
/// report cache-line size. A separate cell cannot collide with any future flag.
///
/// `0` has to stay a legitimate answer, and is: `arch_cpu_features` returns it on every
/// non-x86_64 target and under `OPENH264_NO_SIMD=1`.
static CPU_FEATURES: AtomicU32 = AtomicU32::new(0);
static CPU_FEATURES_READY: AtomicBool = AtomicBool::new(false);

/// Whether this build has a vector kernel set that is enabled right now.
///
/// **Not "does this CPU have SSE2".** It reads the `WELS_CPU_SSE2` bit because that is
/// the slot bit upstream's tables are keyed on (see [`arch_cpu_features`]), but the
/// question every caller is asking is the portable one: on x86_64 the answer is the
/// hardware's, off x86_64 it is `--features wide`'s, and under `OPENH264_NO_SIMD=1` it
/// is `false` everywhere. [`has_avx2`] is the one probe that really is about an
/// instruction set, and it is only consulted from x86 table arms.
#[inline(always)]
pub fn has_simd() -> bool {
    (detect_cpu_features() & WELS_CPU_SSE2) != 0
}

/// Returns true if AVX2 is supported and not disabled by `OPENH264_NO_SIMD=1`.
///
/// Unlike SSE2 this is not x86_64 baseline, so it is a real runtime question: the AVX2
/// SAD kernels execute `vpsadbw` and fault on any pre-Haswell Intel or pre-Excavator
/// AMD part.
///
/// The `cfg!` folds the branch away for a build that already guarantees AVX2, and is
/// false by default on every `x86_64-*` target. **It cannot replace the runtime test:**
/// `-C target-feature=+avx2` applies to the whole crate, so LLVM would vectorise
/// everything else with it too and the `cdylib` a C consumer `dlopen`s would fault on
/// an older CPU. Per-function AVX2 codegen is `#[target_feature(enable = "avx2")]`,
/// which `sad_16x_avx2` carries. On such a build this answers `true` without consulting
/// `OPENH264_NO_SIMD`, which is consistent — that binary is AVX2 throughout.
#[inline(always)]
pub fn has_avx2() -> bool {
    cfg!(target_feature = "avx2") || (detect_cpu_features() & WELS_CPU_AVX2) != 0
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    fn src_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    fn rs_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).expect("read_dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    /// The text before the file's test module, so a helper inside one is never mistaken
    /// for an entry point and a name used only by a test never counts as "reached".
    ///
    /// The cut is at `#[cfg(test)]\nmod `, not at `#[cfg(test)]` alone: twenty-one sites
    /// in this crate put that attribute on a test-only `use` in the middle of a file's
    /// imports, and cutting there would discard the whole file below it — which is
    /// exactly what it did on `encoder/sample.rs`, hiding every SAD dispatch site and
    /// making this test report the entire SAD family as unreached.
    fn without_tests(src: &str) -> &str {
        src.split("#[cfg(test)]\nmod ").next().unwrap_or(src)
    }

    /// The `pub`/`pub(crate)` fn names declared in one kernel module.
    fn entry_points(module: &str) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for f in rs_files(&src_root().join("simd").join(module)) {
            let text = std::fs::read_to_string(&f).expect("read kernel file");
            let file = f.file_name().unwrap().to_string_lossy().into_owned();
            let mut rest = without_tests(&text);
            while let Some(i) = rest.find("pub ").or_else(|| rest.find("pub(crate) ")) {
                let tail = &rest[i..];
                let tail = tail.strip_prefix("pub(crate) ").or_else(|| tail.strip_prefix("pub ")).unwrap();
                let tail = tail.strip_prefix("unsafe ").unwrap_or(tail);
                if let Some(tail) = tail.strip_prefix("fn ") {
                    let name: String = tail.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    if !name.is_empty() {
                        out.insert(name, file.clone());
                    }
                }
                rest = &rest[i + 4..];
            }
        }
        out
    }

    /// **Kernels internal to a module**, which no dispatch site can or should name: the
    /// generic workers the shaped entry points instantiate (`sad_16x` and friends), the
    /// 16-sample inner loops of the deblocking filters, the `#[target_feature]` body
    /// `satd_4x4` delegates to, and `wide`'s load/store/permute helpers.
    ///
    /// This list is the only thing maintained by hand, and it fails **closed**: a new
    /// kernel that nothing dispatches is not on it, so the test names it. Forgetting to
    /// add a genuine helper costs one line and a failing test, never silent coverage.
    const INTERNAL: &[&str] = &[
        "deblock_chroma_eq4_16", "deblock_chroma_lt4_16", "deblock_luma_eq4_16", "deblock_luma_lt4_16",
        "sad_16x", "sad_16x_avx2", "sad_4x", "sad_8x",
        "sample_sad_four_16x", "sample_sad_four_4x", "sample_sad_four_8x",
        "satd_4x4_sse2_impl",
        "hsum_i16", "load16", "load4", "load8", "load_w", "low4", "low8", "merge_lo64",
        "narrow", "rotate_quads", "store_w", "swap_adjacent", "swap_halves", "transpose4_lo",
        "widen_hi", "widen_lo",
    ];

    /// **A kernel that is written but never reached is the failure this tier is most
    /// exposed to**, and it is invisible to everything else: the output of a parity port
    /// is the scalar's output, so a slot left holding the scalar passes every parity
    /// test, the conformance suites and the byte-parity sweep. It has happened twice
    /// here — the two Hadamard "kernels" that were the scalar copied verbatim, and the
    /// `BLOCK_4x4`/`8x4`/`4x8` SAD slots that had kernels written and tested but no
    /// table entry.
    ///
    /// So ask it of the kernels directly: every entry point either is named somewhere
    /// outside `src/simd/`, or is on [`INTERNAL`]. This reads source text rather than
    /// comparing function pointers, which is what lets it survive the scalar
    /// `simd::kernels` alias — under which two distinct forwards to the same scalar
    /// body have different addresses, so the pointer-identity spelling this replaced
    /// would have gone green while nothing was accelerated.
    ///
    /// What it does not claim: that the site *executes* (a reference inside a `#[cfg]`
    /// block that is off still counts), or that a slot holds the *right* kernel. Neither
    /// did the assertions it replaced.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn every_kernel_is_named_by_a_dispatch_site() {
        // **Reached means "named through the alias", not "this token appears".** The
        // kernels deliberately share their names with the scalars they were ported from
        // — `copy_8x16` is both the kernel and the scalar — so a bare-token search finds
        // every kernel name in the codec whether or not anything dispatches it, and
        // reports success for a slot that was never wired. Follow `kernels::…::` instead.
        //
        // The exception is the two sites that glob-import a kernel module and then call
        // its entries bare; for those, every token in the file counts. Both are
        // `intra_pred`, whose `enc_`/`dec_` names no scalar shares.
        let mut used: BTreeSet<String> = BTreeSet::new();
        for f in rs_files(&src_root()) {
            if f.components().any(|c| c.as_os_str() == "simd") {
                continue;
            }
            let text = std::fs::read_to_string(&f).expect("read codec file");
            let text = without_tests(&text);

            let mut rest = text;
            while let Some(i) = rest.find("kernels::") {
                let tail = &rest[i + "kernels::".len()..];
                // `<module>::<kernel>` — take the second segment.
                let seg: String = tail.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let after = &tail[seg.len()..];
                if let Some(after) = after.strip_prefix("::") {
                    let name: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    if !name.is_empty() {
                        used.insert(name);
                    }
                }
                rest = &rest[i + "kernels::".len()..];
            }

            if text.contains("use kernels::") && text.contains("::*;") {
                used.extend(
                    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .filter(|t| !t.is_empty())
                        .map(str::to_owned),
                );
            }
        }

        let mut unreached = Vec::new();
        for module in ["x86_64", "wide"] {
            for (name, file) in entry_points(module) {
                if INTERNAL.contains(&name.as_str()) || used.contains(&name) {
                    continue;
                }
                unreached.push(format!("{module}/{file}: {name}"));
            }
        }
        assert!(
            unreached.is_empty(),
            "these kernels exist but no dispatch site names them — wire them, or add them \
             to `INTERNAL` if they are module-internal: {unreached:#?}"
        );
    }

    /// The three kernel sets have to agree on their entry points, or `simd::kernels`
    /// would resolve differently per build and a dispatch site would compile on one
    /// target and not another. Cheaper to learn here than from a cross-compile.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn the_kernel_sets_expose_the_same_entry_points() {
        let sets: Vec<(&str, BTreeSet<String>)> = ["x86_64", "wide", "scalar"]
            .iter()
            .map(|m| {
                let names = entry_points(m)
                    .into_keys()
                    .filter(|n| !INTERNAL.contains(&n.as_str()))
                    .collect();
                (*m, names)
            })
            .collect();
        for (name, set) in &sets[1..] {
            let (base_name, base) = &sets[0];
            let missing: Vec<_> = base.difference(set).collect();
            let extra: Vec<_> = set.difference(base).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "`simd::{name}` does not match `simd::{base_name}` — missing {missing:?}, extra {extra:?}"
            );
        }
    }
}
