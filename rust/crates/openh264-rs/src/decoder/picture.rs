// Copyright (c) 2013, Cisco Systems
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

//! # Reconstructed Picture & Reference Frame Management (`picture.h`)
//!
//! Translated from `codec/decoder/core/inc/picture.h`.
//!
//! Defines the [`SPicture`] structure and [`PPicture`] pointer typedef representing
//! decoded video frame buffers in OpenH264. An [`SPicture`] serves multiple core roles:
//!
//! 1. **Reconstruction Target Canvas**: Memory container for uncompressed YUV 4:2:0
//!    planar pixel samples assembled during macroblock reconstruction and in-loop deblocking.
//! 2. **Reference Frame Storage (DPB)**: Stored in the Decoded Picture Buffer (`SPicBuff`)
//!    and referenced during inter-prediction motion compensation.
//! 3. **Direct Mode & Temporal MV Cache**: Retains macroblock types (`pMbType`),
//!    motion vectors (`pMv`), and reference picture indices (`pRefIndex`) for B-slice
//!    temporal direct mode derivation.
//! 4. **Multi-Threaded Row Synchronization**: Embeds row-level event barriers (`pReadyEvent`)
//!    used by worker threads during parallel macroblock row reconstruction.
//! 5. **Error Concealment Metadata**: Tracks macroblock decoding integrity flags and error
//!    propagation counters (`iMbEcedNum`, `iMbEcedPropNum`).

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

// Constants matching OpenH264 common definitions (`wels_const_common.h` and `wels_common_defs.h`)

/// Number of 4x4 sub-blocks in a 16x16 macroblock.
pub const MB_BLOCK4x4_NUM: usize = 16;

/// Motion vector coordinate dimension (X and Y components).
pub const MV_A: usize = 2;

/// Reference picture list index 0.
pub const LIST_0: usize = 0;

/// Reference picture list index 1.
pub const LIST_1: usize = 1;

/// Total number of reference picture lists (LIST_0 and LIST_1).
pub const LIST_A: usize = 2;

/// Standard border padding width/height in pixels for SIMD memory alignment and FIR filtering.
pub const PADDING_LENGTH: i32 = 32;

/// Memory resolution alignment boundary in bytes.
pub const PICTURE_RESOLUTION_ALIGNMENT: i32 = 32;

/// Base H.264 slice types matching `EWelsSliceType` in `wels_common_defs.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsSliceType {
    #[default]
    P_SLICE = 0,
    B_SLICE = 1,
    I_SLICE = 2,
    SP_SLICE = 3,
    SI_SLICE = 4,
    UNKNOWN_SLICE = 5,
}

/// Decoder synchronization event representation matching `SWelsDecEvent` in `wels_decoder_thread.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsDecEvent {
    pub manualReset: i32,
    pub isSignaled: i32,
    pub c: [u8; 48],
    pub m: [u8; 40],
}

impl Default for SWelsDecEvent {
    fn default() -> Self {
        Self {
            manualReset: 0,
            isSignaled: 0,
            c: [0; 48],
            m: [0; 40],
        }
    }
}

/// Reconstructed Picture definition.
///
/// Expresses reference pictures stored in the DPB, reconstructed pictures for display output,
/// and temporal motion vector caches for direct-mode B-slice decoding.
///
/// Matches C++ `struct SPicture` from `codec/decoder/core/inc/picture.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPicture {
    // =========================================================================
    // Payload Pixel Buffers & Geometries
    // =========================================================================

    /// Pointer to the first allocated byte for each plane buffer (including padding border margins).
    /// Index 0: Y (Luma), 1: Cb (Chroma U), 2: Cr (Chroma V), 3: Reserved.
    pub pBuffer: [*mut u8; 4],

    /// Pointer to the top-left visible pixel (0, 0) for each color plane respectively.
    pub pData: [*mut u8; 4],

    /// Memory line stride (bytes per row) for each picture plane.
    pub iLinesize: [i32; 4],

    /// Number of planes introduced by the color space format (typically 3 for YUV 4:2:0).
    pub iPlanes: i32,

    // =========================================================================
    // Error Concealment & Syntax Flags
    // =========================================================================

    /// Flag indicating whether the picture is an IDR (Instantaneous Decoder Refresh) keyframe.
    pub bIdrFlag: bool,

    /// Active picture width in luma pixels (from Sequence Parameter Set).
    pub iWidthInPixel: i32,

    /// Active picture height in luma pixels (from Sequence Parameter Set).
    pub iHeightInPixel: i32,

    /// Picture Order Count (POC) parsed from the slice header.
    pub iFramePoc: i32,

    // =========================================================================
    // Reference Picture Management
    // =========================================================================

    /// `true` if this picture is currently marked as a reference frame in the DPB.
    pub bUsedAsRef: bool,

    /// `true` if this picture is marked as a Long-Term Reference (LTR) frame.
    pub bIsLongRef: bool,

    /// Reference usage counter. Prevents buffer recycling while held by threads or DPB lists.
    pub iRefCount: i8,

    /// Callback function pointer invoked to clear reference marking and unreference the picture.
    pub pSetUnRef: Option<unsafe extern "C" fn(*mut SPicture)>,

    /// `true` if all macroblocks in this picture were completely and cleanly decoded from the bitstream.
    pub bIsComplete: bool,

    // =========================================================================
    // Scalable Video Coding (SVC) & Identification Tags
    // =========================================================================

    /// SVC Temporal Layer Identifier (T_id in [0, 7]).
    pub uiTemporalId: u8,

    /// SVC Spatial Dependency Layer Identifier (D_id in [0, 7]).
    pub uiSpatialId: u8,

    /// SVC Quality Layer Identifier (Q_id in [0, 15]).
    pub uiQualityId: u8,

    /// `frame_num` syntax element parsed from slice header.
    pub iFrameNum: i32,

    /// Normalized frame wrap number used during reference picture list construction.
    pub iFrameWrapNum: i32,

    /// Long-term frame index assigned via MMCO commands (`long_term_frame_idx`).
    pub iLongTermFrameIdx: i32,

    /// Derived long-term picture number (`long_term_pic_num`).
    pub uiLongTermPicNum: u32,

    /// Bound Sequence Parameter Set ID (`seq_parameter_set_id`).
    pub iSpsId: i32,

    /// Bound Picture Parameter Set ID (`pic_parameter_set_id`).
    pub iPpsId: i32,

    /// Presentation timestamp (PTS) in microseconds / clock ticks.
    pub uiTimeStamp: u64,

    /// Monotonic relative decoding timestamp counter.
    pub uiDecodingTimeStamp: u32,

    /// Index of this picture node inside the parent `SPicBuff` buffer pool.
    pub iPicBuffIdx: i32,

    /// Primary slice type of the picture (`I_SLICE`, `P_SLICE`, `B_SLICE`, etc.).
    pub eSliceType: EWelsSliceType,

    /// Multi-slice picture flag where each slice group contains exactly one slice.
    pub bIsUngroupedMultiSlice: bool,

    /// Flag indicating that this picture begins a new video sequence.
    pub bNewSeqBegin: bool,

    /// Total number of macroblocks in this picture that underwent error concealment.
    pub iMbEcedNum: i32,

    /// Number of macroblocks affected by error-concealment propagation.
    pub iMbEcedPropNum: i32,

    /// Total macroblock count in this picture (iMbWidth * iMbHeight).
    pub iMbNum: i32,

    // =========================================================================
    // Macroblock Level Metadata & Direct Mode Caches
    // =========================================================================

    /// Array of boolean flags indicating clean decoding status per macroblock (`[iMbNum]`).
    pub pMbCorrectlyDecodedFlag: *mut bool,

    /// Non-Zero Count (NZC) transform coefficient table for multi-threaded decoding context sharing.
    pub pNzc: *mut [i8; 24],

    /// Array of macroblock coding types (`MB_TYPE_*`) used for direct mode derivation (`[iMbNum]`).
    pub pMbType: *mut u32,

    /// Motion vectors per 4x4 block for `LIST_0` and `LIST_1` (used for B-slice direct mode).
    pub pMv: [*mut [[i16; MV_A]; MB_BLOCK4x4_NUM]; LIST_A],

    /// Reference frame indices per 4x4 block for `LIST_0` and `LIST_1` (used for direct mode).
    pub pRefIndex: [*mut [i8; MB_BLOCK4x4_NUM]; LIST_A],

    /// Pointers to active reference pictures in `LIST_0` and `LIST_1` used for motion compensation.
    pub pRefPic: [[*mut SPicture; 17]; LIST_A],

    /// Macroblock row-level synchronization event array for multi-threaded decoding (`[iMbHeight]`).
    pub pReadyEvent: *mut SWelsDecEvent,
}

/// Pointer typedef for reconstructed pictures matching `typedef struct SPicture* PPicture;`.
pub type PPicture = *mut SPicture;

impl Default for SPicture {
    fn default() -> Self {
        Self {
            pBuffer: [std::ptr::null_mut(); 4],
            pData: [std::ptr::null_mut(); 4],
            iLinesize: [0; 4],
            iPlanes: 0,
            bIdrFlag: false,
            iWidthInPixel: 0,
            iHeightInPixel: 0,
            iFramePoc: 0,
            bUsedAsRef: false,
            bIsLongRef: false,
            iRefCount: 0,
            pSetUnRef: None,
            bIsComplete: false,
            uiTemporalId: 0,
            uiSpatialId: 0,
            uiQualityId: 0,
            iFrameNum: 0,
            iFrameWrapNum: 0,
            iLongTermFrameIdx: 0,
            uiLongTermPicNum: 0,
            iSpsId: 0,
            iPpsId: 0,
            uiTimeStamp: 0,
            uiDecodingTimeStamp: 0,
            iPicBuffIdx: 0,
            eSliceType: EWelsSliceType::UNKNOWN_SLICE,
            bIsUngroupedMultiSlice: false,
            bNewSeqBegin: false,
            iMbEcedNum: 0,
            iMbEcedPropNum: 0,
            iMbNum: 0,
            pMbCorrectlyDecodedFlag: std::ptr::null_mut(),
            pNzc: std::ptr::null_mut(),
            pMbType: std::ptr::null_mut(),
            pMv: [std::ptr::null_mut(); LIST_A],
            pRefIndex: [std::ptr::null_mut(); LIST_A],
            pRefPic: [[std::ptr::null_mut(); 17]; LIST_A],
            pReadyEvent: std::ptr::null_mut(),
        }
    }
}

impl SPicture {
    /// Constructs a new zeroed [`SPicture`] instance.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks if the picture buffer is free and available for recycling in the DPB pool.
    ///
    /// A picture node in `SPicBuff` is eligible for reuse if and only if:
    /// `!bUsedAsRef && iRefCount <= 0`
    #[inline]
    pub fn is_free(&self) -> bool {
        !self.bUsedAsRef && self.iRefCount <= 0
    }

    /// Unreferences the picture, releasing reference marks or invoking the `pSetUnRef` callback.
    ///
    /// # Safety
    /// Must only be called with a valid pointer / mutable reference to an initialized picture.
    pub unsafe fn unref(&mut self) {
        if let Some(func) = self.pSetUnRef {
            unsafe {
                func(self as *mut SPicture);
            }
        } else {
            self.bUsedAsRef = false;
            self.bIsLongRef = false;
            if self.iRefCount > 0 {
                self.iRefCount -= 1;
            }
        }
    }

    /// Calculates active `pData[0..2]` plane start pointers from physical `pBuffer[0..2]` bases
    /// and line strides using the standard OpenH264 border padding formula.
    ///
    /// - Luma Y: `pData[0] = pBuffer[0] + (1 + iLinesize[0]) * padding_length`
    /// - Chroma Cb: `pData[1] = pBuffer[1] + ((1 + iLinesize[1]) * padding_length) / 2`
    /// - Chroma Cr: `pData[2] = pBuffer[2] + ((1 + iLinesize[2]) * padding_length) / 2`
    ///
    /// # Safety
    /// Requires `pBuffer[0..2]` to point to allocated memory buffers of sufficient size.
    pub unsafe fn calculate_data_pointers(&mut self, padding_length: i32) {
        if !self.pBuffer[0].is_null() {
            let offset_y = (1 + self.iLinesize[0]) * padding_length;
            self.pData[0] = unsafe { self.pBuffer[0].offset(offset_y as isize) };
        }
        if !self.pBuffer[1].is_null() {
            let offset_cb = ((1 + self.iLinesize[1]) * padding_length) / 2;
            self.pData[1] = unsafe { self.pBuffer[1].offset(offset_cb as isize) };
        }
        if !self.pBuffer[2].is_null() {
            let offset_cr = ((1 + self.iLinesize[2]) * padding_length) / 2;
            self.pData[2] = unsafe { self.pBuffer[2].offset(offset_cr as isize) };
        }
    }
}

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_picture_initialization() {
        let pic = SPicture::new();
        assert!(pic.is_free());
        assert_eq!(pic.bUsedAsRef, false);
        assert_eq!(pic.iRefCount, 0);
        assert_eq!(pic.eSliceType, EWelsSliceType::UNKNOWN_SLICE);
        assert_eq!(pic.pBuffer[0], std::ptr::null_mut());
    }

    #[test]
    fn test_picture_unref() {
        let mut pic = SPicture::new();
        pic.bUsedAsRef = true;
        pic.iRefCount = 1;
        assert!(!pic.is_free());

        unsafe {
            pic.unref();
        }

        assert!(pic.is_free());
        assert_eq!(pic.bUsedAsRef, false);
        assert_eq!(pic.iRefCount, 0);
    }
}
