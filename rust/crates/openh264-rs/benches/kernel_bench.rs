//! Per-kernel timing of the three implementations of each SIMD kernel: the scalar
//! reference, the `core::arch` intrinsics in `simd::x86_64`, and — when built with
//! `--features wide` — the `wide`-crate kernels in `simd::wide`.
//!
//! One process, one set of inputs, three closures per row. Every row is checked
//! before it is timed: each implementation runs once on its own fresh copy of the
//! inputs and the three checksums must agree, or the row is reported as a mismatch
//! and the process exits non-zero. Timing is the best of `BENCH_REPEATS` blocks of
//! calls, each block sized to run about `BENCH_BLOCK_MS` milliseconds, reported as
//! nanoseconds per call. Best-of, because on a desktop the slow blocks are the
//! scheduler and the fast ones are the kernel.
//!
//! The inputs are the shapes the encoder hands each kernel: 64-byte-stride planes of
//! noise anchored off the alignment, coefficient blocks over the quantiser's real
//! range, and for the deblocking filters a smooth gradient so the filter conditions
//! hold on most lines (an all-noise plane takes the early-out on every line, which
//! times the compare and nothing else).
//!
//! Run as:
//!
//! ```text
//! cargo bench --bench kernel_bench --features wide
//! ```
//!
//! Environment knobs: `BENCH_REPEATS` (default 7), `BENCH_BLOCK_MS` (default 20),
//! `BENCH_FILTER=<substring>` to run only matching rows.

#![allow(non_snake_case, unused_variables)]

use std::hint::black_box;
use std::time::Instant;

use openh264_rs::common::deblocking_common as dbk;
use openh264_rs::common::mc;
use openh264_rs::common::sad_common::{sample_sad, sample_sad_four};
use openh264_rs::decoder::decode_mb_aux::idct_res_add_pred_c;
use openh264_rs::encoder::decode_mb_aux as dec_aux;
use openh264_rs::encoder::encode_mb_aux as enc_aux;
use openh264_rs::encoder::get_intra_predictor as ipred;
use openh264_rs::encoder::rec_view::RecCursor;
use openh264_rs::encoder::sample as satd_ref;
use openh264_rs::safe::plane::{PaddedPlane, PlaneCursor, PlaneCursorMut};
use openh264_rs::simd::x86_64 as isa;
#[cfg(feature = "wide")]
use openh264_rs::simd::wide as wd;

// ============================================================================
// Harness
// ============================================================================

struct Row {
    name: &'static str,
    /// ns per call: scalar, intrinsics, wide (`None` when not built).
    ns: [Option<f64>; 3],
    /// The three checksums agreed.
    consistent: bool,
}

fn repeats() -> usize {
    std::env::var("BENCH_REPEATS").ok().and_then(|v| v.parse().ok()).unwrap_or(7)
}

fn block_ms() -> f64 {
    std::env::var("BENCH_BLOCK_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(20.0)
}

/// Best-of-`repeats` nanoseconds per call of `f`, over blocks of about `block_ms`.
fn time_one<F: FnMut(bool) -> u64>(mut f: F) -> f64 {
    // Size the block from a short probe.
    let probe = 2_000u32;
    let t = Instant::now();
    for _ in 0..probe {
        black_box(f(black_box(false)));
    }
    let per_call = t.elapsed().as_secs_f64() / probe as f64;
    let iters = ((block_ms() / 1000.0) / per_call.max(1e-9)).clamp(1_000.0, 50_000_000.0) as u32;

    let mut best = f64::MAX;
    for _ in 0..repeats() {
        let t = Instant::now();
        for _ in 0..iters {
            black_box(f(black_box(false)));
        }
        let ns = t.elapsed().as_nanos() as f64 / iters as f64;
        best = best.min(ns);
    }
    best
}

/// Checks the three implementations agree, then times each. A closure is called
/// with `true` once, to produce a checksum of its whole output; with `false` in the
/// timed loops, where it returns something cheap the optimiser cannot drop.
fn run3<S, I, W>(rows: &mut Vec<Row>, name: &'static str, mut scalar: S, mut intrinsics: I, wide: Option<W>)
where
    S: FnMut(bool) -> u64,
    I: FnMut(bool) -> u64,
    W: FnMut(bool) -> u64,
{
    if let Ok(filter) = std::env::var("BENCH_FILTER") {
        if !name.contains(&filter) {
            return;
        }
    }
    let mut wide = wide;
    let c_scalar = scalar(true);
    let c_isa = intrinsics(true);
    let c_wide = wide.as_mut().map(|w| w(true));
    let consistent = c_scalar == c_isa && c_wide.map_or(true, |c| c == c_scalar);
    if !consistent {
        eprintln!(" MISMATCH {name}: scalar {c_scalar:#x} intrinsics {c_isa:#x} wide {c_wide:x?}");
    }
    let ns_s = time_one(&mut scalar);
    let ns_i = time_one(&mut intrinsics);
    let ns_w = wide.as_mut().map(|w| time_one(w));
    rows.push(Row { name, ns: [Some(ns_s), Some(ns_i), ns_w], consistent });
    eprint!(".");
}

/// A `run3` row whose wide arm exists only under the feature.
macro_rules! row {
    ($rows:expr, $name:expr, $scalar:expr, $isa:expr, $wide:expr) => {{
        #[cfg(feature = "wide")]
        run3(&mut $rows, $name, $scalar, $isa, Some($wide));
        #[cfg(not(feature = "wide"))]
        run3(&mut $rows, $name, $scalar, $isa, None::<fn(bool) -> u64>);
    }};
}

// ============================================================================
// Inputs and checksums
// ============================================================================

fn lcg(seed: &mut u64) -> u32 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*seed >> 32) as u32
}

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut s = seed;
    (0..len).map(|_| lcg(&mut s) as u8).collect()
}

/// Noise of amplitude `amp` around a slow ramp — what a reconstructed picture looks
/// like near a block edge that the deblocking filter will actually touch.
fn smooth(stride: usize, rows: usize, amp: u32, seed: u64) -> Vec<u8> {
    let mut s = seed;
    let mut v = vec![0u8; stride * rows];
    for y in 0..rows {
        for x in 0..stride {
            let base = 96 + ((x + y) / 4) as u32;
            v[y * stride + x] = (base + lcg(&mut s) % (amp + 1)) as u8;
        }
    }
    v
}

fn coeffs<const N: usize>(seed: u64, range: i32) -> [i16; N] {
    let mut s = seed;
    core::array::from_fn(|_| ((lcg(&mut s) as i32 % (2 * range + 1)) - range) as i16)
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn fnv16(v: &[i16]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in v {
        h ^= x as u16 as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

const STRIDE: usize = 64;
const ROWS: usize = 64;
/// Anchor off every 16-byte boundary, as motion search lands.
const ANCHOR: usize = 20 * STRIDE + 19;

/// Two source planes and one destination, fresh per implementation.
struct Planes {
    a: Vec<u8>,
    b: Vec<u8>,
    out: Vec<u8>,
}

impl Planes {
    fn new() -> Self {
        Self { a: noise(STRIDE * ROWS, 1), b: noise(STRIDE * ROWS, 2), out: vec![0; STRIDE * ROWS] }
    }
    fn ca(&self) -> PlaneCursor<'_> {
        PlaneCursor::new(&self.a, ANCHOR, STRIDE)
    }
    fn cb(&self) -> PlaneCursor<'_> {
        PlaneCursor::new(&self.b, ANCHOR, STRIDE)
    }
    fn dst(&mut self) -> PlaneCursorMut<'_> {
        PlaneCursorMut::new(&mut self.out, ANCHOR, STRIDE)
    }
}

/// Field-level cursors, so one closure can borrow `a`/`b` and `out` at once.
fn cur(v: &[u8]) -> PlaneCursor<'_> {
    PlaneCursor::new(v, ANCHOR, STRIDE)
}

fn cur_mut(v: &mut [u8]) -> PlaneCursorMut<'_> {
    PlaneCursorMut::new(v, ANCHOR, STRIDE)
}

fn out_sum(out: &[u8], check: bool) -> u64 {
    if check { fnv(out) } else { out[ANCHOR] as u64 }
}

// ============================================================================
// The rows
// ============================================================================

fn sad_rows(rows: &mut Vec<Row>) {
    let p = Planes::new();
    let (a, b) = (p.ca(), p.cb());
    row!(*rows, "sad 16x16",
        |_| sample_sad::<16, 16, _>(&black_box(a), &black_box(b)) as u64,
        |_| isa::sad::sample_sad_16x16_sse2(&black_box(a), &black_box(b)) as u64,
        |_| wd::sad::sample_sad_16x16_sse2(&black_box(a), &black_box(b)) as u64);
    row!(*rows, "sad 8x8",
        |_| sample_sad::<8, 8, _>(&black_box(a), &black_box(b)) as u64,
        |_| isa::sad::sample_sad_8x8_sse2(&black_box(a), &black_box(b)) as u64,
        |_| wd::sad::sample_sad_8x8_sse2(&black_box(a), &black_box(b)) as u64);
    row!(*rows, "sad 4x4",
        |_| sample_sad::<4, 4, _>(&black_box(a), &black_box(b)) as u64,
        |_| isa::sad::sample_sad_4x4_sse2(&black_box(a), &black_box(b)) as u64,
        |_| wd::sad::sample_sad_4x4_sse2(&black_box(a), &black_box(b)) as u64);
    let four = |s: &mut [i32; 4]| (s[0] as u64) | (s[1] as u64) << 16 | (s[2] as u64) << 32 | (s[3] as u64) << 48;
    row!(*rows, "sad-four 16x16",
        |_| { let mut s = [0; 4]; sample_sad_four::<16, 16, _>(&black_box(a), &black_box(b), &mut s); four(&mut s) },
        |_| { let mut s = [0; 4]; isa::sad::sample_sad_four_16x16_sse2(&black_box(a), &black_box(b), &mut s); four(&mut s) },
        |_| { let mut s = [0; 4]; wd::sad::sample_sad_four_16x16_sse2(&black_box(a), &black_box(b), &mut s); four(&mut s) });
    row!(*rows, "sad-four 8x8",
        |_| { let mut s = [0; 4]; sample_sad_four::<8, 8, _>(&black_box(a), &black_box(b), &mut s); four(&mut s) },
        |_| { let mut s = [0; 4]; isa::sad::sample_sad_four_8x8_sse2(&black_box(a), &black_box(b), &mut s); four(&mut s) },
        |_| { let mut s = [0; 4]; wd::sad::sample_sad_four_8x8_sse2(&black_box(a), &black_box(b), &mut s); four(&mut s) });
}

fn satd_rows(rows: &mut Vec<Row>) {
    let p = Planes::new();
    let (a, b) = (p.ca(), p.cb());
    row!(*rows, "satd 4x4",
        |_| satd_ref::satd_4x4(&black_box(a), &black_box(b)) as u64,
        |_| isa::satd::satd_4x4_sse2(&black_box(a), &black_box(b)) as u64,
        |_| wd::satd::satd_4x4_sse2(&black_box(a), &black_box(b)) as u64);
    row!(*rows, "satd 8x8",
        |_| satd_ref::satd_8x8(&black_box(a), &black_box(b)) as u64,
        |_| isa::satd::satd_8x8_sse2(&black_box(a), &black_box(b)) as u64,
        |_| wd::satd::satd_8x8_sse2(&black_box(a), &black_box(b)) as u64);
    row!(*rows, "satd 16x16",
        |_| satd_ref::satd_16x16(&black_box(a), &black_box(b)) as u64,
        |_| isa::satd::satd_16x16_sse2(&black_box(a), &black_box(b)) as u64,
        |_| wd::satd::satd_16x16_sse2(&black_box(a), &black_box(b)) as u64);
}

fn mc_rows(rows: &mut Vec<Row>) {
    let (mut s0, mut s1, mut s2) = (Planes::new(), Planes::new(), Planes::new());
    row!(*rows, "mc pixel_avg 16x16",
        |c| { let (a, b) = (cur(black_box(&s0.a)), cur(black_box(&s0.b))); mc::pixel_avg_c(&mut cur_mut(&mut s0.out), &black_box(a), &black_box(b), 16, 16); out_sum(&s0.out, c) },
        |c| { let (a, b) = (cur(black_box(&s1.a)), cur(black_box(&s1.b))); isa::mc::pixel_avg_sse2(&mut cur_mut(&mut s1.out), &black_box(a), &black_box(b), 16, 16); out_sum(&s1.out, c) },
        |c| { let (a, b) = (cur(black_box(&s2.a)), cur(black_box(&s2.b))); wd::mc::pixel_avg_sse2(&mut cur_mut(&mut s2.out), &black_box(a), &black_box(b), 16, 16); out_sum(&s2.out, c) });
    row!(*rows, "mc hor_ver20 16x16",
        |c| { let a = cur(black_box(&s0.a)); mc::mc_hor_ver20_c(&a, &mut cur_mut(&mut s0.out), 16, 16); out_sum(&s0.out, c) },
        |c| { let a = cur(black_box(&s1.a)); isa::mc::mc_hor_ver20_sse2(&a, &mut cur_mut(&mut s1.out), 16, 16); out_sum(&s1.out, c) },
        |c| { let a = cur(black_box(&s2.a)); wd::mc::mc_hor_ver20_sse2(&a, &mut cur_mut(&mut s2.out), 16, 16); out_sum(&s2.out, c) });
    row!(*rows, "mc hor_ver02 16x16",
        |c| { let a = cur(black_box(&s0.a)); mc::mc_hor_ver02_c(&a, &mut cur_mut(&mut s0.out), 16, 16); out_sum(&s0.out, c) },
        |c| { let a = cur(black_box(&s1.a)); isa::mc::mc_hor_ver02_sse2(&a, &mut cur_mut(&mut s1.out), 16, 16); out_sum(&s1.out, c) },
        |c| { let a = cur(black_box(&s2.a)); wd::mc::mc_hor_ver02_sse2(&a, &mut cur_mut(&mut s2.out), 16, 16); out_sum(&s2.out, c) });
    row!(*rows, "mc hor_ver22 16x16",
        |c| { let a = cur(black_box(&s0.a)); mc::mc_hor_ver22_c(&a, &mut cur_mut(&mut s0.out), 16, 16); out_sum(&s0.out, c) },
        |c| { let a = cur(black_box(&s1.a)); isa::mc::mc_hor_ver22_sse2(&a, &mut cur_mut(&mut s1.out), 16, 16); out_sum(&s1.out, c) },
        |c| { let a = cur(black_box(&s2.a)); wd::mc::mc_hor_ver22_sse2(&a, &mut cur_mut(&mut s2.out), 16, 16); out_sum(&s2.out, c) });
    row!(*rows, "mc hor_ver02 8x8",
        |c| { let a = cur(black_box(&s0.a)); mc::mc_hor_ver02_c(&a, &mut cur_mut(&mut s0.out), 8, 8); out_sum(&s0.out, c) },
        |c| { let a = cur(black_box(&s1.a)); isa::mc::mc_hor_ver02_sse2(&a, &mut cur_mut(&mut s1.out), 8, 8); out_sum(&s1.out, c) },
        |c| { let a = cur(black_box(&s2.a)); wd::mc::mc_hor_ver02_sse2(&a, &mut cur_mut(&mut s2.out), 8, 8); out_sum(&s2.out, c) });
    // Quarter-pel (1, 3): the horizontal filter averaged with the centre filter.
    row!(*rows, "mc luma qpel(1,3) 16x16",
        |c| { let a = cur(black_box(&s0.a)); mc::mc_luma_c(&a, &mut cur_mut(&mut s0.out), black_box(1), black_box(3), 16, 16); out_sum(&s0.out, c) },
        |c| { let a = cur(black_box(&s1.a)); isa::mc::mc_luma_sse2(&a, &mut cur_mut(&mut s1.out), black_box(1), black_box(3), 16, 16); out_sum(&s1.out, c) },
        |c| { let a = cur(black_box(&s2.a)); wd::mc::mc_luma_sse2(&a, &mut cur_mut(&mut s2.out), black_box(1), black_box(3), 16, 16); out_sum(&s2.out, c) });
    row!(*rows, "mc chroma (3,5) 8x8",
        |c| { let a = cur(black_box(&s0.a)); mc::mc_chroma_with_frag_mv(&a, &mut cur_mut(&mut s0.out), black_box(3), black_box(5), 8, 8); out_sum(&s0.out, c) },
        |c| { let a = cur(black_box(&s1.a)); isa::mc::mc_chroma_sse2(&a, &mut cur_mut(&mut s1.out), black_box(3), black_box(5), 8, 8); out_sum(&s1.out, c) },
        |c| { let a = cur(black_box(&s2.a)); wd::mc::mc_chroma_sse2(&a, &mut cur_mut(&mut s2.out), black_box(3), black_box(5), 8, 8); out_sum(&s2.out, c) });
    row!(*rows, "mc chroma (3,5) 4x4",
        |c| { let a = cur(black_box(&s0.a)); mc::mc_chroma_with_frag_mv(&a, &mut cur_mut(&mut s0.out), black_box(3), black_box(5), 4, 4); out_sum(&s0.out, c) },
        |c| { let a = cur(black_box(&s1.a)); isa::mc::mc_chroma_sse2(&a, &mut cur_mut(&mut s1.out), black_box(3), black_box(5), 4, 4); out_sum(&s1.out, c) },
        |c| { let a = cur(black_box(&s2.a)); wd::mc::mc_chroma_sse2(&a, &mut cur_mut(&mut s2.out), black_box(3), black_box(5), 4, 4); out_sum(&s2.out, c) });
}

fn dct_rows(rows: &mut Vec<Row>) {
    let p = Planes::new();
    let (a, b) = (p.ca(), p.cb());
    let sum16 = |d: &[i16; 16], c: bool| if c { fnv16(d) } else { d[0] as u16 as u64 };
    let sum64 = |d: &[i16; 64], c: bool| if c { fnv16(d) } else { d[0] as u16 as u64 };
    row!(*rows, "dct 4x4",
        |c| { let mut d = [0i16; 16]; enc_aux::dct_4x4(&mut d, &black_box(a), &black_box(b)); sum16(&d, c) },
        |c| { let mut d = [0i16; 16]; isa::dct::dct_4x4_sse2(&mut d, &black_box(a), &black_box(b)); sum16(&d, c) },
        |c| { let mut d = [0i16; 16]; wd::dct::dct_4x4_sse2(&mut d, &black_box(a), &black_box(b)); sum16(&d, c) });
    row!(*rows, "dct four 4x4",
        |c| { let mut d = [0i16; 64]; enc_aux::dct_four_4x4(&mut d, &black_box(a), &black_box(b)); sum64(&d, c) },
        |c| { let mut d = [0i16; 64]; isa::dct::dct_four_4x4_sse2(&mut d, &black_box(a), &black_box(b)); sum64(&d, c) },
        |c| { let mut d = [0i16; 64]; wd::dct::dct_four_4x4_sse2(&mut d, &black_box(a), &black_box(b)); sum64(&d, c) });

    // Residuals over the decoder's real range, on a fresh prediction per call.
    let res: [i16; 16] = coeffs(7, 2000);
    let (mut r0, mut r1, mut r2) = (Planes::new(), Planes::new(), Planes::new());
    row!(*rows, "idct t4 in place",
        |c| { dec_aux::idct_t4_rec_in_place_c(&mut cur_mut(&mut r0.out), black_box(&res)); out_sum(&r0.out, c) },
        |c| { isa::dct::idct_t4_rec_in_place_sse2(&mut cur_mut(&mut r1.out), black_box(&res)); out_sum(&r1.out, c) },
        |c| { wd::dct::idct_t4_rec_in_place_sse2(&mut cur_mut(&mut r2.out), black_box(&res)); out_sum(&r2.out, c) });
    row!(*rows, "idct res_add_pred (decoder)",
        |c| { idct_res_add_pred_c(&mut cur_mut(&mut r0.out), black_box(&res)); out_sum(&r0.out, c) },
        |c| { isa::dct::idct_res_add_pred_sse2(&mut cur_mut(&mut r1.out), black_box(&res)); out_sum(&r1.out, c) },
        |c| { wd::dct::idct_res_add_pred_sse2(&mut cur_mut(&mut r2.out), black_box(&res)); out_sum(&r2.out, c) });
    let dc: [i16; 16] = coeffs(9, 3000);
    row!(*rows, "idct i16x16 dc",
        |c| { let p = cur(black_box(&r0.b)); dec_aux::idct_rec_i16x16_dc_c(&mut cur_mut(&mut r0.out), &p, black_box(&dc)); out_sum(&r0.out, c) },
        |c| { let p = cur(black_box(&r1.b)); isa::dct::idct_rec_i16x16_dc_sse2(&mut cur_mut(&mut r1.out), &p, black_box(&dc)); out_sum(&r1.out, c) },
        |c| { let p = cur(black_box(&r2.b)); wd::dct::idct_rec_i16x16_dc_sse2(&mut cur_mut(&mut r2.out), &p, black_box(&dc)); out_sum(&r2.out, c) });
}

fn quant_rows(rows: &mut Vec<Row>) {
    let ff = enc_aux::G_KI_QUANT_INTER_FF.0[22];
    let mf = enc_aux::g_kiQuantMF[22];
    let in16: [i16; 16] = coeffs(11, 1500);
    let in64: [i16; 64] = coeffs(13, 1500);
    let sum16 = |d: &[i16; 16], c: bool| if c { fnv16(d) } else { d[0] as u16 as u64 };
    let sum64 = |d: &[i16; 64], c: bool| if c { fnv16(d) } else { d[0] as u16 as u64 };
    row!(*rows, "quant 4x4",
        |c| { let mut d = black_box(in16); enc_aux::quant_4x4(&mut d, black_box(&ff), black_box(&mf)); sum16(&d, c) },
        |c| { let mut d = black_box(in16); isa::quant::quant_4x4_sse2(&mut d, black_box(&ff), black_box(&mf)); sum16(&d, c) },
        |c| { let mut d = black_box(in16); wd::quant::quant_4x4_sse2(&mut d, black_box(&ff), black_box(&mf)); sum16(&d, c) });
    row!(*rows, "quant four 4x4 max",
        |c| { let mut d = black_box(in64); let mut m = [0; 4]; enc_aux::quant_four_4x4_max(&mut d, black_box(&ff), black_box(&mf), &mut m); sum64(&d, c) ^ m[0] as u64 },
        |c| { let mut d = black_box(in64); let mut m = [0; 4]; isa::quant::quant_four_4x4_max_sse2(&mut d, black_box(&ff), black_box(&mf), &mut m); sum64(&d, c) ^ m[0] as u64 },
        |c| { let mut d = black_box(in64); let mut m = [0; 4]; wd::quant::quant_four_4x4_max_sse2(&mut d, black_box(&ff), black_box(&mf), &mut m); sum64(&d, c) ^ m[0] as u64 });
    let dq: [u16; 8] = [10, 13, 16, 13, 10, 13, 16, 13];
    row!(*rows, "dequant four 4x4",
        |c| { let mut d = black_box(in64); dec_aux::dequant_four_4x4(&mut d, black_box(&dq)); sum64(&d, c) },
        |c| { let mut d = black_box(in64); isa::quant::dequant_four_4x4_sse2(&mut d, black_box(&dq)); sum64(&d, c) },
        |c| { let mut d = black_box(in64); wd::quant::dequant_four_4x4_sse2(&mut d, black_box(&dq)); sum64(&d, c) });
    let mb: [i16; 241] = coeffs(17, 2000);
    row!(*rows, "hadamard t4 dc",
        |c| { let mut d = [0i16; 16]; enc_aux::hadamard_t4_dc(&mut d, black_box(&mb)); sum16(&d, c) },
        |c| { let mut d = [0i16; 16]; isa::quant::hadamard_t4_dc_sse2(&mut d, black_box(&mb)); sum16(&d, c) },
        |c| { let mut d = [0i16; 16]; wd::quant::hadamard_t4_dc_sse2(&mut d, black_box(&mb)); sum16(&d, c) });
    row!(*rows, "dequant ihadamard 4x4",
        |c| { let mut d = black_box(in16); dec_aux::dequant_ihadamard_4x4(&mut d, black_box(13)); sum16(&d, c) },
        |c| { let mut d = black_box(in16); isa::quant::dequant_ihadamard_4x4_sse2(&mut d, black_box(13)); sum16(&d, c) },
        |c| { let mut d = black_box(in16); wd::quant::dequant_ihadamard_4x4_sse2(&mut d, black_box(13)); sum16(&d, c) });
    // A typical quantised block: mostly zeros.
    let mut sparse = [0i16; 16];
    for (i, v) in sparse.iter_mut().enumerate() {
        if i % 3 == 0 { *v = (i as i16) - 5; }
    }
    row!(*rows, "get_none_zero_count",
        |_| enc_aux::get_none_zero_count(black_box(&sparse)) as u64,
        |_| isa::quant::get_none_zero_count_sse2(black_box(&sparse)) as u64,
        |_| wd::quant::get_none_zero_count_sse2(black_box(&sparse)) as u64);
    row!(*rows, "calculate_single_ctr 4x4",
        |_| enc_aux::calculate_single_ctr_4x4(black_box(&sparse)) as u64,
        |_| isa::score::calculate_single_ctr_4x4_sse2(black_box(&sparse)) as u64,
        |_| wd::score::calculate_single_ctr_4x4_sse2(black_box(&sparse)) as u64);
}

fn copy_rows(rows: &mut Vec<Row>) {
    let src = noise(STRIDE * ROWS, 21);
    let (mut d0, mut d1, mut d2) = (vec![0u8; STRIDE * ROWS], vec![0u8; STRIDE * ROWS], vec![0u8; STRIDE * ROWS]);
    let mut s0 = src.clone();
    let mut s1 = src.clone();
    let mut s2 = src.clone();
    let sum = |d: &[u8], c: bool| if c { fnv(d) } else { d[ANCHOR] as u64 };
    row!(*rows, "copy 16x16",
        |c| { let (d, s) = (RecCursor::over_owned(&mut d0, ANCHOR, STRIDE), RecCursor::over_owned(black_box(&mut s0), ANCHOR, STRIDE)); enc_aux::WelsCopy16x16_c(&d, &s); sum(&d0, c) },
        |c| { let (d, s) = (RecCursor::over_owned(&mut d1, ANCHOR, STRIDE), RecCursor::over_owned(black_box(&mut s1), ANCHOR, STRIDE)); isa::copy::copy_16x16_sse2(&d, &s); sum(&d1, c) },
        |c| { let (d, s) = (RecCursor::over_owned(&mut d2, ANCHOR, STRIDE), RecCursor::over_owned(black_box(&mut s2), ANCHOR, STRIDE)); wd::copy::copy_16x16_sse2(&d, &s); sum(&d2, c) });
    row!(*rows, "copy 8x8",
        |c| { let (d, s) = (RecCursor::over_owned(&mut d0, ANCHOR, STRIDE), RecCursor::over_owned(black_box(&mut s0), ANCHOR, STRIDE)); enc_aux::WelsCopy8x8_c(&d, &s); sum(&d0, c) },
        |c| { let (d, s) = (RecCursor::over_owned(&mut d1, ANCHOR, STRIDE), RecCursor::over_owned(black_box(&mut s1), ANCHOR, STRIDE)); isa::copy::copy_8x8_sse2(&d, &s); sum(&d1, c) },
        |c| { let (d, s) = (RecCursor::over_owned(&mut d2, ANCHOR, STRIDE), RecCursor::over_owned(black_box(&mut s2), ANCHOR, STRIDE)); wd::copy::copy_8x8_sse2(&d, &s); sum(&d2, c) });
}

fn intra_rows(rows: &mut Vec<Row>) {
    let mut plane = noise(STRIDE * ROWS, 31);
    let rec = RecCursor::over_owned(&mut plane, ANCHOR, STRIDE);
    let s256 = |p: &[u8; 256], c: bool| if c { fnv(p) } else { p[0] as u64 };
    let s64 = |p: &[u8; 64], c: bool| if c { fnv(p) } else { p[0] as u64 };
    let s16 = |p: &[u8; 16], c: bool| if c { fnv(p) } else { p[0] as u64 };
    row!(*rows, "intra 16x16 plane",
        |c| { let mut p = [0u8; 256]; ipred::WelsI16x16LumaPredPlane_c(&mut p, black_box(&rec)); s256(&p, c) },
        |c| { let mut p = [0u8; 256]; isa::intra_pred::enc_i16x16_luma_pred_plane_sse2(&mut p, black_box(&rec)); s256(&p, c) },
        |c| { let mut p = [0u8; 256]; wd::intra_pred::enc_i16x16_luma_pred_plane_sse2(&mut p, black_box(&rec)); s256(&p, c) });
    row!(*rows, "intra 16x16 dc",
        |c| { let mut p = [0u8; 256]; ipred::WelsI16x16LumaPredDc_c(&mut p, black_box(&rec)); s256(&p, c) },
        |c| { let mut p = [0u8; 256]; isa::intra_pred::enc_i16x16_luma_pred_dc_sse2(&mut p, black_box(&rec)); s256(&p, c) },
        |c| { let mut p = [0u8; 256]; wd::intra_pred::enc_i16x16_luma_pred_dc_sse2(&mut p, black_box(&rec)); s256(&p, c) });
    row!(*rows, "intra chroma plane",
        |c| { let mut p = [0u8; 64]; ipred::WelsIChromaPredPlane_c(&mut p, black_box(&rec)); s64(&p, c) },
        |c| { let mut p = [0u8; 64]; isa::intra_pred::enc_chroma_pred_plane_sse2(&mut p, black_box(&rec)); s64(&p, c) },
        |c| { let mut p = [0u8; 64]; wd::intra_pred::enc_chroma_pred_plane_sse2(&mut p, black_box(&rec)); s64(&p, c) });
    row!(*rows, "intra 4x4 dc",
        |c| { let mut p = [0u8; 16]; ipred::WelsI4x4LumaPredDc_c(&mut p, black_box(&rec)); s16(&p, c) },
        |c| { let mut p = [0u8; 16]; isa::intra_pred::enc_i4x4_luma_pred_dc_sse2(&mut p, black_box(&rec)); s16(&p, c) },
        |c| { let mut p = [0u8; 16]; wd::intra_pred::enc_i4x4_luma_pred_dc_sse2(&mut p, black_box(&rec)); s16(&p, c) });
    row!(*rows, "intra 4x4 ddl",
        |c| { let mut p = [0u8; 16]; ipred::WelsI4x4LumaPredDDL_c(&mut p, black_box(&rec)); s16(&p, c) },
        |c| { let mut p = [0u8; 16]; isa::intra_pred::enc_i4x4_luma_pred_ddl_sse2(&mut p, black_box(&rec)); s16(&p, c) },
        |c| { let mut p = [0u8; 16]; wd::intra_pred::enc_i4x4_luma_pred_ddl_sse2(&mut p, black_box(&rec)); s16(&p, c) });
}

fn deblock_rows(rows: &mut Vec<Row>) {
    fn plane(seed: u64) -> PaddedPlane {
        let mut p = PaddedPlane::new(32, 32, 16, STRIDE);
        let s = smooth(STRIDE, 64, 6, seed);
        p.as_mut_slice().copy_from_slice(&s);
        p
    }
    let sum = |p: &PaddedPlane, c: bool| if c { fnv(p.as_slice()) } else { p.at(0, 0) as u64 };
    let (alpha, beta, tc) = (40, 20, [3i8, 2, 3, 1]);
    let st = STRIDE as isize;

    for (label, sx, sy) in [("horizontal edge", st, 1isize), ("vertical edge", 1isize, st)] {
        let (mut p0, mut p1, mut p2) = (plane(41), plane(41), plane(41));
        let name: &'static str = match (label, "luma lt4") {
            ("horizontal edge", _) => "deblock luma lt4, horizontal edge",
            _ => "deblock luma lt4, vertical edge",
        };
        row!(*rows, name,
            |c| { dbk::deblock_luma_lt4_scalar(&mut p0.cursor_mut(8, 8), sx, sy, black_box(alpha), black_box(beta), black_box(&tc)); sum(&p0, c) },
            |c| { isa::deblock::deblock_luma_lt4_sse2(&mut p1.cursor_mut(8, 8), sx, sy, black_box(alpha), black_box(beta), black_box(&tc)); sum(&p1, c) },
            |c| { wd::deblock::deblock_luma_lt4_sse2(&mut p2.cursor_mut(8, 8), sx, sy, black_box(alpha), black_box(beta), black_box(&tc)); sum(&p2, c) });

        let (mut p0, mut p1, mut p2) = (plane(43), plane(43), plane(43));
        let name: &'static str = if label == "horizontal edge" {
            "deblock luma eq4, horizontal edge"
        } else {
            "deblock luma eq4, vertical edge"
        };
        row!(*rows, name,
            |c| { dbk::deblock_luma_eq4_scalar(&mut p0.cursor_mut(8, 8), sx, sy, black_box(alpha), black_box(beta)); sum(&p0, c) },
            |c| { isa::deblock::deblock_luma_eq4_sse2(&mut p1.cursor_mut(8, 8), sx, sy, black_box(alpha), black_box(beta)); sum(&p1, c) },
            |c| { wd::deblock::deblock_luma_eq4_sse2(&mut p2.cursor_mut(8, 8), sx, sy, black_box(alpha), black_box(beta)); sum(&p2, c) });

        let (mut b0, mut r0, mut b1, mut r1, mut b2, mut r2) =
            (plane(45), plane(46), plane(45), plane(46), plane(45), plane(46));
        let name: &'static str = if label == "horizontal edge" {
            "deblock chroma lt4, horizontal edge"
        } else {
            "deblock chroma lt4, vertical edge"
        };
        row!(*rows, name,
            |c| { dbk::deblock_chroma_lt4_scalar(&mut b0.cursor_mut(4, 4), &mut r0.cursor_mut(4, 4), sx, sy, black_box(alpha), black_box(beta), black_box(&tc)); sum(&b0, c) ^ sum(&r0, c) },
            |c| { isa::deblock::deblock_chroma_lt4_sse2(&mut b1.cursor_mut(4, 4), &mut r1.cursor_mut(4, 4), sx, sy, black_box(alpha), black_box(beta), black_box(&tc)); sum(&b1, c) ^ sum(&r1, c) },
            |c| { wd::deblock::deblock_chroma_lt4_sse2(&mut b2.cursor_mut(4, 4), &mut r2.cursor_mut(4, 4), sx, sy, black_box(alpha), black_box(beta), black_box(&tc)); sum(&b2, c) ^ sum(&r2, c) });
    }
}

// ============================================================================
// Report
// ============================================================================

fn main() {
    let mut rows = Vec::new();
    eprint!(" timing");
    sad_rows(&mut rows);
    satd_rows(&mut rows);
    mc_rows(&mut rows);
    dct_rows(&mut rows);
    quant_rows(&mut rows);
    copy_rows(&mut rows);
    intra_rows(&mut rows);
    deblock_rows(&mut rows);
    eprintln!();

    let has_wide = rows.iter().any(|r| r.ns[2].is_some());
    println!("=========================================================================================================");
    println!(" Per-kernel cost, ns per call, best of {} blocks of ~{} ms", repeats(), block_ms());
    if !has_wide {
        println!(" (built without --features wide: no wide column)");
    }
    println!("=========================================================================================================");
    println!(
        " {:<38} {:>9} {:>9} {:>9}   {:>9} {:>9} {:>9}  {}",
        "kernel", "scalar", "intrin", "wide", "intrin/sc", "wide/sc", "wide/intr", "agree"
    );
    println!("---------------------------------------------------------------------------------------------------------");
    let fmt = |v: Option<f64>| v.map_or("-".to_string(), |x| format!("{x:.1}"));
    let ratio = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) if b > 0.0 => format!("{:.2}x", a / b),
        _ => "-".to_string(),
    };
    let (mut ln_isa, mut ln_wide, mut ln_wi, mut n, mut mismatches) = (0.0f64, 0.0f64, 0.0f64, 0usize, 0usize);
    for r in &rows {
        let [s, i, w] = r.ns;
        println!(
            " {:<38} {:>9} {:>9} {:>9}   {:>9} {:>9} {:>9}  {}",
            r.name,
            fmt(s),
            fmt(i),
            fmt(w),
            ratio(s, i),
            ratio(s, w),
            ratio(i, w),
            if r.consistent { "yes" } else { "MISMATCH" }
        );
        if let (Some(s), Some(i)) = (s, i) {
            ln_isa += (s / i).ln();
            if let Some(w) = w {
                ln_wide += (s / w).ln();
                ln_wi += (i / w).ln();
            }
            n += 1;
        }
        if !r.consistent {
            mismatches += 1;
        }
    }
    println!("---------------------------------------------------------------------------------------------------------");
    if n > 0 {
        let g = |x: f64| (x / n as f64).exp();
        println!(
            " {:<38} {:>9} {:>9} {:>9}   {:>8.2}x {:>8} {:>8}",
            "geometric mean of the ratios",
            "",
            "",
            "",
            g(ln_isa),
            if has_wide { format!("{:.2}x", g(ln_wide)) } else { "-".into() },
            if has_wide { format!("{:.2}x", g(ln_wi)) } else { "-".into() }
        );
    }
    if mismatches > 0 {
        eprintln!("\n {mismatches} row(s) disagree between implementations; the timings above are of different work.");
        std::process::exit(1);
    }
}
