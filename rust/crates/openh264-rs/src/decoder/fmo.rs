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

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use core::ffi::{c_char, c_void};
use crate::common::memory_align::CMemoryAlign;
use crate::decoder::parameter_sets::{PPps, PSps};

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
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagFmo {
    /// Heap-allocated array storing the slice group ID for each macroblock.
    pub pMbAllocMap: *mut u8,
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
pub type PFmo = *mut TagFmo;

impl Default for TagFmo {
    fn default() -> Self {
        Self {
            pMbAllocMap: std::ptr::null_mut(),
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

#[inline]
unsafe fn free_mb_alloc_map(pMa: *mut CMemoryAlign, pPtr: *mut u8) {
    if pPtr.is_null() {
        return;
    }
    let tag = b"_fmo->pMbAllocMap\0".as_ptr() as *const c_char;
    if !pMa.is_null() {
        (*pMa).WelsFree(pPtr as *mut c_void, tag);
    } else {
        crate::common::memory_align::WelsFree(pPtr as *mut c_void, tag);
    }
}

#[inline]
unsafe fn mallocz_mb_alloc_map(pMa: *mut CMemoryAlign, size: u32) -> *mut u8 {
    let tag = b"_fmo->pMbAllocMap\0".as_ptr() as *const c_char;
    let ptr = if !pMa.is_null() {
        (*pMa).WelsMallocz(size, tag)
    } else {
        crate::common::memory_align::WelsMallocz(size, tag)
    };
    ptr as *mut u8
}

// ============================================================================
// Core FMO Map Generation Routines
// ============================================================================

/// Generates the macroblock allocation map for Interleaved Slice Groups (Type 0).
///
/// # Safety
/// `pFmo` and `pPps` must point to valid structures or null.
pub unsafe fn FmoGenerateMbAllocMapType0(pFmo: PFmo, pPps: PPps) -> i32 {
    if pFmo.is_null() || pPps.is_null() {
        return ERR_INFO_INVALID_PARAM;
    }
    let uiNumSliceGroups = (*pPps).uiNumSliceGroups;
    let iMbNum = (*pFmo).iCountMbNum;
    let pMbAllocMap = (*pFmo).pMbAllocMap;

    if pMbAllocMap.is_null()
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
                    *pMbAllocMap.add((i + j) as usize) = uiGroup;
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
/// # Safety
/// `pFmo` and `pPps` must point to valid structures or null.
pub unsafe fn FmoGenerateMbAllocMapType1(pFmo: PFmo, pPps: PPps, kiMbWidth: i32) -> i32 {
    if pFmo.is_null() || pPps.is_null() {
        return ERR_INFO_INVALID_PARAM;
    }
    let uiNumSliceGroups = (*pPps).uiNumSliceGroups;
    let iMbNum = (*pFmo).iCountMbNum;
    let pMbAllocMap = (*pFmo).pMbAllocMap;

    if pMbAllocMap.is_null()
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
        *pMbAllocMap.add(i as usize) = val as u8;
        i += 1;
    }

    ERR_NONE
}

/// Internal helper allocating `pMbAllocMap` and dispatching map generation according to PPS parameters.
///
/// # Safety
/// Pointers must be valid or null.
pub unsafe fn FmoGenerateSliceGroup(
    pFmo: PFmo,
    kpPps: PPps,
    kiMbWidth: i32,
    kiMbHeight: i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    if pFmo.is_null() || kpPps.is_null() {
        return ERR_INFO_INVALID_PARAM;
    }

    let iNumMb = kiMbWidth * kiMbHeight;
    if iNumMb <= 0 {
        return ERR_INFO_INVALID_PARAM;
    }

    free_mb_alloc_map(pMa, (*pFmo).pMbAllocMap);
    (*pFmo).pMbAllocMap = mallocz_mb_alloc_map(pMa, iNumMb as u32);
    if (*pFmo).pMbAllocMap.is_null() {
        return ERR_INFO_OUT_OF_MEMORY;
    }

    (*pFmo).iCountMbNum = iNumMb;

    if (*kpPps).uiNumSliceGroups < 2 && iNumMb > 0 {
        std::ptr::write_bytes((*pFmo).pMbAllocMap, 0, iNumMb as usize);
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
/// # Safety
/// Pointers must be valid or null.
pub unsafe fn InitFmo(
    pFmo: PFmo,
    pPps: PPps,
    kiMbWidth: i32,
    kiMbHeight: i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    FmoGenerateSliceGroup(pFmo, pPps, kiMbWidth, kiMbHeight, pMa)
}

/// Frees all dynamically allocated memory across the FMO context array.
///
/// # Safety
/// `pFmo` must point to an array of `TagFmo` of size at least `kiCnt`.
pub unsafe fn UninitFmoList(
    pFmo: PFmo,
    kiCnt: i32,
    kiAvail: i32,
    pMa: *mut CMemoryAlign,
) {
    if pFmo.is_null() || kiAvail <= 0 || kiCnt < kiAvail {
        return;
    }

    let mut iFreeNodes: i32 = 0;
    for i in 0..kiCnt {
        let pIter = pFmo.add(i as usize);
        if (*pIter).bActiveFlag {
            if !(*pIter).pMbAllocMap.is_null() {
                let tag = b"pIter->pMbAllocMap\0".as_ptr() as *const c_char;
                if !pMa.is_null() {
                    (*pMa).WelsFree((*pIter).pMbAllocMap as *mut c_void, tag);
                } else {
                    crate::common::memory_align::WelsFree(
                        (*pIter).pMbAllocMap as *mut c_void,
                        tag,
                    );
                }
                (*pIter).pMbAllocMap = std::ptr::null_mut();
            }
            (*pIter).iSliceGroupCount = 0;
            (*pIter).iSliceGroupType = -1;
            (*pIter).iCountMbNum = 0;
            (*pIter).bActiveFlag = false;
            iFreeNodes += 1;
            if iFreeNodes >= kiAvail {
                break;
            }
        }
    }
}

/// Detects whether SPS or PPS parameter sets have changed relative to the cached FMO state.
///
/// # Safety
/// `pFmo` must point to a valid structure or null.
pub unsafe fn FmoParamSetsChanged(
    pFmo: PFmo,
    kiCountNumMb: i32,
    kiSliceGroupType: i32,
    kiSliceGroupCount: i32,
) -> bool {
    if pFmo.is_null() {
        return false;
    }
    !(*pFmo).bActiveFlag
        || (kiCountNumMb != (*pFmo).iCountMbNum)
        || (kiSliceGroupType != (*pFmo).iSliceGroupType)
        || (kiSliceGroupCount != (*pFmo).iSliceGroupCount)
}

/// Updates/inserts an FMO parameter unit for the active access unit.
///
/// # Safety
/// Pointers must be valid or null.
pub unsafe fn FmoParamUpdate(
    pFmo: PFmo,
    pSps: PSps,
    pPps: PPps,
    pActiveFmoNum: *mut i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    if pFmo.is_null() || pSps.is_null() || pPps.is_null() || pActiveFmoNum.is_null() {
        return ERR_INFO_INVALID_PARAM;
    }

    let kuiMbWidth = (*pSps).iMbWidth;
    let kuiMbHeight = (*pSps).iMbHeight;
    let mut iRet = ERR_NONE;

    if FmoParamSetsChanged(
        pFmo,
        (kuiMbWidth * kuiMbHeight) as i32,
        (*pPps).uiSliceGroupMapType as i32,
        (*pPps).uiNumSliceGroups as i32,
    ) {
        iRet = InitFmo(pFmo, pPps, kuiMbWidth as i32, kuiMbHeight as i32, pMa);
        if iRet != ERR_NONE {
            return iRet;
        }

        if !(*pFmo).bActiveFlag && *pActiveFmoNum < MAX_PPS_COUNT {
            *pActiveFmoNum += 1;
            (*pFmo).bActiveFlag = true;
        }
    }

    iRet
}

/// Converts a linear macroblock index (`kiMbXy`) to its corresponding slice group ID.
///
/// # Safety
/// `pFmo` must point to a valid structure or null.
pub unsafe fn FmoMbToSliceGroup(pFmo: PFmo, kiMbXy: MB_XY_T) -> i32 {
    if pFmo.is_null() {
        return -1;
    }

    let kiMbNum = (*pFmo).iCountMbNum;
    let kpMbMap = (*pFmo).pMbAllocMap;

    if kiMbXy < 0 || kiMbXy >= kiMbNum || kpMbMap.is_null() {
        return -1;
    }

    *kpMbMap.add(kiMbXy as usize) as i32
}

/// Returns the next successive macroblock in raster sequence belonging to the same slice group.
///
/// # Safety
/// `pFmo` must point to a valid structure or null.
pub unsafe fn FmoNextMb(pFmo: PFmo, kiMbXy: MB_XY_T) -> MB_XY_T {
    if pFmo.is_null() {
        return -1;
    }

    let kiTotalMb = (*pFmo).iCountMbNum;
    let kpMbMap = (*pFmo).pMbAllocMap;
    if kpMbMap.is_null() {
        return -1;
    }

    let iGroup = FmoMbToSliceGroup(pFmo, kiMbXy);
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
        if *kpMbMap.add(iNextMb as usize) == kuiSliceGroupIdc {
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
        use crate::decoder::parameter_sets::{SPps, SSps};

    #[test]
    fn test_fmo_default_and_types() {
        let fmo = TagFmo::default();
        assert!(fmo.pMbAllocMap.is_null());
        assert_eq!(fmo.iCountMbNum, 0);
        assert_eq!(fmo.iSliceGroupCount, 0);
        assert_eq!(fmo.iSliceGroupType, -1);
        assert!(!fmo.bActiveFlag);
    }

    #[test]
    fn test_fmo_single_slice_group() {
        unsafe {
            let mut fmo = TagFmo::default();
            let mut pps = SPps::default();
            pps.uiNumSliceGroups = 1;
            let mut ma = CMemoryAlign::new(16);

            let ret = InitFmo(&mut fmo, &mut pps, 4, 4, &mut ma);
            assert_eq!(ret, ERR_NONE);
            assert_eq!(fmo.iCountMbNum, 16);
            assert_eq!(fmo.iSliceGroupCount, 1);
            assert!(!fmo.pMbAllocMap.is_null());

            for i in 0..16 {
                assert_eq!(FmoMbToSliceGroup(&mut fmo, i), 0);
            }
            assert_eq!(FmoNextMb(&mut fmo, 0), 1);
            assert_eq!(FmoNextMb(&mut fmo, 14), 15);
            assert_eq!(FmoNextMb(&mut fmo, 15), -1);

            let mut fmo_list = [fmo];
            fmo_list[0].bActiveFlag = true;
            UninitFmoList(fmo_list.as_mut_ptr(), 1, 1, &mut ma);
            assert!(fmo_list[0].pMbAllocMap.is_null());
            assert!(!fmo_list[0].bActiveFlag);
        }
    }

    #[test]
    fn test_fmo_type0_interleaved() {
        unsafe {
            let mut fmo = TagFmo::default();
            let mut pps = SPps::default();
            pps.uiNumSliceGroups = 2;
            pps.uiSliceGroupMapType = 0;
            pps.uiRunLength[0] = 3;
            pps.uiRunLength[1] = 2;
            let mut ma = CMemoryAlign::new(16);

            let ret = InitFmo(&mut fmo, &mut pps, 5, 2, &mut ma);
            assert_eq!(ret, ERR_NONE);
            assert_eq!(fmo.iCountMbNum, 10);
            assert_eq!(fmo.iSliceGroupCount, 2);
            assert_eq!(fmo.iSliceGroupType, 0);

            // Pattern: 3 in group 0, 2 in group 1, 3 in group 0, 2 in group 1
            // Indices: 0,1,2 -> 0; 3,4 -> 1; 5,6,7 -> 0; 8,9 -> 1
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 0), 0);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 1), 0);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 2), 0);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 3), 1);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 4), 1);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 5), 0);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 6), 0);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 7), 0);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 8), 1);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 9), 1);

            // Traversal for group 0
            assert_eq!(FmoNextMb(&mut fmo, 0), 1);
            assert_eq!(FmoNextMb(&mut fmo, 2), 5);
            assert_eq!(FmoNextMb(&mut fmo, 7), -1);

            // Traversal for group 1
            assert_eq!(FmoNextMb(&mut fmo, 3), 4);
            assert_eq!(FmoNextMb(&mut fmo, 4), 8);
            assert_eq!(FmoNextMb(&mut fmo, 8), 9);
            assert_eq!(FmoNextMb(&mut fmo, 9), -1);

            let mut fmo_list = [fmo];
            fmo_list[0].bActiveFlag = true;
            UninitFmoList(fmo_list.as_mut_ptr(), 1, 1, &mut ma);
        }
    }

    #[test]
    fn test_fmo_type1_dispersed() {
        unsafe {
            let mut fmo = TagFmo::default();
            let mut pps = SPps::default();
            pps.uiNumSliceGroups = 2;
            pps.uiSliceGroupMapType = 1;
            let mut ma = CMemoryAlign::new(16);

            let ret = InitFmo(&mut fmo, &mut pps, 4, 4, &mut ma);
            assert_eq!(ret, ERR_NONE);
            assert_eq!(fmo.iCountMbNum, 16);
            assert_eq!(fmo.iSliceGroupCount, 2);
            assert_eq!(fmo.iSliceGroupType, 1);

            // Checkerboard grid 4x4:
            // Row 0: 0, 1, 0, 1
            // Row 1: 1, 0, 1, 0
            // Row 2: 0, 1, 0, 1
            // Row 3: 1, 0, 1, 0
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 0), 0);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 1), 1);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 2), 0);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 3), 1);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 4), 1);
            assert_eq!(FmoMbToSliceGroup(&mut fmo, 5), 0);

            assert_eq!(FmoNextMb(&mut fmo, 0), 2);
            assert_eq!(FmoNextMb(&mut fmo, 1), 3);
            assert_eq!(FmoNextMb(&mut fmo, 3), 4);

            let mut fmo_list = [fmo];
            fmo_list[0].bActiveFlag = true;
            UninitFmoList(fmo_list.as_mut_ptr(), 1, 1, &mut ma);
        }
    }

    #[test]
    fn test_fmo_param_update() {
        unsafe {
            let mut fmo = TagFmo::default();
            let mut sps = SSps::default();
            sps.iMbWidth = 4;
            sps.iMbHeight = 4;
            let mut pps = SPps::default();
            pps.uiNumSliceGroups = 2;
            pps.uiSliceGroupMapType = 1;
            let mut active_fmo_num: i32 = 0;
            let mut ma = CMemoryAlign::new(16);

            let ret = FmoParamUpdate(&mut fmo, &mut sps, &mut pps, &mut active_fmo_num, &mut ma);
            assert_eq!(ret, ERR_NONE);
            assert_eq!(active_fmo_num, 1);
            assert!(fmo.bActiveFlag);

            let mut fmo_list = [fmo];
            UninitFmoList(fmo_list.as_mut_ptr(), 1, 1, &mut ma);
        }
    }
}
