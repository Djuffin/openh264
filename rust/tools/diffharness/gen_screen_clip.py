#!/usr/bin/env python3
"""Synthetic scrolling-text clip generator for the `scc` sweep preset (P10.1, A3).

    usage: gen_screen_clip.py --width W --height H --frames N --scroll K
                              [--cut-every C] [--hold-every D] --seed S --out PATH

Pure Python 3, standard library only, and deterministic for a given argument list:
the same arguments produce the same bytes on every run and every machine. The
`res/` clips are camera video — their text-free, unscrolled frames seldom satisfy
the scroll detector's line test (`CheckLine`, `ScrollDetectionFuncs.cpp:37`: a row
qualifies with four or more distinct values, or with two or three values and more
than three changes) and never scroll by whole rows — so without this clip nothing
in Phase 10 would ever exercise `JudgeScrollSkip`, `WelsMotionEstimateSearchScrolled`,
`SetScrollingMvToMd` or `CheckDirectionalMv`.

What a clip is:

* **Random source**: a 31-bit LCG, `state = (state * 1103515245 + 12345) & 0x7fffffff`,
  `next() -> state >> 8`, seeded from `--seed`.
* **Alphabet**: 16 glyphs, each a 7-wide x 10-tall bitmask drawn from the LCG once
  per page; glyph 0 is blank; every other glyph re-draws until it has at least 12
  set bits, so a glyph is visibly a glyph.
* **Page**: a luma canvas `W` wide and `H + K * N + 32` tall, paper value 235. Text
  lines start at row 4 and repeat every 12 rows; a line holds `(W - 16) // 8` cells;
  cell `c` draws its glyph at columns `8 + 8c .. 8 + 8c + 6`. Cells are grouped into
  words of 3-7 cells; each word takes one ink from `[16, 48, 96, 144]` — the inks
  rotate word by word from a per-line start drawn from the LCG, so a line with four
  or more words carries all four (four inks plus paper is the five distinct values
  `CheckLine` needs); every fifth line is instead a rule — one row of value 200
  across the width at the line's middle (a two-value row with many changes, the
  other way a line qualifies).
* **Frames**: frame `f` shows page rows `[o_f, o_f + H)` with `o_f = (f mod C) * K`
  when `--cut-every C` is given (a new page, drawn with seed `S + f // C`, at every
  cut) and `o_f = f * K` otherwise. If `--hold-every D` is given, every frame with
  `f mod D == D - 1` repeats the previous frame exactly. Chroma planes are flat 128,
  which also makes the chroma-SAD test in `JudgeStaticSkip`/`JudgeScrollSkip` pass
  wherever luma says static or scrolled — the point of the clip.
* **Output**: raw I420, `Y` then `U` then `V`, `N` frames, no header; the file is
  exactly `N * W * H * 3 / 2` bytes.

Ground truth (for P10.2's unit tests and P10.3's probes): between an in-page,
non-hold frame and its predecessor the content has moved **up** by `K` rows, so
`ScrollDetectionCore` (`ScrollDetectionFuncs.cpp:110`) reports
`bScrollDetectFlag = 1, iScrollMvY = +K, iScrollMvX = 0` (`iScrollMvY` is "previous
position minus current position": the current test line is found `K` rows *lower*
in the previous frame; upstream's gtest moves content *down* by 512 and expects
`-512`, the same convention). A cut frame is a `LARGE_CHANGED_SCENE`; a hold frame
is all `COLLOCATED_STATIC`. Note that `o_f` is a function of `f` alone, so the frame
*after* a hold frame sits `2K` rows from the hold's content — its predecessor being
the hold, the visible step there is `2K`, not `K`.
"""
import argparse
import sys

PAPER = 235
RULE = 200
INKS = [16, 48, 96, 144]
GLYPH_W = 7
GLYPH_H = 10
GLYPH_MIN_BITS = 12
FIRST_LINE_ROW = 4
LINE_PITCH = 12
CELL_PITCH = 8
LEFT_MARGIN = 8
PAGE_TAIL = 32


class Lcg:
    """The 31-bit linear congruential generator of the docstring."""

    def __init__(self, seed):
        self.state = seed & 0x7FFFFFFF

    def next(self):
        self.state = (self.state * 1103515245 + 12345) & 0x7FFFFFFF
        return self.state >> 8


def draw_glyphs(rng):
    """16 glyphs as lists of 10 seven-bit row masks; glyph 0 is blank."""
    glyphs = [[0] * GLYPH_H]
    for _ in range(1, 16):
        while True:
            rows = [rng.next() & 0x7F for _ in range(GLYPH_H)]
            if sum(bin(r).count("1") for r in rows) >= GLYPH_MIN_BITS:
                break
        glyphs.append(rows)
    return glyphs


def draw_page(width, height, seed):
    """One page of text: a `width` x `height` luma canvas, row-major."""
    rng = Lcg(seed)
    glyphs = draw_glyphs(rng)
    page = bytearray([PAPER]) * (width * height)
    cells = (width - 16) // 8
    line = 0
    top = FIRST_LINE_ROW
    while top + GLYPH_H <= height:
        if line % 5 == 4:
            y = top + GLYPH_H // 2
            page[y * width : (y + 1) * width] = bytes([RULE]) * width
        else:
            ink_base = rng.next() % 4
            c = 0
            word = 0
            while c < cells:
                word_len = 3 + rng.next() % 5
                ink = INKS[(ink_base + word) % 4]
                for _ in range(word_len):
                    if c >= cells:
                        break
                    glyph = glyphs[rng.next() % 16]
                    x0 = LEFT_MARGIN + CELL_PITCH * c
                    for r in range(GLYPH_H):
                        bits = glyph[r]
                        row = (top + r) * width
                        for b in range(GLYPH_W):
                            if (bits >> (GLYPH_W - 1 - b)) & 1:
                                page[row + x0 + b] = ink
                    c += 1
                word += 1
        line += 1
        top += LINE_PITCH
    return page


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--width", type=int, required=True)
    ap.add_argument("--height", type=int, required=True)
    ap.add_argument("--frames", type=int, required=True)
    ap.add_argument("--scroll", type=int, required=True, help="K: rows per frame")
    ap.add_argument("--cut-every", type=int, default=0, help="C: a new page every C frames (0 = never)")
    ap.add_argument("--hold-every", type=int, default=0, help="D: frame f repeats f-1 when f mod D == D-1 (0 = never)")
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args(argv)
    if a.width <= 16 or a.height <= 0 or a.frames <= 0 or a.scroll < 0 or a.cut_every < 0 or a.hold_every < 0:
        ap.error("width > 16, height > 0, frames > 0, scroll/cut-every/hold-every >= 0")
    if a.width % 2 or a.height % 2:
        ap.error("width and height must be even (I420)")

    W, H, N, K, C, D = a.width, a.height, a.frames, a.scroll, a.cut_every, a.hold_every
    page_h = H + K * N + PAGE_TAIL
    pages = {}

    def page_for(f):
        idx = f // C if C else 0
        if idx not in pages:
            pages[idx] = draw_page(W, page_h, a.seed + idx)
        return pages[idx]

    chroma = bytes([128]) * (W * H // 2)  # U then V, each W/2 x H/2, both flat
    prev = None
    with open(a.out, "wb") as fp:
        for f in range(N):
            if D and f % D == D - 1 and prev is not None:
                luma = prev
            else:
                o = (f % C) * K if C else f * K
                page = page_for(f)
                luma = bytes(page[o * W : (o + H) * W])
            fp.write(luma)
            fp.write(chroma)
            prev = luma
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
