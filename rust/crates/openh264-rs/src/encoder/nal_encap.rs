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

//! # OpenH264 Encoder: NAL Unit Encapsulation Engine
//!
//! Translated from `codec/encoder/core/inc/nal_encap.h` and `codec/encoder/core/src/nal_encap.cpp`.
//!
//! Handles NAL unit start position demarcations, unescaped RBSP payload accounting,
//! Annex B start code prefix injection (`0x00000001`), standard 1-byte AVC and 4-byte SVC
//! extension NAL headers, emulation prevention byte escaping (`0x000003`), and SVC prefix NAL serialization.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use core::ffi::c_void;

// ============================================================================
// Constants & Return Codes
// ============================================================================

/// Size in bytes of the Annex B 4-byte start code prefix (`0x00 0x00 0x00 0x01`).
pub const NAL_HEADER_SIZE: usize = 4;

/// Return codes matching OpenH264 encoder error specifications.
pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x01;
pub const ENC_RETURN_UNSUPPORTED_PARA: i32 = 0x02;
pub const ENC_RETURN_UNEXPECTED: i32 = 0x04;
pub const ENC_RETURN_CORRECTED: i32 = 0x08;
pub const ENC_RETURN_INVALIDINPUT: i32 = 0x10;
pub const ENC_RETURN_MEMOVERFLOWFOUND: i32 = 0x20;
pub const ENC_RETURN_VLCOVERFLOWFOUND: i32 = 0x40;
pub const ENC_RETURN_KNOWN_ISSUE: i32 = 0x80;

// ============================================================================
// Enums & Structs
// ============================================================================

pub use crate::common::wels_common_defs::{
    EWelsNalRefIdc, EWelsNalUnitType, SNalUnitHeader, SNalUnitHeaderExt,
};
pub use crate::safe::bits::BsWriter;

/// Raw payload data descriptor for a NAL unit before encapsulation.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsNalRaw {
    pub pRawData: *mut u8,
    pub iPayloadSize: i32,
    pub sNalExt: SNalUnitHeaderExt,
    pub iStartPos: i32,
}

impl Default for SWelsNalRaw {
    fn default() -> Self {
        Self {
            pRawData: core::ptr::null_mut(),
            iPayloadSize: 0,
            sNalExt: SNalUnitHeaderExt::default(),
            iStartPos: 0,
        }
    }
}

/// The one place that turns an encoder output buffer back into a slice.
///
/// SHIM(phase3) -> the raw `pBsBuffer` allocations. `BsWriter` is a position and
/// nothing else, so the buffer has to be expressed at each write; until **T3.6**
/// gives `SWelsEncoderOutput` and `SWelsSliceBs` owned buffers, that means
/// rebuilding a slice from a `WelsMallocz`'d pointer and the `uiSize` recorded
/// beside it. One helper does that arithmetic and nothing else does it, exactly as
/// T3.1b's reader-side helper did until T3.3 deleted it.
///
/// **T3.5 narrowed what this guards.** The CABAC arithmetic coder used to hold
/// its own `m_pBufStart`/`m_pBufCur`/`m_pBufEnd` pointer triple and reach the
/// output without passing through here; its cursor is three `usize` offsets now,
/// so both entropy coders write through this one boundary on the same
/// convention. What remains on the far side is the *allocation*, not any cursor
/// — which is precisely the residue T3.6 removes.
///
/// # Safety
/// `ptr` must be non-null and point to `len` writable bytes that outlive `'a`, with
/// no other live reference to them — which is what `pBsBuffer` plus its own
/// `uiSize` is, and what the task-claiming invariant gives per thread.
#[inline]
pub unsafe fn bs_buffer<'a>(ptr: *mut u8, len: u32) -> &'a mut [u8] {
    debug_assert!(!ptr.is_null(), "a writer's buffer must be allocated first");
    unsafe { core::slice::from_raw_parts_mut(ptr, len as usize) }
}

/// Top-level frame bitstream output container and NAL descriptor list manager.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsEncoderOutput {
    pub pBsBuffer: *mut u8,
    pub uiSize: u32,
    pub sBsWrite: BsWriter,
    pub sNalList: *mut SWelsNalRaw,
    pub pNalLen: *mut i32,
    pub iCountNals: i32,
    pub iNalIndex: i32,
    pub iLayerBsIndex: i32,
}

impl Default for SWelsEncoderOutput {
    fn default() -> Self {
        Self {
            pBsBuffer: core::ptr::null_mut(),
            uiSize: 0,
            sBsWrite: BsWriter::new(),
            sNalList: core::ptr::null_mut(),
            pNalLen: core::ptr::null_mut(),
            iCountNals: 0,
            iNalIndex: 0,
            iLayerBsIndex: 0,
        }
    }
}

/// Thread-local bitstream state allocated per slice.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsSliceBs {
    pub pBs: *mut u8,
    pub uiBsSize: u32,
    pub uiBsPos: u32,
    pub pBsBuffer: *mut u8,
    pub uiSize: u32,
    pub sBsWrite: BsWriter,
    pub sNalList: [SWelsNalRaw; 2],
    pub iNalLen: [i32; 2],
    pub iNalIndex: i32,
}

impl Default for SWelsSliceBs {
    fn default() -> Self {
        Self {
            pBs: core::ptr::null_mut(),
            uiBsSize: 0,
            uiBsPos: 0,
            pBsBuffer: core::ptr::null_mut(),
            uiSize: 0,
            sBsWrite: BsWriter::new(),
            sNalList: [SWelsNalRaw::default(), SWelsNalRaw::default()],
            iNalLen: [0, 0],
            iNalIndex: 0,
        }
    }
}

// ============================================================================
// Bitstream Helper Functions
// ============================================================================

// One writer family, `vlc_encoder.rs`'s, which is the transliteration of the C++
// `codec/common/inc/golomb_common.h`. This module used to declare its own copy of
// the five functions below (`phase0_findings.md` F2's third row). Two divergences
// died with it, both in this module's favour of doing *less* than the C++:
//
//   * `BsWriteBits` guarded `iLen == 0` explicitly where the canonical relies on
//     `(1 << 0) - 1 == 0` masking the value to nothing. Same result, always.
//   * `BsFlush` stored only the `4 - iLeftBits / 8` bytes it advanced over, where
//     the canonical — and `golomb_common.h:104` — always stores a full 32-bit
//     word and advances by the same 1..=4. F2's inventory did not list this one;
//     it covered `BsWriteBits` only. The bytes that differ are the up-to-three
//     past the new write position, which the next write overwrites before anything
//     reads them, and which are past the last NAL's end when nothing follows. The
//     sweeps are the proof: this is the C++'s own behaviour, and 341/341 both
//     profiles hold across the change.
pub use crate::encoder::vlc_encoder::{
    BsFlush, BsGetBitsPos, BsRbspTrailingBits, BsWriteBits, BsWriteOneBit,
};

// ============================================================================
// Core NAL Encapsulation Functions
// ============================================================================

/// Initializes a new raw NAL unit entry in the global encoder output context.
///
/// # Safety
/// - `pEncoderOuput` must point to a valid and properly initialized `SWelsEncoderOutput` structure.
/// - `pEncoderOuput.sNalList` must have enough capacity for `iNalIndex`.
#[inline]
pub unsafe extern "C" fn WelsLoadNal(
    pEncoderOuput: *mut SWelsEncoderOutput,
    kiType: i32,
    kiNalRefIdc: i32,
) {
    if pEncoderOuput.is_null() || (*pEncoderOuput).sNalList.is_null() {
        return;
    }
    let pWelsEncoderOuput = &mut *pEncoderOuput;
    let pRawNal = &mut *pWelsEncoderOuput
        .sNalList
        .add(pWelsEncoderOuput.iNalIndex as usize);
    let sNalUnitHeader = &mut pRawNal.sNalExt.sNalUnitHeader;
    let kiStartPos = BsGetBitsPos(&pWelsEncoderOuput.sBsWrite) >> 3;

    sNalUnitHeader.eNalUnitType = EWelsNalUnitType::from(kiType);
    sNalUnitHeader.uiNalRefIdc = kiNalRefIdc as u8;
    sNalUnitHeader.uiForbiddenZeroBit = 0;

    pRawNal.pRawData = if !pWelsEncoderOuput.pBsBuffer.is_null() {
        pWelsEncoderOuput.pBsBuffer.add(kiStartPos as usize)
    } else {
        std::ptr::null_mut()
    };
    pRawNal.iStartPos = kiStartPos;
    pRawNal.iPayloadSize = 0;
}

/// Finalizes the raw NAL unit currently being written in `pEncoderOuput`.
///
/// # Safety
/// - `pEncoderOuput` must point to a valid `SWelsEncoderOutput` structure.
#[inline]
pub unsafe extern "C" fn WelsUnloadNal(pEncoderOuput: *mut SWelsEncoderOutput) {
    if pEncoderOuput.is_null() || (*pEncoderOuput).sNalList.is_null() {
        return;
    }
    let pWelsEncoderOuput = &mut *pEncoderOuput;
    let pIdx = &mut pWelsEncoderOuput.iNalIndex;
    let pRawNal = &mut *pWelsEncoderOuput.sNalList.add(*pIdx as usize);
    let kiEndPos = BsGetBitsPos(&pWelsEncoderOuput.sBsWrite) >> 3;

    /* count payload size of raw NAL */
    pRawNal.iPayloadSize = kiEndPos - pRawNal.iStartPos;

    *pIdx += 1;
}

/// Initializes a raw NAL unit entry for a thread-local slice bitstream context.
///
/// # Safety
/// - `pSliceBs` must point to a valid `SWelsSliceBs` structure.
#[inline]
pub unsafe extern "C" fn WelsLoadNalForSlice(
    pSliceBs: *mut SWelsSliceBs,
    kiType: i32,
    kiNalRefIdc: i32,
) {
    let pSlice = &mut *pSliceBs;
    let pRawNal = &mut pSlice.sNalList[pSlice.iNalIndex as usize];
    let sNalUnitHeader = &mut pRawNal.sNalExt.sNalUnitHeader;
    let pBitStringAux = &pSlice.sBsWrite;
    let kiStartPos = BsGetBitsPos(pBitStringAux) >> 3;

    sNalUnitHeader.eNalUnitType = EWelsNalUnitType::from(kiType);
    sNalUnitHeader.uiNalRefIdc = kiNalRefIdc as u8;
    sNalUnitHeader.uiForbiddenZeroBit = 0;

    pRawNal.pRawData = pSlice.pBsBuffer.add(kiStartPos as usize);
    pRawNal.iStartPos = kiStartPos;
    pRawNal.iPayloadSize = 0;
}

/// Finalizes the slice-thread-local raw NAL unit payload size and advances the NAL index.
///
/// # Safety
/// - `pSliceBs` must point to a valid `SWelsSliceBs` structure.
#[inline]
pub unsafe extern "C" fn WelsUnloadNalForSlice(pSliceBs: *mut SWelsSliceBs) {
    let pSlice = &mut *pSliceBs;
    let pIdx = &mut pSlice.iNalIndex;
    let pRawNal = &mut pSlice.sNalList[*pIdx as usize];
    let pBitStringAux = &pSlice.sBsWrite;
    let kiEndPos = BsGetBitsPos(pBitStringAux) >> 3;

    /* count payload size of raw NAL */
    pRawNal.iPayloadSize = kiEndPos - pRawNal.iStartPos;
    *pIdx += 1;
}

/// Encapsulates an unescaped raw NAL payload into an Annex B compliant byte stream (EBSP).
///
/// Prepends the 4-byte start code prefix (`0x00000001`), packs the 1-byte base NAL header
/// or 4-byte SVC extension header, and performs emulation prevention byte insertion (`0x03`).
///
/// # Safety
/// - `pRawNal` must point to a valid `SWelsNalRaw` descriptor.
/// - If `kbNALExt` is true, `pNalHeaderExt` must point to a valid `SNalUnitHeaderExt`.
/// - `pDst` must point to a writable memory region of at least `kiDstBufferLen` bytes.
#[inline]
pub unsafe extern "C" fn WelsEncodeNal(
    pRawNal: *mut SWelsNalRaw,
    pNalHeaderExt: *mut c_void,
    kiDstBufferLen: i32,
    pDst: *mut c_void,
    pDstLen: *mut i32,
) -> i32 {
    if pRawNal.is_null() || pDst.is_null() || pDstLen.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let raw_nal = &*pRawNal;
    let nal_type = raw_nal.sNalExt.sNalUnitHeader.eNalUnitType;
    let kbNALExt = nal_type == EWelsNalUnitType::NAL_UNIT_PREFIX
        || nal_type == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;

    let iAssumedNeededLength =
        (NAL_HEADER_SIZE as i32) + (if kbNALExt { 3 } else { 0 }) + raw_nal.iPayloadSize + 1;

    if iAssumedNeededLength <= 0 {
        return ENC_RETURN_UNEXPECTED;
    }

    // Since for each 0x000 need a 0x03, the needed length will not exceed (iAssumedNeededLength + iAssumedNeededLength / 3).
    // Here adjusted to >> 1 to omit division.
    if kiDstBufferLen < (iAssumedNeededLength + (iAssumedNeededLength >> 1)) {
        return ENC_RETURN_MEMALLOCERR;
    }

    let pDstStart = pDst as *mut u8;
    let mut pDstPointer = pDstStart;
    let mut pSrcPointer = raw_nal.pRawData;
    let pSrcEnd = raw_nal.pRawData.add(raw_nal.iPayloadSize as usize);
    let mut iZeroCount: i32 = 0;

    if !pDstLen.is_null() {
        *pDstLen = 0;
    }

    // 4-byte Annex B start code prefix: 0x00 0x00 0x00 0x01
    let kuiStartCodePrefix: [u8; 4] = [0, 0, 0, 1];
    core::ptr::copy_nonoverlapping(kuiStartCodePrefix.as_ptr(), pDstPointer, 4);
    pDstPointer = pDstPointer.add(4);

    // 1-Byte NAL Unit Header
    let nri = raw_nal.sNalExt.sNalUnitHeader.uiNalRefIdc;
    let utype = raw_nal.sNalExt.sNalUnitHeader.eNalUnitType as u8;
    *pDstPointer = (nri << 5) | (utype & 0x1f);
    pDstPointer = pDstPointer.add(1);

    if kbNALExt {
        let sNalExt = &*(pNalHeaderExt as *const SNalUnitHeaderExt);

        // Extension Byte 1: reserved_one_bit (0x80) | idr_flag (bit 6)
        *pDstPointer = 0x80 | ((sNalExt.bIdrFlag as u8) << 6);
        pDstPointer = pDstPointer.add(1);

        // Extension Byte 2: no_inter_layer_pred_flag (0x80) | dependency_id (bits 6..4)
        *pDstPointer = 0x80 | ((sNalExt.uiDependencyId) << 4);
        pDstPointer = pDstPointer.add(1);

        // Extension Byte 3: temporal_id (bits 7..5) | discardable_flag (bit 3) | reserved_three_2bits (0x07)
        *pDstPointer = ((sNalExt.uiTemporalId) << 5)
            | ((sNalExt.bDiscardableFlag as u8) << 3)
            | 0x07;
        pDstPointer = pDstPointer.add(1);
    }

    // Emulation prevention escaping loop
    while pSrcPointer < pSrcEnd {
        let byte_val = *pSrcPointer;
        if iZeroCount == 2 && byte_val <= 3 {
            // Add emulation prevention byte 0x03
            *pDstPointer = 3;
            pDstPointer = pDstPointer.add(1);
            iZeroCount = 0;
        }
        if byte_val == 0 {
            iZeroCount += 1;
        } else {
            iZeroCount = 0;
        }
        *pDstPointer = byte_val;
        pDstPointer = pDstPointer.add(1);
        pSrcPointer = pSrcPointer.add(1);
    }

    let iNalLength = pDstPointer.offset_from(pDstStart) as i32;
    if !pDstLen.is_null() {
        *pDstLen = iNalLength;
    }

    ENC_RETURN_SUCCESS
}

/// Writes the RBSP payload for an SVC Prefix NAL unit (NAL unit type 14).
///
/// # Safety
/// - `pBitStringAux` must point to a valid `BsWriter`, and `buf` must be the
///   buffer that writer is positioned in.
#[inline]
pub unsafe fn WelsWriteSVCPrefixNal(
    buf: &mut [u8],
    pBitStringAux: *mut BsWriter,
    kiNalRefIdc: i32,
    _kbIdrFlag: bool,
) -> i32 {
    if kiNalRefIdc > 0 {
        let pBs = &mut *pBitStringAux;
        BsWriteOneBit(buf, pBs, 0); // bStoreRefBasePicFlag = false
        BsWriteOneBit(buf, pBs, 0); // additional_prefix_nal_unit_extension_flag = false
        BsRbspTrailingBits(buf, pBs);
    }
    0
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wels_encode_nal_standard_avc() {
        let mut raw_payload = [0x00, 0x00, 0x01, 0xAA, 0x00, 0x00, 0x00, 0xBB];
        let mut raw_nal = SWelsNalRaw::default();
        raw_nal.pRawData = raw_payload.as_mut_ptr();
        raw_nal.iPayloadSize = raw_payload.len() as i32;
        raw_nal.sNalExt.sNalUnitHeader.eNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        raw_nal.sNalExt.sNalUnitHeader.uiNalRefIdc = EWelsNalRefIdc::NRI_PRI_HIGHEST as u8;

        let mut dst_buffer = [0u8; 128];
        let mut dst_len: i32 = 0;

        let ret = unsafe {
            WelsEncodeNal(
                &mut raw_nal,
                core::ptr::null_mut(),
                dst_buffer.len() as i32,
                dst_buffer.as_mut_ptr() as *mut c_void,
                &mut dst_len,
            )
        };

        assert_eq!(ret, ENC_RETURN_SUCCESS);
        assert!(dst_len > 0);

        // Check start code prefix: 00 00 00 01
        assert_eq!(&dst_buffer[0..4], &[0x00, 0x00, 0x00, 0x01]);

        // Check 1-byte NAL header: (3 << 5) | 1 = 0x61
        assert_eq!(dst_buffer[4], (3 << 5) | 1);

        // Check escaped bytes:
        // [0x00, 0x00, 0x01] -> [0x00, 0x00, 0x03, 0x01]
        // [0xAA]
        // [0x00, 0x00, 0x00] -> [0x00, 0x00, 0x03, 0x00]
        // [0xBB]
        let expected_payload = [0x00, 0x00, 0x03, 0x01, 0xAA, 0x00, 0x00, 0x03, 0x00, 0xBB];
        assert_eq!(&dst_buffer[5..5 + expected_payload.len()], &expected_payload);
        assert_eq!(dst_len as usize, 5 + expected_payload.len());
    }

    #[test]
    fn test_wels_encode_nal_svc_extension() {
        let mut raw_payload = [0x12, 0x34];
        let mut raw_nal = SWelsNalRaw::default();
        raw_nal.pRawData = raw_payload.as_mut_ptr();
        raw_nal.iPayloadSize = raw_payload.len() as i32;
        raw_nal.sNalExt.sNalUnitHeader.eNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;
        raw_nal.sNalExt.sNalUnitHeader.uiNalRefIdc = EWelsNalRefIdc::NRI_PRI_HIGH as u8;

        let mut ext_header = SNalUnitHeaderExt::default();
        ext_header.bIdrFlag = true;
        ext_header.uiDependencyId = 2;
        ext_header.uiTemporalId = 3;
        ext_header.bDiscardableFlag = true;

        let mut dst_buffer = [0u8; 128];
        let mut dst_len: i32 = 0;

        let ret = unsafe {
            WelsEncodeNal(
                &mut raw_nal,
                &mut ext_header as *mut _ as *mut c_void,
                dst_buffer.len() as i32,
                dst_buffer.as_mut_ptr() as *mut c_void,
                &mut dst_len,
            )
        };

        assert_eq!(ret, ENC_RETURN_SUCCESS);

        // Start code: 00 00 00 01
        assert_eq!(&dst_buffer[0..4], &[0x00, 0x00, 0x00, 0x01]);

        // Base NAL header: (2 << 5) | 20 = 0x40 | 0x14 = 0x54
        assert_eq!(dst_buffer[4], (2 << 5) | 20);

        // Ext Byte 1: 0x80 | (1 << 6) = 0xC0
        assert_eq!(dst_buffer[5], 0xC0);

        // Ext Byte 2: 0x80 | (2 << 4) = 0xA0
        assert_eq!(dst_buffer[6], 0xA0);

        // Ext Byte 3: (3 << 5) | (1 << 3) | 0x07 = 0x60 | 0x08 | 0x07 = 0x6F
        assert_eq!(dst_buffer[7], 0x6F);

        // Payload bytes
        assert_eq!(&dst_buffer[8..10], &[0x12, 0x34]);
        assert_eq!(dst_len, 10);
    }

    #[test]
    fn test_wels_encode_nal_buffer_too_small() {
        let mut raw_payload = [0x00; 100];
        let mut raw_nal = SWelsNalRaw::default();
        raw_nal.pRawData = raw_payload.as_mut_ptr();
        raw_nal.iPayloadSize = 100;
        raw_nal.sNalExt.sNalUnitHeader.eNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;

        let mut dst_buffer = [0u8; 10]; // Much too small
        let mut dst_len: i32 = 0;

        let ret = unsafe {
            WelsEncodeNal(
                &mut raw_nal,
                core::ptr::null_mut(),
                dst_buffer.len() as i32,
                dst_buffer.as_mut_ptr() as *mut c_void,
                &mut dst_len,
            )
        };

        assert_eq!(ret, ENC_RETURN_MEMALLOCERR);
    }

    #[test]
    fn test_wels_load_and_unload_nal_slice() {
        let mut bs_buf = vec![0u8; 1024];
        let mut slice_bs = SWelsSliceBs::default();
        slice_bs.pBsBuffer = bs_buf.as_mut_ptr();
        slice_bs.uiSize = 1024;
        slice_bs.sBsWrite = BsWriter::new();

        unsafe {
            WelsLoadNalForSlice(
                &mut slice_bs,
                EWelsNalUnitType::NAL_UNIT_CODED_SLICE as i32,
                EWelsNalRefIdc::NRI_PRI_HIGH as i32,
            );
            assert_eq!(slice_bs.sNalList[0].iStartPos, 0);

            // Simulate writing 16 bits (2 bytes)
            BsWriteBits(&mut bs_buf, &mut slice_bs.sBsWrite, 16, 0xABCD);
            BsFlush(&mut bs_buf, &mut slice_bs.sBsWrite);

            WelsUnloadNalForSlice(&mut slice_bs);
            assert_eq!(slice_bs.sNalList[0].iPayloadSize, 2);
            assert_eq!(slice_bs.iNalIndex, 1);
        }
    }
}
