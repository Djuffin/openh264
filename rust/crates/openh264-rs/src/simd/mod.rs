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

/// The NEON kernels. Compiled out under Miri, which cannot interpret them — see the
/// module's own header — so that lane keeps the scalar forwards it always had.
#[cfg(all(target_arch = "aarch64", not(miri)))]
pub mod aarch64;

#[cfg(feature = "wide")]
pub mod wide;

/// The scalar forwards, compiled only where they are what [`kernels`] names.
#[cfg(any(feature = "scalar", not(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri)), feature = "wide"))))]
pub mod scalar;

/// **The kernel set, and the whole of the dispatch.** Every direct call site and every
/// `WELS_CPU_SSE2` table install names its kernel `kernels::<family>::<kernel>`, and
/// this alias decides what that resolves to. Each dispatch file imports it once at
/// module level rather than spelling the path per call, because a kernel shares its
/// name with the scalar it was ported from — `pixel_avg` is both — and the module
/// qualifier is what tells them apart.
///
/// **The alias is total, and it is the selection.** Every build lands on exactly one
/// arm, and there is no runtime test in front of it:
///
/// | build | resolves to | what runs |
/// |---|---|---|
/// | x86_64, default | [`x86_64`] | `core::arch` SSE2 intrinsics |
/// | aarch64, default | [`aarch64`] | `core::arch` NEON intrinsics, ported from upstream's arm64 asm |
/// | `--features wide` | [`wide`] | portable `wide` lanes — NEON on aarch64 |
/// | `--features scalar` | [`scalar`] | forwards to the scalar body |
/// | no kernels for this target, or Miri on aarch64 | [`scalar`] | likewise |
///
/// `scalar` wins over `wide`, which wins over the default, so the two feature flags
/// compose rather than conflict.
///
/// **This is how the reference does it too.** Upstream dispatches every kernel through
/// its `pfXxx` tables under `#if defined(X86_ASM)`, has no environment switch anywhere
/// in `codec/`, and gets a scalar build from `USE_ASM=No` at the Makefile.
/// `--features scalar` is that flag. Selecting a kernel set is a build-time question,
/// asked once here, and the sites carry no `#[cfg]` and no branch.
///
/// The four modules export the same entry points with the same signatures, which
/// `tests::the_kernel_sets_expose_the_same_entry_points` holds; the sites therefore do
/// not change when the selection does, only this block does. The intrinsic set for
/// the host and [`wide`] are both compiled whenever they can be, which is what lets
/// `benches/kernel_bench.rs` time the implementations of one kernel in one process.
#[cfg(all(target_arch = "x86_64", not(feature = "wide"), not(feature = "scalar")))]
pub use x86_64 as kernels;
#[cfg(all(target_arch = "aarch64", not(miri), not(feature = "wide"), not(feature = "scalar")))]
pub use aarch64 as kernels;
#[cfg(all(feature = "wide", not(feature = "scalar")))]
pub use wide as kernels;
#[cfg(any(feature = "scalar", not(any(target_arch = "x86_64", all(target_arch = "aarch64", not(miri)), feature = "wide"))))]
pub use scalar as kernels;

use crate::common::cpu_core::*;

/// Detects available CPU SIMD features, once per process.
///
/// **What selects scalar is the build, not the environment.** `OPENH264_NO_SIMD` used
/// to clear this word, and it was removed with the per-call `has_simd()` sites it was
/// the other half of: with the twenty-two direct sites now calling [`kernels`]
/// unconditionally, clearing the word would take the `pfXxx` tables scalar and leave
/// motion compensation, deblocking and the IDCTs on the vector kernels — a switch that
/// half-applies is worse than none. `--features scalar` is the switch now, and it is
/// the reference's own (`USE_ASM=No`); see the per-arch `arch_cpu_features`.
///
/// Latching keeps this to one probe per process. Out-of-line initialiser so the
/// steady-state read is an acquire load and a compare.
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
    let flags = arch_cpu_features();
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
    // `--features scalar` is this port's `USE_ASM=No`: `kernels` is the scalar set, so
    // there is no vector kernel for any slot and no bit to report. The `pfXxx` tables
    // then install the scalar arm directly rather than a forward to it.
    if cfg!(feature = "scalar") {
        return 0;
    }

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

/// The aarch64 probe. Upstream's is `cpu.cpp`'s `WelsCPUFeatureDetect` for
/// `HAVE_NEON_AARCH64`, which does no runtime detection at all — NEON is mandatory on
/// every AArch64 CPU — and returns `WELS_CPU_VFPv3 | WELS_CPU_NEON`. This answers the
/// same way, with one more bit.
///
/// **`WELS_CPU_SSE2` names a slot here, not an instruction set.** It is upstream's
/// flag word (`codec/common/src/cpu.cpp`), which the port keeps because the `pfXxx`
/// tables are built from it exactly as the C++ builds them — but what the bit *means*
/// to this port is "there is a vector kernel for this slot". Upstream's tables test
/// `WELS_CPU_NEON` on arm64 and `WELS_CPU_SSE2` on x86 to install the same slots; the
/// port's tables test the one bit on every target, so the aarch64 kernel set — or
/// [`wide`], whose lanes lower to NEON here — reports it. `WELS_CPU_NEON` is set
/// alongside because it is what the hardware is; nothing dispatches on it.
///
/// **`WELS_CPU_AVX2` is deliberately left clear.** It is the one place a second
/// runtime test picks a second kernel for a slot already filled, and there is no
/// wider register file here to pick it for: neither set's `_avx2` entry points are a
/// different kernel on this target.
///
/// `--features scalar` is `USE_ASM=No`, as on x86_64: the word is `0` and the tables
/// install their scalar arms. So is a Miri run without `--features wide`, where the
/// NEON module is compiled out and `kernels` is the scalar set.
#[cfg(target_arch = "aarch64")]
fn arch_cpu_features() -> u32 {
    let neon_kernels = cfg!(all(not(miri), not(feature = "wide")));
    if cfg!(feature = "scalar") || !(neon_kernels || cfg!(feature = "wide")) {
        return 0;
    }
    WELS_CPU_SSE2 | WELS_CPU_NEON
}

/// Off x86_64 and aarch64 the answer is a question about the build, not about the
/// CPU: [`wide`] compiles and runs wherever the crate does, so under `--features
/// wide` the slots are filled and `WELS_CPU_SSE2` — the slot bit, see above — is
/// set. Without the feature there is no kernel set to point at, so every bit stays
/// clear and every dispatch site takes its scalar fallback.
///
/// Split out per arch rather than `#[cfg]`-ing a block inside one function, so that
/// `flags` is only `mut` where it is actually mutated (`lib.rs` denies `unused_mut`).
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn arch_cpu_features() -> u32 {
    if cfg!(all(feature = "wide", not(feature = "scalar"))) { WELS_CPU_SSE2 } else { 0 }
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
/// `0` has to stay a legitimate answer, and is: `arch_cpu_features` returns it under
/// `--features scalar` and on every non-x86_64 target without `--features wide`.
static CPU_FEATURES: AtomicU32 = AtomicU32::new(0);
static CPU_FEATURES_READY: AtomicBool = AtomicBool::new(false);

/// Returns true if this build has the AVX2 kernels and the CPU can run them.
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
/// which `sad_16x_avx2` carries. On such a build this answers `true` on the `cfg!`
/// alone, which is consistent — that binary is AVX2 throughout. Under `--features
/// scalar` the feature word is `0`, so the `cfg!` is the only way it can be true; also
/// consistent, since a `+avx2` build asked for AVX2 everywhere.
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
        for module in ["x86_64", "aarch64", "wide"] {
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

    /// The four kernel sets have to agree on their entry points, or `simd::kernels`
    /// would resolve differently per build and a dispatch site would compile on one
    /// target and not another. Cheaper to learn here than from a cross-compile.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn the_kernel_sets_expose_the_same_entry_points() {
        let sets: Vec<(&str, BTreeSet<String>)> = ["x86_64", "aarch64", "wide", "scalar"]
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
