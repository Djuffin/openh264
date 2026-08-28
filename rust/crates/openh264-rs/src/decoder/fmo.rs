// Copyright (c) 2009-2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
//    * Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//
//    * Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in
//      the documentation and/or other materials provided with the
//      distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

//! # OpenH264 Decoder: Flexible Macroblock Ordering (FMO)
//!
//! Translated from `codec/decoder/core/inc/fmo.h` and `codec/decoder/core/src/fmo.cpp`.
//!
//! Flexible Macroblock Ordering (FMO) partitions the macroblock grid into up to 8 slice
//! groups to provide spatial error resilience over lossy packet networks. This module implements:
//! - Allocation and lifecycle management of the macroblock allocation map (`pMbAllocMap`).
//! - Interleaved slice group map generation (Type 0).
//! - Dispersed checkerboard slice group map generation (Type 1).
//! - Parameter set change detection (`FmoParamSetsChanged`) and synchronization (`FmoParamUpdate`).
//! - Fast O(1) macroblock-to-slice-group queries (`FmoMbToSliceGroup`) and sequential iterators (`FmoNextMb`).

#![deny(unsafe_code)]
// Phase 5 W6 (T5.V4). The module's nine `unsafe fn` were all the same shape — a raw
// pointer, a null test at the top, and a body that dereferences it — so the
// conversion is `Option<&TagFmo>`/`Option<&mut TagFmo>` and the null test stays
// exactly where it was, spelled as a `let … else`. `UninitFmoList` walked
// `pFmo.add(i)` over an array and takes `&mut [TagFmo]`; its `kiCnt` was the array's
// length, which a slice carries.
//
// `pMa: *mut CMemoryAlign` left four signatures as the dead parameter it had been
// since T5.R3 deleted the allocator helpers — nothing in any of these bodies had read
// it since.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]
#![forbid(unsafe_code)]

use core::ffi::{c_char, c_void};
use crate::decoder::parameter_sets::{SPps, SSps};

// ============================================================================
// Constants, Types & Error Codes
// ============================================================================

/// Macroblock linear 1D raster index type.
pub type MB_XY_T = i32;

/// Maximum number of slice groups allowed per Picture Parameter Set in H.264.
pub const MAX_SLICEGROUP_IDS: u32 = 8;

/// Maximum number of Picture Parameter Sets supported in the decoder context.
pub const MAX_PPS_COUNT: i32 = 256;

// Error codes matching OpenH264 decoder specifications.
pub const ERR_NONE: i32 = 0;
pub const ERR_INFO_OUT_OF_MEMORY: i32 = 1;
pub const ERR_INFO_INVALID_PARAM: i32 = 4;
pub const ERR_INFO_UNSUPPORTED_FMOTYPE: i32 = 1063;

/// Flexible Macroblock Ordering (FMO) context structure.
///
/// **T5.R3: the map is owned.** It was the last `WelsMallocz`/`WelsFree` pair in
/// `src/decoder/`, allocated per parameter set and freed by `UninitFmoList` walking an
/// array of these — which is exactly the shape a `Vec` makes unforgettable. The struct
/// loses `Copy` with the raw pointer (a `Vec` field cannot be bitwise-copied) and
/// `sFmoList`'s 256 entries are therefore written at construction rather than left to
/// the context's zeroed shell (S21).
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TagFmo {
    /// Slice group ID per macroblock, `iCountMbNum` long — empty when there is no map,
    /// which is the state the old null pointer named.
    pub pMbAllocMap: Vec<u8>,
    /// Total number of macroblocks in the active picture (iMbWidth * iMbHeight).
    pub iCountMbNum: i32,
    /// Number of slice groups configured for this FMO context (1..8).
    pub iSliceGroupCount: i32,
    /// H.264 slice group map type (0 = Interleaved, 1 = Dispersed, etc.).
    pub iSliceGroupType: i32,
    /// Flag indicating whether this FMO context instance is active.
    pub bActiveFlag: bool,
    /// Padding bytes to maintain 32-bit/64-bit alignment.
    pub uiReserved: [u8; 3],
}

pub type SFmo = TagFmo;

impl Default for TagFmo {
    fn default() -> Self {
        Self {
            pMbAllocMap: Vec::new(),
            iCountMbNum: 0,
            iSliceGroupCount: 0,
            iSliceGroupType: -1,
            bActiveFlag: false,
            uiReserved: [0; 3],
        }
    }
}

// ============================================================================
// Internal Memory Helpers
// ============================================================================

// T5.R3: `free_mb_alloc_map` and `mallocz_mb_alloc_map` stood here — the module's
// half of the decoder's last `WelsFree`/`WelsMallocz` pair, each with its own
// `pMa`-or-global arm and its own allocation tag. A `Vec<u8>` is the allocation, the
// zeroing and the free, so there is nothing left for either helper to do.

// ============================================================================
// Core FMO Map Generation Routines
// ============================================================================

/// Generates the macroblock allocation map for Interleaved Slice Groups (Type 0).
///
pub fn FmoGenerateMbAllocMapType0(pFmo: &mut TagFmo, pPps: &SPps) -> i32 {
    let uiNumSliceGroups = (*pPps).uiNumSliceGroups;
    let iMbNum = (*pFmo).iCountMbNum;
    let pMbAllocMap = &mut (*pFmo).pMbAllocMap;

    if pMbAllocMap.is_empty()
        || iMbNum <= 0
        || uiNumSliceGroups == 0
        || uiNumSliceGroups > MAX_SLICEGROUP_IDS
    {
        return ERR_INFO_INVALID_PARAM;
    }

    let mut i: i32 = 0;
    while i < iMbNum {
        let mut uiGroup: u8 = 0;
        while (uiGroup as u32) < uiNumSliceGroups && i < iMbNum {
            let kiRunIdx = (*pPps).uiRunLength[uiGroup as usize] as i32;
            let mut j: i32 = 0;
            loop {
                if (i + j) < iMbNum {
                    pMbAllocMap[(i + j) as usize] = uiGroup;
                }
                j += 1;
                if !(j < kiRunIdx && (i + j) < iMbNum) {
                    break;
                }
            }
            if kiRunIdx > 0 {
                i += kiRunIdx;
            } else {
                i += j;
            }
            uiGroup += 1;
        }
    }

    ERR_NONE
}

/// Generates the macroblock allocation map for Dispersed Slice Groups (Type 1).
///
pub fn FmoGenerateMbAllocMapType1(pFmo: &mut TagFmo, pPps: &SPps, kiMbWidth: i32) -> i32 {
    let uiNumSliceGroups = (*pPps).uiNumSliceGroups;
    let iMbNum = (*pFmo).iCountMbNum;
    let pMbAllocMap = &mut (*pFmo).pMbAllocMap;

    if pMbAllocMap.is_empty()
        || iMbNum <= 0
        || kiMbWidth <= 0
        || uiNumSliceGroups == 0
        || uiNumSliceGroups > MAX_SLICEGROUP_IDS
    {
        return ERR_INFO_INVALID_PARAM;
    }

    let mut i: i32 = 0;
    while i < iMbNum {
        let col = (i % kiMbWidth) as u32;
        let row = (i / kiMbWidth) as u32;
        let val = (col + ((row * uiNumSliceGroups) >> 1)) % uiNumSliceGroups;
        pMbAllocMap[i as usize] = val as u8;
        i += 1;
    }

    ERR_NONE
}

/// Internal helper allocating `pMbAllocMap` and dispatching map generation according to PPS parameters.
///
pub fn FmoGenerateSliceGroup(
    pFmo: Option<&mut TagFmo>,
    kpPps: Option<&SPps>,
    kiMbWidth: i32,
    kiMbHeight: i32,
) -> i32 {
    let (Some(pFmo), Some(kpPps)) = (pFmo, kpPps) else {
        return ERR_INFO_INVALID_PARAM;
    };

    let iNumMb = kiMbWidth * kiMbHeight;
    if iNumMb <= 0 {
        return ERR_INFO_INVALID_PARAM;
    }

    // The free-then-allocate pair the two deleted helpers were: assigning the new map
    // drops the old one, and `vec![0; n]` is `WelsMallocz`'s zeroing. The
    // `ERR_INFO_OUT_OF_MEMORY` arm goes with the null return it tested for — the same
    // argument T5.H3 made for the layer's grid.
    (*pFmo).pMbAllocMap = vec![0u8; iNumMb as usize];

    (*pFmo).iCountMbNum = iNumMb;

    if (*kpPps).uiNumSliceGroups < 2 && iNumMb > 0 {
        // The C's `memset(pMbAllocMap, 0, iNumMb)` on this arm, kept where it stood
        // even though the allocation above already zeroed: it is what the single
        // slice-group map *is*, not a leftover of the allocator.
        (*pFmo).pMbAllocMap.fill(0);
        (*pFmo).iSliceGroupCount = 1;
        return ERR_NONE;
    }

    let mut iErr: i32 = 0;
    let bResolutionChanged = false;

    if bResolutionChanged
        || ((*kpPps).uiSliceGroupMapType as i32 != (*pFmo).iSliceGroupType)
        || ((*kpPps).uiNumSliceGroups as i32 != (*pFmo).iSliceGroupCount)
    {
        match (*kpPps).uiSliceGroupMapType {
            0 => {
                iErr = FmoGenerateMbAllocMapType0(pFmo, kpPps);
            }
            1 => {
                iErr = FmoGenerateMbAllocMapType1(pFmo, kpPps, kiMbWidth);
            }
            2..=6 => {
                // Reserved for other slice group types
                iErr = 1;
            }
            _ => {
                return ERR_INFO_UNSUPPORTED_FMOTYPE;
            }
        }
    }

    if 0 == iErr {
        (*pFmo).iSliceGroupCount = (*kpPps).uiNumSliceGroups as i32;
        (*pFmo).iSliceGroupType = (*kpPps).uiSliceGroupMapType as i32;
    }

    iErr
}

// ============================================================================
// Public FMO API Functions
// ============================================================================

/// Initializes a Flexible Macroblock Ordering (FMO) context.
///
pub fn InitFmo(
    pFmo: Option<&mut TagFmo>,
    pPps: Option<&SPps>,
    kiMbWidth: i32,
    kiMbHeight: i32,
) -> i32 {
    FmoGenerateSliceGroup(pFmo, pPps, kiMbWidth, kiMbHeight)
}

/// Frees all dynamically allocated memory across the FMO context array.
///
/// The C's `kiCnt` is the array's length and `pFmo` its base, so the pair is one
/// slice; `kiCnt < kiAvail` — a count smaller than the number of entries to clear —
/// is the same refusal it always was, now spelled against `pFmo.len()`.
pub fn UninitFmoList(pFmo: &mut [TagFmo], kiAvail: i32) {
    let kiCnt = pFmo.len() as i32;
    if kiAvail <= 0 || kiCnt < kiAvail {
        return;
    }

    let mut iFreeNodes: i32 = 0;
    for pIter in pFmo.iter_mut() {
        if pIter.bActiveFlag {
            // T5.R3: the free is the assignment, and it needs neither the allocator
            // nor a null test — an already-empty map drops nothing.
            pIter.pMbAllocMap = Vec::new();
            pIter.iSliceGroupCount = 0;
            pIter.iSliceGroupType = -1;
            pIter.iCountMbNum = 0;
            pIter.bActiveFlag = false;
            iFreeNodes += 1;
            if iFreeNodes >= kiAvail {
                break;
            }
        }
    }
}

/// Detects whether SPS or PPS parameter sets have changed relative to the cached FMO state.
///
pub fn FmoParamSetsChanged(
    pFmo: Option<&TagFmo>,
    kiCountNumMb: i32,
    kiSliceGroupType: i32,
    kiSliceGroupCount: i32,
) -> bool {
    let Some(pFmo) = pFmo else {
        return false;
    };
    !(*pFmo).bActiveFlag
        || (kiCountNumMb != (*pFmo).iCountMbNum)
        || (kiSliceGroupType != (*pFmo).iSliceGroupType)
        || (kiSliceGroupCount != (*pFmo).iSliceGroupCount)
}

/// Updates/inserts an FMO parameter unit for the active access unit.
///
/// # Safety
/// Pointers must be valid or null.
pub fn FmoParamUpdate(
    pFmo: Option<&mut TagFmo>,
    pSps: Option<&SSps>,
    pPps: Option<&SPps>,
    pActiveFmoNum: &mut i32,
) -> i32 {
    let (Some(pFmo), Some(pSps), Some(pPps)) = (pFmo, pSps, pPps) else {
        return ERR_INFO_INVALID_PARAM;
    };

    let kuiMbWidth = pSps.iMbWidth;
    let kuiMbHeight = pSps.iMbHeight;
    let mut iRet = ERR_NONE;

    if FmoParamSetsChanged(
        Some(pFmo),
        (kuiMbWidth * kuiMbHeight) as i32,
        pPps.uiSliceGroupMapType as i32,
        pPps.uiNumSliceGroups as i32,
    ) {
        iRet = InitFmo(Some(pFmo), Some(pPps), kuiMbWidth as i32, kuiMbHeight as i32);
        if iRet != ERR_NONE {
            return iRet;
        }

        if !pFmo.bActiveFlag && *pActiveFmoNum < MAX_PPS_COUNT {
            *pActiveFmoNum += 1;
            pFmo.bActiveFlag = true;
        }
    }

    iRet
}

/// Converts a linear macroblock index (`kiMbXy`) to its corresponding slice group ID.
///
pub fn FmoMbToSliceGroup(pFmo: Option<&TagFmo>, kiMbXy: MB_XY_T) -> i32 {
    let Some(pFmo) = pFmo else {
        return -1;
    };

    let kiMbNum = (*pFmo).iCountMbNum;
    let kpMbMap = &(*pFmo).pMbAllocMap;

    if kiMbXy < 0 || kiMbXy >= kiMbNum || kpMbMap.is_empty() {
        return -1;
    }

    kpMbMap[kiMbXy as usize] as i32
}

/// Returns the next successive macroblock in raster sequence belonging to the same slice group.
///
pub fn FmoNextMb(pFmo: Option<&TagFmo>, kiMbXy: MB_XY_T) -> MB_XY_T {
    let Some(pFmo) = pFmo else {
        return -1;
    };

    let kiTotalMb = (*pFmo).iCountMbNum;
    let kpMbMap = &(*pFmo).pMbAllocMap;
    if kpMbMap.is_empty() {
        return -1;
    }

    let iGroup = FmoMbToSliceGroup(Some(pFmo), kiMbXy);
    if iGroup < 0 {
        return -1;
    }
    let kuiSliceGroupIdc = iGroup as u8;

    let mut iNextMb: MB_XY_T = kiMbXy;
    loop {
        iNextMb += 1;
        if iNextMb >= kiTotalMb {
            iNextMb = -1;
            break;
        }
        if kpMbMap[iNextMb as usize] == kuiSliceGroupIdc {
            break;
        }
    }

    iNextMb
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
        use crate::decoder::parameter_sets::{SPps, SSps};

    #[test]
    fn test_fmo_default_and_types() {
        let fmo = TagFmo::default();
        assert!(fmo.pMbAllocMap.is_empty());
        assert_eq!(fmo.iCountMbNum, 0);
        assert_eq!(fmo.iSliceGroupCount, 0);
        assert_eq!(fmo.iSliceGroupType, -1);
        assert!(!fmo.bActiveFlag);
    }

    #[test]
    fn test_fmo_single_slice_group() {
        let mut fmo = TagFmo::default();
        let mut pps = SPps::default();
        pps.uiNumSliceGroups = 1;

        let ret = InitFmo(Some(&mut fmo), Some(&pps), 4, 4);
        assert_eq!(ret, ERR_NONE);
        assert_eq!(fmo.iCountMbNum, 16);
        assert_eq!(fmo.iSliceGroupCount, 1);
        assert!(!fmo.pMbAllocMap.is_empty());

        for i in 0..16 {
            assert_eq!(FmoMbToSliceGroup(Some(&fmo), i), 0);
        }
        assert_eq!(FmoNextMb(Some(&fmo), 0), 1);
        assert_eq!(FmoNextMb(Some(&fmo), 14), 15);
        assert_eq!(FmoNextMb(Some(&fmo), 15), -1);

        let mut fmo_list = [fmo];
        fmo_list[0].bActiveFlag = true;
        UninitFmoList(&mut fmo_list, 1);
        assert!(fmo_list[0].pMbAllocMap.is_empty());
        assert!(!fmo_list[0].bActiveFlag);
    }

    #[test]
    fn test_fmo_type0_interleaved() {
        let mut fmo = TagFmo::default();
        let mut pps = SPps::default();
        pps.uiNumSliceGroups = 2;
        pps.uiSliceGroupMapType = 0;
        pps.uiRunLength[0] = 3;
        pps.uiRunLength[1] = 2;

        let ret = InitFmo(Some(&mut fmo), Some(&pps), 5, 2);
        assert_eq!(ret, ERR_NONE);
        assert_eq!(fmo.iCountMbNum, 10);
        assert_eq!(fmo.iSliceGroupCount, 2);
        assert_eq!(fmo.iSliceGroupType, 0);

        // Pattern: 3 in group 0, 2 in group 1, 3 in group 0, 2 in group 1
        // Indices: 0,1,2 -> 0; 3,4 -> 1; 5,6,7 -> 0; 8,9 -> 1
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 0), 0);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 1), 0);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 2), 0);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 3), 1);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 4), 1);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 5), 0);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 6), 0);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 7), 0);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 8), 1);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 9), 1);

        // Traversal for group 0
        assert_eq!(FmoNextMb(Some(&fmo), 0), 1);
        assert_eq!(FmoNextMb(Some(&fmo), 2), 5);
        assert_eq!(FmoNextMb(Some(&fmo), 7), -1);

        // Traversal for group 1
        assert_eq!(FmoNextMb(Some(&fmo), 3), 4);
        assert_eq!(FmoNextMb(Some(&fmo), 4), 8);
        assert_eq!(FmoNextMb(Some(&fmo), 8), 9);
        assert_eq!(FmoNextMb(Some(&fmo), 9), -1);

        let mut fmo_list = [fmo];
        fmo_list[0].bActiveFlag = true;
        UninitFmoList(&mut fmo_list, 1);
    }

    #[test]
    fn test_fmo_type1_dispersed() {
        let mut fmo = TagFmo::default();
        let mut pps = SPps::default();
        pps.uiNumSliceGroups = 2;
        pps.uiSliceGroupMapType = 1;

        let ret = InitFmo(Some(&mut fmo), Some(&pps), 4, 4);
        assert_eq!(ret, ERR_NONE);
        assert_eq!(fmo.iCountMbNum, 16);
        assert_eq!(fmo.iSliceGroupCount, 2);
        assert_eq!(fmo.iSliceGroupType, 1);

        // Checkerboard grid 4x4:
        // Row 0: 0, 1, 0, 1
        // Row 1: 1, 0, 1, 0
        // Row 2: 0, 1, 0, 1
        // Row 3: 1, 0, 1, 0
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 0), 0);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 1), 1);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 2), 0);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 3), 1);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 4), 1);
        assert_eq!(FmoMbToSliceGroup(Some(&fmo), 5), 0);

        assert_eq!(FmoNextMb(Some(&fmo), 0), 2);
        assert_eq!(FmoNextMb(Some(&fmo), 1), 3);
        assert_eq!(FmoNextMb(Some(&fmo), 3), 4);

        let mut fmo_list = [fmo];
        fmo_list[0].bActiveFlag = true;
        UninitFmoList(&mut fmo_list, 1);
    }

    #[test]
    fn test_fmo_param_update() {
        let mut fmo = TagFmo::default();
        let mut sps = SSps::default();
        sps.iMbWidth = 4;
        sps.iMbHeight = 4;
        let mut pps = SPps::default();
        pps.uiNumSliceGroups = 2;
        pps.uiSliceGroupMapType = 1;
        let mut active_fmo_num: i32 = 0;

        let ret = FmoParamUpdate(Some(&mut fmo), Some(&sps), Some(&pps), &mut active_fmo_num);
        assert_eq!(ret, ERR_NONE);
        assert_eq!(active_fmo_num, 1);
        assert!(fmo.bActiveFlag);

        let mut fmo_list = [fmo];
        UninitFmoList(&mut fmo_list, 1);
    }
}
