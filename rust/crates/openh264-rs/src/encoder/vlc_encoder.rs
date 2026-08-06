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

pub const CHROMA_DC_NC_OFFSET: usize = 17;
pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_VLCOVERFLOWFOUND: i32 = 0x40;

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

/// Bitstream auxiliary state for CAVLC/Exp-Golomb serialization.
///
/// Single definition in [`crate::common::wels_common_defs`] — `SBitStringAux` is a
/// common-layer type (`codec/common/inc/wels_common_defs.h:232`), not an encoder one.
pub use crate::common::wels_common_defs::{PBitStringAux, SBitStringAux, TagBitStringAux};

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
#[repr(align(16))]
pub struct EncNcMapTable(pub [u8; 18]);

pub const g_kuiEncNcMapTable: [u8; 18] = [
    0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4,
];

/// Mapping table for zerosLeft context clamping (0..7).
/// Matches `g_kuiZeroLeftMap[16]` in `codec/encoder/core/src/set_mb_syn_cavlc.cpp`.
pub const g_kuiZeroLeftMap: [u8; 16] = [
    0, 1, 2, 3, 4, 5, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7,
];

/// Unsigned Exp-Golomb bit length lookup table.
pub const g_kuiGolombUELength: &[u32] = &[
    1, 3, 3, 5, 5, 5, 5, 7, 7, 7, 7, 7, 7, 7, 7, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 17,
];

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

/// Write a 32-bit big-endian integer to memory buffer.
#[inline(always)]
pub unsafe fn WRITE_BE_32(ptr: *mut u8, val: u32) {
    unsafe {
        *ptr.add(0) = (val >> 24) as u8;
        *ptr.add(1) = (val >> 16) as u8;
        *ptr.add(2) = (val >> 8) as u8;
        *ptr.add(3) = val as u8;
    }
}

/// Initialize bitstream writing auxiliary structure.
#[inline(always)]
pub unsafe fn InitBits(pBs: *mut SBitStringAux, kpBuf: *const u8, kiSize: i32) -> i32 {
    unsafe {
        let ptr = kpBuf as *mut u8;
        (*pBs).pStartBuf = ptr;
        (*pBs).pCurBuf = ptr;
        (*pBs).pEndBuf = ptr.add(kiSize as usize);
        (*pBs).iLeftBits = 32;
        (*pBs).uiCurBits = 0;
        kiSize
    }
}

/// Write `iLen` bits of `kuiValue` into the bitstream.
#[inline(always)]
pub unsafe fn BsWriteBits(pBs: *mut SBitStringAux, mut iLen: i32, kuiValue: u32) -> i32 {
    unsafe {
        if iLen < (*pBs).iLeftBits {
            (*pBs).uiCurBits = ((*pBs).uiCurBits << iLen) | kuiValue;
            (*pBs).iLeftBits -= iLen;
        } else {
            iLen -= (*pBs).iLeftBits;
            (*pBs).uiCurBits = ((*pBs).uiCurBits << (*pBs).iLeftBits) | (kuiValue >> iLen);
            WRITE_BE_32((*pBs).pCurBuf, (*pBs).uiCurBits);
            (*pBs).pCurBuf = (*pBs).pCurBuf.add(4);
            (*pBs).uiCurBits = kuiValue & ((1u32 << iLen) - 1);
            (*pBs).iLeftBits = 32 - iLen;
        }
        0
    }
}

/// Write a single bit into the bitstream.
#[inline(always)]
pub unsafe fn BsWriteOneBit(pBs: *mut SBitStringAux, kuiValue: u32) -> i32 {
    unsafe {
        BsWriteBits(pBs, 1, kuiValue);
        0
    }
}

/// Flush remaining bits in the 32-bit bit accumulator to the output buffer.
#[inline(always)]
pub unsafe fn BsFlush(pBs: *mut SBitStringAux) -> i32 {
    unsafe {
        if (*pBs).iLeftBits < 32 {
            WRITE_BE_32((*pBs).pCurBuf, (*pBs).uiCurBits << (*pBs).iLeftBits);
            (*pBs).pCurBuf = (*pBs).pCurBuf.add(4 - ((*pBs).iLeftBits as usize / 8));
            (*pBs).iLeftBits = 32;
            (*pBs).uiCurBits = 0;
        }
        0
    }
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
#[inline(always)]
pub unsafe fn BsWriteUE(pBs: *mut SBitStringAux, kuiValue: u32) -> i32 {
    unsafe {
        let mut iTmpValue = kuiValue + 1;
        if kuiValue < 256 {
            BsWriteBits(
                pBs,
                g_kuiGolombUELength[kuiValue as usize] as i32,
                kuiValue + 1,
            );
        } else {
            let mut n: u32 = 0;
            if (iTmpValue & 0xffff0000) != 0 {
                iTmpValue >>= 16;
                n += 16;
            }
            if (iTmpValue & 0xff00) != 0 {
                iTmpValue >>= 8;
                n += 8;
            }
            n += g_kuiGolombUELength[(iTmpValue - 1) as usize] >> 1;
            BsWriteBits(pBs, ((n << 1) + 1) as i32, kuiValue + 1);
        }
        0
    }
}

/// Write a signed Exp-Golomb code (`se(v)`).
#[inline(always)]
pub unsafe fn BsWriteSE(pBs: *mut SBitStringAux, kiValue: i32) -> i32 {
    unsafe {
        if kiValue == 0 {
            BsWriteOneBit(pBs, 1);
        } else if kiValue > 0 {
            let iTmpValue = ((kiValue as u32) << 1) - 1;
            BsWriteUE(pBs, iTmpValue);
        } else {
            let iTmpValue = ((-kiValue) as u32) << 1;
            BsWriteUE(pBs, iTmpValue);
        }
        0
    }
}

/// Write a truncated Exp-Golomb code (`te(v)`).
#[inline(always)]
pub unsafe fn BsWriteTE(pBs: *mut SBitStringAux, kiX: i32, kuiValue: u32) {
    unsafe {
        if kiX == 1 {
            BsWriteOneBit(pBs, if kuiValue == 0 { 1 } else { 0 });
        } else {
            BsWriteUE(pBs, kuiValue);
        }
    }
}

/// Get the current bitstream write cursor position in bits.
#[inline(always)]
pub unsafe fn BsGetBitsPos(pBs: *const SBitStringAux) -> i32 {
    unsafe {
        ((((*pBs).pCurBuf as isize - (*pBs).pStartBuf as isize) as i32) << 3) + 32 - (*pBs).iLeftBits
    }
}

/// Write RBSP trailing stop bit and flush to byte alignment.
#[inline(always)]
pub unsafe fn BsRbspTrailingBits(pBs: *mut SBitStringAux) -> i32 {
    unsafe {
        BsWriteOneBit(pBs, 1);
        BsFlush(pBs);
        0
    }
}

/// Align bitstream to byte boundary.
#[inline(always)]
pub unsafe fn BsAlign(pBs: *mut SBitStringAux) {
    unsafe {
        let rem = (*pBs).iLeftBits & 7;
        if rem != 0 {
            (*pBs).uiCurBits <<= rem;
            (*pBs).uiCurBits |= (1 << rem) - 1;
            (*pBs).iLeftBits &= !7;
        }
        BsFlush(pBs);
    }
}

// ============================================================================
// Inlined CAVLC Serializers (from `codec/encoder/core/inc/vlc_encoder.h`)
// ============================================================================

/// Write coeff_token for Luma and Chroma AC residual blocks.
#[inline(always)]
pub unsafe fn WriteTotalCoeffTrailingones(
    pBs: *mut SBitStringAux,
    uiNc: u8,
    uiTotalCoeff: u8,
    uiTrailingOnes: u8,
) -> i32 {
    unsafe {
        let kuiNcIdx = g_kuiEncNcMapTable[uiNc as usize] as usize;
        let kpCoeffToken =
            &g_kuiVlcCoeffToken[kuiNcIdx][uiTotalCoeff as usize][uiTrailingOnes as usize];
        BsWriteBits(pBs, kpCoeffToken[1] as i32, kpCoeffToken[0] as u32)
    }
}

/// Write coeff_token for 2x2 Chroma DC residual blocks.
#[inline(always)]
pub unsafe fn WriteTotalcoeffTrailingonesChroma(
    pBs: *mut SBitStringAux,
    uiTotalCoeff: u8,
    uiTrailingOnes: u8,
) -> i32 {
    unsafe {
        let kpCoeffToken = &g_kuiVlcCoeffToken[4][uiTotalCoeff as usize][uiTrailingOnes as usize];
        BsWriteBits(pBs, kpCoeffToken[1] as i32, kpCoeffToken[0] as u32)
    }
}

/// Write level_prefix unary codeword.
#[inline(always)]
pub unsafe fn WriteLevelPrefix(pBs: *mut SBitStringAux, kuiZeroCount: u32) -> i32 {
    unsafe {
        BsWriteBits(pBs, (kuiZeroCount + 1) as i32, 1);
        0
    }
}

/// Write total_zeros for 4x4 residual blocks.
#[inline(always)]
pub unsafe fn WriteTotalZeros(
    pBs: *mut SBitStringAux,
    uiTotalCoeff: u32,
    uiTotalZeros: u32,
) -> i32 {
    unsafe {
        let kpTotalZeros = &g_kuiVlcTotalZeros[uiTotalCoeff as usize][uiTotalZeros as usize];
        BsWriteBits(pBs, kpTotalZeros[1] as i32, kpTotalZeros[0] as u32)
    }
}

/// Write total_zeros for 2x2 Chroma DC blocks.
#[inline(always)]
pub unsafe fn WriteTotalZerosChromaDc(
    pBs: *mut SBitStringAux,
    uiTotalCoeff: u32,
    uiTotalZeros: u32,
) -> i32 {
    unsafe {
        let kpTotalZerosChromaDc =
            &g_kuiVlcTotalZerosChromaDc[uiTotalCoeff as usize][uiTotalZeros as usize];
        BsWriteBits(pBs, kpTotalZerosChromaDc[1] as i32, kpTotalZerosChromaDc[0] as u32)
    }
}

/// Write run_before zero run length.
#[inline(always)]
pub unsafe fn WriteRunBefore(
    pBs: *mut SBitStringAux,
    uiZeroLeft: u8,
    uiRunBefore: u8,
) -> i32 {
    unsafe {
        let kpRunBefore = &g_kuiVlcRunBefore[uiZeroLeft as usize][uiRunBefore as usize];
        BsWriteBits(pBs, kpRunBefore[1] as i32, kpRunBefore[0] as u32)
    }
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

/// Serialize a 4x4 transform residual block via CAVLC.
/// Matches `WriteBlockResidualCavlc` in `codec/encoder/core/src/set_mb_syn_cavlc.cpp`.
pub unsafe fn WriteBlockResidualCavlc(
    pfCavlcParamCal: Option<unsafe extern "C" fn(*const i16, *mut u8, *mut i16, *mut i32, i32) -> i32>,
    pCoffLevel: *mut i16,
    iEndIdx: i32,
    iCalRunLevelFlag: i32,
    iResidualProperty: i32,
    iNC: i8,
    pBs: *mut SBitStringAux,
) -> i32 {
    unsafe {
        let mut iLevel = [0i16; 16];
        let mut uiRun = [0u8; 16];

        let mut iTotalCoeffs: i32 = 0;
        let mut iTrailingOnes: i32 = 0;
        let mut iTotalZeros: i32 = 0;
        let mut iZerosLeft: i32;
        let mut uiSign: u32 = 0;
        let mut iLevelCode: i32;
        let mut iLevelPrefix: i32;
        let mut iLevelSuffix: i32;
        let mut uiSuffixLength: i32;
        let mut iLevelSuffixSize: i32;
        let mut iValue: i32;
        let mut iThreshold: i32;
        let mut iZeroLeft: i32;
        let mut n: i32;

        let mut pBufPtr = (*pBs).pCurBuf;
        let mut uiCurBits = (*pBs).uiCurBits;
        let mut iLeftBits = (*pBs).iLeftBits;

        macro_rules! cavlc_bs_write {
            ($n:expr, $v:expr) => {{
                let mut _n = $n;
                let _v = $v as u32;
                if _n < iLeftBits {
                    uiCurBits = (uiCurBits << _n) | _v;
                    iLeftBits -= _n;
                } else {
                    _n -= iLeftBits;
                    uiCurBits = (uiCurBits << iLeftBits) | (_v >> _n);
                    WRITE_BE_32(pBufPtr, uiCurBits);
                    pBufPtr = pBufPtr.add(4);
                    uiCurBits = _v & ((1u32 << _n) - 1);
                    iLeftBits = 32 - _n;
                }
            }};
        }

        if iCalRunLevelFlag != 0 {
            let cal_fn = pfCavlcParamCal.unwrap_or(CavlcParamCal_c);
            iTotalZeros = cal_fn(
                pCoffLevel,
                uiRun.as_mut_ptr(),
                iLevel.as_mut_ptr(),
                &mut iTotalCoeffs,
                iEndIdx,
            );
            let iCount = if iTotalCoeffs > 3 { 3 } else { iTotalCoeffs };
            for i in 0..iCount {
                if iLevel[i as usize].abs() == 1 {
                    iTrailingOnes += 1;
                    uiSign <<= 1;
                    if iLevel[i as usize] < 0 {
                        uiSign |= 1;
                    }
                } else {
                    break;
                }
            }
        }

        let nc_idx = g_kuiEncNcMapTable[iNC as usize] as usize;
        let upCoeffToken =
            &g_kuiVlcCoeffToken[nc_idx][iTotalCoeffs as usize][iTrailingOnes as usize];
        iValue = upCoeffToken[0] as i32;
        n = upCoeffToken[1] as i32;

        if iTotalCoeffs == 0 {
            cavlc_bs_write!(n, iValue);
            (*pBs).pCurBuf = pBufPtr;
            (*pBs).uiCurBits = uiCurBits;
            (*pBs).iLeftBits = iLeftBits;
            return ENC_RETURN_SUCCESS;
        }

        n += iTrailingOnes;
        iValue = (iValue << iTrailingOnes) + (uiSign as i32);
        cavlc_bs_write!(n, iValue);

        uiSuffixLength = if iTotalCoeffs > 10 && iTrailingOnes < 3 { 1 } else { 0 };

        for i in iTrailingOnes..iTotalCoeffs {
            let iVal = iLevel[i as usize] as i32;
            iLevelCode = (iVal - 1) * 2;
            let sign_bit = ((iLevelCode as u32) >> 31) as i32;
            iLevelCode = (iLevelCode ^ sign_bit) + (sign_bit << 1);
            if i == iTrailingOnes && iTrailingOnes < 3 {
                iLevelCode -= 2;
            }

            iLevelPrefix = iLevelCode >> uiSuffixLength;
            iLevelSuffixSize = uiSuffixLength;
            iLevelSuffix = iLevelCode - (iLevelPrefix << uiSuffixLength);

            if iLevelPrefix >= 14 && iLevelPrefix < 30 && uiSuffixLength == 0 {
                iLevelPrefix = 14;
                iLevelSuffix = iLevelCode - iLevelPrefix;
                iLevelSuffixSize = 4;
            } else if iLevelPrefix >= 15 {
                iLevelPrefix = 15;
                iLevelSuffix = iLevelCode - (iLevelPrefix << uiSuffixLength);
                if (iLevelSuffix >> 11) != 0 {
                    return ENC_RETURN_VLCOVERFLOWFOUND;
                }
                if uiSuffixLength == 0 {
                    iLevelSuffix -= 15;
                }
                iLevelSuffixSize = 12;
            }

            n = iLevelPrefix + 1 + iLevelSuffixSize;
            iValue = (1 << iLevelSuffixSize) | iLevelSuffix;
            cavlc_bs_write!(n, iValue);

            if uiSuffixLength == 0 {
                uiSuffixLength += 1;
            }
            iThreshold = 3 << (uiSuffixLength - 1);
            if (iVal > iThreshold || iVal < -iThreshold) && uiSuffixLength < 6 {
                uiSuffixLength += 1;
            }
        }

        if iTotalCoeffs < iEndIdx + 1 {
            if iResidualProperty != ECtxBlockCat::CHROMA_DC as i32 {
                let upTotalZeros =
                    &g_kuiVlcTotalZeros[iTotalCoeffs as usize][iTotalZeros as usize];
                n = upTotalZeros[1] as i32;
                iValue = upTotalZeros[0] as i32;
                cavlc_bs_write!(n, iValue);
            } else {
                let upTotalZeros =
                    &g_kuiVlcTotalZerosChromaDc[iTotalCoeffs as usize][iTotalZeros as usize];
                n = upTotalZeros[1] as i32;
                iValue = upTotalZeros[0] as i32;
                cavlc_bs_write!(n, iValue);
            }
        }

        iZerosLeft = iTotalZeros;
        let mut i = 0;
        while i + 1 < iTotalCoeffs && iZerosLeft > 0 {
            let uirun = uiRun[i as usize];
            iZeroLeft = g_kuiZeroLeftMap[iZerosLeft as usize] as i32;
            n = g_kuiVlcRunBefore[iZeroLeft as usize][uirun as usize][1] as i32;
            iValue = g_kuiVlcRunBefore[iZeroLeft as usize][uirun as usize][0] as i32;
            cavlc_bs_write!(n, iValue);
            iZerosLeft -= uirun as i32;
            i += 1;
        }

        (*pBs).pCurBuf = pBufPtr;
        (*pBs).uiCurBits = uiCurBits;
        (*pBs).iLeftBits = iLeftBits;
        ENC_RETURN_SUCCESS
    }
}
