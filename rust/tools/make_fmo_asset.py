#!/usr/bin/env python3
"""Build `res/fmo_2groups_64x64.264` — the port's first FMO stream.

Phase 5 session S, F43. `decoder_core.rs` declared an `FmoNextMb` stub returning
`iMbIdx + 1` — raster order, the one answer flexible macroblock ordering exists
to *not* give — and nothing wrote `pCtx->pFmo`, so `fmo.rs` was unreachable in
production. The suite could not see it: **no stream in `res/` has a PPS with
`num_slice_groups_minus1 > 0`** (all 74 scanned), so every asset decodes the same
whether `FmoNextMb` is real or raster.

This constructs one. The design is chosen so the stub is *provably* wrong on it
rather than merely unexercised:

  * 64x64, so 4x4 = 16 macroblocks, and two slice groups with
    `slice_group_map_type = 0` and both run lengths 1 — the interleaved map, so
    `mapUnitToSliceGroupMap[i] = i % 2`. Group 0 owns the even raster indices,
    group 1 the odd ones.
  * One slice per group. Slice 0 starts at macroblock 0 and walks 0, 2, 4, …;
    slice 1 starts at 1 and walks 1, 3, 5, …. Under the stub each slice walks
    consecutive indices instead, so **every macroblock after the first of each
    slice lands in the wrong place** and half the frame is never written.
  * Every macroblock is I_PCM carrying one flat luma value, distinct per raster
    position. No prediction, no transform, no CAVLC residual: the decoded frame
    *is* the values this script wrote, so a misordering is visible as itself
    rather than as a diffuse hash change. Deblocking is disabled in the slice
    header (I_PCM already forces qp 0, which disables the filter; saying so in
    the header removes the dependence on that).

Baseline profile, `pic_order_cnt_type = 2`, one IDR frame.

The golden is the C++ decoder's, as everywhere: print it with

    rust/tools/ecref/build.sh && \
    DYLD_LIBRARY_PATH=. rust/tools/ecref/ecref res/fmo_2groups_64x64.264 -1
"""

import os
import sys

MB_W, MB_H = 4, 4                 # 64x64
N_MB = MB_W * MB_H
RUN_LENGTHS = (1, 1)              # interleaved: map[i] = i % 2


class BitWriter:
    def __init__(self):
        self.bits = []

    def u(self, n, v):
        for i in range(n - 1, -1, -1):
            self.bits.append((v >> i) & 1)

    def u1(self, v):
        self.bits.append(v & 1)

    def ue(self, v):
        # exp-Golomb: v+1 in binary, prefixed by that many-1 zeros
        code = v + 1
        n = code.bit_length()
        self.u(n - 1, 0)
        self.u(n, code)

    def se(self, v):
        self.ue(2 * v - 1 if v > 0 else -2 * v)

    def aligned(self):
        return len(self.bits) % 8 == 0

    def align_zero(self):
        while not self.aligned():
            self.u1(0)

    def trailing(self):
        self.u1(1)
        self.align_zero()

    def bytes(self):
        assert self.aligned(), "writer must be byte-aligned"
        out = bytearray()
        for i in range(0, len(self.bits), 8):
            b = 0
            for j in range(8):
                b = (b << 1) | self.bits[i + j]
            out.append(b)
        return bytes(out)


def emulation_prevent(payload):
    """RBSP -> EBSP: insert 0x03 so no 00 00 0{0,1,2,3} survives."""
    out = bytearray()
    zeros = 0
    for b in payload:
        if zeros >= 2 and b <= 3:
            out.append(3)
            zeros = 0
        out.append(b)
        zeros = zeros + 1 if b == 0 else 0
    return bytes(out)


def nal(ref_idc, unit_type, rbsp):
    return b"\x00\x00\x00\x01" + bytes([(ref_idc << 5) | unit_type]) + emulation_prevent(rbsp)


def sps():
    w = BitWriter()
    w.u(8, 66)                    # profile_idc: Baseline
    w.u(8, 0x80)                  # constraint_set0_flag, rest zero
    w.u(8, 30)                    # level_idc 3.0
    w.ue(0)                       # seq_parameter_set_id
    w.ue(0)                       # log2_max_frame_num_minus4 -> frame_num is u(4)
    w.ue(2)                       # pic_order_cnt_type 2: decode order is output order
    w.ue(1)                       # max_num_ref_frames
    w.u1(0)                       # gaps_in_frame_num_value_allowed_flag
    w.ue(MB_W - 1)                # pic_width_in_mbs_minus1
    w.ue(MB_H - 1)                # pic_height_in_map_units_minus1
    w.u1(1)                       # frame_mbs_only_flag
    w.u1(1)                       # direct_8x8_inference_flag
    w.u1(0)                       # frame_cropping_flag
    w.u1(0)                       # vui_parameters_present_flag
    w.trailing()
    return w.bytes()


def pps():
    w = BitWriter()
    w.ue(0)                       # pic_parameter_set_id
    w.ue(0)                       # seq_parameter_set_id
    w.u1(0)                       # entropy_coding_mode_flag: CAVLC
    w.u1(0)                       # bottom_field_pic_order_in_frame_present_flag
    w.ue(len(RUN_LENGTHS) - 1)    # num_slice_groups_minus1
    w.ue(0)                       # slice_group_map_type 0: interleaved
    for r in RUN_LENGTHS:
        w.ue(r - 1)               # run_length_minus1[group]
    w.ue(0)                       # num_ref_idx_l0_default_active_minus1
    w.ue(0)                       # num_ref_idx_l1_default_active_minus1
    w.u1(0)                       # weighted_pred_flag
    w.u(2, 0)                     # weighted_bipred_idc
    w.se(0)                       # pic_init_qp_minus26
    w.se(0)                       # pic_init_qs_minus26
    w.se(0)                       # chroma_qp_index_offset
    w.u1(1)                       # deblocking_filter_control_present_flag
    w.u1(0)                       # constrained_intra_pred_flag
    w.u1(0)                       # redundant_pic_cnt_present_flag
    w.trailing()
    return w.bytes()


def slice_group_map():
    """The map the decoder is required to build (7.4.2.2 / 8.2.2.1)."""
    m = [0] * N_MB
    i, group = 0, 0
    while i < N_MB:
        run = RUN_LENGTHS[group % len(RUN_LENGTHS)]
        for j in range(run):
            if i + j < N_MB:
                m[i + j] = group % len(RUN_LENGTHS)
        i += run
        group += 1
    return m


def luma_of(mb):
    """One flat value per raster position, distinct and never 0x00."""
    return 16 + mb * 13


def slice_nal(group, members):
    w = BitWriter()
    w.ue(members[0])              # first_mb_in_slice
    w.ue(7)                       # slice_type 7: I, and all slices in the picture are I
    w.ue(0)                       # pic_parameter_set_id
    w.u(4, 0)                     # frame_num
    w.ue(0)                       # idr_pic_id
    # pic_order_cnt_type == 2: no POC syntax
    w.u1(0)                       # no_output_of_prior_pics_flag
    w.u1(0)                       # long_term_reference_flag
    w.se(0)                       # slice_qp_delta
    w.ue(1)                       # disable_deblocking_filter_idc: off

    for mb in members:
        w.ue(25)                  # mb_type 25 in an I slice: I_PCM
        w.align_zero()            # pcm_alignment_zero_bit
        y = luma_of(mb)
        for _ in range(256):
            w.u(8, y)
        for _ in range(64):       # Cb
            w.u(8, 0x80)
        for _ in range(64):       # Cr
            w.u(8, 0x80)
    w.trailing()
    return nal(3, 5, w.bytes())   # nal_ref_idc 3, IDR


def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else "res"
    m = slice_group_map()
    groups = {}
    for mb in range(N_MB):
        groups.setdefault(m[mb], []).append(mb)

    stream = nal(3, 7, sps()) + nal(3, 8, pps())
    for g in sorted(groups):
        stream += slice_nal(g, groups[g])

    path = os.path.join(out_dir, "fmo_2groups_64x64.264")
    with open(path, "wb") as f:
        f.write(stream)

    print(f"wrote {path} ({len(stream)} bytes)")
    print(f"slice group map (raster order): {m}")
    for g in sorted(groups):
        print(f"  group {g}: macroblocks {groups[g]}")
    print("  under a raster-order FmoNextMb stub, slice 0 would instead write")
    print(f"  macroblocks {list(range(groups[0][0], groups[0][0] + len(groups[0])))}")


if __name__ == "__main__":
    main()
