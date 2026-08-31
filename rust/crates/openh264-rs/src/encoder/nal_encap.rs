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


// ============================================================================
// Constants & Return Codes
// ============================================================================

#![deny(unsafe_code)]

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
///
/// **The payload is `iStartPos .. iStartPos + iPayloadSize` of a buffer this record
/// does not name.** The C++ `pRawData` (`buffer + iStartPos`, stamped at load) is
/// gone: it was redundant with the offset from the day it was written, and storing
/// it was the encoder probe's fourth finding (session A) — the writer's fresh
/// `&mut sBsBuffer[..]` killed the stored pointer between load and encode. Phase 3
/// left it because one type cannot hold offsets into two owners; it can hold an
/// offset into *no* owner, and the caller of [`WelsEncodeNal`] names the buffer —
/// the frame's `pOut->sBsBuffer` for the frame list, the thread buffer for a
/// slice's list. Phase 6 session B.
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

/// The one place that turns an encoder output buffer back into a slice.
///
/// SHIM(phase3) -> **the thread pool's own bitstream buffers, and nothing else
/// now** — `SSliceThreading.pThreadBsBuffer[i]`, one raw allocation per worker,
/// **Phase 7's** (F12/P10) with the pool that claims them. `BsWriter` is a position
/// and nothing else, so the buffer has to be expressed at each write, and for those
/// buffers it still means rebuilding a slice from a raw pointer and the `uiSize`
/// recorded beside it. One helper does that arithmetic and nothing else does it,
/// exactly as T3.1b's reader-side helper did until T3.3 deleted it.
///
/// **T3.6 took `SWelsEncoderOutput` off this path**: its buffer is a `Vec<u8>`,
/// so its callers slice it directly and the length is `len()` rather than a
/// field. **Phase 6 session B took `SWelsSliceBs.pBsBuffer` off it**: that field
/// was a cache of `pThreadBsBuffer[uiBufferIdx]` and is gone; the three readers
/// (`slice_bs_buffer`'s thread arm, the task's prefix NAL, `WriteSliceBs`) resolve
/// the pool slot by index through `thread_bs_buffer` and come here for the slice.
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
/// no other live reference to them — which is what a claimed `pThreadBsBuffer[i]`
/// plus the slice's `uiSize` is, and what the task-claiming invariant gives per
/// thread.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn bs_buffer<'a>(ptr: *mut u8, len: u32) -> &'a mut [u8] {
    debug_assert!(!ptr.is_null(), "a writer's buffer must be allocated first");
    unsafe { core::slice::from_raw_parts_mut(ptr, len as usize) }
}

/// Top-level frame bitstream output container and NAL descriptor list manager.
///
/// **T3.6 made the three allocations owned.** They were `WelsMallocz`'d pointers
/// with their lengths recorded beside them (`uiSize`, `iCountNals`); they are
/// `Vec`s now, and the lengths are gone because a `Vec` already knows — the
/// T3.3 standard, which says extents are `buf.len()` and not fields. The
/// `CMemoryAlign` entries that allocated them and the four
/// `WelsUninitEncoderExt` entries that freed them are gone with them.
///
/// `Copy`/`Clone` are gone too, necessarily: this owns its buffers now, and a
/// bitwise copy of an owner is a double free waiting to happen. Nothing copied
/// it — the compiler confirmed that when the derive came off.
///
/// The `sNalList` entries carry offsets into `sBsBuffer` and no pointer: see
/// `SWelsNalRaw` — the caller of `WelsEncodeNal` passes `&sBsBuffer[..]` beside
/// the entry.
#[derive(Debug)]
pub struct SWelsEncoderOutput {
    pub sBsBuffer: Vec<u8>,
    pub sBsWrite: BsWriter,
    pub sNalList: Vec<SWelsNalRaw>,
    pub sNalLen: Vec<i32>,
    pub iNalIndex: i32,
    pub iLayerBsIndex: i32,
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
        }
    }
}

impl SWelsEncoderOutput {
    /// The frame output, constructed on the heap with its buffers sized.
    ///
    /// This is what replaced `RequestMemorySvc`'s four `WelsMallocz` calls, and
    /// the reason it is a constructor rather than four assignments is **S21**:
    /// the old code wrote into zeroed memory, which is a valid `*mut u8` and is
    /// *not* a valid `Vec`. Assigning a `Vec` into a zeroed field drops the
    /// zeroed one first — UB at a distance, invisible to every test, which is
    /// the incident S21 exists to prevent. There is no zeroed intermediate
    /// state here: the struct is built whole and then boxed.
    ///
    /// `WelsMallocz` zeroed what it returned, so the buffers start zeroed too.
    ///
    /// **The rest of the S21 audit, written down because "it seems to work" is
    /// not the standard.** There is exactly one construction path — this one,
    /// at `encoder_ext.rs`'s `RequestMemorySvc` — and the struct is reached only
    /// through `sWelsEncCtx::pOut`, *a raw pointer*. That matters: the encoder
    /// context is built by `mem::zeroed()` (`encoder_context.rs:516`, behind
    /// `Box::into_raw`), and zero is a valid null `*mut SWelsEncoderOutput`
    /// while it would **not** be a valid `Vec`. Because `pOut` is a pointer
    /// rather than a by-value member, the wholesale zeroing never reaches these
    /// fields, and no `MaybeUninit` shell is needed — unlike the decoder
    /// context, which embeds its owned buffers directly and needs
    /// `new_boxed`'s shell for exactly that reason.
    ///
    /// The same reasoning is what kept the S20 closure small: nothing embeds
    /// this struct by value, so flipping its fields moved no other layout.
    pub fn new_boxed(kiBsLen: usize, kiCountNals: usize) -> Box<Self> {
        Box::new(Self {
            sBsBuffer: vec![0u8; kiBsLen],
            sBsWrite: BsWriter::new(),
            sNalList: vec![SWelsNalRaw::default(); kiCountNals],
            sNalLen: vec![0i32; kiCountNals],
            iNalIndex: 0,
            iLayerBsIndex: 0,
        })
    }
}

/// Thread-local bitstream state allocated per slice.
///
/// **`pBsBuffer` is gone (Phase 6 session B).** The C++ field was the thread
/// bitstream buffer the slice's writer is positioned in, and it was a cache:
/// both stamp sites wrote `pSliceThreading->pThreadBsBuffer[idx]` with the same
/// `idx` `InitOneSliceInThread` stores in `SSlice.uiBufferIdx`, so the slot is
/// already named and `thread_bs_buffer` resolves it at each use — nothing aliases
/// the pool's allocation from inside this struct any more. What is still raw here
/// **`pBs` is owned since T7.C4** — an `Option<Vec<u8>>` where the C++ has a
/// `CMemoryAlign` block. `InitSliceBsBuffer` fills it when the slice writes
/// independently and leaves it `None` when the slice shares the frame's buffer, and
/// **`is_some()` is the one bit `slice_writer`/`slice_bs_buffer` read** — the same
/// discriminator the raw pointer's nullness carried, which is why the conversion moves
/// nothing else. `Option<Vec<u8>>` rather than a bare `Vec<u8>`: an empty `Vec` and an
/// allocated one are the same thing to `is_empty()` if a caller ever asks for a
/// zero-length buffer, and the choice this field records must not be able to collapse.
/// The slice's own drop is what `FreeSliceBuffer`'s walk used to be.
///
/// `repr(C)` and `Copy` come off with the pointer: an `Option<Vec<u8>>` has no C shape
/// and owns its storage. Nothing copied this struct by value — the compiler's answer,
/// not an argument. `uiSize` is the *thread* buffer's length and stays beside the
/// writer that is positioned in it; the pool's buffers are `pThreadBsBuffer`'s, owned
/// at T7.C5.
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
/// **S6.C1**: safe, and the `# Safety` clause retired with the pointer — both of its
/// obligations are the type's now. "Must point to a valid structure" is what `&mut`
/// means, and "must have enough capacity for `iNalIndex`" is what indexing `sNalList`
/// checks. (F231's class, one function at a time.)
#[inline]
pub fn WelsLoadNal(
    pEncoderOuput: &mut SWelsEncoderOutput,
    kiType: i32,
    kiNalRefIdc: i32,
) {
    // **S6.C1**: `&mut`, and no call site changed — all nine already passed
    // `pCtx.pOut.as_deref_mut().expect("pOut lives")` or `&mut *pOut` and were relying
    // on the coercion. The `is_null()` disjunct goes with the parameter; the
    // `sNalList.is_empty()` one is the real guard and stays, answering for a list the
    // allocator never sized.
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
///
/// **S6.C1**: safe; the `# Safety` clause retired with the pointer it described.
#[inline]
// unsafe-cat: port-raw(Phase 9)
// **T9.X — this is not a C-ABI boundary, and the tag stays `port-raw`.** The brief
// calls it one of "the two `unsafe extern \"C\"` unload fns ... lawful remainder".
// It carries no export attribute and is installed into no dispatch slot, so the
// calling convention was a vestige of the raw translation rather than an ABI
// crossing. **T9.X2 dropped the `extern "C"` on that evidence** — and re-ran the
// enumeration first, because X's own count was short.
//
// X recorded "five callers ... `encoder_ext.rs:2254`, `:2263`, `:2318`, `:3161`,
// `:3606`". There are **nine**, and X's list misses four of them:
// `encoder_ext.rs:3814` and `wels_encoder_ext.rs:404`, `:442`, `:541`. The verdict
// is unchanged — all nine are ordinary Rust calls, which is the whole question —
// but a conclusion carried by an enumeration is only as good as the enumeration,
// and the second file was never grepped. S64, on its own evidence. See F180.
//
// **S6.C1 finishes it**: the parameter is `&mut SWelsEncoderOutput`, none of the nine
// callers changed — every one already passed a reference and was relying on the
// coercion — and the tag and allow retire together.
pub fn WelsUnloadNal(pEncoderOuput: &mut SWelsEncoderOutput) {
    // **S6.C1**, as `WelsLoadNal` above.
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
///
/// # Safety
/// - `pSliceBs` must point to a valid `SWelsSliceBs` structure.
#[inline]
// unsafe-cat: fork-shared(S63)
// **T9.X — adjudicated: the seam's, not the bitstream's (H2's, not X's).** G left
// this tag unattributed. Every production caller is in `slice_multi_threading.rs`
// (`:1369`, `:1379`, `:1442`, `:1732`) and the walker puts the body inside the fork:
//     WelsLoadNalForSlice <- EncodeOneSliceInJob <- fork seed (thread::scope spawn)
// `SWelsSliceBs` is the per-worker bitstream, so S63 applies: this route's end
// states are interior mutability or lawful raw, and naming it belongs to the
// session that designs the seam.
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
///
/// # Safety
/// - `pSliceBs` must point to a valid `SWelsSliceBs` structure.
#[inline]
// unsafe-cat: fork-shared(S63)
// **T9.X — adjudicated with [`WelsLoadNalForSlice`]: the seam's (H2's).**
//     WelsUnloadNalForSlice <- EncodeOnePartitionSizeLimited <- fork seed
// Note the brief also lists this function's line as one of "the two `unsafe extern
// \"C\"` unload fns ... C-ABI boundary, lawful remainder". It is neither: it is one
// of the three MT tags the same brief asks to adjudicate, and it is fork-reachable.
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
/// record carries the offset and **the caller names the buffer** it is an offset
/// into — the frame's `pOut->sBsBuffer` for the frame NAL list, the thread buffer
/// for a slice's own list — which is what let `SWelsNalRaw` drop its `pRawData`
/// pointer (see the type). `ext` is the SVC extension header, needed exactly when
/// the NAL type is a prefix or an extension slice; the C++ took it as `void*` and
/// cast back to the one type here.
///
/// # Safety
/// `dst` must be null (rejected with `ENC_RETURN_INVALIDINPUT`, as the C++ did) or
/// point to `dst_len` writable bytes that do not overlap `src`.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsEncodeNal(
    raw: &SWelsNalRaw,
    src: &[u8],
    ext: Option<&SNalUnitHeaderExt>,
    dst: *mut u8,
    dst_len: i32,
    out_len: &mut i32,
) -> i32 {
    if dst.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
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

    let pDstStart = dst;
    let mut pDstPointer = pDstStart;
    let payload = &src[raw.iStartPos as usize..(raw.iStartPos + raw.iPayloadSize) as usize];
    let mut iZeroCount: i32 = 0;

    *out_len = 0;

    // 4-byte Annex B start code prefix: 0x00 0x00 0x00 0x01
    let kuiStartCodePrefix: [u8; 4] = [0, 0, 0, 1];
    core::ptr::copy_nonoverlapping(kuiStartCodePrefix.as_ptr(), pDstPointer, 4);
    pDstPointer = pDstPointer.add(4);

    // 1-Byte NAL Unit Header
    let nri = raw.sNalExt.sNalUnitHeader.uiNalRefIdc;
    let utype = raw.sNalExt.sNalUnitHeader.eNalUnitType as u8;
    *pDstPointer = (nri << 5) | (utype & 0x1f);
    pDstPointer = pDstPointer.add(1);

    if kbNALExt {
        // The C++ dereferenced its `void*` here unconditionally; every caller
        // that emits a prefix or extension NAL passes the layer's header.
        let sNalExt = ext.expect("a prefix or extension NAL is encoded with its SVC extension header");

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
    for &byte_val in payload {
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
    }

    *out_len = pDstPointer.offset_from(pDstStart) as i32;

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
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_wels_encode_nal_standard_avc() {
        let raw_payload = [0x00, 0x00, 0x01, 0xAA, 0x00, 0x00, 0x00, 0xBB];
        let mut raw_nal = SWelsNalRaw::default();
        raw_nal.iPayloadSize = raw_payload.len() as i32;
        raw_nal.sNalExt.sNalUnitHeader.eNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        raw_nal.sNalExt.sNalUnitHeader.uiNalRefIdc = EWelsNalRefIdc::NRI_PRI_HIGHEST as u8;

        let mut dst_buffer = [0u8; 128];
        let mut dst_len: i32 = 0;

        let ret = unsafe {
            WelsEncodeNal(
                &raw_nal,
                &raw_payload,
                None,
                dst_buffer.as_mut_ptr(),
                dst_buffer.len() as i32,
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
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
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

        let ret = unsafe {
            WelsEncodeNal(
                &raw_nal,
                &raw_payload,
                Some(&ext_header),
                dst_buffer.as_mut_ptr(),
                dst_buffer.len() as i32,
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
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_wels_encode_nal_buffer_too_small() {
        let raw_payload = [0x00; 100];
        let mut raw_nal = SWelsNalRaw::default();
        raw_nal.iPayloadSize = 100;
        raw_nal.sNalExt.sNalUnitHeader.eNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;

        let mut dst_buffer = [0u8; 10]; // Much too small
        let mut dst_len: i32 = 0;

        let ret = unsafe {
            WelsEncodeNal(
                &raw_nal,
                &raw_payload,
                None,
                dst_buffer.as_mut_ptr(),
                dst_buffer.len() as i32,
                &mut dst_len,
            )
        };

        assert_eq!(ret, ENC_RETURN_MEMALLOCERR);
    }

    #[test]
    // unsafe-cat: C-ABI(test)
    // **T9.X — retagged from `MT`.** This is a test, and it drives a local
    // `SWelsSliceBs::default()` on the calling thread; nothing about it is
    // fork-reachable. Its `unsafe` is the ordinary one of calling an `unsafe extern
    // "C"` item from a test, which is what `C-ABI(test)` already means in this tree.
    // The other two `MT` tags in this file are real and stay.
    #[allow(unsafe_code)]
    fn test_wels_load_and_unload_nal_slice() {
        let mut bs_buf = vec![0u8; 1024];
        let mut slice_bs = SWelsSliceBs::default();
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
