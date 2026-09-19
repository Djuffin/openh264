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

//! # CABAC Binary Arithmetic Encoding Engine
//!
//! C++: `codec/encoder/core/inc/set_mb_syn_cabac.h`,
//! `codec/encoder/core/src/set_mb_syn_cabac.cpp`.
//!
//! The Context-Adaptive Binary Arithmetic Coding (CABAC) entropy encoder for
//! H.264 / AVC: the 64-bit low register (`cabac_low_t`), the context state models
//! (`SStateCtx` / `SCabacCtx`), regular bin decisions, equiprobable and Exp-Golomb
//! bypass coding, the slice terminating symbol and RBSP flush, and carry propagation
//! across output bytes.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
// ============================================================================
// Constants & Bit-Width Definitions
// ============================================================================
#![forbid(unsafe_code)]
use crate::encoder::encoder_context::sWelsEncCtx;

/// Maximum Quantization Parameter (QP) defined in H.264 standard.
pub const WELS_QP_MAX: i32 = 51;

pub use crate::common::cabac_tables::{
    WELS_CONTEXT_COUNT, g_kiCabacGlobalContextIdx, g_kuiCabacRangeLps, g_kuiStateTransTable,
};

/// Internal arithmetic coding lower interval register type (64-bit unsigned).
pub type cabac_low_t = u64;

/// Total bit-width of the `cabac_low_t` arithmetic interval register (64 bits).
pub const CABAC_LOW_WIDTH: usize = cabac_low_t::BITS as usize;

// ============================================================================
// Data Structures
// ============================================================================

/// Packed representation of a CABAC probability context model state and MPS flag.
///
/// Bit layout of `m_uiStateMps`:
/// - Bit 0: Most Probable Symbol (`MPS`, either `0` or `1`).
/// - Bits 1..6: Probability State Index $\sigma \in [0, 63]$.
/// - Bit 7: Always 0.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SStateCtx {
    pub m_uiStateMps: u8,
}

impl SStateCtx {
    #[inline(always)]
    pub const fn new(ui_state_mps: u8) -> Self {
        Self {
            m_uiStateMps: ui_state_mps,
        }
    }

    /// Returns the Most Probable Symbol (`MPS`), `0` or `1`.
    #[inline(always)]
    pub fn Mps(&self) -> u8 {
        self.m_uiStateMps & 1
    }

    /// Returns the 6-bit probability state index $\sigma \in [0, 63]$.
    #[inline(always)]
    pub fn State(&self) -> u8 {
        self.m_uiStateMps >> 1
    }

    /// Packs and updates the 6-bit state index and 1-bit MPS symbol.
    ///
    /// `uiState * 2 + uiMps` (`set_mb_syn_cabac.h:62`), not `(uiState << 1) | (uiMps &
    /// 1)`: the two agree only while `uiMps` is 0 or 1, and the shift form would
    /// overflow-panic in debug for `uiState >= 128`.
    #[inline(always)]
    pub fn Set(&mut self, uiState: u8, uiMps: u8) {
        self.m_uiStateMps = (uiState as u32 * 2 + uiMps as u32) as u8;
    }
}

pub type TagStateCtx = SStateCtx;

/// Full runtime state of the CABAC binary arithmetic encoding engine.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SCabacCtx {
    /// 64-bit arithmetic coding lower bound interval register ($L$).
    pub m_uiLow: cabac_low_t,
    /// Number of valid active bits accumulated in `m_uiLow`.
    pub m_iLowBitCnt: i32,
    /// Number of pending renormalization left-shifts.
    pub m_iRenormCnt: i32,
    /// Current arithmetic coding interval range ($R \in [256, 510]$).
    pub m_uiRange: u32,
    /// Array of 460 packed probability context model state machines.
    pub m_sStateCtx: [SStateCtx; WELS_CONTEXT_COUNT],
    /// Offset of this slice's first byte in the output buffer, not the allocation's
    /// base. `PropagateCarry`'s backward walk stops here.
    pub m_iBufStart: usize,
    /// One past the last byte the caller intends the coder to use.
    ///
    /// Written by `WelsCabacEncodeInit` and read nowhere: the limit it names is not
    /// enforced at any write site.
    pub m_iBufEnd: usize,
    /// Current byte-write cursor, as an offset into the output buffer.
    pub m_iBufCur: usize,
}

impl Default for SCabacCtx {
    fn default() -> Self {
        Self {
            m_uiLow: 0,
            m_iLowBitCnt: 0,
            m_iRenormCnt: 0,
            m_uiRange: 0,
            m_sStateCtx: [SStateCtx { m_uiStateMps: 0 }; WELS_CONTEXT_COUNT],
            m_iBufStart: 0,
            m_iBufEnd: 0,
            m_iBufCur: 0,
        }
    }
}

pub type TagCabacCtx = SCabacCtx;

// ============================================================================
// Lookup Tables
// ============================================================================

/// 5-bit count-leading-zeros lookup table for LPS range renormalization shifts.
pub const g_kiClz5Table: [i8; 32] = [
    6, 5, 4, 4, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];





// ============================================================================
// Core Arithmetic Functions
// ============================================================================

/// Traverses backwards from `iBufCur - 1` towards `iBufStart`, propagating a carry bit.
///
/// If byte `buf[iBufCur - 1]` overflows (`0xFF + 1 = 0x00`), the carry bit ripples
/// backwards to the preceding byte until a non-overflowing byte is incremented.
///
/// Two properties of the loop condition are load-bearing:
///
/// * It is `>`, not `>=`, and is checked before the decrement, so `iBufCur - 1` is
///   only ever evaluated where `iBufCur > iBufStart >= 0`. On `usize` the alternative
///   wraps to `usize::MAX`.
/// * The bound is `iBufStart`, the slice's first byte, not `0`: the walk must not
///   cross into a previous slice's bytes.
///
/// The `!= 0` early exit stops the ripple at the first byte that was not `0xFF`;
/// memory safety does not depend on it.
#[inline]
pub fn PropagateCarry(buf: &mut [u8], mut iBufCur: usize, iBufStart: usize) {
    while iBufCur > iBufStart {
        iBufCur -= 1;
        let val = buf[iBufCur].wrapping_add(1);
        buf[iBufCur] = val;
        if val != 0 {
            break;
        }
    }
}

/// Precomputes the global CABAC context model lookup tables for all 4 models, 52 QPs, and 460 contexts.
#[inline]
pub fn WelsCabacInitContexts(
    contexts: &mut [[[SStateCtx; WELS_CONTEXT_COUNT]; (WELS_QP_MAX + 1) as usize]; 4],
) {
    for iModel in 0..4 {
        for iQp in 0..=(WELS_QP_MAX as usize) {
            for iIdx in 0..WELS_CONTEXT_COUNT {
                let m = g_kiCabacGlobalContextIdx[iIdx][iModel][0] as i32;
                let n = g_kiCabacGlobalContextIdx[iIdx][iModel][1] as i32;
                let iPreCtxState = (((m * (iQp as i32)) >> 4) + n).clamp(1, 126);
                let (uiStateIdx, uiValMps) = if iPreCtxState <= 63 {
                    ((63 - iPreCtxState) as u8, 0u8)
                } else {
                    ((iPreCtxState - 64) as u8, 1u8)
                };
                contexts[iModel][iQp][iIdx].Set(uiStateIdx, uiValMps);
            }
        }
    }
}

/// `WelsCabacInit` — set_mb_syn_cabac.cpp:64. Fills `sWelsCabacContexts[4][52][460]`.
pub extern "C" fn WelsCabacInit(pEncCtx: &mut sWelsEncCtx) {
    WelsCabacInitContexts(&mut pEncCtx.sWelsCabacContexts);
}

/// Initializes the slice's active context models from a precomputed table.
#[inline]
pub fn WelsCabacContextInitFromContexts(
    pCbCtx: &mut SCabacCtx,
    contexts: &[[[SStateCtx; WELS_CONTEXT_COUNT]; (WELS_QP_MAX + 1) as usize]; 4],
    eSliceType: i32,
    iGlobalQp: i32,
    iModel: i32,
) {
    let iIdx = if eSliceType == 2 {
        0usize
    } else {
        (iModel + 1) as usize
    };
    let qp = (iGlobalQp.clamp(0, WELS_QP_MAX)) as usize;
    let model_idx = iIdx.min(3);
    pCbCtx.m_sStateCtx.copy_from_slice(&contexts[model_idx][qp]);
}

/// `WelsCabacContextInit` — set_mb_syn_cabac.cpp:86. Copies the model row for
/// this slice type and QP into the slice's own 460 context states.
pub extern "C" fn WelsCabacContextInit(pCtx: &sWelsEncCtx, pCbCtx: &mut SCabacCtx, iModel: i32) {
    let pEncCtx = pCtx;
    WelsCabacContextInitFromContexts(
        pCbCtx,
        &pEncCtx.sWelsCabacContexts,
        pEncCtx.eSliceType as i32,
        pEncCtx.iGlobalQp,
        iModel,
    );
}

/// Prepares the CABAC arithmetic encoding engine registers at the beginning of a slice NAL unit.
///
/// `iStart` is the slice's first byte as an offset into the output buffer, and
/// `iEnd` the caller's intended limit — which nothing enforces.
pub extern "C" fn WelsCabacEncodeInit(pCbCtx: &mut SCabacCtx, iStart: usize, iEnd: usize) {
    pCbCtx.m_uiLow = 0;
    pCbCtx.m_iLowBitCnt = 9;
    pCbCtx.m_iRenormCnt = 0;
    pCbCtx.m_uiRange = 510;
    pCbCtx.m_iBufStart = iStart;
    pCbCtx.m_iBufEnd = iEnd;
    pCbCtx.m_iBufCur = iStart;
}

/// Flushes accumulated bits from `m_uiLow` to the output bitstream when bit capacity reaches/exceeds 64 bits.
#[inline(never)]
pub fn WelsCabacEncodeUpdateLowNontrivial_(buf: &mut [u8], pCbCtx: &mut SCabacCtx) {
    let mut iLowBitCnt = pCbCtx.m_iLowBitCnt;
    let mut iRenormCnt = pCbCtx.m_iRenormCnt;
    let mut uiLow = pCbCtx.m_uiLow;

    loop {
        // Exactly six bytes forward per iteration — a 4-byte store then two
        // single-byte stores — plus the optional backward carry.
        let mut iBufCur = pCbCtx.m_iBufCur;
        let kiInc = (CABAC_LOW_WIDTH as i32) - 1 - iLowBitCnt;

        uiLow = uiLow.wrapping_shl(kiInc as u32);
        if (uiLow & (1u64 << ((CABAC_LOW_WIDTH as u32) - 1))) != 0 {
            PropagateCarry(buf, iBufCur, pCbCtx.m_iBufStart);
        }

        if CABAC_LOW_WIDTH > 32 {
            let be32 = ((uiLow >> 31) as u32).to_be_bytes();
            buf[iBufCur..iBufCur + 4].copy_from_slice(&be32);
            iBufCur += 4;
        }
        buf[iBufCur] = (uiLow >> 23) as u8;
        iBufCur += 1;
        buf[iBufCur] = (uiLow >> 15) as u8;
        iBufCur += 1;

        iRenormCnt -= kiInc;
        iLowBitCnt = 15;
        uiLow &= (1u64 << iLowBitCnt) - 1;
        pCbCtx.m_iBufCur = iBufCur;

        if (iLowBitCnt + iRenormCnt) <= ((CABAC_LOW_WIDTH as i32) - 1) {
            break;
        }
    }

    pCbCtx.m_iLowBitCnt = iLowBitCnt + iRenormCnt;
    pCbCtx.m_uiLow = uiLow.wrapping_shl(iRenormCnt as u32);
}

/// Inline fast path for updating the 64-bit lower bound register `m_uiLow`.
#[inline(always)]
pub fn WelsCabacEncodeUpdateLow_(buf: &mut [u8], pCbCtx: &mut SCabacCtx) {
    if (pCbCtx.m_iLowBitCnt + pCbCtx.m_iRenormCnt) < (CABAC_LOW_WIDTH as i32) {
        pCbCtx.m_iLowBitCnt += pCbCtx.m_iRenormCnt;
        pCbCtx.m_uiLow = pCbCtx.m_uiLow.wrapping_shl(pCbCtx.m_iRenormCnt as u32);
    } else {
        WelsCabacEncodeUpdateLowNontrivial_(buf, pCbCtx);
    }
    pCbCtx.m_iRenormCnt = 0;
}

/// Out-of-line slow path for Least Probable Symbol (LPS) bin encoding.
///
/// `iCtx` must be in $[0, \text{WELS\_CONTEXT\_COUNT} - 1]$.
#[inline]
pub fn WelsCabacEncodeDecisionLps_(buf: &mut [u8], pCbCtx: &mut SCabacCtx, iCtx: i32) {
    let ctx_idx = iCtx as usize;
    let kiState = pCbCtx.m_sStateCtx[ctx_idx].State() as usize;
    let mut uiRange = pCbCtx.m_uiRange;
    let uiRangeLps = g_kuiCabacRangeLps[kiState][((uiRange & 0xff) >> 6) as usize] as u32;
    uiRange = uiRange.wrapping_sub(uiRangeLps);

    let current_mps = pCbCtx.m_sStateCtx[ctx_idx].Mps();
    let toggle = if kiState == 0 { 1u8 } else { 0u8 };
    pCbCtx.m_sStateCtx[ctx_idx].Set(g_kuiStateTransTable[kiState][0], current_mps ^ toggle);

    WelsCabacEncodeUpdateLow_(buf, pCbCtx);
    pCbCtx.m_uiLow = pCbCtx.m_uiLow.wrapping_add(uiRange as u64);

    let kiRenormAmount = g_kiClz5Table[(uiRangeLps >> 3) as usize] as i32;
    pCbCtx.m_uiRange = uiRangeLps << (kiRenormAmount as u32);
    pCbCtx.m_iRenormCnt = kiRenormAmount;
}

/// Encodes a regular context-modeled binary symbol (`uiBin`).
///
/// `iCtx` must be in $[0, \text{WELS\_CONTEXT\_COUNT} - 1]$.
#[inline(always)]
pub fn WelsCabacEncodeDecision(buf: &mut [u8], pCbCtx: &mut SCabacCtx, iCtx: i32, uiBin: u32) {
    let ctx_idx = iCtx as usize;
    if (uiBin as u8) == pCbCtx.m_sStateCtx[ctx_idx].Mps() {
        let kiState = pCbCtx.m_sStateCtx[ctx_idx].State() as usize;
        let mut uiRange = pCbCtx.m_uiRange;
        let uiRangeLps = g_kuiCabacRangeLps[kiState][((uiRange & 0xff) >> 6) as usize] as u32;
        uiRange = uiRange.wrapping_sub(uiRangeLps);

        let kiRenormAmount = ((uiRange >> 8) ^ 1) as i32;
        pCbCtx.m_uiRange = uiRange << (kiRenormAmount as u32);
        pCbCtx.m_iRenormCnt += kiRenormAmount;
        pCbCtx.m_sStateCtx[ctx_idx].Set(g_kuiStateTransTable[kiState][1], uiBin as u8);
    } else {
        WelsCabacEncodeDecisionLps_(buf, pCbCtx, iCtx);
    }
}

/// Encodes an equiprobable bypass binary decision ($p = 0.5$).
#[inline(always)]
pub fn WelsCabacEncodeBypassOne(buf: &mut [u8], pCbCtx: &mut SCabacCtx, uiBin: i32) {
    let kuiBinBitmask = (uiBin as u32).wrapping_neg();
    pCbCtx.m_iRenormCnt += 1;
    WelsCabacEncodeUpdateLow_(buf, pCbCtx);
    let mask_range = (kuiBinBitmask & pCbCtx.m_uiRange) as u64;
    pCbCtx.m_uiLow = pCbCtx.m_uiLow.wrapping_add(mask_range);
}

/// Encodes terminating syntax elements (`end_of_slice_flag` or `I_PCM` type).
#[inline]
pub fn WelsCabacEncodeTerminate(buf: &mut [u8], pCbCtx: &mut SCabacCtx, uiBin: u32) {
    pCbCtx.m_uiRange = pCbCtx.m_uiRange.wrapping_sub(2);
    if uiBin != 0 {
        WelsCabacEncodeUpdateLow_(buf, pCbCtx);
        pCbCtx.m_uiLow = pCbCtx.m_uiLow.wrapping_add(pCbCtx.m_uiRange as u64);

        let kiRenormAmount: i32 = 7;
        pCbCtx.m_uiRange = 2 << (kiRenormAmount as u32);
        pCbCtx.m_iRenormCnt = kiRenormAmount;

        WelsCabacEncodeUpdateLow_(buf, pCbCtx);
        pCbCtx.m_uiLow |= 0x80;
    } else {
        let kiRenormAmount = ((pCbCtx.m_uiRange >> 8) ^ 1) as i32;
        pCbCtx.m_uiRange <<= kiRenormAmount as u32;
        pCbCtx.m_iRenormCnt += kiRenormAmount;
    }
}

/// Encodes an unsigned integer via multi-bin Exp-Golomb bypass coding.
#[inline]
pub fn WelsCabacEncodeUeBypass(buf: &mut [u8], pCbCtx: &mut SCabacCtx, iExpBits: i32, uiVal: u32) {
    let mut iSufS = uiVal as i32;
    let mut iStopLoop = 0;
    let mut k = iExpBits;
    loop {
        if iSufS >= (1 << k) {
            WelsCabacEncodeBypassOne(buf, pCbCtx, 1);
            iSufS -= 1 << k;
            k += 1;
        } else {
            WelsCabacEncodeBypassOne(buf, pCbCtx, 0);
            while k > 0 {
                k -= 1;
                WelsCabacEncodeBypassOne(buf, pCbCtx, (iSufS >> k) & 1);
            }
            iStopLoop = 1;
        }
        if iStopLoop != 0 {
            break;
        }
    }
}

/// Finalizes and flushes the CABAC bitstream at the end of a slice NAL unit.
#[inline]
pub fn WelsCabacEncodeFlush(buf: &mut [u8], pCbCtx: &mut SCabacCtx) {
    WelsCabacEncodeTerminate(buf, pCbCtx, 1);

    let mut uiLow = pCbCtx.m_uiLow;
    let mut iLowBitCnt = pCbCtx.m_iLowBitCnt;
    let mut iBufCur = pCbCtx.m_iBufCur;

    let shift = (CABAC_LOW_WIDTH as i32) - 1 - iLowBitCnt;
    if shift >= 0 && shift < 64 {
        uiLow = uiLow.wrapping_shl(shift as u32);
    }
    if (uiLow & (1u64 << ((CABAC_LOW_WIDTH as u32) - 1))) != 0 {
        PropagateCarry(buf, iBufCur, pCbCtx.m_iBufStart);
    }
    // One byte per iteration while `iLowBitCnt -= 8` stays
    // non-negative — at most 7, since `iLowBitCnt <= 63`.
    loop {
        iLowBitCnt -= 8;
        if iLowBitCnt < 0 {
            break;
        }
        buf[iBufCur] = (uiLow >> ((CABAC_LOW_WIDTH as u32) - 9)) as u8;
        iBufCur += 1;
        uiLow = uiLow.wrapping_shl(8);
    }

    pCbCtx.m_iBufCur = iBufCur;
}

/// Returns the current byte write cursor `m_iBufCur`, as an offset into the
/// output buffer.
#[inline(always)]
pub fn WelsCabacEncodePos(pCbCtx: &SCabacCtx) -> usize {
    pCbCtx.m_iBufCur
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_ctx_packing() {
        let mut ctx = SStateCtx::default();
        ctx.Set(42, 1);
        assert_eq!(ctx.State(), 42);
        assert_eq!(ctx.Mps(), 1);

        ctx.Set(0, 0);
        assert_eq!(ctx.State(), 0);
        assert_eq!(ctx.Mps(), 0);
    }

    #[test]
    fn test_cabac_encode_init_and_flush() {
        let mut cb_ctx = SCabacCtx::default();
        let mut buf = vec![0u8; 1024];
        let len = buf.len();

        WelsCabacEncodeInit(&mut cb_ctx, 0, len);
        assert_eq!(cb_ctx.m_uiLow, 0);
        assert_eq!(cb_ctx.m_iLowBitCnt, 9);
        assert_eq!(cb_ctx.m_uiRange, 510);
        assert_eq!(cb_ctx.m_iBufCur, 0);

        WelsCabacEncodeBypassOne(&mut buf, &mut cb_ctx, 1);
        WelsCabacEncodeBypassOne(&mut buf, &mut cb_ctx, 0);

        WelsCabacEncodeFlush(&mut buf, &mut cb_ctx);
        assert!(WelsCabacEncodePos(&cb_ctx) > 0);
    }

    /// The walk stops at `m_iBufStart`, leaving the byte below it — a previous
    /// slice's — alone.
    #[test]
    fn test_propagate_carry() {
        let mut buf = [5u8, 0xFFu8, 0u8];
        PropagateCarry(&mut buf, 2, 0);
        assert_eq!(buf[0], 6);
        assert_eq!(buf[1], 0);

        // Same bytes, but the slice starts at 1. The 0xFF still wraps, and the
        // carry out of it must be dropped rather than reaching buf[0].
        let mut buf = [5u8, 0xFFu8, 0u8];
        PropagateCarry(&mut buf, 2, 1);
        assert_eq!(buf[1], 0);
        assert_eq!(buf[0], 5, "carry escaped below m_iBufStart");

        // The degenerate case the comparison exists for: cur == start, where a
        // `pos - 1` would wrap a usize.
        let mut buf = [0xFFu8; 2];
        PropagateCarry(&mut buf, 0, 0);
        assert_eq!(buf, [0xFF, 0xFF]);
    }
}
