//! SIMD acceleration kernels and CPU feature detection for openh264-rs.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

/// The NEON kernels. Compiled out under Miri, which cannot interpret them; that build
/// uses the scalar forwards.
#[cfg(all(target_arch = "aarch64", not(miri)))]
pub mod aarch64;

#[cfg(feature = "wide")]
pub mod wide;

/// The scalar forwards, compiled only where they are what [`kernels`] names.
#[cfg(any(
    feature = "scalar",
    not(any(
        target_arch = "x86_64",
        all(target_arch = "aarch64", not(miri)),
        feature = "wide"
    ))
))]
pub mod scalar;

#[cfg(all(
    target_arch = "aarch64",
    not(miri),
    not(feature = "wide"),
    not(feature = "scalar")
))]
pub use aarch64 as kernels;
#[cfg(any(
    feature = "scalar",
    not(any(
        target_arch = "x86_64",
        all(target_arch = "aarch64", not(miri)),
        feature = "wide"
    ))
))]
pub use scalar as kernels;
#[cfg(all(feature = "wide", not(feature = "scalar")))]
pub use wide as kernels;
/// The kernel set, and the whole of the dispatch. Every direct call site and every
/// `WELS_CPU_SSE2` table install names its kernel `kernels::<family>::<kernel>`, and
/// this alias decides what that resolves to. Dispatch files import it once at module
/// level; a kernel shares its name with the scalar body it replaces, and the module
/// qualifier is what tells them apart.
///
/// Selection is total and build-time: every build lands on exactly one arm, with no
/// runtime test in front of it.
///
/// | build | resolves to | what runs |
/// |---|---|---|
/// | x86_64, default | [`x86_64`] | `core::arch` SSE2 intrinsics |
/// | aarch64, default | [`aarch64`] | `core::arch` NEON intrinsics |
/// | `--features wide` | [`wide`] | portable `wide` lanes — NEON on aarch64 |
/// | `--features scalar` | [`scalar`] | forwards to the scalar body |
/// | no kernels for this target, or Miri on aarch64 | [`scalar`] | likewise |
///
/// `scalar` wins over `wide`, which wins over the default, so the two feature flags
/// compose rather than conflict. Dispatch sites carry no `#[cfg]` and no branch.
///
/// The four modules export the same entry points with the same signatures, so a
/// dispatch site does not change when the selection does. The intrinsic set for the
/// host and [`wide`] are both compiled whenever they can be.
#[cfg(all(target_arch = "x86_64", not(feature = "wide"), not(feature = "scalar")))]
pub use x86_64 as kernels;

use crate::common::cpu_core::*;

/// Detects available CPU SIMD features, once per process. The word says which vector
/// kernels this build has; `--features scalar` makes it `0`.
///
/// Latching keeps this to one probe per process; the initialiser is out of line so the
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

/// Runs once per process.
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
    // Under `--features scalar` there is no vector kernel for any slot, so no bit is
    // reported and the `pfXxx` tables install their scalar arms directly.
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

/// The aarch64 probe — `codec/common/src/cpu.cpp`'s `WelsCPUFeatureDetect` for
/// `HAVE_NEON_AARCH64`. No runtime detection: NEON is mandatory on every AArch64 CPU.
///
/// `WELS_CPU_SSE2` names a slot here, not an instruction set: the bit means "there is
/// a vector kernel for this slot". The `pfXxx` tables test that one bit on every
/// target, so the aarch64 kernel set — or [`wide`], whose lanes lower to NEON here —
/// reports it. `WELS_CPU_NEON` is set alongside; nothing dispatches on it.
///
/// `WELS_CPU_AVX2` stays clear: no `_avx2` entry point is a different kernel on this
/// target, and there is no wider register file to pick one for.
///
/// The word is `0` under `--features scalar`, and under Miri without `--features
/// wide`, where the NEON module is compiled out and `kernels` is the scalar set.
#[cfg(target_arch = "aarch64")]
fn arch_cpu_features() -> u32 {
    let neon_kernels = cfg!(all(not(miri), not(feature = "wide")));
    if cfg!(feature = "scalar") || !(neon_kernels || cfg!(feature = "wide")) {
        return 0;
    }
    WELS_CPU_SSE2 | WELS_CPU_NEON
}

/// Off x86_64 and aarch64 the answer is about the build, not the CPU: [`wide`]
/// compiles and runs wherever the crate does, so under `--features wide` the slots are
/// filled and `WELS_CPU_SSE2` — the slot bit — is set. Without the feature
/// every bit stays clear and every dispatch site takes its scalar fallback.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn arch_cpu_features() -> u32 {
    if cfg!(all(feature = "wide", not(feature = "scalar"))) {
        WELS_CPU_SSE2
    } else {
        0
    }
}

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The process-wide feature word, and whether it has been computed yet.
///
/// Two cells rather than a sentinel bit inside the word: every bit of the `u32` is a
/// real flag (`WELS_CPU_CACHELINE_128` is `0x8000_0000`), and `0` is a legitimate
/// answer.
static CPU_FEATURES: AtomicU32 = AtomicU32::new(0);
static CPU_FEATURES_READY: AtomicBool = AtomicBool::new(false);

/// Returns true if this build has the AVX2 kernels and the CPU can run them.
///
/// Unlike SSE2 this is not x86_64 baseline, so the runtime test is required: the AVX2
/// SAD kernels execute `vpsadbw` and fault on any pre-Haswell Intel or pre-Excavator
/// AMD part.
///
/// The `cfg!` only folds the branch away for a build that already guarantees AVX2, and
/// is false by default on every `x86_64-*` target; it cannot replace the runtime test,
/// because `-C target-feature=+avx2` applies to the whole crate. Per-function AVX2
/// codegen is `#[target_feature(enable = "avx2")]`, which `sad_16x_avx2` carries.
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
    /// The cut is at `#[cfg(test)]\nmod `, not at `#[cfg(test)]` alone: that attribute
    /// also sits on test-only `use` items among a file's imports.
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
                let tail = tail
                    .strip_prefix("pub(crate) ")
                    .or_else(|| tail.strip_prefix("pub "))
                    .unwrap();
                let tail = tail.strip_prefix("unsafe ").unwrap_or(tail);
                if let Some(tail) = tail.strip_prefix("fn ") {
                    let name: String = tail
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        out.insert(name, file.clone());
                    }
                }
                rest = &rest[i + 4..];
            }
        }
        out
    }

    /// Kernels internal to a module, which no dispatch site names: the generic workers
    /// the shaped entry points instantiate (`sad_16x` and friends), the 16-sample inner
    /// loops of the deblocking filters, the `#[target_feature]` body `satd_4x4`
    /// delegates to, and `wide`'s load/store/permute helpers.
    ///
    /// Maintained by hand; it fails closed — an unlisted kernel that nothing dispatches
    /// makes the test fail.
    const INTERNAL: &[&str] = &[
        "deblock_chroma_eq4_16",
        "deblock_chroma_lt4_16",
        "deblock_luma_eq4_16",
        "deblock_luma_lt4_16",
        "sad_16x",
        "sad_16x_avx2",
        "sad_4x",
        "sad_8x",
        "sample_sad_four_16x",
        "sample_sad_four_4x",
        "sample_sad_four_8x",
        "satd_4x4_sse2_impl",
        "hsum_i16",
        "load16",
        "load4",
        "load8",
        "load_w",
        "low4",
        "low8",
        "merge_lo64",
        "narrow",
        "rotate_quads",
        "store_w",
        "swap_adjacent",
        "swap_halves",
        "transpose4_lo",
        "widen_hi",
        "widen_lo",
    ];

    /// Every kernel entry point is either named somewhere outside `src/simd/` or listed
    /// in [`INTERNAL`]. A slot left holding the scalar passes every parity and
    /// conformance test, so an unreached kernel is invisible to everything else.
    ///
    /// The check matches source text rather than function pointers, so it does not
    /// claim that the site executes (a reference inside a disabled `#[cfg]` still
    /// counts) or that a slot holds the right kernel.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn every_kernel_is_named_by_a_dispatch_site() {
        // Reached means "named through the alias", not "this token appears": a kernel
        // shares its name with the scalar body it replaces, so a bare-token search
        // would report success for a slot that was never wired.
        //
        // The exception is the sites that glob-import a kernel module and then call its
        // entries bare; for those, every token in the file counts. Both are
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
                let seg: String = tail
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let after = &tail[seg.len()..];
                if let Some(after) = after.strip_prefix("::") {
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
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
    /// target and not another.
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
