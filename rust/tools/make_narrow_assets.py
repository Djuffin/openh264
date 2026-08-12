#!/usr/bin/env python3
"""Regenerate the three narrow-frame decode assets — Phase 5 session B, F21.

  usage: rust/tools/make_narrow_assets.py [--check]

Writes res/narrow_16x16.264, res/narrow_24x18.264 and
res/narrow_16x16_idr_lost.264 and prints the SHA-1 of each one's C++ decode,
which is the golden the rows in `tests/decoder_conformance_test.rs` carry.
`--check` regenerates into a temporary directory and diffs instead, so a session
can confirm the committed assets are still what this script produces.

Why the assets are built rather than downloaded: `ExpandReferencingPicture`
takes a different arm below 32 luma pixels of frame width, one of the port's
three copies of it had no such arm (F21), and nothing in the corpus is narrower
than 176px — the conformance streams, the diffharness inputs and the malformed
corpus derived from them all sit far above the branch.

Three properties this construction has to have, each of which cost an attempt:

  * **Motion.** The source window *pans* across the clip. A static crop encodes
    to zero MVs, no MV points outside the frame, and the expanded border — the
    only thing F21 changes — never reaches the output.
  * **A recycled picture.** `WelsInitRefList`'s concealment prefetch is the only
    call site the divergent copy served. It memsets the picture it takes to 128
    over the active area only, so a picture fresh from `AllocPicture` (also 128)
    reads identically expanded or not. The stream therefore runs a whole
    sequence first, so the pool has cycled and the prefetched slot still holds
    real samples outside that memset.
  * **No reordering tie.** Concatenating two encodes can leave two buffered
    pictures sharing a `uiDecodingTimeStamp`, which upstream and the port break
    in different directions (see the `test_scalinglist_jm` note in the test
    file) — a real divergence, but not this one, and it would have made the row
    fail at HEAD for the wrong reason. CAVLC-then-CABAC does not tie where
    CABAC-then-CAVLC did.

Requires `make -j8 libraries binaries` (h264dec) and
`rust/tools/diffharness/build.sh` (cxx_enc), same as the sweep.
"""
import hashlib
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
CXX_ENC = ROOT / "rust/tools/diffharness/cxx_enc"
H264DEC = ROOT / "h264dec"
SRC_YUV = ROOT / "res/CiscoVT2people_320x192_12fps.yuv"
SRC_W, SRC_H = 320, 192

# Window origin and per-frame pan, in luma pixels. Even values only: the chroma
# crop has to stay on a sample boundary.
X0, Y0, DX, DY, FRAMES = 100, 60, 6, 4, 24


def crop(dst, dw, dh):
    """Pan a dw x dh window across SRC_YUV for FRAMES frames of I420."""
    data = SRC_YUV.read_bytes()
    fsz = SRC_W * SRC_H * 3 // 2
    navail = len(data) // fsz
    out = bytearray()
    for i in range(FRAMES):
        f = data[(i % navail) * fsz:(i % navail + 1) * fsz]
        y, u, v = f[:SRC_W * SRC_H], f[SRC_W * SRC_H:fsz - SRC_W * SRC_H // 4], f[fsz - SRC_W * SRC_H // 4:]
        cx = min(max(X0 + DX * i, 0), SRC_W - dw) & ~1
        cy = min(max(Y0 + DY * i, 0), SRC_H - dh) & ~1
        for r in range(dh):
            o = (cy + r) * SRC_W + cx
            out += y[o:o + dw]
        for plane in (u, v):
            for r in range(dh // 2):
                o = (cy // 2 + r) * (SRC_W // 2) + cx // 2
                out += plane[o:o + dw // 2]
    dst.write_bytes(bytes(out))


def encode(yuv, w, h, cabac, gop, frames, out):
    subprocess.run([str(CXX_ENC), str(yuv), str(w), str(h), str(frames), "26",
                    str(cabac), str(gop), str(out), "-1", "0"],
                   check=True, capture_output=True, cwd=ROOT)


def nals(data):
    starts = []
    i = 0
    while i < len(data) - 3:
        if data[i:i + 3] == b"\x00\x00\x01":
            starts.append(i - 1 if i and data[i - 1] == 0 else i)
            i += 3
        else:
            i += 1
    return [data[a:b] for a, b in zip(starts, starts[1:] + [len(data)])]


def decode_sha1(stream, tmp):
    yuv = tmp / (stream.stem + ".yuv")
    subprocess.run([str(H264DEC), str(stream), str(yuv)], check=True,
                   capture_output=True, cwd=ROOT)
    return hashlib.sha1(yuv.read_bytes()).hexdigest()


def build(outdir, tmp):
    """Write the three assets into outdir; return {name: golden sha1}."""
    y16, y24 = tmp / "src16.yuv", tmp / "src24.yuv"
    crop(y16, 16, 16)
    crop(y24, 24, 18)

    a16 = outdir / "narrow_16x16.264"
    a24 = outdir / "narrow_24x18.264"
    aec = outdir / "narrow_16x16_idr_lost.264"

    encode(y16, 16, 16, 1, 8, FRAMES, a16)
    encode(y24, 24, 18, 1, 8, FRAMES, a24)

    # Sequence one: CAVLC, profile_idc 66, the full 24 frames — long enough that
    # the picture pool has cycled. Sequence two: CABAC, profile_idc 100, so its
    # SPS differs from the stored one and begins a new sequence (which clears the
    # reference lists), with its IDR NAL removed so the first slice that arrives
    # with those lists empty is a P slice. That is the concealment prefetch.
    first, second = tmp / "seq1.264", tmp / "seq2.264"
    encode(y16, 16, 16, 0, 8, FRAMES, first)
    encode(y16, 16, 16, 1, 8, 8, second)
    units = nals(second.read_bytes())
    idr = next(i for i, n in enumerate(units)
               if n[3 if n[2] == 1 else 4] & 0x1F == 5)
    aec.write_bytes(first.read_bytes() + b"".join(u for i, u in enumerate(units) if i != idr))

    return {p.name: decode_sha1(p, tmp) for p in (a16, a24, aec)}


def main():
    check = "--check" in sys.argv
    with tempfile.TemporaryDirectory() as td:
        tmp = pathlib.Path(td)
        outdir = tmp / "out" if check else ROOT / "res"
        outdir.mkdir(exist_ok=True)
        golds = build(outdir, tmp)
        for name, sha in golds.items():
            note = ""
            if check:
                same = (ROOT / "res" / name).read_bytes() == (outdir / name).read_bytes()
                note = "  MATCHES res/" if same else "  ** DIFFERS from res/ **"
            print(f"{name:28s} {sha}{note}")
        if check and any((ROOT / "res" / n).read_bytes() != (outdir / n).read_bytes()
                         for n in golds):
            return 1
    return 0


sys.exit(main())
