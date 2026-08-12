#!/usr/bin/env python3
"""Regenerate the decode assets this project builds rather than downloads.

  usage: rust/tools/make_narrow_assets.py [--check]

Two families, two reasons, two encoders:

  * **The three narrow-frame assets** — Phase 5 session B, F21. res/narrow_16x16.264,
    res/narrow_24x18.264, res/narrow_16x16_idr_lost.264, built by the C++ encoder.
  * **The macroblock-grid probe asset** — Phase 5 session J, F34. res/grid_48x32.264,
    built by ffmpeg/libx264, for the reason in §"Why libx264" below.

Prints the SHA-1 of each one's C++ decode, which is the golden the rows in
`tests/decoder_conformance_test.rs` carry. `--check` regenerates into a temporary
directory and diffs instead, so a session can confirm the committed assets are
still what this script produces.

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

## The macroblock-grid probe asset (session J, F34)

`decode_slice_loop_runs_under_the_aliasing_checker` — the phase's Miri gate on the
slice-decode loop — decoded `narrow_16x16.264` and nothing else. That stream is
**one macroblock per frame**, so `iLeftAvail` and `iTopAvail` are 0 at every
macroblock in it and no neighbour-reading path has ever executed under the
aliasing checker. F34 is what that costs: a real UB that Miri walked through and
returned green on, because the two lines that make it UB were unreachable.
`grid_48x32.264` is the second probe stream, and it must have all four of:

  * **A real grid.** 48x32 is 3x2 macroblocks — the smallest grid in which some
    macroblock has *all four* neighbours (MB(1,1): left, top, top-left,
    top-right), and which also contains a macroblock missing only its left
    (MB(0,1)) and one missing only its top-right (MB(2,1)). Every availability
    combination the neighbour paths branch on is present, six macroblocks buy
    them all, and each macroblock past that is Miri time.
  * **Motion.** Same panned window as the narrow assets, same reason stated
    differently: a static scene encodes to zero MVs and the MV-prediction
    neighbour reads — `pMv`, `pMvd`, `pRefIndex`, the families this stream was
    built to cover — never see a non-trivial predictor.
  * **CABAC and the 8x8 transform.** F34 sits behind `bTransform8x8ModeFlag` on
    the CABAC path.
  * **B slices.** They are what reach `pDirect`, `pSubMbType` and the LIST_1 half
    of `pMv`/`pMvd`/`pRefIndex`.

### Why libx264 for this one and not the C++ encoder

**OpenH264's encoder cannot emit `transform_8x8_mode_flag` at all.** It is not a
parameter that is off by default: `grep -rn "ransform.8x8" codec/encoder/` returns
nothing, and `WelsWritePpsSyntax` (`codec/encoder/core/src/au_set.cpp:406`) has no
such syntax element to write. Every stream `cxx_enc` produces — including
`narrow_16x16.264`, whose PPS was checked — carries the flag as 0, so the F34 path
is unreachable from that encoder in principle, and a probe stream that cannot
re-find the known miss is not covering the paths it was built for (the F21 rule,
applied to this stream in session J's log).

ffmpeg/libx264 is not a new dependency here: `benches/decode_1080p_bench.rs` builds
all three of its 1080p streams with it, `gates.sh` requires `FFMPEG` to be set, and
S17's UNMEASURED banner exists because of it. The golden is still the C++
*decoder*'s output, as it is for every row in the conformance file — which encoder
produced the bytes does not enter the comparison.

The reproducibility caveat is the same one the cxx_enc assets already carry, one
tool further away: `--check` compares against whatever libx264 is installed, so a
libx264 upgrade can make it report DIFFERS on a byte-identical-in-intent asset.
The committed asset is the artifact; the recipe below is how it was made.

Requires `make -j8 libraries binaries` (h264dec),
`rust/tools/diffharness/build.sh` (cxx_enc), and ffmpeg with libx264.
"""
import hashlib
import os
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

# The grid probe (session J). 48x32 is 3x2 macroblocks; six frames is one full
# `keyint` plus its wrap, so the stream carries an IDR, P slices and B slices.
GRID_W, GRID_H, GRID_FRAMES = 48, 32, 6

# One line, so the command that made the asset is the command in the log.
# 8x8dct=1 is the whole reason this asset is not built by cxx_enc; bframes=2
# with ref=2 is what puts B slices and a second reference list in it.
X264_PARAMS = (
    "cabac=1:8x8dct=1:bframes=2:ref=2:me=hex:subme=7"
    ":keyint=8:scenecut=0:weightp=1:aq-mode=0"
)


def crop(dst, dw, dh, frames=FRAMES):
    """Pan a dw x dh window across SRC_YUV for `frames` frames of I420."""
    data = SRC_YUV.read_bytes()
    fsz = SRC_W * SRC_H * 3 // 2
    navail = len(data) // fsz
    out = bytearray()
    for i in range(frames):
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


def ffmpeg_bin():
    """FFMPEG, then PATH, then the two Homebrew/`/usr` locations — the same
    resolution order `benches/decode_1080p_bench.rs::resolve_ffmpeg` uses."""
    cand = [os.environ["FFMPEG"]] if os.environ.get("FFMPEG") else []
    cand += ["ffmpeg", "/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/usr/bin/ffmpeg"]
    for c in cand:
        if shutil.which(c):
            return c
    # S17: refuse loudly. A silent skip here writes no asset and the caller's
    # --check then compares the committed file against nothing.
    raise SystemExit("make_narrow_assets: no ffmpeg found; set FFMPEG=/path/to/ffmpeg")


def encode_x264(yuv, w, h, frames, out, qp=26):
    """The grid probe asset. See the module docstring's "Why libx264"."""
    subprocess.run([ffmpeg_bin(), "-y", "-loglevel", "error",
                    "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", f"{w}x{h}",
                    "-r", "30", "-i", str(yuv), "-frames:v", str(frames),
                    "-c:v", "libx264", "-profile:v", "high", "-qp", str(qp),
                    "-x264-params", X264_PARAMS, "-f", "h264", str(out)],
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
    """Write the four assets into outdir; return {name: golden sha1}."""
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

    # The grid probe (session J, F34). Different encoder, same panned source.
    ygrid = tmp / "srcgrid.yuv"
    crop(ygrid, GRID_W, GRID_H, GRID_FRAMES)
    agrid = outdir / f"grid_{GRID_W}x{GRID_H}.264"
    encode_x264(ygrid, GRID_W, GRID_H, GRID_FRAMES, agrid)

    return {p.name: decode_sha1(p, tmp) for p in (a16, a24, aec, agrid)}


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
