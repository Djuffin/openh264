#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! Context-Adaptive Variable-Length Coding (CAVLC) Entropy Encoding Subsystem.
//!
//! Translated from `codec/encoder/core/inc/vlc_encoder.h`,
//! `codec/encoder/core/src/encoder_data_tables.cpp`, and `codec/encoder/core/src/set_mb_syn_cavlc.cpp`.

// `CHROMA_DC_NC_OFFSET`, `ENC_RETURN_SUCCESS` and `ENC_RETURN_VLCOVERFLOWFOUND`
// were declared here with no reader anywhere in the crate — three more copies of
// names that live in `svc_set_mb_syn_cavlc.rs` (which holds `CHROMA_DC_NC_OFFSET`
// as an `i8`, the width its two call sites want) and `svc_encode_slice.rs`. They
// died with the writer dedupe, which is what makes duplicates findable: routing
// the callers here is what showed nothing had ever routed to these.

/// Residual transform block category.
/// Matches `ECtxBlockCat` in `codec/encoder/core/inc/set_mb_syn_cavlc.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ECtxBlockCat {
    LUMA_DC = 0,
    LUMA_AC = 1,
    LUMA_4x4 = 2,
    CHROMA_DC = 3,
    CHROMA_AC = 4,
}

/// The encoder's write position.
///
/// `BsWriter` is a detached cursor — `{pos, cur_bits, left_bits}`, no buffer
/// reference (plan §2.1.3). The buffer belongs to whoever allocated it and arrives
/// as `&mut [u8]` on every call, which is what took `pStartBuf`/`pCurBuf`/`pEndBuf`
/// out of this module. See `safe::bits` for the semantics and the differential
/// tests that pin them.
pub use crate::safe::bits::BsWriter;

/// CAVLC codeword table item.
/// Matches `TagCavlcTableItem` in `codec/encoder/core/inc/set_mb_syn_cavlc.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct TagCavlcTableItem {
    pub uiBits: u16,
    pub uiLen: u8,
    pub uiSuffixLength: u8,
}

pub type SCavlcTableItem = TagCavlcTableItem;

// ============================================================================
// CAVLC Lookup Tables
// ============================================================================

/// Mapping table from neighbor non-zero coefficient count (nC) to VLC table index.
/// Matches `g_kuiEncNcMapTable[18]` in `codec/encoder/core/src/encoder_data_tables.cpp`.
///
/// One definition: `svc_set_mb_syn_cavlc.rs` carried a byte-identical second copy
/// until the writer dedupe, and re-exports this one now.
//
// A `#[repr(align(16))] pub struct EncNcMapTable(pub [u8; 18])` sat here too, with
// no constructor and no reader — the C++ has no such type; the alignment attribute
// on the array in `encoder_data_tables.cpp` was transliterated into a newtype that
// nothing ever wrapped.
pub const g_kuiEncNcMapTable: [u8; 18] = [
    0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4,
];

/// Mapping table for zerosLeft context clamping (0..7).
/// Matches `g_kuiZeroLeftMap[16]` in `codec/encoder/core/src/set_mb_syn_cavlc.cpp`.
pub const g_kuiZeroLeftMap: [u8; 16] = [
    0, 1, 2, 3, 4, 5, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7,
];

// `g_kuiGolombUELength` is a common-layer table (`common_tables.cpp:886`).
// This module used to declare its own copy; see the canonical definition for
// what the divergent copies got wrong.
pub use crate::common::wels_common_defs::g_kuiGolombUELength;

/// Coeff token lookup table: `[nc_idx][total_coeff][trailing_ones][0--value, 1--bit count]`
/// Dimensions: `[5][17][4][2]`
pub const g_kuiVlcCoeffToken: [[[[u8; 2]; 4]; 17]; 5] = [
    // 0 <= nc < 2
    [
        [[1, 1], [0, 0], [0, 0], [0, 0]], // 0
        [[5, 6], [1, 2], [0, 0], [0, 0]], // 1
        [[7, 8], [4, 6], [1, 3], [0, 0]], // 2
        [[7, 9], [6, 8], [5, 7], [3, 5]], // 3
        [[7, 10], [6, 9], [5, 8], [3, 6]], // 4
        [[7, 11], [6, 10], [5, 9], [4, 7]], // 5
        [[15, 13], [6, 11], [5, 10], [4, 8]], // 6
        [[11, 13], [14, 13], [5, 11], [4, 9]], // 7
        [[8, 13], [10, 13], [13, 13], [4, 10]], // 8
        [[15, 14], [14, 14], [9, 13], [4, 11]], // 9
        [[11, 14], [10, 14], [13, 14], [12, 13]], // 10
        [[15, 15], [14, 15], [9, 14], [12, 14]], // 11
        [[11, 15], [10, 15], [13, 15], [8, 14]], // 12
        [[15, 16], [1, 15], [9, 15], [12, 15]], // 13
        [[11, 16], [14, 16], [13, 16], [8, 15]], // 14
        [[7, 16], [10, 16], [9, 16], [12, 16]], // 15
        [[4, 16], [6, 16], [5, 16], [8, 16]], // 16
    ],
    // 2 <= nc < 4
    [
        [[3, 2], [0, 0], [0, 0], [0, 0]], // 0
        [[11, 6], [2, 2], [0, 0], [0, 0]], // 1
        [[7, 6], [7, 5], [3, 3], [0, 0]], // 2
        [[7, 7], [10, 6], [9, 6], [5, 4]], // 3
        [[7, 8], [6, 6], [5, 6], [4, 4]], // 4
        [[4, 8], [6, 7], [5, 7], [6, 5]], // 5
        [[7, 9], [6, 8], [5, 8], [8, 6]], // 6
        [[15, 11], [6, 9], [5, 9], [4, 6]], // 7
        [[11, 11], [14, 11], [13, 11], [4, 7]], // 8
        [[15, 12], [10, 11], [9, 11], [4, 9]], // 9
        [[11, 12], [14, 12], [13, 12], [12, 11]], // 10
        [[8, 12], [10, 12], [9, 12], [8, 11]], // 11
        [[15, 13], [14, 13], [13, 13], [12, 12]], // 12
        [[11, 13], [10, 13], [9, 13], [12, 13]], // 13
        [[7, 13], [11, 14], [6, 13], [8, 13]], // 14
        [[9, 14], [8, 14], [10, 14], [1, 13]], // 15
        [[7, 14], [6, 14], [5, 14], [4, 14]], // 16
    ],
    // 4 <= nc < 8
    [
        [[15, 4], [0, 0], [0, 0], [0, 0]], // 0
        [[15, 6], [14, 4], [0, 0], [0, 0]], // 1
        [[11, 6], [15, 5], [13, 4], [0, 0]], // 2
        [[8, 6], [12, 5], [14, 5], [12, 4]], // 3
        [[15, 7], [10, 5], [11, 5], [11, 4]], // 4
        [[11, 7], [8, 5], [9, 5], [10, 4]], // 5
        [[9, 7], [14, 6], [13, 6], [9, 4]], // 6
        [[8, 7], [10, 6], [9, 6], [8, 4]], // 7
        [[15, 8], [14, 7], [13, 7], [13, 5]], // 8
        [[11, 8], [14, 8], [10, 7], [12, 6]], // 9
        [[15, 9], [10, 8], [13, 8], [12, 7]], // 10
        [[11, 9], [14, 9], [9, 8], [12, 8]], // 11
        [[8, 9], [10, 9], [13, 9], [8, 8]], // 12
        [[13, 10], [7, 9], [9, 9], [12, 9]], // 13
        [[9, 10], [12, 10], [11, 10], [10, 10]], // 14
        [[5, 10], [8, 10], [7, 10], [6, 10]], // 15
        [[1, 10], [4, 10], [3, 10], [2, 10]], // 16
    ],
    // 8 <= nc
    [
        [[3, 6], [0, 0], [0, 0], [0, 0]], // 0
        [[0, 6], [1, 6], [0, 0], [0, 0]], // 1
        [[4, 6], [5, 6], [6, 6], [0, 0]], // 2
        [[8, 6], [9, 6], [10, 6], [11, 6]], // 3
        [[12, 6], [13, 6], [14, 6], [15, 6]], // 4
        [[16, 6], [17, 6], [18, 6], [19, 6]], // 5
        [[20, 6], [21, 6], [22, 6], [23, 6]], // 6
        [[24, 6], [25, 6], [26, 6], [27, 6]], // 7
        [[28, 6], [29, 6], [30, 6], [31, 6]], // 8
        [[32, 6], [33, 6], [34, 6], [35, 6]], // 9
        [[36, 6], [37, 6], [38, 6], [39, 6]], // 10
        [[40, 6], [41, 6], [42, 6], [43, 6]], // 11
        [[44, 6], [45, 6], [46, 6], [47, 6]], // 12
        [[48, 6], [49, 6], [50, 6], [51, 6]], // 13
        [[52, 6], [53, 6], [54, 6], [55, 6]], // 14
        [[56, 6], [57, 6], [58, 6], [59, 6]], // 15
        [[60, 6], [61, 6], [62, 6], [63, 6]], // 16
    ],
    // nc == -1 (Chroma DC)
    [
        [[1, 2], [0, 0], [0, 0], [0, 0]], // 0
        [[7, 6], [1, 1], [0, 0], [0, 0]], // 1
        [[4, 6], [6, 6], [1, 3], [0, 0]], // 2
        [[3, 6], [3, 7], [2, 7], [5, 6]], // 3
        [[2, 6], [3, 8], [2, 8], [0, 7]], // 4
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 5
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 6
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 7
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 8
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 9
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 10
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 11
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 12
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 13
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 14
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 15
        [[0, 0], [0, 0], [0, 0], [0, 0]], // 16
    ],
];

/// Level prefix codeword and bit length lookup table: `[prefix][0--value, 1--bit count]`
pub const g_kuiVlcLevelPrefix: [[u8; 2]; 15] = [
    [1, 1], [1, 2], [1, 3], [1, 4], [1, 5], [1, 6], [1, 7], [1, 8],
    [1, 9], [1, 10], [1, 11], [1, 12], [1, 13], [1, 14], [1, 15],
];

/// Total zeros table for 4x4 blocks: `[total_coeff][total_zeros][0--value, 1--bit count]`
/// Dimensions: `[16][16][2]`
pub const g_kuiVlcTotalZeros: [[[u8; 2]; 16]; 16] = [
    // 0 not available
    [
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 1
    [
        [1, 1], [3, 3], [2, 3], [3, 4], [2, 4], [3, 5], [2, 5], [3, 6],
        [2, 6], [3, 7], [2, 7], [3, 8], [2, 8], [3, 9], [2, 9], [1, 9],
    ],
    // 2
    [
        [7, 3], [6, 3], [5, 3], [4, 3], [3, 3], [5, 4], [4, 4], [3, 4],
        [2, 4], [3, 5], [2, 5], [3, 6], [2, 6], [1, 6], [0, 6], [0, 0],
    ],
    // 3
    [
        [5, 4], [7, 3], [6, 3], [5, 3], [4, 4], [3, 4], [4, 3], [3, 3],
        [2, 4], [3, 5], [2, 5], [1, 6], [1, 5], [0, 6], [0, 0], [0, 0],
    ],
    // 4
    [
        [3, 5], [7, 3], [5, 4], [4, 4], [6, 3], [5, 3], [4, 3], [3, 4],
        [3, 3], [2, 4], [2, 5], [1, 5], [0, 5], [0, 0], [0, 0], [0, 0],
    ],
    // 5
    [
        [5, 4], [4, 4], [3, 4], [7, 3], [6, 3], [5, 3], [4, 3], [3, 3],
        [2, 4], [1, 5], [1, 4], [0, 5], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 6
    [
        [1, 6], [1, 5], [7, 3], [6, 3], [5, 3], [4, 3], [3, 3], [2, 3],
        [1, 4], [1, 3], [0, 6], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 7
    [
        [1, 6], [1, 5], [5, 3], [4, 3], [3, 3], [3, 2], [2, 3], [1, 4],
        [1, 3], [0, 6], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 8
    [
        [1, 6], [1, 4], [1, 5], [3, 3], [3, 2], [2, 2], [2, 3], [1, 3],
        [0, 6], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 9
    [
        [1, 6], [0, 6], [1, 4], [3, 2], [2, 2], [1, 3], [1, 2], [1, 5],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 10
    [
        [1, 5], [0, 5], [1, 3], [3, 2], [2, 2], [1, 2], [1, 4], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 11
    [
        [0, 4], [1, 4], [1, 3], [2, 3], [1, 1], [3, 3], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 12
    [
        [0, 4], [1, 4], [1, 2], [1, 1], [1, 3], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 13
    [
        [0, 3], [1, 3], [1, 1], [1, 2], [0, 0], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 14
    [
        [0, 2], [1, 2], [1, 1], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 15
    [
        [0, 1], [1, 1], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
];

/// Total zeros table for 2x2 Chroma DC blocks: `[total_coeff][total_zeros][0--value, 1--bit count]`
/// Dimensions: `[4][4][2]`
pub const g_kuiVlcTotalZerosChromaDc: [[[u8; 2]; 4]; 4] = [
    [[0, 0], [0, 0], [0, 0], [0, 0]],
    [[1, 1], [1, 2], [1, 3], [0, 3]],
    [[1, 1], [1, 2], [0, 2], [0, 0]],
    [[1, 1], [0, 1], [0, 0], [0, 0]],
];

/// Total zeros table for 4:2:2 Chroma DC blocks: `[total_coeff][total_zeros][0--value, 1--bit count]`
/// Dimensions: `[8][8][2]`
pub const g_kuiVlcTotalZerosChromaDc422: [[[u8; 2]; 8]; 8] = [
    [[0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0]],
    [[1, 1], [1, 2], [1, 3], [1, 4], [1, 5], [1, 6], [1, 7], [0, 7]],
    [[7, 3], [6, 3], [5, 3], [4, 3], [3, 3], [2, 3], [1, 3], [0, 3]],
    [[5, 4], [7, 3], [6, 3], [5, 3], [4, 4], [3, 4], [0, 4], [0, 0]],
    [[3, 5], [7, 3], [5, 4], [4, 4], [6, 3], [0, 0], [0, 0], [0, 0]],
    [[5, 4], [4, 4], [3, 4], [7, 3], [0, 0], [0, 0], [0, 0], [0, 0]],
    [[1, 6], [1, 5], [7, 3], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0]],
    [[1, 1], [0, 1], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0]],
];

/// Run before lookup table: `[zeros_left_idx][run_before][0--value, 1--bit count]`
/// Dimensions: `[8][15][2]`
pub const g_kuiVlcRunBefore: [[[u8; 2]; 15]; 8] = [
    // 0 not available
    [
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 1
    [
        [1, 1], [0, 1], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 2
    [
        [1, 1], [1, 2], [0, 2], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 3
    [
        [3, 2], [2, 2], [1, 2], [0, 2], [0, 0], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 4
    [
        [3, 2], [2, 2], [1, 2], [1, 3], [0, 3], [0, 0], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 5
    [
        [3, 2], [2, 2], [3, 3], [2, 3], [1, 3], [0, 3], [0, 0], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // 6
    [
        [3, 2], [0, 3], [1, 3], [3, 3], [2, 3], [5, 3], [4, 3], [0, 0],
        [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    // >6
    [
        [7, 3], [6, 3], [5, 3], [4, 3], [3, 3], [2, 3], [1, 3], [1, 4],
        [1, 5], [1, 6], [1, 7], [1, 8], [1, 9], [1, 10], [1, 11],
    ],
];

// ============================================================================
// Bitstream Helper Functions
// ============================================================================

// The five pointer fields `SBitStringAux` carried are gone from this module. What
// was `WRITE_BE_32(pCurBuf, …); pCurBuf += 4` is now a store into
// `buf[pos..pos + 4]`, which is where the bounds come from: the C++ writer has no
// end-of-buffer check at all, sizing being the caller's contract, so a panic here
// is a pre-existing sizing bug made loud rather than new behaviour on any
// in-contract path (plan §2.2.2). Note the contract includes four bytes of
// headroom at the write position — both the accumulator flush and `BsFlush` store
// a full word even when they advance by one byte.
//
// `InitBits` is **deleted** rather than converted. It declared `kpBuf: *const u8`,
// stored it as `pStartBuf: *mut u8`, and the writer wrote through it, so every
// honest caller produced a pointer with no write provenance and the first
// `BsFlush` was Undefined Behaviour — `phase2_findings.md` F13's third site, the
// one that is a signature lying about what the function does rather than a caller
// mistake. There is nothing to amend: the buffer is now a `&mut [u8]` the caller
// already holds, and the only state left to initialise is `BsWriter::new()`.
//
// `WRITE_BE_32` goes with it; its only callers were the two writer bodies.

/// Write `iLen` bits of `kuiValue` into the bitstream.
#[inline(always)]
pub fn BsWriteBits(buf: &mut [u8], pBs: &mut BsWriter, iLen: i32, kuiValue: u32) -> i32 {
    pBs.write_bits(buf, iLen, kuiValue);
    0
}

/// Write a single bit into the bitstream.
#[inline(always)]
pub fn BsWriteOneBit(buf: &mut [u8], pBs: &mut BsWriter, kuiValue: u32) -> i32 {
    pBs.write_one_bit(buf, kuiValue);
    0
}

/// Flush remaining bits in the 32-bit bit accumulator to the output buffer.
#[inline(always)]
pub fn BsFlush(buf: &mut [u8], pBs: &mut BsWriter) -> i32 {
    pBs.flush(buf);
    0
}

/// Calculate the bit length of an unsigned Exp-Golomb code.
#[inline(always)]
pub fn BsSizeUE(kiValue: u32) -> u32 {
    if kiValue < 256 {
        g_kuiGolombUELength[kiValue as usize]
    } else {
        let mut n: u32 = 0;
        let mut iTmpValue = kiValue + 1;
        if (iTmpValue & 0xffff0000) != 0 {
            iTmpValue >>= 16;
            n += 16;
        }
        if (iTmpValue & 0xff00) != 0 {
            iTmpValue >>= 8;
            n += 8;
        }
        n += g_kuiGolombUELength[(iTmpValue - 1) as usize] >> 1;
        (n << 1) + 1
    }
}

/// Calculate the bit length of a signed Exp-Golomb code.
#[inline(always)]
pub fn BsSizeSE(kiValue: i32) -> u32 {
    if kiValue == 0 {
        1
    } else if kiValue > 0 {
        let iTmpValue = ((kiValue as u32) << 1) - 1;
        BsSizeUE(iTmpValue)
    } else {
        let iTmpValue = ((-kiValue) as u32) << 1;
        BsSizeUE(iTmpValue)
    }
}

/// Write an unsigned Exp-Golomb code (`ue(v)`).
///
/// The C++ takes the code length from `g_kuiGolombUELength` below 256 and from a
/// two-step reduction above it; both compute `2 * floor(log2(value + 1)) + 1`,
/// which is what `BsWriter::write_ue` reaches directly. Differential-proven since
/// Phase 1, and `BsSizeUE` above still spells the table form out for the
/// mode-decision cost functions that want the length without writing anything.
#[inline(always)]
pub fn BsWriteUE(buf: &mut [u8], pBs: &mut BsWriter, kuiValue: u32) -> i32 {
    pBs.write_ue(buf, kuiValue);
    0
}

/// Write a signed Exp-Golomb code (`se(v)`).
#[inline(always)]
pub fn BsWriteSE(buf: &mut [u8], pBs: &mut BsWriter, kiValue: i32) -> i32 {
    pBs.write_se(buf, kiValue);
    0
}

/// Write a truncated Exp-Golomb code (`te(v)`).
#[inline(always)]
pub fn BsWriteTE(buf: &mut [u8], pBs: &mut BsWriter, kiX: i32, kuiValue: u32) {
    pBs.write_te(buf, kiX, kuiValue);
}

/// Get the current bitstream write cursor position in bits.
#[inline(always)]
pub fn BsGetBitsPos(pBs: &BsWriter) -> i32 {
    pBs.bits_pos()
}

/// Write RBSP trailing stop bit and flush to byte alignment.
#[inline(always)]
pub fn BsRbspTrailingBits(buf: &mut [u8], pBs: &mut BsWriter) -> i32 {
    pBs.rbsp_trailing_bits(buf);
    0
}

/// Align bitstream to byte boundary, padding with one bits.
#[inline(always)]
pub fn BsAlign(buf: &mut [u8], pBs: &mut BsWriter) {
    pBs.align(buf);
}

// ============================================================================
// Inlined CAVLC Serializers (from `codec/encoder/core/inc/vlc_encoder.h`)
// ============================================================================

/// Write coeff_token for Luma and Chroma AC residual blocks.
#[inline(always)]
pub fn WriteTotalCoeffTrailingones(
    buf: &mut [u8],
    pBs: &mut BsWriter,
    uiNc: u8,
    uiTotalCoeff: u8,
    uiTrailingOnes: u8,
) -> i32 {
    let kuiNcIdx = g_kuiEncNcMapTable[uiNc as usize] as usize;
    let kpCoeffToken =
        &g_kuiVlcCoeffToken[kuiNcIdx][uiTotalCoeff as usize][uiTrailingOnes as usize];
    BsWriteBits(buf, pBs, kpCoeffToken[1] as i32, kpCoeffToken[0] as u32)
}

/// Write coeff_token for 2x2 Chroma DC residual blocks.
#[inline(always)]
pub fn WriteTotalcoeffTrailingonesChroma(
    buf: &mut [u8],
    pBs: &mut BsWriter,
    uiTotalCoeff: u8,
    uiTrailingOnes: u8,
) -> i32 {
    let kpCoeffToken = &g_kuiVlcCoeffToken[4][uiTotalCoeff as usize][uiTrailingOnes as usize];
    BsWriteBits(buf, pBs, kpCoeffToken[1] as i32, kpCoeffToken[0] as u32)
}

/// Write level_prefix unary codeword.
#[inline(always)]
pub fn WriteLevelPrefix(buf: &mut [u8], pBs: &mut BsWriter, kuiZeroCount: u32) -> i32 {
    BsWriteBits(buf, pBs, (kuiZeroCount + 1) as i32, 1);
    0
}

/// Write total_zeros for 4x4 residual blocks.
#[inline(always)]
pub fn WriteTotalZeros(
    buf: &mut [u8],
    pBs: &mut BsWriter,
    uiTotalCoeff: u32,
    uiTotalZeros: u32,
) -> i32 {
    let kpTotalZeros = &g_kuiVlcTotalZeros[uiTotalCoeff as usize][uiTotalZeros as usize];
    BsWriteBits(buf, pBs, kpTotalZeros[1] as i32, kpTotalZeros[0] as u32)
}

/// Write total_zeros for 2x2 Chroma DC blocks.
#[inline(always)]
pub fn WriteTotalZerosChromaDc(
    buf: &mut [u8],
    pBs: &mut BsWriter,
    uiTotalCoeff: u32,
    uiTotalZeros: u32,
) -> i32 {
    let kpTotalZerosChromaDc =
        &g_kuiVlcTotalZerosChromaDc[uiTotalCoeff as usize][uiTotalZeros as usize];
    BsWriteBits(buf, pBs, kpTotalZerosChromaDc[1] as i32, kpTotalZerosChromaDc[0] as u32)
}

/// Write run_before zero run length.
#[inline(always)]
pub fn WriteRunBefore(
    buf: &mut [u8],
    pBs: &mut BsWriter,
    uiZeroLeft: u8,
    uiRunBefore: u8,
) -> i32 {
    let kpRunBefore = &g_kuiVlcRunBefore[uiZeroLeft as usize][uiRunBefore as usize];
    BsWriteBits(buf, pBs, kpRunBefore[1] as i32, kpRunBefore[0] as u32)
}

// ============================================================================
// CAVLC Parameter Extraction and Block Residual Serialization
// ============================================================================

/// C-reference parameter extraction kernel for CAVLC.
/// Scans quantized coefficients in reverse zigzag order, isolating non-zero levels and zero runs.
pub unsafe extern "C" fn CavlcParamCal_c(
    pCoffLevel: *const i16,
    pRun: *mut u8,
    pLevel: *mut i16,
    pTotalCoeff: *mut i32,
    mut iLastIndex: i32,
) -> i32 {
    unsafe {
        let mut iTotalZeros = 0i32;
        let mut iTotalCoeffs = 0i32;

        while iLastIndex >= 0 && *pCoffLevel.add(iLastIndex as usize) == 0 {
            iLastIndex -= 1;
        }

        while iLastIndex >= 0 {
            let mut iCountZero = 0i32;
            *pLevel.add(iTotalCoeffs as usize) = *pCoffLevel.add(iLastIndex as usize);
            iLastIndex -= 1;

            while iLastIndex >= 0 && *pCoffLevel.add(iLastIndex as usize) == 0 {
                iCountZero += 1;
                iLastIndex -= 1;
            }
            iTotalZeros += iCountZero;
            *pRun.add(iTotalCoeffs as usize) = iCountZero as u8;
            iTotalCoeffs += 1;
        }

        *pTotalCoeff = iTotalCoeffs;
        iTotalZeros
    }
}

// `WriteBlockResidualCavlc` is deliberately NOT defined here. The live
// definition is svc_set_mb_syn_cavlc.rs:299, where all ten call sites resolve
// and which every CAVLC sweep exercises. A second, longer copy sat here with no
// caller at all until Phase 5.4 removed it -- see the --dups audit in
// rust/docs/encoder_port_status.md.
