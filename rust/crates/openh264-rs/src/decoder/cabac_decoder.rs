//! Rust translation of OpenH264 CABAC Decoder Engine (`cabac_decoder.h` and `cabac_decoder.cpp`).
//!
//! # The read-extent audit (T3.2 step 0)
//!
//! [`phase3_findings.md`](../../../docs/phase3_findings.md) **§F16** happened because a
//! readable extent was derived from *one half* of the reader and then claimed for all
//! of it — "covers every read the family can make, at any position, for any operation"
//! was a quantifier over a set nobody had enumerated. This module's extent claim is
//! therefore **per site**, and the enumeration is the deliverable.
//!
//! ## Every buffer access the engine can issue
//!
//! Two functions load bytes; one does position arithmetic and loads nothing; nothing
//! else in this file touches the buffer. `DecodeBinCabac`, `DecodeBypassCabac`,
//! `DecodeTerminateCabac`, `DecodeUnaryBinCabac`, `DecodeExpBypassCabac`,
//! `DecodeUEGLevelCabac` and `DecodeUEGMvCabac` reach it **only** through
//! [`Read32BitsCabac`]; the per-bin path issues no load of its own.
//!
//! ### 1. [`InitCabacDecEngineFromBS`] — the 5-byte prime. Max index `len + 2`.
//!
//! `curr = pos - remaining_bytes`, guarded by the C++ `pCurr < pEndBuf - 1`, i.e.
//! `curr <= len - 2`; it then loads `curr[0..=4]`, so the largest index it can touch
//! is `len + 2`. `remaining_bytes = ((-left_bits) >> 3) + 2 ∈ [0, 4]`, because
//! `left_bits ∈ [-16, 15]` on every path that reaches here (`init` and
//! `init_read_bits` set −16, `dump_bits` refills from `>= 0` down by 16, `end_cavlc`
//! sets `-16 + (idx & 7)`). **Needs `avail >= len + 3`**, which is why this is the one
//! site that takes the wider [`BsReader::buf`] window rather than the RBSP one, and it
//! reads through `get` so a violated contract is an error return, not a panic and not
//! a read past the allocation.
//!
//! ### 2. [`Read32BitsCabac`] — the 4/3/2/1 end ladder. Max index `len - 1`.
//!
//! This is F16's named suspect and the audit's answer is that **it never reads past the
//! RBSP**, because its own selector is measured against `pBuffEnd`:
//!
//! | `iLeftBytes` | loads | largest index |
//! |---|---|---|
//! | `<= 0` | none — error return | — |
//! | `1` / `2` / `3` | `curr[0..n)` | `curr + n - 1 = len - 1` |
//! | `>= 4` | `curr[0..4)` | `curr + 3 <= len - 1` |
//!
//! So the ladder is bounded by `len`, needs `avail >= len` and nothing more, and can be
//! handed a slice of exactly `len` bytes — [`BsReader::rbsp_window`]. That makes
//! `buf.len()` *be* `pBuffEnd - pBuffStart`, so the engine computes no extent of its
//! own and there is no second `readable_from`-shaped site to keep coherent.
//!
//! `iLeftBytes` is genuinely negative in practice: init leaves the position at
//! `curr + 5 <= len + 3`, so a stream truncated into its first CABAC bytes enters the
//! ladder at `-3`, takes the `<= 0` arm and returns `ERR_CABAC_NO_BS_TO_READ` having
//! loaded nothing. The port expresses the predicate as the **comparison** `pos >= len`
//! rather than as a subtraction for exactly that reason — `len - pos` in `usize` would
//! wrap to a huge positive and select the 4-byte arm, which would be a new
//! out-of-bounds read where the raw code errored.
//!
//! ### 3. [`RestoreCabacDecEngineToBS`] — **no load**.
//!
//! Position only, stated explicitly because the claim is per-site and "reads nothing"
//! is one of the answers. See its own doc comment for why the rewind cannot underflow.
//!
//! ## Where the two numbers come from
//!
//! `len` = `cursor.len()` = `pBuffEnd - pBuffStart`, the logical RBSP end, one owner.
//! The readable extent past it is **derived from the owning [`RawDataBuffer`] at call
//! time** (`window_from`), one owner. `window.len() >= len + 4` structurally
//! (`WelsDecodeBs` sizes every payload with four bytes to spare), which covers site
//! 1's `len + 2`. The staleness hazard this paragraph used to carry — a stored
//! `avail` going stale when `ExpandBsBuffer` grew the buffer, F16's second instance —
//! closed at T3.3: there is no stored extent left to go stale.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

pub const WELS_CABAC_HALF: u64 = 0x01FE;
pub const WELS_CABAC_QUARTER: u64 = 0x0100;
pub const WELS_CONTEXT_COUNT: usize = 460;
pub const WELS_QP_MAX: i32 = 51;

pub const ERR_NONE: i32 = 0;
pub const ERR_LEVEL_MB_DATA: i32 = 7;
pub const ERR_INFO_INVALID_ACCESS: i32 = 2;
pub const ERR_CABAC_NO_BS_TO_READ: i32 = 201;
pub const ERR_CABAC_UNEXPECTED_VALUE: i32 = 202;

pub const I_SLICE: u8 = 2;

#[inline(always)]
pub const fn GENERATE_ERROR_NO(iErrLevel: i32, iErrInfo: i32) -> i32 {
    (iErrLevel << 16) | (iErrInfo & 0xFFFF)
}

#[inline(always)]
pub const fn WELS_CLIP3(val: i32, min_val: i32, max_val: i32) -> i32 {
    if val < min_val {
        min_val
    } else if val > max_val {
        max_val
    } else {
        val
    }
}

pub const g_kRenormTable256: [u8; 256] = [
    6, 6, 6, 6, 6, 6, 6, 6, 5, 5, 5, 5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];

pub const g_kMvdBinPos2Ctx: [i16; 8] = [0, 1, 2, 3, 3, 3, 3, 3];

pub const g_kuiCabacRangeLps: [[u8; 4]; 64] = [
    [128, 176, 208, 240], [128, 167, 197, 227], [128, 158, 187, 216], [123, 150, 178, 205],
    [116, 142, 169, 195], [111, 135, 160, 185], [105, 128, 152, 175], [100, 122, 144, 166],
    [95, 116, 137, 158],  [90, 110, 130, 150],  [85, 104, 123, 142],  [81, 99, 117, 135],
    [77, 94, 111, 128],   [73, 89, 105, 122],   [69, 85, 100, 116],   [66, 80, 95, 110],
    [62, 76, 90, 104],    [59, 72, 86, 99],     [56, 69, 81, 94],     [53, 65, 77, 89],
    [51, 62, 73, 85],     [48, 59, 69, 80],     [46, 56, 66, 76],     [43, 53, 63, 72],
    [41, 50, 59, 69],     [39, 48, 56, 65],     [37, 45, 54, 62],     [35, 43, 51, 59],
    [33, 41, 48, 56],     [32, 39, 46, 53],     [30, 37, 43, 50],     [29, 35, 41, 48],
    [27, 33, 39, 45],     [26, 31, 37, 43],     [24, 30, 35, 41],     [23, 28, 33, 39],
    [22, 27, 32, 37],     [21, 26, 30, 35],     [20, 24, 29, 33],     [19, 23, 27, 31],
    [18, 22, 26, 30],     [17, 21, 25, 28],     [16, 20, 23, 27],     [15, 19, 22, 25],
    [14, 18, 21, 24],     [14, 17, 20, 23],     [13, 16, 19, 22],     [12, 15, 18, 21],
    [12, 14, 17, 20],     [11, 14, 16, 19],     [11, 13, 15, 18],     [10, 12, 15, 17],
    [10, 12, 14, 16],     [9, 11, 13, 15],      [9, 11, 12, 14],      [8, 10, 12, 14],
    [8, 9, 11, 13],       [7, 9, 11, 12],       [7, 9, 10, 12],       [7, 8, 10, 11],
    [6, 8, 9, 11],        [6, 7, 9, 10],        [6, 7, 8, 9],         [2, 2, 2, 2],
];

pub const g_kuiStateTransTable: [[u8; 2]; 64] = [
    [0, 1],   [0, 2],   [1, 3],   [2, 4],   [2, 5],   [4, 6],   [4, 7],   [5, 8],
    [6, 9],   [7, 10],  [8, 11],  [9, 12],  [9, 13],  [11, 14], [11, 15], [12, 16],
    [13, 17], [13, 18], [15, 19], [15, 20], [16, 21], [16, 22], [18, 23], [18, 24],
    [19, 25], [19, 26], [21, 27], [21, 28], [22, 29], [22, 30], [23, 31], [24, 32],
    [24, 33], [25, 34], [26, 35], [26, 36], [27, 37], [27, 38], [28, 39], [29, 40],
    [29, 41], [30, 42], [30, 43], [30, 44], [31, 45], [32, 46], [32, 47], [33, 48],
    [33, 49], [33, 50], [34, 51], [34, 52], [35, 53], [35, 54], [35, 55], [36, 56],
    [36, 57], [36, 58], [37, 59], [37, 60], [37, 61], [38, 62], [38, 62], [63, 63],
];

pub const CTX_NA: i8 = 0;

pub const g_kiCabacGlobalContextIdx: [[[i8; 2]; 4]; WELS_CONTEXT_COUNT] = [
    // 0-10 Table 9-12
    [[20, -15], [20, -15], [20, -15], [20, -15]],
    [[2, 54], [2, 54], [2, 54], [2, 54]],
    [[3, 74], [3, 74], [3, 74], [3, 74]],
    [[20, -15], [20, -15], [20, -15], [20, -15]],
    [[2, 54], [2, 54], [2, 54], [2, 54]],
    [[3, 74], [3, 74], [3, 74], [3, 74]],
    [[-28, 127], [-28, 127], [-28, 127], [-28, 127]],
    [[-23, 104], [-23, 104], [-23, 104], [-23, 104]],
    [[-6, 53], [-6, 53], [-6, 53], [-6, 53]],
    [[-1, 54], [-1, 54], [-1, 54], [-1, 54]],
    [[7, 51], [7, 51], [7, 51], [7, 51]],
    // 11-23 Table 9-13
    [[CTX_NA, CTX_NA], [23, 33], [22, 25], [29, 16]],
    [[CTX_NA, CTX_NA], [23, 2], [34, 0], [25, 0]],
    [[CTX_NA, CTX_NA], [21, 0], [16, 0], [14, 0]],
    [[CTX_NA, CTX_NA], [1, 9], [-2, 9], [-10, 51]],
    [[CTX_NA, CTX_NA], [0, 49], [4, 41], [-3, 62]],
    [[CTX_NA, CTX_NA], [-37, 118], [-29, 118], [-27, 99]],
    [[CTX_NA, CTX_NA], [5, 57], [2, 65], [26, 16]],
    [[CTX_NA, CTX_NA], [-13, 78], [-6, 71], [-4, 85]],
    [[CTX_NA, CTX_NA], [-11, 65], [-13, 79], [-24, 102]],
    [[CTX_NA, CTX_NA], [1, 62], [5, 52], [5, 57]],
    [[CTX_NA, CTX_NA], [12, 49], [9, 50], [6, 57]],
    [[CTX_NA, CTX_NA], [-4, 73], [-3, 70], [-17, 73]],
    [[CTX_NA, CTX_NA], [17, 50], [10, 54], [14, 57]],
    // 24-39 Table 9-14
    [[CTX_NA, CTX_NA], [18, 64], [26, 34], [20, 40]],
    [[CTX_NA, CTX_NA], [9, 43], [19, 22], [20, 10]],
    [[CTX_NA, CTX_NA], [29, 0], [40, 0], [29, 0]],
    [[CTX_NA, CTX_NA], [26, 67], [57, 2], [54, 0]],
    [[CTX_NA, CTX_NA], [16, 90], [41, 36], [37, 42]],
    [[CTX_NA, CTX_NA], [9, 104], [26, 69], [12, 97]],
    [[CTX_NA, CTX_NA], [-46, 127], [-45, 127], [-32, 127]],
    [[CTX_NA, CTX_NA], [-20, 104], [-15, 101], [-22, 117]],
    [[CTX_NA, CTX_NA], [1, 67], [-4, 76], [-2, 74]],
    [[CTX_NA, CTX_NA], [-13, 78], [-6, 71], [-4, 85]],
    [[CTX_NA, CTX_NA], [-11, 65], [-13, 79], [-24, 102]],
    [[CTX_NA, CTX_NA], [1, 62], [5, 52], [5, 57]],
    [[CTX_NA, CTX_NA], [-6, 86], [6, 69], [-6, 93]],
    [[CTX_NA, CTX_NA], [-17, 95], [-13, 90], [-14, 88]],
    [[CTX_NA, CTX_NA], [-6, 61], [0, 52], [-6, 44]],
    [[CTX_NA, CTX_NA], [9, 45], [8, 43], [4, 55]],
    // 40-53 Table 9-15
    [[CTX_NA, CTX_NA], [-3, 69], [-2, 69], [-11, 89]],
    [[CTX_NA, CTX_NA], [-6, 81], [-5, 82], [-15, 103]],
    [[CTX_NA, CTX_NA], [-11, 96], [-10, 96], [-21, 116]],
    [[CTX_NA, CTX_NA], [6, 55], [2, 59], [19, 57]],
    [[CTX_NA, CTX_NA], [7, 67], [2, 75], [20, 58]],
    [[CTX_NA, CTX_NA], [-5, 86], [-3, 87], [4, 84]],
    [[CTX_NA, CTX_NA], [2, 88], [-3, 100], [6, 96]],
    [[CTX_NA, CTX_NA], [0, 58], [1, 56], [1, 63]],
    [[CTX_NA, CTX_NA], [-3, 76], [-3, 74], [-5, 85]],
    [[CTX_NA, CTX_NA], [-10, 94], [-6, 85], [-13, 106]],
    [[CTX_NA, CTX_NA], [5, 54], [0, 59], [5, 63]],
    [[CTX_NA, CTX_NA], [4, 69], [-3, 81], [6, 75]],
    [[CTX_NA, CTX_NA], [-3, 81], [-7, 86], [-3, 90]],
    [[CTX_NA, CTX_NA], [0, 88], [-5, 95], [-1, 101]],
    // 54-59 Table 9-16
    [[CTX_NA, CTX_NA], [-7, 67], [-1, 66], [3, 55]],
    [[CTX_NA, CTX_NA], [-5, 74], [-1, 77], [-4, 79]],
    [[CTX_NA, CTX_NA], [-4, 74], [1, 70], [-2, 75]],
    [[CTX_NA, CTX_NA], [-5, 80], [-2, 86], [-12, 97]],
    [[CTX_NA, CTX_NA], [-7, 72], [-5, 72], [-7, 50]],
    [[CTX_NA, CTX_NA], [1, 58], [0, 61], [1, 60]],
    // 60-69 Table 9-17
    [[0, 41], [0, 41], [0, 41], [0, 41]],
    [[0, 63], [0, 63], [0, 63], [0, 63]],
    [[0, 63], [0, 63], [0, 63], [0, 63]],
    [[0, 63], [0, 63], [0, 63], [0, 63]],
    [[-9, 83], [-9, 83], [-9, 83], [-9, 83]],
    [[4, 86], [4, 86], [4, 86], [4, 86]],
    [[0, 97], [0, 97], [0, 97], [0, 97]],
    [[-7, 72], [-7, 72], [-7, 72], [-7, 72]],
    [[13, 41], [13, 41], [13, 41], [13, 41]],
    [[3, 62], [3, 62], [3, 62], [3, 62]],
    // 70-104 Table 9-18
    [[0, 11], [0, 45], [13, 15], [7, 34]],
    [[1, 55], [-4, 78], [7, 51], [-9, 88]],
    [[0, 69], [-3, 96], [2, 80], [-20, 127]],
    [[-17, 127], [-27, 126], [-39, 127], [-36, 127]],
    [[-13, 102], [-28, 98], [-18, 91], [-17, 91]],
    [[0, 82], [-25, 101], [-17, 96], [-14, 95]],
    [[-7, 74], [-23, 67], [-26, 81], [-25, 84]],
    [[-21, 107], [-28, 82], [-35, 98], [-25, 86]],
    [[-27, 127], [-20, 94], [-24, 102], [-12, 89]],
    [[-31, 127], [-16, 83], [-23, 97], [-17, 91]],
    [[-24, 127], [-22, 110], [-27, 119], [-31, 127]],
    [[-18, 95], [-21, 91], [-24, 99], [-14, 76]],
    [[-27, 127], [-18, 102], [-21, 110], [-18, 103]],
    [[-21, 114], [-13, 93], [-18, 102], [-13, 90]],
    [[-30, 127], [-29, 127], [-36, 127], [-37, 127]],
    [[-17, 123], [-7, 92], [0, 80], [11, 80]],
    [[-12, 115], [-5, 89], [-5, 89], [5, 76]],
    [[-16, 122], [-7, 96], [-7, 94], [2, 84]],
    [[-11, 115], [-13, 108], [-4, 92], [5, 78]],
    [[-12, 63], [-3, 46], [0, 39], [-6, 55]],
    [[-2, 68], [-1, 65], [0, 65], [4, 61]],
    [[-15, 84], [-1, 57], [-15, 84], [-14, 83]],
    [[-13, 104], [-9, 93], [-35, 127], [-37, 127]],
    [[-3, 70], [-3, 74], [-2, 73], [-5, 79]],
    [[-8, 93], [-9, 92], [-12, 104], [-11, 104]],
    [[-10, 90], [-8, 87], [-9, 91], [-11, 91]],
    [[-30, 127], [-23, 126], [-31, 127], [-30, 127]],
    [[-1, 74], [5, 54], [3, 55], [0, 65]],
    [[-6, 97], [6, 60], [7, 56], [-2, 79]],
    [[-7, 91], [6, 59], [7, 55], [0, 72]],
    [[-20, 127], [6, 69], [8, 61], [-4, 92]],
    [[-4, 56], [-1, 48], [-3, 53], [-6, 56]],
    [[-5, 82], [0, 68], [0, 68], [3, 68]],
    [[-7, 76], [-4, 69], [-7, 74], [-8, 71]],
    [[-22, 125], [-8, 88], [-9, 88], [-13, 98]],
    // 105-165 Table 9-19
    [[-7, 93], [-2, 85], [-13, 103], [-4, 86]],
    [[-11, 87], [-6, 78], [-13, 91], [-12, 88]],
    [[-3, 77], [-1, 75], [-9, 89], [-5, 82]],
    [[-5, 71], [-7, 77], [-14, 92], [-3, 72]],
    [[-4, 63], [2, 54], [-8, 76], [-4, 67]],
    [[-4, 68], [5, 50], [-12, 87], [-8, 72]],
    [[-12, 84], [-3, 68], [-23, 110], [-16, 89]],
    [[-7, 62], [1, 50], [-24, 105], [-9, 69]],
    [[-7, 65], [6, 42], [-10, 78], [-1, 59]],
    [[8, 61], [-4, 81], [-20, 112], [5, 66]],
    [[5, 56], [1, 63], [-17, 99], [4, 57]],
    [[-2, 66], [-4, 70], [-78, 127], [-4, 71]],
    [[1, 64], [0, 67], [-70, 127], [-2, 71]],
    [[0, 61], [2, 57], [-50, 127], [2, 58]],
    [[-2, 78], [-2, 76], [-46, 127], [-1, 74]],
    [[1, 50], [11, 35], [-4, 66], [-4, 44]],
    [[7, 52], [4, 64], [-5, 78], [-1, 69]],
    [[10, 35], [1, 61], [-4, 71], [0, 62]],
    [[0, 44], [11, 35], [-8, 72], [-7, 51]],
    [[11, 38], [18, 25], [2, 59], [-4, 47]],
    [[1, 45], [12, 24], [-1, 55], [-6, 42]],
    [[0, 46], [13, 29], [-7, 70], [-3, 41]],
    [[5, 44], [13, 36], [-6, 75], [-6, 53]],
    [[31, 17], [-10, 93], [-8, 89], [8, 76]],
    [[1, 51], [-7, 73], [-34, 119], [-9, 78]],
    [[7, 50], [-2, 73], [-3, 75], [-11, 83]],
    [[28, 19], [13, 46], [32, 20], [9, 52]],
    [[16, 33], [9, 49], [30, 22], [0, 67]],
    [[14, 62], [-7, 100], [-44, 127], [-5, 90]],
    [[-13, 108], [9, 53], [0, 54], [1, 67]],
    [[-15, 100], [2, 53], [-5, 61], [-15, 72]],
    [[-13, 101], [5, 53], [0, 58], [-5, 75]],
    [[-13, 91], [-2, 61], [-1, 60], [-8, 80]],
    [[-12, 94], [0, 56], [-3, 61], [-21, 83]],
    [[-10, 88], [0, 56], [-8, 67], [-21, 64]],
    [[-16, 84], [-13, 63], [-25, 84], [-13, 31]],
    [[-10, 86], [-5, 60], [-14, 74], [-25, 64]],
    [[-7, 83], [-1, 62], [-5, 65], [-29, 94]],
    [[-13, 87], [4, 57], [5, 52], [9, 75]],
    [[-19, 94], [-6, 69], [2, 57], [17, 63]],
    [[1, 70], [4, 57], [0, 61], [-8, 74]],
    [[0, 72], [14, 39], [-9, 69], [-5, 35]],
    [[-5, 74], [4, 51], [-11, 70], [-2, 27]],
    [[18, 59], [13, 68], [18, 55], [13, 91]],
    [[-8, 102], [3, 64], [-4, 71], [3, 65]],
    [[-15, 100], [1, 61], [0, 58], [-7, 69]],
    [[0, 95], [9, 63], [7, 61], [8, 77]],
    [[-4, 75], [7, 50], [9, 41], [-10, 66]],
    [[2, 72], [16, 39], [18, 25], [3, 62]],
    [[-11, 75], [5, 44], [9, 32], [-3, 68]],
    [[-3, 71], [4, 52], [5, 43], [-20, 81]],
    [[15, 46], [11, 48], [9, 47], [0, 30]],
    [[-13, 69], [-5, 60], [0, 44], [1, 7]],
    [[0, 62], [-1, 59], [0, 51], [-3, 23]],
    [[0, 65], [0, 59], [2, 46], [-21, 74]],
    [[21, 37], [22, 33], [19, 38], [16, 66]],
    [[-15, 72], [5, 44], [-4, 66], [-23, 124]],
    [[9, 57], [14, 43], [15, 38], [17, 37]],
    [[16, 54], [-1, 78], [12, 42], [44, -18]],
    [[0, 62], [0, 60], [9, 34], [50, -34]],
    [[12, 72], [9, 69], [0, 89], [-22, 127]],
    // 166-226 Table 9-20
    [[24, 0], [11, 28], [4, 45], [4, 39]],
    [[15, 9], [2, 40], [10, 28], [0, 42]],
    [[8, 25], [3, 44], [10, 31], [7, 34]],
    [[13, 18], [0, 49], [33, -11], [11, 29]],
    [[15, 9], [0, 46], [52, -43], [8, 31]],
    [[13, 19], [2, 44], [18, 15], [6, 37]],
    [[10, 37], [2, 51], [28, 0], [7, 42]],
    [[12, 18], [0, 47], [35, -22], [3, 40]],
    [[6, 29], [4, 39], [38, -25], [8, 33]],
    [[20, 33], [2, 62], [34, 0], [13, 43]],
    [[15, 30], [6, 46], [39, -18], [13, 36]],
    [[4, 45], [0, 54], [32, -12], [4, 47]],
    [[1, 58], [3, 54], [102, -94], [3, 55]],
    [[0, 62], [2, 58], [0, 0], [2, 58]],
    [[7, 61], [4, 63], [56, -15], [6, 60]],
    [[12, 38], [6, 51], [33, -4], [8, 44]],
    [[11, 45], [6, 57], [29, 10], [11, 44]],
    [[15, 39], [7, 53], [37, -5], [14, 42]],
    [[11, 42], [6, 52], [51, -29], [7, 48]],
    [[13, 44], [6, 55], [39, -9], [4, 56]],
    [[16, 45], [11, 45], [52, -34], [4, 52]],
    [[12, 41], [14, 36], [69, -58], [13, 37]],
    [[10, 49], [8, 53], [67, -63], [9, 49]],
    [[30, 34], [-1, 82], [44, -5], [19, 58]],
    [[18, 42], [7, 55], [32, 7], [10, 48]],
    [[10, 55], [-3, 78], [55, -29], [12, 45]],
    [[17, 51], [15, 46], [32, 1], [0, 69]],
    [[17, 46], [22, 31], [0, 0], [20, 33]],
    [[0, 89], [-1, 84], [27, 36], [8, 63]],
    [[26, -19], [25, 7], [33, -25], [35, -18]],
    [[22, -17], [30, -7], [34, -30], [33, -25]],
    [[26, -17], [28, 3], [36, -28], [28, -3]],
    [[30, -25], [28, 4], [38, -28], [24, 10]],
    [[28, -20], [32, 0], [38, -27], [27, 0]],
    [[33, -23], [34, -1], [34, -18], [34, -14]],
    [[37, -27], [30, 6], [35, -16], [52, -44]],
    [[33, -23], [30, 6], [34, -14], [39, -24]],
    [[40, -28], [32, 9], [32, -8], [19, 17]],
    [[38, -17], [31, 19], [37, -6], [31, 25]],
    [[33, -11], [26, 27], [35, 0], [36, 29]],
    [[40, -15], [26, 30], [30, 10], [24, 33]],
    [[41, -6], [37, 20], [28, 18], [34, 15]],
    [[38, 1], [28, 34], [26, 25], [30, 20]],
    [[41, 17], [17, 70], [29, 41], [22, 73]],
    [[30, -6], [1, 67], [0, 75], [20, 34]],
    [[27, 3], [5, 59], [2, 72], [19, 31]],
    [[26, 22], [9, 67], [8, 77], [27, 44]],
    [[37, -16], [16, 30], [14, 35], [19, 16]],
    [[35, -4], [18, 32], [18, 31], [15, 36]],
    [[38, -8], [18, 35], [17, 35], [15, 36]],
    [[38, -3], [22, 29], [21, 30], [21, 28]],
    [[37, 3], [24, 31], [17, 45], [25, 21]],
    [[38, 5], [23, 38], [20, 42], [30, 20]],
    [[42, 0], [18, 43], [18, 45], [31, 12]],
    [[35, 16], [20, 41], [27, 26], [27, 16]],
    [[39, 22], [11, 63], [16, 54], [24, 42]],
    [[14, 48], [9, 59], [7, 66], [0, 93]],
    [[27, 37], [9, 64], [16, 56], [14, 56]],
    [[21, 60], [-1, 94], [11, 73], [15, 57]],
    [[12, 68], [-2, 89], [10, 67], [26, 38]],
    [[2, 97], [-9, 108], [-10, 116], [-24, 127]],
    // 227-275 Table 9-21
    [[-3, 71], [-6, 76], [-23, 112], [-24, 115]],
    [[-6, 42], [-2, 44], [-15, 71], [-22, 82]],
    [[-5, 50], [0, 45], [-7, 61], [-9, 62]],
    [[-3, 54], [0, 52], [0, 53], [0, 53]],
    [[-2, 62], [-3, 64], [-5, 66], [0, 59]],
    [[0, 58], [-2, 59], [-11, 77], [-14, 85]],
    [[1, 63], [-4, 70], [-9, 80], [-13, 89]],
    [[-2, 72], [-4, 75], [-9, 84], [-13, 94]],
    [[-1, 74], [-8, 82], [-10, 87], [-11, 92]],
    [[-9, 91], [-17, 102], [-34, 127], [-29, 127]],
    [[-5, 67], [-9, 77], [-21, 101], [-21, 100]],
    [[-5, 27], [3, 24], [-3, 39], [-14, 57]],
    [[-3, 39], [0, 42], [-5, 53], [-12, 67]],
    [[-2, 44], [0, 48], [-7, 61], [-11, 71]],
    [[0, 46], [0, 55], [-11, 75], [-10, 77]],
    [[-16, 64], [-6, 59], [-15, 77], [-21, 85]],
    [[-8, 68], [-7, 71], [-17, 91], [-16, 88]],
    [[-10, 78], [-12, 83], [-25, 107], [-23, 104]],
    [[-6, 77], [-11, 87], [-25, 111], [-15, 98]],
    [[-10, 86], [-30, 119], [-28, 122], [-37, 127]],
    [[-12, 92], [1, 58], [-11, 76], [-10, 82]],
    [[-15, 55], [-3, 29], [-10, 44], [-8, 48]],
    [[-10, 60], [-1, 36], [-10, 52], [-8, 61]],
    [[-6, 62], [1, 38], [-10, 57], [-8, 66]],
    [[-4, 65], [2, 43], [-9, 58], [-7, 70]],
    [[-12, 73], [-6, 55], [-16, 72], [-14, 75]],
    [[-8, 76], [0, 58], [-7, 69], [-10, 79]],
    [[-7, 80], [0, 64], [-4, 69], [-9, 83]],
    [[-9, 88], [-3, 74], [-5, 74], [-12, 92]],
    [[-17, 110], [-10, 90], [-9, 86], [-18, 108]],
    [[-11, 97], [0, 70], [2, 66], [-4, 79]],
    [[-20, 84], [-4, 29], [-9, 34], [-22, 69]],
    [[-11, 79], [5, 31], [1, 32], [-16, 75]],
    [[-6, 73], [7, 42], [11, 31], [-2, 58]],
    [[-4, 74], [1, 59], [5, 52], [1, 58]],
    [[-13, 86], [-2, 58], [-2, 55], [-13, 78]],
    [[-13, 96], [-3, 72], [-2, 67], [-9, 83]],
    [[-11, 97], [-3, 81], [0, 73], [-4, 81]],
    [[-19, 117], [-11, 97], [-8, 89], [-13, 99]],
    [[-8, 78], [0, 58], [3, 52], [-13, 81]],
    [[-5, 33], [8, 5], [7, 4], [-6, 38]],
    [[-4, 48], [10, 14], [10, 8], [-13, 62]],
    [[-2, 53], [14, 18], [17, 8], [-6, 58]],
    [[-3, 62], [13, 27], [16, 19], [-2, 59]],
    [[-13, 71], [2, 40], [3, 37], [-16, 73]],
    [[-10, 79], [0, 58], [-1, 61], [-10, 76]],
    [[-12, 86], [-3, 70], [-5, 73], [-13, 86]],
    [[-13, 90], [-6, 79], [-1, 70], [-9, 83]],
    [[-14, 97], [-8, 85], [-4, 78], [-10, 87]],
    // 276 no use
    [[CTX_NA, CTX_NA], [CTX_NA, CTX_NA], [CTX_NA, CTX_NA], [CTX_NA, CTX_NA]],
    // 277-337 Table 9-22
    [[-6, 93], [-13, 106], [-21, 126], [-22, 127]],
    [[-6, 84], [-16, 106], [-23, 124], [-25, 127]],
    [[-8, 79], [-10, 87], [-20, 110], [-25, 120]],
    [[0, 66], [-21, 114], [-26, 126], [-27, 127]],
    [[-1, 71], [-18, 110], [-25, 124], [-19, 114]],
    [[0, 62], [-14, 98], [-17, 105], [-23, 117]],
    [[-2, 60], [-22, 110], [-27, 121], [-25, 118]],
    [[-2, 59], [-21, 106], [-27, 117], [-26, 117]],
    [[-5, 75], [-18, 103], [-17, 102], [-24, 113]],
    [[-3, 62], [-21, 107], [-26, 117], [-28, 118]],
    [[-4, 58], [-23, 108], [-27, 116], [-31, 120]],
    [[-9, 66], [-26, 112], [-33, 122], [-37, 124]],
    [[-1, 79], [-10, 96], [-10, 95], [-10, 94]],
    [[0, 71], [-12, 95], [-14, 100], [-15, 102]],
    [[3, 68], [-5, 91], [-8, 95], [-10, 99]],
    [[10, 44], [-9, 93], [-17, 111], [-13, 106]],
    [[-7, 62], [-22, 94], [-28, 114], [-50, 127]],
    [[15, 36], [-5, 86], [-6, 89], [-5, 92]],
    [[14, 40], [9, 67], [-2, 80], [17, 57]],
    [[16, 27], [-4, 80], [-4, 82], [-5, 86]],
    [[12, 29], [-10, 85], [-9, 85], [-13, 94]],
    [[1, 44], [-1, 70], [-8, 81], [-12, 91]],
    [[20, 36], [7, 60], [-1, 72], [-2, 77]],
    [[18, 32], [9, 58], [5, 64], [0, 71]],
    [[5, 42], [5, 61], [1, 67], [-1, 73]],
    [[1, 48], [12, 50], [9, 56], [4, 64]],
    [[10, 62], [15, 50], [0, 69], [-7, 81]],
    [[17, 46], [18, 49], [1, 69], [5, 64]],
    [[9, 64], [17, 54], [7, 69], [15, 57]],
    [[-12, 104], [10, 41], [-7, 69], [1, 67]],
    [[-11, 97], [7, 46], [-6, 67], [0, 68]],
    [[-16, 96], [-1, 51], [-16, 77], [-10, 67]],
    [[-7, 88], [7, 49], [-2, 64], [1, 68]],
    [[-8, 85], [8, 52], [2, 61], [0, 77]],
    [[-7, 85], [9, 41], [-6, 67], [2, 64]],
    [[-9, 85], [6, 47], [-3, 64], [0, 68]],
    [[-13, 88], [2, 55], [2, 57], [-5, 78]],
    [[4, 66], [13, 41], [-3, 65], [7, 55]],
    [[-3, 77], [10, 44], [-3, 66], [5, 59]],
    [[-3, 76], [6, 50], [0, 62], [2, 65]],
    [[-6, 76], [5, 53], [9, 51], [14, 54]],
    [[10, 58], [13, 49], [-1, 66], [15, 44]],
    [[-1, 76], [4, 63], [-2, 71], [5, 60]],
    [[-1, 83], [6, 64], [-2, 75], [2, 70]],
    [[-7, 99], [-2, 69], [-1, 70], [-2, 76]],
    [[-14, 95], [-2, 59], [-9, 72], [-18, 86]],
    [[2, 95], [6, 70], [14, 60], [12, 70]],
    [[0, 76], [10, 44], [16, 37], [5, 64]],
    [[-5, 74], [9, 31], [0, 47], [-12, 70]],
    [[0, 70], [12, 43], [18, 35], [11, 55]],
    [[-11, 75], [3, 53], [11, 37], [5, 56]],
    [[1, 68], [14, 34], [12, 41], [0, 69]],
    [[0, 65], [10, 38], [10, 41], [2, 65]],
    [[-14, 73], [-3, 52], [2, 48], [-6, 74]],
    [[3, 62], [13, 40], [12, 41], [5, 54]],
    [[4, 62], [17, 32], [13, 41], [7, 54]],
    [[-1, 68], [7, 44], [0, 59], [-6, 76]],
    [[-13, 75], [7, 38], [3, 50], [-11, 82]],
    [[11, 55], [13, 50], [19, 40], [-2, 77]],
    [[5, 64], [10, 57], [3, 66], [-2, 77]],
    [[12, 70], [26, 43], [18, 50], [25, 42]],
    // 338-398 Table 9-23
    [[15, 6], [14, 11], [19, -6], [17, -13]],
    [[6, 19], [11, 14], [18, -6], [16, -9]],
    [[7, 16], [9, 11], [14, 0], [17, -12]],
    [[12, 14], [18, 11], [26, -12], [27, -21]],
    [[18, 13], [21, 9], [31, -16], [37, -30]],
    [[13, 11], [23, -2], [33, -25], [41, -40]],
    [[13, 15], [32, -15], [33, -22], [42, -41]],
    [[15, 16], [32, -15], [37, -28], [48, -47]],
    [[12, 23], [34, -21], [39, -30], [39, -32]],
    [[13, 23], [39, -23], [42, -30], [46, -40]],
    [[15, 20], [42, -33], [47, -42], [52, -51]],
    [[14, 26], [41, -31], [45, -36], [46, -41]],
    [[14, 44], [46, -28], [49, -34], [52, -39]],
    [[17, 40], [38, -12], [41, -17], [43, -19]],
    [[17, 47], [21, 29], [32, 9], [32, 11]],
    [[24, 17], [45, -24], [69, -71], [61, -55]],
    [[21, 21], [53, -45], [63, -63], [56, -46]],
    [[25, 22], [48, -26], [66, -64], [62, -50]],
    [[31, 27], [65, -43], [77, -74], [81, -67]],
    [[22, 29], [43, -19], [54, -39], [45, -20]],
    [[19, 35], [39, -10], [52, -35], [35, -2]],
    [[14, 50], [30, 9], [41, -10], [28, 15]],
    [[10, 57], [18, 26], [36, 0], [34, 1]],
    [[7, 63], [20, 27], [40, -1], [39, 1]],
    [[-2, 77], [0, 57], [30, 14], [30, 17]],
    [[-4, 82], [-14, 82], [28, 26], [20, 38]],
    [[-3, 94], [-5, 75], [23, 37], [18, 45]],
    [[9, 69], [-19, 97], [12, 55], [15, 54]],
    [[-12, 109], [-35, 125], [11, 65], [0, 79]],
    [[36, -35], [27, 0], [37, -33], [36, -16]],
    [[36, -34], [28, 0], [39, -36], [37, -14]],
    [[32, -26], [31, -4], [40, -37], [37, -17]],
    [[37, -30], [27, 6], [38, -30], [32, 1]],
    [[44, -32], [34, 8], [46, -33], [34, 15]],
    [[34, -18], [30, 10], [42, -30], [29, 15]],
    [[34, -15], [24, 22], [40, -24], [24, 25]],
    [[40, -15], [33, 19], [49, -29], [34, 22]],
    [[33, -7], [22, 32], [38, -12], [31, 16]],
    [[35, -5], [26, 31], [40, -10], [35, 18]],
    [[33, 0], [21, 41], [38, -3], [31, 28]],
    [[38, 2], [26, 44], [46, -5], [33, 41]],
    [[33, 13], [23, 47], [31, 20], [36, 28]],
    [[23, 35], [16, 65], [29, 30], [27, 47]],
    [[13, 58], [14, 71], [25, 44], [21, 62]],
    [[29, -3], [8, 60], [12, 48], [18, 31]],
    [[26, 0], [6, 63], [11, 49], [19, 26]],
    [[22, 30], [17, 65], [26, 45], [36, 24]],
    [[31, -7], [21, 24], [22, 22], [24, 23]],
    [[35, -15], [23, 20], [23, 22], [27, 16]],
    [[34, -3], [26, 23], [27, 21], [24, 30]],
    [[34, 3], [27, 32], [33, 20], [31, 29]],
    [[36, -1], [28, 23], [26, 28], [22, 41]],
    [[34, 5], [28, 24], [30, 24], [22, 42]],
    [[32, 11], [23, 40], [27, 34], [16, 60]],
    [[35, 5], [24, 32], [18, 42], [15, 52]],
    [[34, 12], [28, 29], [25, 39], [14, 60]],
    [[39, 11], [23, 42], [18, 50], [3, 78]],
    [[30, 29], [19, 57], [12, 70], [-16, 123]],
    [[34, 26], [22, 53], [21, 54], [21, 53]],
    [[29, 39], [22, 61], [14, 71], [22, 56]],
    [[19, 66], [11, 86], [11, 83], [25, 61]],
    [[31, 21], [12, 40], [25, 32], [21, 33]],
    [[31, 31], [11, 51], [21, 49], [19, 50]],
    [[25, 50], [14, 59], [21, 54], [17, 61]],
    // 402-459 Table 9-24
    [[-17, 120], [-4, 79], [-5, 85], [-3, 78]],
    [[-20, 112], [-7, 71], [-6, 81], [-8, 74]],
    [[-18, 114], [-5, 69], [-10, 77], [-9, 72]],
    [[-11, 85], [-9, 70], [-7, 81], [-10, 72]],
    [[-15, 92], [-8, 66], [-17, 80], [-18, 75]],
    [[-14, 89], [-10, 68], [-18, 73], [-12, 71]],
    [[-26, 71], [-19, 73], [-4, 74], [-11, 63]],
    [[-15, 81], [-12, 69], [-10, 83], [-5, 70]],
    [[-14, 80], [-16, 70], [-9, 71], [-17, 75]],
    [[0, 68], [-15, 67], [-9, 67], [-14, 72]],
    [[-14, 70], [-20, 62], [-1, 61], [-16, 67]],
    [[-24, 56], [-19, 70], [-8, 66], [-8, 53]],
    [[-23, 68], [-16, 66], [-14, 66], [-14, 59]],
    [[-24, 50], [-22, 65], [0, 59], [-9, 52]],
    [[-11, 74], [-20, 63], [2, 59], [-11, 68]],
    [[23, -13], [9, -2], [17, -10], [9, -2]],
    [[26, -13], [26, -9], [32, -13], [30, -10]],
    [[40, -15], [33, -9], [42, -9], [31, -4]],
    [[49, -14], [39, -7], [49, -5], [33, -1]],
    [[44, 3], [41, -2], [53, 0], [33, 7]],
    [[45, 6], [45, 3], [64, 3], [31, 12]],
    [[44, 34], [49, 9], [68, 10], [37, 23]],
    [[33, 54], [45, 27], [66, 27], [31, 38]],
    [[19, 82], [36, 59], [47, 57], [20, 64]],
    [[-3, 75], [-6, 66], [-5, 71], [-9, 71]],
    [[-1, 23], [-7, 35], [0, 24], [-7, 37]],
    [[1, 34], [-7, 42], [-1, 36], [-8, 44]],
    [[1, 43], [-8, 45], [-2, 42], [-11, 49]],
    [[0, 54], [-5, 48], [-2, 52], [-10, 56]],
    [[-2, 55], [-12, 56], [-9, 57], [-12, 59]],
    [[0, 61], [-6, 60], [-6, 63], [-8, 63]],
    [[1, 64], [-5, 62], [-4, 65], [-9, 67]],
    [[0, 68], [-8, 66], [-4, 67], [-6, 68]],
    [[-9, 92], [-8, 76], [-7, 82], [-10, 79]],
    [[-14, 106], [-5, 85], [-3, 81], [-3, 78]],
    [[-13, 97], [-6, 81], [-3, 76], [-8, 74]],
    [[-15, 90], [-10, 77], [-7, 72], [-9, 72]],
    [[-12, 90], [-7, 81], [-6, 78], [-10, 72]],
    [[-18, 88], [-17, 80], [-12, 72], [-18, 75]],
    [[-10, 73], [-18, 73], [-14, 68], [-12, 71]],
    [[-9, 79], [-4, 74], [-3, 70], [-11, 63]],
    [[-14, 86], [-10, 83], [-6, 76], [-5, 70]],
    [[-10, 73], [-9, 71], [-5, 66], [-17, 75]],
    [[-10, 70], [-9, 67], [-5, 62], [-14, 72]],
    [[-10, 69], [-1, 61], [0, 57], [-16, 67]],
    [[-5, 66], [-8, 66], [-4, 61], [-8, 53]],
    [[-9, 64], [-14, 66], [-9, 60], [-14, 59]],
    [[-5, 58], [0, 59], [1, 54], [-9, 52]],
    [[2, 59], [2, 59], [2, 58], [-11, 68]],
    [[21, -10], [21, -13], [17, -10], [9, -2]],
    [[24, -11], [33, -14], [32, -13], [30, -10]],
    [[28, -8], [39, -7], [42, -9], [31, -4]],
    [[28, -1], [46, -2], [49, -5], [33, -1]],
    [[29, 3], [51, 2], [53, 0], [33, 7]],
    [[29, 9], [60, 6], [64, 3], [31, 12]],
    [[35, 20], [61, 17], [68, 10], [37, 23]],
    [[29, 36], [55, 34], [66, 27], [31, 38]],
    [[14, 67], [42, 62], [47, 57], [20, 64]],
];

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SWelsCabacCtx {
    pub uiState: u8,
    pub uiMPS: u8,
}

pub type PWelsCabacCtx = *mut SWelsCabacCtx;

/// The arithmetic-decoding engine state — a **detached position**, per plan §2.1.3.
///
/// The C++ carries a pointer triple (`pBuffStart`/`pBuffCurr`/`pBuffEnd`) alongside
/// the arithmetic registers. All three are gone:
///
/// | C++ field | here | why |
/// |---|---|---|
/// | `uiRange`, `uiOffset`, `iBitsLeft` | unchanged | the arithmetic state |
/// | `pBuffCurr - pBuffStart` | `pos` | the position, the only thing that moves |
/// | `pBuffStart` | — | the buffer is the caller's; it is passed per call |
/// | `pBuffEnd` | — | `buf.len()` of the RBSP window ([`BsReader::rbsp_window`]) |
///
/// Field order is deliberate: `uiRange` and `uiOffset` stay adjacent so the pair load
/// the release build already emits for them (`ldp x9, x11, [x0]`) survives the
/// conversion.
///
/// `WelsMalloczHelper` zeroes this at allocation (`decoder_core.rs:3591`), and a zeroed
/// engine is inert rather than null-pointered: `pos = 0` with an empty window takes the
/// ladder's error arm.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SWelsCabacDecEngine {
    pub uiRange: u64,
    pub uiOffset: u64,
    pub iBitsLeft: i32,
    /// Byte offset into the slice's RBSP — the C++ `pBuffCurr - pBuffStart`.
    pub pos: usize,
}

pub type PWelsCabacDecEngine = *mut SWelsCabacDecEngine;

pub use crate::decoder::bit_stream::{BsReader, RawDataBuffer};

pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};


// 1. CABAC context initialization
pub unsafe fn WelsCabacGlobalInit(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    unsafe {
        for iModel in 0..4 {
            for iQp in 0..=WELS_QP_MAX {
                for iIdx in 0..WELS_CONTEXT_COUNT {
                    let m = g_kiCabacGlobalContextIdx[iIdx][iModel][0] as i32;
                    let n = g_kiCabacGlobalContextIdx[iIdx][iModel][1] as i32;
                    let iPreCtxState = WELS_CLIP3(((m * iQp) >> 4) + n, 1, 126);
                    let uiValMps: u8;
                    let uiStateIdx: u8;
                    if iPreCtxState <= 63 {
                        uiStateIdx = (63 - iPreCtxState) as u8;
                        uiValMps = 0;
                    } else {
                        uiStateIdx = (iPreCtxState - 64) as u8;
                        uiValMps = 1;
                    }
                    (*pCtx).sWelsCabacContexts[iModel][iQp as usize][iIdx].uiState = uiStateIdx;
                    (*pCtx).sWelsCabacContexts[iModel][iQp as usize][iIdx].uiMPS = uiValMps;
                }
            }
        }
        (*pCtx).bCabacInited = true;
    }
}

pub unsafe fn WelsCabacContextInit(
    pCtx: PWelsDecoderContext,
    eSliceType: u8,
    iCabacInitIdc: i32,
    iQp: i32,
) {
    if pCtx.is_null() {
        return;
    }
    unsafe {
        let iIdx = if eSliceType as i32 == I_SLICE as i32 {
            0
        } else {


            (iCabacInitIdc + 1) as usize
        };
        if !(*pCtx).bCabacInited {
            WelsCabacGlobalInit(pCtx);
        }
        let qp_idx = iQp as usize;
        let model_idx = iIdx;
        std::ptr::copy_nonoverlapping(
            (*pCtx).sWelsCabacContexts[model_idx][qp_idx].as_ptr(),
            cabac_ctx_base(pCtx),
            WELS_CONTEXT_COUNT,
        );
    }
}

// 2. Decoding engine initialization
//
/// Primes the engine from the CAVLC cursor's position — **audit site 1**, the only
/// place in this module that reads past the RBSP (`len + 2`, needing `avail >= len+3`).
///
/// # The rewind cannot underflow, and here is why it cannot *today*
///
/// `curr = pos - remaining_bytes` with `remaining_bytes ∈ [0, 4]` (derived in the
/// module docs from `left_bits ∈ [-16, 15]`). Every path into this function has primed
/// the cursor since the last write to `pos` — `DecInitBits` → `BsCursor::init` sets
/// `pos = 4` for the slice-header path, and `InitReadBits` → `init_read_bits` does
/// `pos += 4` for the I_PCM re-entry at `parse_mb_syn_cabac.rs:3317` — and both leave
/// **`pos >= 4`**. The bound is tight rather than generous: a cursor primed and not yet
/// advanced gives `pos = 4, remaining_bytes = 4`, landing exactly on `curr = 0`. The
/// `debug_assert` is there because that reasoning is about *callers*, and callers
/// change.
pub unsafe fn InitCabacDecEngineFromBS(
    pDecEngine: PWelsCabacDecEngine,
    pBsAux: &mut BsReader,
    raw: &RawDataBuffer,
) -> i32 {
    if pDecEngine.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let pos = pBsAux.cursor.pos() as isize;
        let len = pBsAux.cursor.len() as isize;
        let iRemainingBits = -pBsAux.cursor.left_bits();
        let iRemainingBytes = ((iRemainingBits >> 3) + 2) as isize;
        debug_assert!(
            (0..=4).contains(&iRemainingBytes),
            "left_bits {} out of [-16, 15]",
            pBsAux.cursor.left_bits()
        );
        debug_assert!(
            pos >= iRemainingBytes,
            "CABAC init rewind underflows: pos {} - {} (a primed cursor has pos >= 4)",
            pos,
            iRemainingBytes
        );
        let iCurr = pos - iRemainingBytes;
        // `pCurr >= pEndBuf - 1`, in offsets. Signed on both sides: `len` is at least 1
        // (`BsCursor::init` rejects a non-positive payload) but the arithmetic is
        // written so a zero-length window compares rather than wraps.
        if iCurr >= len - 1 {
            return ERR_INFO_INVALID_ACCESS;
        }
        let curr = iCurr as usize;

        // The wider window — this is the one site that needs the readable extent past
        // the RBSP, and it is derived from the owning buffer at call time (F16's
        // rule). The guard above bounds `curr <= len - 2`, so `curr + 5 <= len + 3 <=
        // window.len()`; the `get` is therefore unreachable-None, and routes a
        // violated contract to the error path instead of past the end of the
        // allocation (F4/F16's shape).
        let buf = raw.window_from(pBsAux.start);
        let b = match buf.get(curr..curr + 5) {
            Some(b) => b,
            None => return ERR_INFO_INVALID_ACCESS,
        };

        let mut uiOffset = ((b[0] as u64) << 16) | ((b[1] as u64) << 8) | (b[2] as u64);
        uiOffset <<= 16;
        uiOffset |= ((b[3] as u64) << 8) | (b[4] as u64);

        (*pDecEngine).uiOffset = uiOffset;
        (*pDecEngine).iBitsLeft = 31;
        (*pDecEngine).pos = curr + 5;
        (*pDecEngine).uiRange = WELS_CABAC_HALF;
        pBsAux.cursor.hand_off_to_cabac();

        ERR_NONE
    }
}

/// Hands the position back to the CAVLC cursor — **audit site 3, which reads nothing**.
///
/// The C++ wrote four fields into `SBitStringAux` and re-stored `pStartBuf` from
/// `pBuffStart`, which was a no-op restore of the base it had been given. What actually
/// moves is the position, and it is now the single `usize` assignment plan §2.2.2
/// predicted.
///
/// # The rewind cannot underflow either
///
/// `pos - (bits_left >> 3)` with `bits_left <= 63` (init sets 31; `Read32BitsCabac`
/// adds at most 32 before any consumer subtracts), so the rewind is at most 7 bytes.
/// The quantity is invariant under a refill — `pos += k` and `bits_left += 8k` cancel —
/// and only *increases* under renormalisation, starting from `curr + 5 - 3 = curr + 2`.
/// On the error path `bits_left` goes negative, the arithmetic shift goes negative, and
/// the position moves *forward*: the raw code's behaviour, reproduced by doing this in
/// `isize` and casting once, exactly where the raw code cast its `offset_from`.
pub unsafe fn RestoreCabacDecEngineToBS(pDecEngine: PWelsCabacDecEngine, pBsAux: &mut BsReader) {
    if pDecEngine.is_null() {
        return;
    }
    unsafe {
        let back = ((*pDecEngine).iBitsLeft >> 3) as isize;
        let pos = (*pDecEngine).pos as isize - back;
        debug_assert!(pos >= 0, "CABAC restore rewind underflows: pos {}", pos);
        (*pDecEngine).pos = pos as usize;
        (*pDecEngine).iBitsLeft = 0;
        pBsAux.cursor.restore_from_cabac((*pDecEngine).pos);
    }
}

/// The CABAC context array as a raw base pointer, with **no reference in between**.
///
/// `cabac_ctx_base(pCtx)` takes `&mut` of the array first, which retags
/// `Unique` over the whole of it and kills every pointer previously derived from it.
/// `ParseSignificantMapCabac` keeps **two** live at once (`pMapCtx` and `pLastCtx`),
/// so the second derivation invalidated the first and every read through it was UB —
/// F13's `as_mut_ptr()` shape, the one T5.B2 found six times in `manage_dec_ref.rs`,
/// here in the CABAC parser. `addr_of_mut!` creates no reference, so every pointer
/// this hands out carries the context allocation's own provenance and none can
/// invalidate another (S29).
///
/// # Safety
/// `pCtx` must be a live decoder context; the caller indexes within
/// `WELS_CONTEXT_COUNT`, exactly as the `as_mut_ptr().add(..)` spelling did.
#[inline(always)]
pub unsafe fn cabac_ctx_base(pCtx: PWelsDecoderContext) -> *mut SWelsCabacCtx {
    unsafe { std::ptr::addr_of_mut!((*pCtx).pCabacCtx).cast::<SWelsCabacCtx>() }
}

/// The RBSP window the CABAC engine reads, for a context mid-slice.
///
/// One deref chain, once per parsing function rather than once per bin, and the
/// window is derived from the owning [`RawDataBuffer`] at call time
/// ([`RawDataBuffer::rbsp_window`] is the single authority). `SHIM(phase5)` — the
/// `pBitStringAux` pointer it walks is Phase 5's to remove, and the accessor dies
/// with it.
///
/// # Safety
/// `pCtx` must be a live decoder context inside slice decoding, so `pCurDqLayer` and
/// its `pBitStringAux` are set — the same precondition every caller in
/// `parse_mb_syn_cabac.rs` already relies on for `pCabacDecEngine`.
#[inline(always)]
pub unsafe fn cabac_rbsp_window<'a>(pCtx: PWelsDecoderContext) -> &'a [u8] {
    unsafe {
        let raw: &'a RawDataBuffer = &(*pCtx).sRawData;
        raw.rbsp_window(&*(*(*pCtx).pCurDqLayer).pBitStringAux)
    }
}

// 3. Actual decoding
/// The refill — **audit site 2**, the 4/3/2/1 end ladder, bounded by `len - 1`.
///
/// `win` is the RBSP window ([`BsReader::rbsp_window`]): `win.len()` **is** the C++
/// `pBuffEnd - pBuffStart`, so the selector is the slice's own length and the engine
/// computes no extent of its own.
///
/// The `pos >= win.len()` test is the C++ `iLeftBytes <= 0` written as a comparison
/// rather than a subtraction — see the module docs: `pos` legitimately exceeds
/// `win.len()` after init on a truncated stream, and `win.len() - pos` in `usize` would
/// wrap to a huge positive and select the 4-byte arm.
///
/// # Why the arms are `first_chunk`, and why the order is inverted (S1 step 3)
///
/// Written the obvious way — `match tail.len() { 3 => …, 2 => …, 1 => …, _ => … }` with
/// `tail[i]` indexing inside each arm — the release build **re-checked the length in
/// the `_` arm** and emitted three `panic_bounds_check` paths, plus four separate
/// `ldrb`s where the raw pointer version had one `ldr`+`rev`. LLVM propagated "not 1,
/// 2 or 3" into the arm but not "therefore >= 4".
///
/// `first_chunk::<N>()` states the width as a *type* instead of leaving it to be
/// re-derived, which is the exact-span trim (S9) at four-byte scale: the `Some` arm
/// carries a `&[u8; N]`, so the load folds and the checks vanish. Testing `>= 4` first
/// puts the common case at the top of the chain; the four widths are a disjoint
/// partition, so the order is free. The final `else` is `tail.len() == 1` — `0` was
/// rejected by the guard above — and its `tail[0]` folds on that fact.
///
/// `#[inline(always)]`: the raw pointer version was inlined into `DecodeBinCabac` by
/// the cost model alone. Adding a slice parameter tipped it over, and the call cost
/// `DecodeBinCabac` a stack frame *on every bin*, refill or not. This pins the
/// reference's shape rather than leaving it to a heuristic (S8).
#[inline(always)]
pub unsafe fn Read32BitsCabac(
    win: &[u8],
    pDecEngine: PWelsCabacDecEngine,
    uiValue: *mut u32,
    iNumBitsRead: *mut i32,
) -> i32 {
    if pDecEngine.is_null() {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_NO_BS_TO_READ);
    }
    unsafe {
        let pos = (*pDecEngine).pos;
        if !iNumBitsRead.is_null() {
            *iNumBitsRead = 0;
        }
        if !uiValue.is_null() {
            *uiValue = 0;
        }
        if pos >= win.len() {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_NO_BS_TO_READ);
        }
        let tail = &win[pos..];
        let (v, n, width) = if let Some(b) = tail.first_chunk::<4>() {
            (u32::from_be_bytes(*b), 32, 4)
        } else if let Some(b) = tail.first_chunk::<3>() {
            (
                ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32),
                24,
                3,
            )
        } else if let Some(b) = tail.first_chunk::<2>() {
            (((b[0] as u32) << 8) | (b[1] as u32), 16, 2)
        } else {
            (tail[0] as u32, 8, 1)
        };
        if !uiValue.is_null() {
            *uiValue = v;
        }
        (*pDecEngine).pos = pos + width;
        if !iNumBitsRead.is_null() {
            *iNumBitsRead = n;
        }
        ERR_NONE
    }
}

pub unsafe fn DecodeBinCabac(
    win: &[u8],
    pDecEngine: PWelsCabacDecEngine,
    pBinCtx: PWelsCabacCtx,
    uiBit: *mut u32,
) -> i32 {
    if pDecEngine.is_null() || pBinCtx.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let iErrorInfo: i32;
        let uiState = (*pBinCtx).uiState as usize;
        let mut uiBinVal = (*pBinCtx).uiMPS as u32;
        let mut uiOffset = (*pDecEngine).uiOffset;
        let mut uiRange = (*pDecEngine).uiRange;

        let mut iRenorm: i32 = 1;
        let range_idx = ((uiRange >> 6) & 0x03) as usize;
        let uiRangeLPS = g_kuiCabacRangeLps[uiState][range_idx] as u64;
        uiRange -= uiRangeLPS;

        if uiOffset >= (uiRange << (*pDecEngine).iBitsLeft) {
            // LPS
            uiOffset -= uiRange << (*pDecEngine).iBitsLeft;
            uiBinVal ^= 1;
            if uiState == 0 {
                (*pBinCtx).uiMPS ^= 1;
            }
            (*pBinCtx).uiState = g_kuiStateTransTable[uiState][0];
            iRenorm = g_kRenormTable256[uiRangeLPS as usize] as i32;
            uiRange = uiRangeLPS << iRenorm;
        } else {
            // MPS
            (*pBinCtx).uiState = g_kuiStateTransTable[uiState][1];
            if uiRange >= WELS_CABAC_QUARTER {
                (*pDecEngine).uiRange = uiRange;
                if !uiBit.is_null() {
                    *uiBit = uiBinVal;
                }
                return ERR_NONE;
            } else {
                uiRange <<= 1;
            }
        }

        // Renorm
        (*pDecEngine).uiRange = uiRange;
        (*pDecEngine).iBitsLeft -= iRenorm;
        if !uiBit.is_null() {
            *uiBit = uiBinVal;
        }

        if (*pDecEngine).iBitsLeft > 0 {
            (*pDecEngine).uiOffset = uiOffset;
            return ERR_NONE;
        }

        let mut uiVal: u32 = 0;
        let mut iNumBitsRead: i32 = 0;
        iErrorInfo = Read32BitsCabac(win, pDecEngine, &mut uiVal, &mut iNumBitsRead);
        (*pDecEngine).uiOffset = (uiOffset << iNumBitsRead) | (uiVal as u64);
        (*pDecEngine).iBitsLeft += iNumBitsRead;

        if iErrorInfo != 0 && (*pDecEngine).iBitsLeft < 0 {
            return iErrorInfo;
        }
        ERR_NONE
    }
}

pub unsafe fn DecodeBypassCabac(
    win: &[u8],
    pDecEngine: PWelsCabacDecEngine,
    uiBinVal: *mut u32,
) -> i32 {
    if pDecEngine.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let iErrorInfo: i32;
        let mut iBitsLeft = (*pDecEngine).iBitsLeft;
        let mut uiOffset = (*pDecEngine).uiOffset;

        if iBitsLeft <= 0 {
            let mut uiVal: u32 = 0;
            let mut iNumBitsRead: i32 = 0;
            iErrorInfo = Read32BitsCabac(win, pDecEngine, &mut uiVal, &mut iNumBitsRead);
            uiOffset = (uiOffset << iNumBitsRead) | (uiVal as u64);
            iBitsLeft = iNumBitsRead;
            if iErrorInfo != 0 && iBitsLeft == 0 {
                return iErrorInfo;
            }
        }

        iBitsLeft -= 1;
        let uiRangeValue = (*pDecEngine).uiRange << iBitsLeft;
        if uiOffset >= uiRangeValue {
            (*pDecEngine).iBitsLeft = iBitsLeft;
            (*pDecEngine).uiOffset = uiOffset - uiRangeValue;
            if !uiBinVal.is_null() {
                *uiBinVal = 1;
            }
            return ERR_NONE;
        }

        (*pDecEngine).iBitsLeft = iBitsLeft;
        (*pDecEngine).uiOffset = uiOffset;
        if !uiBinVal.is_null() {
            *uiBinVal = 0;
        }
        ERR_NONE
    }
}

pub unsafe fn DecodeTerminateCabac(
    win: &[u8],
    pDecEngine: PWelsCabacDecEngine,
    uiBinVal: *mut u32,
) -> i32 {
    if pDecEngine.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let mut iErrorInfo = ERR_NONE;
        let uiRange = (*pDecEngine).uiRange - 2;
        let uiOffset = (*pDecEngine).uiOffset;

        if uiOffset >= (uiRange << (*pDecEngine).iBitsLeft) {
            if !uiBinVal.is_null() {
                *uiBinVal = 1;
            }
        } else {
            if !uiBinVal.is_null() {
                *uiBinVal = 0;
            }
            // Renorm
            if uiRange < WELS_CABAC_QUARTER {
                let iRenorm = g_kRenormTable256[uiRange as usize] as i32;
                (*pDecEngine).uiRange = uiRange << iRenorm;
                (*pDecEngine).iBitsLeft -= iRenorm;
                if (*pDecEngine).iBitsLeft < 0 {
                    let mut uiVal: u32 = 0;
                    let mut iNumBitsRead: i32 = 0;
                    iErrorInfo = Read32BitsCabac(win, pDecEngine, &mut uiVal, &mut iNumBitsRead);
                    (*pDecEngine).uiOffset = ((*pDecEngine).uiOffset << iNumBitsRead) | (uiVal as u64);
                    (*pDecEngine).iBitsLeft += iNumBitsRead;
                }
                if iErrorInfo != 0 && (*pDecEngine).iBitsLeft < 0 {
                    return iErrorInfo;
                }
                return ERR_NONE;
            } else {
                (*pDecEngine).uiRange = uiRange;
                return ERR_NONE;
            }
        }
        ERR_NONE
    }
}

// 4. Unary parsing
pub unsafe fn DecodeUnaryBinCabac(
    win: &[u8],
    pDecEngine: PWelsCabacDecEngine,
    pBinCtx: PWelsCabacCtx,
    iCtxOffset: i32,
    uiSymVal: *mut u32,
) -> i32 {
    if pDecEngine.is_null() || pBinCtx.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        if !uiSymVal.is_null() {
            *uiSymVal = 0;
        }
        let mut uiFirstBin: u32 = 0;
        let err = DecodeBinCabac(win, pDecEngine, pBinCtx, &mut uiFirstBin);
        if err != 0 {
            return err;
        }
        if uiFirstBin == 0 {
            if !uiSymVal.is_null() {
                *uiSymVal = 0;
            }
            return ERR_NONE;
        }

        let pCtx = pBinCtx.offset(iCtxOffset as isize);
        let mut sym_val: u32 = 0;
        loop {
            let mut uiCode: u32 = 0;
            let err = DecodeBinCabac(win, pDecEngine, pCtx, &mut uiCode);
            if err != 0 {
                return err;
            }
            sym_val += 1;
            if uiCode == 0 {
                break;
            }
        }
        if !uiSymVal.is_null() {
            *uiSymVal = sym_val;
        }
        ERR_NONE
    }
}

// 5. EXGk parsing
pub unsafe fn DecodeExpBypassCabac(
    win: &[u8],
    pDecEngine: PWelsCabacDecEngine,
    mut iCount: i32,
    uiSymVal: *mut u32,
) -> i32 {
    if pDecEngine.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let mut uiCode: u32 = 0;
        let mut iSymTmp: i32 = 0;
        let mut iSymTmp2: i32 = 0;
        if !uiSymVal.is_null() {
            *uiSymVal = 0;
        }

        loop {
            let err = DecodeBypassCabac(win, pDecEngine, &mut uiCode);
            if err != 0 {
                return err;
            }
            if uiCode == 1 {
                iSymTmp += 1 << iCount;
                iCount += 1;
            }
            if uiCode == 0 || iCount == 16 {
                break;
            }
        }

        if iCount == 16 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_UNEXPECTED_VALUE);
        }

        while iCount > 0 {
            iCount -= 1;
            let err = DecodeBypassCabac(win, pDecEngine, &mut uiCode);
            if err != 0 {
                return err;
            }
            if uiCode == 1 {
                iSymTmp2 |= 1 << iCount;
            }
        }

        if !uiSymVal.is_null() {
            *uiSymVal = (iSymTmp + iSymTmp2) as u32;
        }
        ERR_NONE
    }
}

pub unsafe fn DecodeUEGLevelCabac(
    win: &[u8],
    pDecEngine: PWelsCabacDecEngine,
    pBinCtx: PWelsCabacCtx,
    uiBinVal: *mut u32,
) -> u32 {
    if pDecEngine.is_null() || pBinCtx.is_null() {
        return ERR_INFO_INVALID_ACCESS as u32;
    }
    unsafe {
        let mut uiCode: u32 = 0;
        let err = DecodeBinCabac(win, pDecEngine, pBinCtx, &mut uiCode);
        if err != 0 {
            return err as u32;
        }
        if uiCode == 0 {
            if !uiBinVal.is_null() {
                *uiBinVal = 0;
            }
            return ERR_NONE as u32;
        }

        let mut uiTmp: u32 = 0;
        let mut uiCount: u32 = 1;
        uiCode = 0;

        loop {
            let err = DecodeBinCabac(win, pDecEngine, pBinCtx, &mut uiTmp);
            if err != 0 {
                return err as u32;
            }
            uiCode += 1;
            uiCount += 1;
            if uiTmp == 0 || uiCount == 13 {
                break;
            }
        }

        if uiTmp != 0 {
            let err = DecodeExpBypassCabac(win, pDecEngine, 0, &mut uiTmp);
            if err != 0 {
                return err as u32;
            }
            uiCode += uiTmp + 1;
        }

        if !uiBinVal.is_null() {
            *uiBinVal = uiCode;
        }
        ERR_NONE as u32
    }
}

pub unsafe fn DecodeUEGMvCabac(
    win: &[u8],
    pDecEngine: PWelsCabacDecEngine,
    pBinCtx: PWelsCabacCtx,
    iMaxC: u32,
    uiCode: *mut u32,
) -> i32 {
    if pDecEngine.is_null() || pBinCtx.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    unsafe {
        let mut first_code: u32 = 0;
        let err = DecodeBinCabac(
            win,
            pDecEngine,
            pBinCtx.offset(g_kMvdBinPos2Ctx[0] as isize),
            &mut first_code,
        );
        if err != 0 {
            return err;
        }
        if first_code == 0 {
            if !uiCode.is_null() {
                *uiCode = 0;
            }
            return ERR_NONE;
        }

        let mut uiTmp: u32 = 0;
        let mut uiCount: usize = 1;
        let mut code: u32 = 0;

        loop {
            let ctx_offset = g_kMvdBinPos2Ctx[uiCount] as isize;
            uiCount += 1;
            let err = DecodeBinCabac(win, pDecEngine, pBinCtx.offset(ctx_offset), &mut uiTmp);
            if err != 0 {
                return err;
            }
            code += 1;
            if uiTmp == 0 || uiCount == 8 {
                break;
            }
        }

        if uiTmp != 0 {
            let err = DecodeExpBypassCabac(win, pDecEngine, 3, &mut uiTmp);
            if err != 0 {
                return err;
            }
            code += uiTmp + 1;
        }

        if !uiCode.is_null() {
            *uiCode = code;
        }
        ERR_NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_renorm_table_bounds() {
        assert_eq!(g_kRenormTable256.len(), 256);
        assert_eq!(g_kRenormTable256[0], 6);
        assert_eq!(g_kRenormTable256[7], 6);
        assert_eq!(g_kRenormTable256[8], 5);
        assert_eq!(g_kRenormTable256[128], 1);
        assert_eq!(g_kRenormTable256[255], 1);
    }

    #[test]
    fn test_cabac_global_init() {
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.eSliceType = crate::decoder::slice::EWelsSliceType::I_SLICE;
        ctx.bCabacInited = false;
        unsafe {
            WelsCabacGlobalInit(&mut *ctx);
            assert!(ctx.bCabacInited);
            WelsCabacContextInit(&mut *ctx, crate::decoder::slice::EWelsSliceType::I_SLICE as u8, 0, 26);
        }
    }

    // -----------------------------------------------------------------------
    // The CAVLC↔CABAC handoff.
    //
    // Both readers now live in one position space, and the whole handoff is a
    // `usize` in each direction. The plan (§T3.2) calls a round-trip test at a
    // known bit offset "cheap and permanent"; this is it. It is the only place
    // the two cursors' agreement is asserted directly rather than inferred from
    // a decoded frame, and it runs in both profiles.
    // -----------------------------------------------------------------------

    use crate::decoder::bit_stream::{DecInitBits, READER_SLOP};

    /// An RBSP plus the slack every real caller has (`decoder_core.rs:3644`).
    fn rbsp_with_slack(payload: &[u8]) -> Vec<u8> {
        let mut v = payload.to_vec();
        v.extend_from_slice(&[0u8; READER_SLOP + 1]);
        v
    }

    #[test]
    fn cavlc_to_cabac_and_back_restores_the_cursor_at_a_known_offset() {
        // 16 bytes of RBSP; the CAVLC side consumes 20 bits, so the handoff
        // happens mid-byte with a partially-spent accumulator — the case where
        // `iRemainingBytes`'s rewind actually does something.
        let payload: [u8; 16] = [
            0xA5, 0x3C, 0x91, 0x08, 0xFF, 0x00, 0x7E, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE,
            0xF0, 0x11,
        ];
        let raw = RawDataBuffer::from_vec(rbsp_with_slack(&payload));
        let mut bs = BsReader::default();
        let mut engine = SWelsCabacDecEngine::default();

        unsafe {
            let err = DecInitBits(&mut bs, &raw, 0, (payload.len() * 8) as i32);
            assert_eq!(err, ERR_NONE);

            let (b, cursor) = bs.split(&raw);
            for n in [8, 8, 4] {
                cursor.get_bits(b, n).expect("20 bits of a 16-byte RBSP");
            }
            let bits_consumed = 20;
            // Where the CAVLC side stands: `pos` is the refill point, and the
            // accumulator holds the bits between the two.
            let pos_before = bs.cursor.pos();
            let left_before = bs.cursor.left_bits();

            assert_eq!(InitCabacDecEngineFromBS(&mut engine, &mut bs, &raw), ERR_NONE);

            // The engine started where the *bits* had got to, not where the
            // refill pointer had: `curr = pos - ((-left_bits >> 3) + 2)`, and it
            // primed five bytes from there.
            let remaining_bytes = (((-left_before) >> 3) + 2) as usize;
            assert_eq!(engine.pos, pos_before - remaining_bytes + 5);
            assert_eq!(engine.iBitsLeft, 31);
            assert_eq!(engine.uiRange, WELS_CABAC_HALF);
            // The handoff spent the cursor's accumulator, and the position is
            // untouched until the engine gives it back.
            assert_eq!(bs.cursor.left_bits(), 0);
            assert_eq!(bs.cursor.pos(), pos_before);

            // Consume some bins so the engine's position is genuinely its own,
            // then hand back.
            let win = raw.rbsp_window(&bs);
            assert_eq!(win.len(), payload.len(), "the window is the RBSP, not the allocation");
            let mut ctx = SWelsCabacCtx { uiState: 20, uiMPS: 1 };
            let mut bit: u32 = 0;
            for _ in 0..64 {
                assert_eq!(DecodeBinCabac(win, &mut engine, &mut ctx, &mut bit), ERR_NONE);
            }
            let engine_pos = engine.pos;
            let engine_bits = engine.iBitsLeft;

            RestoreCabacDecEngineToBS(&mut engine, &mut bs);

            // One `usize`, both directions, and the full cursor state after it.
            assert_eq!(engine.pos, engine_pos - ((engine_bits >> 3) as usize));
            assert_eq!(bs.cursor.pos(), engine.pos);
            assert_eq!(bs.cursor.cur_bits(), 0);
            assert_eq!(bs.cursor.left_bits(), 0);
            assert_eq!(bs.cursor.cavlc_bit_pos_state(), 0);
            assert_eq!(engine.iBitsLeft, 0);
            // `len`/`bits` describe the RBSP and must survive the whole trip.
            assert_eq!(bs.cursor.len(), payload.len());
            assert_eq!(bs.cursor.bits(), (payload.len() * 8) as i32);

            // And the cursor is usable again: re-prime and read, exactly as
            // `ParseIPCMInfoCabac` does after its own restore.
            let (b, cursor) = bs.split(&raw);
            assert_eq!(
                crate::decoder::bit_stream::InitReadBits(b, cursor, 1),
                ERR_NONE
            );
            assert!(cursor.get_bits(b, 8).is_ok());
            let _ = bits_consumed;
        }
    }

    #[test]
    fn the_end_ladder_stops_at_the_rbsp_and_never_reads_the_slack() {
        // Audit site 2: the ladder is bounded by `len`, not by `avail`. Drive
        // the engine off the end of a short RBSP and assert it errors with the
        // position at `len` rather than walking into the slack bytes that the
        // allocation genuinely has.
        let payload: [u8; 8] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let buf = rbsp_with_slack(&payload);
        let mut engine = SWelsCabacDecEngine::default();
        let win = &buf[..payload.len()];

        unsafe {
            let mut value: u32 = 0;
            let mut bits: i32 = 0;
            // Walk the ladder from the last four bytes down: 4, then 3/2/1.
            engine.pos = 4;
            assert_eq!(Read32BitsCabac(win, &mut engine, &mut value, &mut bits), ERR_NONE);
            assert_eq!((bits, engine.pos), (32, 8));
            // At the end: no bytes left, error, nothing loaded, position frozen.
            assert_eq!(
                Read32BitsCabac(win, &mut engine, &mut value, &mut bits),
                GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_NO_BS_TO_READ)
            );
            assert_eq!((value, bits, engine.pos), (0, 0, 8));

            for (start, want_bits, want_pos) in [(5usize, 24, 8), (6, 16, 8), (7, 8, 8)] {
                engine.pos = start;
                assert_eq!(Read32BitsCabac(win, &mut engine, &mut value, &mut bits), ERR_NONE);
                assert_eq!((bits, engine.pos), (want_bits, want_pos));
            }

            // `pos` past the end — reachable after init on a truncated stream,
            // and the C++ `iLeftBytes` is negative there. The comparison form
            // must take the error arm; a `usize` subtraction would wrap and
            // select the 4-byte load.
            for start in [9usize, 12, 64] {
                engine.pos = start;
                assert_eq!(
                    Read32BitsCabac(win, &mut engine, &mut value, &mut bits),
                    GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_NO_BS_TO_READ)
                );
                assert_eq!(engine.pos, start);
            }
        }
    }

    #[test]
    fn init_rejects_a_position_at_the_end_guard_rather_than_reading_there() {
        // The C++ guard is `pCurr >= pEndBuf - 1`, and the rewind is what puts
        // `pCurr` behind `pos`. A cursor parked at the very end of a short RBSP
        // must be refused, not primed.
        let payload: [u8; 5] = [0x80, 0x00, 0x00, 0x00, 0x00];
        let raw = RawDataBuffer::from_vec(rbsp_with_slack(&payload));
        let mut bs = BsReader::default();
        let mut engine = SWelsCabacDecEngine::default();
        unsafe {
            assert_eq!(
                DecInitBits(&mut bs, &raw, 0, (payload.len() * 8) as i32),
                ERR_NONE
            );
            // Park the cursor at the end: pos == len, left_bits == 0 gives
            // remaining_bytes == 2, so curr == len - 2 == 3 >= len - 1 == 4 is
            // false... push it one further to land on the guard.
            bs.cursor.set_pos(payload.len() + 1);
            bs.cursor.restore_from_cabac(payload.len() + 1); // left_bits = 0
            assert_eq!(
                InitCabacDecEngineFromBS(&mut engine, &mut bs, &raw),
                ERR_INFO_INVALID_ACCESS
            );
            // Nothing was written into the engine.
            assert_eq!(engine, SWelsCabacDecEngine::default());
        }
    }
}
