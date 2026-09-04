//! The `wide`-crate kernel set — every kernel in [`super::x86_64`], written a second
//! time against `wide` 1.3's lane types instead of `core::arch` intrinsics.
//!
//! # Why a second copy exists
//!
//! To measure one thing: what it costs, and what it buys, to write these kernels in
//! safe, portable SIMD. Every file here is `#![forbid(unsafe_code)]`; the intrinsic
//! set is `unsafe` at every load, store and `target_feature` boundary. The two are
//! run against each other by `benches/kernel_bench.rs`, which times all three in one
//! process; whole-encoder numbers come from building `c_vs_rust_bench` once per
//! feature, since the kernel set is chosen when the binary is built.
//!
//! # The rules the port follows, so the comparison measures the API and not the author
//!
//! - **Same names, same signatures, same data access.** A kernel is named for the
//!   operation, never for an instruction set: `deblock_luma_lt4` is the same name
//!   here as in [`super::x86_64`], because it is the same kernel — the module path is
//!   what says which one you get. Each also reads its samples the same way (`row_n`,
//!   `row_view`, `block_span`), so [`super::kernels`] can alias either module and the
//!   bench sees the same bounds checks on both sides.
//!
//!   The one surviving suffix is `_avx2`, on the two 16-wide SAD entry points. That
//!   is not an exception to the rule: within [`super::x86_64`] those are a *second*
//!   kernel for the same slot, chosen by a second runtime test, so the name has to
//!   distinguish them from the baseline pair. This module fills the same two slots to
//!   keep the alias total, with 128-bit bodies that step two rows — see below.
//! - **Same algorithm.** Where the intrinsic kernel does a step in scalar code (the
//!   forward DCT's row pass, the IDCT's row pass, the plane predictors' coefficient
//!   sums) this does too. A kernel is restructured only where `wide` has no way to
//!   spell the intrinsic's operation.
//! - **What has to be emulated is said at the site**, and summed up here:
//!   - no `psadbw`: SAD is `max - min` on bytes, two zero-extends and adds, and a
//!     `pmaddwd`-against-ones reduce (`sad.rs`);
//!   - no `pavgb`: the rounded average is `(a | b) - ((a ^ b) >> 1)` with the bit
//!     that crosses a byte masked off (`mc.rs`);
//!   - no lane permutes on integer vectors (`swizzle` is `pshufb`, an SSSE3
//!     instruction, and scalar on the SSE2 baseline): the permutes are written as
//!     array casts in [`lanes`] and left to LLVM to turn back into
//!     `pshufd`/`pshuflw`/`punpck`;
//!   - no runtime feature dispatch: `u8x32` is two SSE2 halves unless the whole
//!     crate is built with `-C target-feature=+avx2`, so the `_avx2` entry points
//!     here are 128-bit kernels that step two rows;
//!   - no 16-byte load from `&[Cell<u8>]`: the block copies are the scalar's per-cell
//!     walk with the span check hoisted (`copy.rs`).
//!
//! # Where `wide` ends and `bytemuck` begins
//!
//! `wide` re-exports `bytemuck`, and `bytemuck::cast` is the only way to move between
//! lane types of one width (`i16x8` ↔ `u16x8`), to glue two halves into a 256-bit
//! type (`[i32x4; 2]` → `i32x8`), or to spell a permute. It is a size-checked
//! transmute between `Pod` types, and it is safe.

#![forbid(unsafe_code)]

pub mod copy;
pub mod dct;
pub mod deblock;
pub mod intra_pred;
pub mod mc;
pub mod quant;
pub mod sad;
pub mod satd;
pub mod score;

/// Loads, stores, widenings and permutes shared by the kernels.
///
/// The loads take a slice and copy its head into a zeroed 16-byte array, which is
/// what `movd`/`movq`/`movdqu` do into the low lanes of a register; LLVM folds the
/// copy into the load. The permutes are written as array casts because `wide` has no
/// integer-lane shuffle below SSSE3 — see the module header.
pub(crate) mod lanes {
    use wide::bytemuck::cast;
    use wide::{i16x8, u8x16};

    /// Sixteen bytes into all sixteen lanes.
    #[inline(always)]
    pub fn load16(r: &[u8]) -> u8x16 {
        u8x16::new(r[..16].try_into().expect("16 bytes"))
    }

    /// The first `W` bytes of `r` into the low `W` lanes, the rest zero.
    #[inline(always)]
    pub fn load_w<const W: usize>(r: &[u8]) -> u8x16 {
        let mut a = [0u8; 16];
        a[..W].copy_from_slice(&r[..W]);
        u8x16::new(a)
    }

    /// Eight bytes into the low eight lanes.
    #[inline(always)]
    pub fn load8(r: &[u8]) -> u8x16 {
        load_w::<8>(r)
    }

    /// Four bytes into the low four lanes.
    #[inline(always)]
    pub fn load4(r: &[u8]) -> u8x16 {
        load_w::<4>(r)
    }

    /// The low `W` lanes, out.
    #[inline(always)]
    pub fn store_w<const W: usize>(out: &mut [u8], v: u8x16) {
        out[..W].copy_from_slice(&v.to_array()[..W]);
    }

    /// The low eight lanes as an array.
    #[inline(always)]
    pub fn low8(v: u8x16) -> [u8; 8] {
        v.to_array()[..8].try_into().expect("8 lanes")
    }

    /// The low four lanes as an array.
    #[inline(always)]
    pub fn low4(v: u8x16) -> [u8; 4] {
        v.to_array()[..4].try_into().expect("4 lanes")
    }

    /// Zero-extends the low eight bytes to words — `punpcklbw` against zero.
    #[inline(always)]
    pub fn widen_lo(v: u8x16) -> i16x8 {
        i16x8::from_u8x16_low(v)
    }

    /// Zero-extends the high eight bytes to words — `punpckhbw` against zero.
    #[inline(always)]
    pub fn widen_hi(v: u8x16) -> i16x8 {
        i16x8::from_u8x16_high(v)
    }

    /// Saturates sixteen words to bytes — `packuswb`.
    #[inline(always)]
    pub fn narrow(lo: i16x8, hi: i16x8) -> u8x16 {
        u8x16::narrow_i16x8(lo, hi)
    }

    /// Sum of the eight lanes as an `i32`, without passing through an `i16` total:
    /// `pmaddwd` against ones widens on the way in, then a four-lane reduce.
    #[inline(always)]
    pub fn hsum_i16(v: i16x8) -> i32 {
        v.dot(i16x8::ONE).reduce_add()
    }

    /// Lanes `4..8` set — the mask that selects a vector's upper half in `blend`.
    pub const HIGH_HALF: i16x8 = i16x8::new([0, 0, 0, 0, -1, -1, -1, -1]);

    /// Lanes `2, 3` and `6, 7` set — the upper pair of each four-lane group.
    pub const QUAD_HIGH_PAIR: i16x8 = i16x8::new([0, 0, -1, -1, 0, 0, -1, -1]);

    /// `[hi | lo]` — swaps the two 64-bit halves (`pshufd 0x4E`).
    #[inline(always)]
    pub fn swap_halves(v: i16x8) -> i16x8 {
        let a: [u64; 2] = cast(v);
        cast([a[1], a[0]])
    }

    /// Swaps the two 32-bit halves of each 64-bit half (`pshufd 0xB1`): in four-lane
    /// groups, `[a b c d]` becomes `[c d a b]`.
    #[inline(always)]
    pub fn rotate_quads(v: i16x8) -> i16x8 {
        let a: [u32; 4] = cast(v);
        cast([a[1], a[0], a[3], a[2]])
    }

    /// Swaps each adjacent pair of lanes (`pshuflw`/`pshufhw 0xB1`).
    #[inline(always)]
    pub fn swap_adjacent(v: i16x8) -> i16x8 {
        let a: [u16; 8] = cast(v);
        cast([a[1], a[0], a[3], a[2], a[5], a[4], a[7], a[6]])
    }

    /// `[a.lo | b.lo]` — the low 64 bits of each, side by side (`punpcklqdq`).
    #[inline(always)]
    pub fn merge_lo64(a: i16x8, b: i16x8) -> i16x8 {
        let x: [u64; 2] = cast(a);
        let y: [u64; 2] = cast(b);
        cast([x[0], y[0]])
    }

    /// Transposes the 4x4 matrix held in the **low four lanes** of four vectors:
    /// column `j` of the input comes back as the low four lanes of output `j`. The
    /// upper lanes of every output are zero.
    ///
    /// The intrinsic twin is two `punpcklwd`, two `punpckldq` and two `psrldq`; this
    /// is the same permutation as one array expression, for LLVM to lower.
    #[inline(always)]
    pub fn transpose4_lo(v0: i16x8, v1: i16x8, v2: i16x8, v3: i16x8) -> (i16x8, i16x8, i16x8, i16x8) {
        let (a, b, c, d) = (v0.to_array(), v1.to_array(), v2.to_array(), v3.to_array());
        (
            i16x8::new([a[0], b[0], c[0], d[0], 0, 0, 0, 0]),
            i16x8::new([a[1], b[1], c[1], d[1], 0, 0, 0, 0]),
            i16x8::new([a[2], b[2], c[2], d[2], 0, 0, 0, 0]),
            i16x8::new([a[3], b[3], c[3], d[3], 0, 0, 0, 0]),
        )
    }
}
