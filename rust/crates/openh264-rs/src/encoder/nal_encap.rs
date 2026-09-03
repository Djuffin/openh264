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
    unused_variables
)]


// ============================================================================
// Constants & Return Codes
// ============================================================================

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

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

// `sNalLen`'s elements are `AtomicI32` so that the C-ABI pointer `nal_len_ptr`
// hands the application stays valid while the encoder keeps writing lengths.
use std::sync::atomic::{AtomicI32, Ordering};

/// Raw payload data descriptor for a NAL unit before encapsulation.
///
/// The payload is `iStartPos .. iStartPos + iPayloadSize` of a buffer this record
/// does not name: the caller of [`WelsEncodeNal`] names the buffer — the frame's
/// `pOut->sBsBuffer` for the frame list, the thread buffer for a slice's list.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsNalRaw {
    pub iPayloadSize: i32,
    pub sNalExt: SNalUnitHeaderExt,
    pub iStartPos: i32,
}

impl Default for SWelsNalRaw {
    fn default() -> Self {
        Self {
            iPayloadSize: 0,
            sNalExt: SNalUnitHeaderExt::default(),
            iStartPos: 0,
        }
    }
}


/// Top-level frame bitstream output container and NAL descriptor list manager.
///
/// The `sNalList` entries carry offsets into `sBsBuffer` and no pointer: see
/// `SWelsNalRaw` — the caller of `WelsEncodeNal` passes `&sBsBuffer[..]` beside
/// the entry.
#[derive(Debug)]
pub struct SWelsEncoderOutput {
    pub sBsBuffer: Vec<u8>,
    pub sBsWrite: BsWriter,
    pub sNalList: Vec<SWelsNalRaw>,
    pub sNalLen: Vec<AtomicI32>,
    pub iNalIndex: i32,
    pub iLayerBsIndex: i32,
    /// Where the current layer's NAL lengths start in [`sNalLen`](Self::sNalLen),
    /// the safe half of `SLayerBSInfo::pNalLengthInByte`.
    ///
    /// The ABI struct the application walks carries a `*mut i32` per layer, each
    /// the previous layer's pointer advanced by that layer's NAL count. The
    /// storage behind every one of them is this struct's own `sNalLen`, so the
    /// pointer is a *derived* value and the position is the real state. The
    /// encoder reads and writes lengths by index; the pointer is stamped from
    /// this by [`nal_len_ptr`](Self::nal_len_ptr) wherever the ABI needs it.
    pub iNalLenBase: usize,
}

impl Default for SWelsEncoderOutput {
    fn default() -> Self {
        Self {
            sBsBuffer: Vec::new(),
            sBsWrite: BsWriter::new(),
            sNalList: Vec::new(),
            sNalLen: Vec::new(),
            iNalIndex: 0,
            iLayerBsIndex: 0,
            iNalLenBase: 0,
        }
    }
}

impl SWelsEncoderOutput {
    /// The frame output, constructed on the heap with its buffers sized.
    ///
    /// `WelsMallocz` zeroed what it returned, so the buffers start zeroed too.
    pub fn new_boxed(kiBsLen: usize, kiCountNals: usize) -> Box<Self> {
        Box::new(Self {
            sBsBuffer: vec![0u8; kiBsLen],
            sBsWrite: BsWriter::new(),
            sNalList: vec![SWelsNalRaw::default(); kiCountNals],
            sNalLen: (0..kiCountNals).map(|_| AtomicI32::new(0)).collect(),
            iNalIndex: 0,
            iLayerBsIndex: 0,
            iNalLenBase: 0,
        })
    }

    /// The current layer's NAL-length slot **as the C-ABI pointer the application
    /// walks** — `sNalLen`'s tail from [`iNalLenBase`](Self::iNalLenBase).
    ///
    /// A base equal to the array's length yields the one-past-the-end address,
    /// which the application never dereferences (its `iNalCount` is zero there).
    #[inline]
    pub fn nal_len_ptr(&self) -> *mut i32 {
        let kiBase = self.iNalLenBase.min(self.sNalLen.len());
        self.sNalLen[kiBase..].as_ptr().cast::<i32>().cast_mut()
    }

    /// The NAL length at `kiIdx` **within the current layer** — the safe form of
    /// `*pNalLengthInByte.add(kiIdx)`.
    #[inline]
    pub fn nal_len_at(&self, kiIdx: usize) -> i32 {
        self.sNalLen[self.iNalLenBase + kiIdx].load(Ordering::Relaxed)
    }

    /// [`nal_len_at`](Self::nal_len_at)'s write half.
    ///
    /// `Relaxed` is the right ordering: these slots are published to the
    /// application by `EncodeFrame`'s own return, which is the synchronisation
    /// edge, and no worker reads another worker's slot.
    #[inline]
    pub fn set_nal_len_at(&self, kiIdx: usize, kiLen: i32) {
        let kiBase = self.iNalLenBase;
        self.sNalLen[kiBase + kiIdx].store(kiLen, Ordering::Relaxed);
    }

    /// Advance to the next layer's slot — the safe form of the ABI chain's
    /// `next.pNalLengthInByte = prev.pNalLengthInByte.add(iNalCount)`.
    #[inline]
    pub fn advance_nal_len_base(&mut self, kiCount: usize) {
        self.iNalLenBase += kiCount;
    }
}

/// Thread-local bitstream state allocated per slice.
///
/// `pBs` is an `Option<Vec<u8>>` where the C++ has a `CMemoryAlign` block:
/// `InitSliceBsBuffer` fills it when the slice writes independently and leaves it
/// `None` when the slice shares the frame's buffer, and `is_some()` is the one bit
/// `slice_writer`/`slice_bs_buffer` read. `uiSize` is the *thread* buffer's length
/// and stays beside the writer that is positioned in it.
#[derive(Debug)]
pub struct SWelsSliceBs {
    pub pBs: Option<Vec<u8>>,
    pub uiBsSize: u32,
    pub uiBsPos: u32,
    pub uiSize: u32,
    pub sBsWrite: BsWriter,
    pub sNalList: [SWelsNalRaw; 2],
    pub iNalLen: [i32; 2],
    pub iNalIndex: i32,
}

impl Default for SWelsSliceBs {
    fn default() -> Self {
        Self {
            pBs: None,
            uiBsSize: 0,
            uiBsPos: 0,
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
// `codec/common/inc/golomb_common.h`.
pub use crate::encoder::vlc_encoder::{
    BsFlush, BsGetBitsPos, BsRbspTrailingBits, BsWriteBits, BsWriteOneBit,
};

// ============================================================================
// Core NAL Encapsulation Functions
// ============================================================================

/// Initializes a new raw NAL unit entry in the global encoder output context.
#[inline]
pub fn WelsLoadNal(
    pEncoderOuput: &mut SWelsEncoderOutput,
    kiType: i32,
    kiNalRefIdc: i32,
) {
    if pEncoderOuput.sNalList.is_empty() {
        return;
    }
    let pWelsEncoderOuput = pEncoderOuput;
    let iNalIndex = pWelsEncoderOuput.iNalIndex as usize;
    let kiStartPos = BsGetBitsPos(&pWelsEncoderOuput.sBsWrite) >> 3;
    let pRawNal = &mut pWelsEncoderOuput.sNalList[iNalIndex];
    let sNalUnitHeader = &mut pRawNal.sNalExt.sNalUnitHeader;

    sNalUnitHeader.eNalUnitType = EWelsNalUnitType::from(kiType);
    sNalUnitHeader.uiNalRefIdc = kiNalRefIdc as u8;
    sNalUnitHeader.uiForbiddenZeroBit = 0;

    pRawNal.iStartPos = kiStartPos;
    pRawNal.iPayloadSize = 0;
}

/// Finalizes the raw NAL unit currently being written in `pEncoderOuput`.
#[inline]
pub fn WelsUnloadNal(pEncoderOuput: &mut SWelsEncoderOutput) {
    if pEncoderOuput.sNalList.is_empty() {
        return;
    }
    let pWelsEncoderOuput = pEncoderOuput;
    let kiEndPos = BsGetBitsPos(&pWelsEncoderOuput.sBsWrite) >> 3;
    let iIdx = pWelsEncoderOuput.iNalIndex as usize;
    let pRawNal = &mut pWelsEncoderOuput.sNalList[iIdx];

    /* count payload size of raw NAL */
    pRawNal.iPayloadSize = kiEndPos - pRawNal.iStartPos;

    pWelsEncoderOuput.iNalIndex += 1;
}

/// Initializes a raw NAL unit entry for a thread-local slice bitstream context.
#[inline]
pub extern "C" fn WelsLoadNalForSlice(
    pSliceBs: &mut SWelsSliceBs,
    kiType: i32,
    kiNalRefIdc: i32,
) {
    let pSlice = pSliceBs;
    let pRawNal = &mut pSlice.sNalList[pSlice.iNalIndex as usize];
    let sNalUnitHeader = &mut pRawNal.sNalExt.sNalUnitHeader;
    let kiStartPos = BsGetBitsPos(&pSlice.sBsWrite) >> 3;

    sNalUnitHeader.eNalUnitType = EWelsNalUnitType::from(kiType);
    sNalUnitHeader.uiNalRefIdc = kiNalRefIdc as u8;
    sNalUnitHeader.uiForbiddenZeroBit = 0;

    pRawNal.iStartPos = kiStartPos;
    pRawNal.iPayloadSize = 0;
}

/// Finalizes the slice-thread-local raw NAL unit payload size and advances the NAL index.
#[inline]
pub extern "C" fn WelsUnloadNalForSlice(pSliceBs: &mut SWelsSliceBs) {
    let pSlice = pSliceBs;
    let pIdx = &mut pSlice.iNalIndex;
    let pRawNal = &mut pSlice.sNalList[*pIdx as usize];
    let kiEndPos = BsGetBitsPos(&pSlice.sBsWrite) >> 3;

    /* count payload size of raw NAL */
    pRawNal.iPayloadSize = kiEndPos - pRawNal.iStartPos;
    *pIdx += 1;
}

/// Encapsulates an unescaped raw NAL payload into an Annex B compliant byte stream (EBSP).
///
/// Prepends the 4-byte start code prefix (`0x00000001`), packs the 1-byte base NAL header
/// or 4-byte SVC extension header, and performs emulation prevention byte insertion (`0x03`).
///
/// The payload is `src[raw.iStartPos .. raw.iStartPos + raw.iPayloadSize]`: the
/// record carries the offset and the caller names the buffer it is an offset
/// into — the frame's `pOut->sBsBuffer` for the frame NAL list, the thread buffer
/// for a slice's own list. `ext` is the SVC extension header, needed exactly when
/// the NAL type is a prefix or an extension slice; the C++ took it as `void*` and
/// cast back to the one type here.
#[inline]
pub fn WelsEncodeNal(
    raw: &SWelsNalRaw,
    src: &[u8],
    ext: Option<&SNalUnitHeaderExt>,
    dst: Option<&mut [u8]>,
    out_len: &mut i32,
) -> i32 {
    let Some(dst) = dst else {
        return ENC_RETURN_INVALIDINPUT;
    };
    let dst_len = dst.len() as i32;
    let nal_type = raw.sNalExt.sNalUnitHeader.eNalUnitType;
    let kbNALExt = nal_type == EWelsNalUnitType::NAL_UNIT_PREFIX
        || nal_type == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;

    let iAssumedNeededLength =
        (NAL_HEADER_SIZE as i32) + (if kbNALExt { 3 } else { 0 }) + raw.iPayloadSize + 1;

    if iAssumedNeededLength <= 0 {
        return ENC_RETURN_UNEXPECTED;
    }

    // Since for each 0x000 need a 0x03, the needed length will not exceed (iAssumedNeededLength + iAssumedNeededLength / 3).
    // Here adjusted to >> 1 to omit division.
    if dst_len < (iAssumedNeededLength + (iAssumedNeededLength >> 1)) {
        return ENC_RETURN_MEMALLOCERR;
    }

    let mut iDstPos = 0usize;
    let payload = &src[raw.iStartPos as usize..(raw.iStartPos + raw.iPayloadSize) as usize];
    let mut iZeroCount: i32 = 0;

    *out_len = 0;

    // 4-byte Annex B start code prefix: 0x00 0x00 0x00 0x01
    let kuiStartCodePrefix: [u8; 4] = [0, 0, 0, 1];
    dst[iDstPos..iDstPos + 4].copy_from_slice(&kuiStartCodePrefix);
    iDstPos += 4;

    // 1-Byte NAL Unit Header
    let nri = raw.sNalExt.sNalUnitHeader.uiNalRefIdc;
    let utype = raw.sNalExt.sNalUnitHeader.eNalUnitType as u8;
    dst[iDstPos] = (nri << 5) | (utype & 0x1f);
    iDstPos += 1;

    if kbNALExt {
        // The C++ dereferenced its `void*` here unconditionally; every caller
        // that emits a prefix or extension NAL passes the layer's header.
        let sNalExt = ext.expect("a prefix or extension NAL is encoded with its SVC extension header");

        // Extension Byte 1: reserved_one_bit (0x80) | idr_flag (bit 6)
        dst[iDstPos] = 0x80 | ((sNalExt.bIdrFlag as u8) << 6);
        iDstPos += 1;

        // Extension Byte 2: no_inter_layer_pred_flag (0x80) | dependency_id (bits 6..4)
        dst[iDstPos] = 0x80 | ((sNalExt.uiDependencyId) << 4);
        iDstPos += 1;

        // Extension Byte 3: temporal_id (bits 7..5) | discardable_flag (bit 3) | reserved_three_2bits (0x07)
        dst[iDstPos] = ((sNalExt.uiTemporalId) << 5)
            | ((sNalExt.bDiscardableFlag as u8) << 3)
            | 0x07;
        iDstPos += 1;
    }

    // Emulation prevention escaping loop
    for &byte_val in payload {
        if iZeroCount == 2 && byte_val <= 3 {
            // Add emulation prevention byte 0x03
            dst[iDstPos] = 3;
            iDstPos += 1;
            iZeroCount = 0;
        }
        if byte_val == 0 {
            iZeroCount += 1;
        } else {
            iZeroCount = 0;
        }
        dst[iDstPos] = byte_val;
        iDstPos += 1;
    }

    *out_len = iDstPos as i32;

    ENC_RETURN_SUCCESS
}

/// Writes the RBSP payload for an SVC Prefix NAL unit (NAL unit type 14).
///
/// # Safety
/// - `pBsWriter` must point to a valid `BsWriter`, and `buf` must be the
///   buffer that writer is positioned in.
#[inline]
pub fn WelsWriteSVCPrefixNal(
    buf: &mut [u8],
    pBsWriter: &mut BsWriter,
    kiNalRefIdc: i32,
    _kbIdrFlag: bool,
) -> i32 {
    if kiNalRefIdc > 0 {
        let pBs = &mut *pBsWriter;
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
        let raw_payload = [0x00, 0x00, 0x01, 0xAA, 0x00, 0x00, 0x00, 0xBB];
        let mut raw_nal = SWelsNalRaw::default();
        raw_nal.iPayloadSize = raw_payload.len() as i32;
        raw_nal.sNalExt.sNalUnitHeader.eNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        raw_nal.sNalExt.sNalUnitHeader.uiNalRefIdc = EWelsNalRefIdc::NRI_PRI_HIGHEST as u8;

        let mut dst_buffer = [0u8; 128];
        let mut dst_len: i32 = 0;

        let ret = WelsEncodeNal(
            &raw_nal,
            &raw_payload,
            None,
            Some(&mut dst_buffer),
            &mut dst_len,
        );

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
        let raw_payload = [0x12, 0x34];
        let mut raw_nal = SWelsNalRaw::default();
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

        let ret = WelsEncodeNal(
            &raw_nal,
            &raw_payload,
                Some(&ext_header),
            Some(&mut dst_buffer),
            &mut dst_len,
        );

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
        let raw_payload = [0x00; 100];
        let mut raw_nal = SWelsNalRaw::default();
        raw_nal.iPayloadSize = 100;
        raw_nal.sNalExt.sNalUnitHeader.eNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;

        let mut dst_buffer = [0u8; 10]; // Much too small
        let mut dst_len: i32 = 0;

        let ret = WelsEncodeNal(
            &raw_nal,
            &raw_payload,
            None,
            Some(&mut dst_buffer),
            &mut dst_len,
        );

        assert_eq!(ret, ENC_RETURN_MEMALLOCERR);
    }

    #[test]
    fn test_wels_load_and_unload_nal_slice() {
        let mut bs_buf = vec![0u8; 1024];
        let mut slice_bs = SWelsSliceBs::default();
        slice_bs.uiSize = 1024;
        slice_bs.sBsWrite = BsWriter::new();

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
