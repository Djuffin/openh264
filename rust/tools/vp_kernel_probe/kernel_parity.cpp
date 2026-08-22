// Step 0: _c vs AArch64 NEON downsampler parity probe.
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>

namespace WelsVP {
void DyadicBilinearDownsampler_c (uint8_t*, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
void DyadicBilinearQuarterDownsampler_c (uint8_t*, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
void DyadicBilinearOneThirdDownsampler_c (uint8_t*, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
void GeneralBilinearFastDownsampler_c (uint8_t*, const int32_t, const int32_t, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
void GeneralBilinearAccurateDownsampler_c (uint8_t*, const int32_t, const int32_t, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
}
extern "C" {
void DyadicBilinearDownsampler_AArch64_neon (uint8_t*, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
void DyadicBilinearDownsamplerWidthx32_AArch64_neon (uint8_t*, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
void DyadicBilinearQuarterDownsampler_AArch64_neon (uint8_t*, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
void DyadicBilinearOneThirdDownsampler_AArch64_neon (uint8_t*, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
void GeneralBilinearAccurateDownsamplerWrap_AArch64_neon (uint8_t*, const int32_t, const int32_t, const int32_t, uint8_t*, const int32_t, const int32_t, const int32_t);
}

static uint32_t s = 12345;
static uint8_t rnd() { s = s * 1103515245u + 12345u; return (uint8_t)((s >> 16) & 0xff); }

// Compare dstW x dstH window of two dst buffers with stride dstStride.
static int diff(const std::vector<uint8_t>& a, const std::vector<uint8_t>& b,
                int dstStride, int dstW, int dstH, const char* name) {
  int nd = 0; int firstj=-1, firsti=-1, va=0, vb=0;
  for (int j = 0; j < dstH; j++)
    for (int i = 0; i < dstW; i++) {
      uint8_t x = a[j*dstStride+i], y = b[j*dstStride+i];
      if (x != y) { if (nd==0){firstj=j;firsti=i;va=x;vb=y;} nd++; }
    }
  printf("%-58s %s", name, nd ? "DIFFER" : "IDENTICAL");
  if (nd) printf("  (%d/%d px, first @ r%d c%d: c=%d neon=%d)", nd, dstW*dstH, firstj, firsti, va, vb);
  printf("\n");
  return nd;
}

static int align32(int v){ return (v + 31) & ~31; }
static int align16(int v){ return (v + 15) & ~15; }

int main() {
  int bad = 0;
  // ---- dyadic 2:1, the x16 path (src stride 16-aligned, not 32-aligned) ----
  // and the x32 path (src stride 32-aligned).  Widths as DownsampleHalfAverage
  // passes them: WELS_ALIGN(srcW & ~1, 32 or 16).
  struct { int w, h; } cases[] = {{320,192},{640,384},{1280,720},{160,96},{176,144},{352,288},{640,360},{320,180}};
  for (auto c : cases) {
    for (int stride32 : {1, 0}) {
      int srcStride = stride32 ? align32(c.w) : align16(c.w);
      if (!stride32 && (srcStride & 31) == 0) srcStride += 16;   // force non-32-aligned
      int srcH = c.h;
      int dstStride = align32(c.w/2);
      std::vector<uint8_t> src((size_t)srcStride*(srcH+8));
      for (auto& v : src) v = rnd();
      std::vector<uint8_t> d1((size_t)dstStride*(srcH/2+8), 0xCD), d2 = d1;
      int passW = stride32 ? align32(c.w & ~1) : align16(c.w & ~1);
      if (stride32) {
        DyadicBilinearDownsamplerWidthx32_AArch64_neon(d2.data(), dstStride, src.data(), srcStride, passW, srcH);
      } else {
        DyadicBilinearDownsampler_AArch64_neon(d2.data(), dstStride, src.data(), srcStride, passW, srcH);
      }
      WelsVP::DyadicBilinearDownsampler_c(d1.data(), dstStride, src.data(), srcStride, passW, srcH);
      char nm[128];
      snprintf(nm, sizeof nm, "half %dx%d srcStride=%d (%s)", c.w, c.h, srcStride, stride32?"x32":"x16");
      bad += diff(d1, d2, dstStride, passW/2, srcH/2, nm);
    }
  }
  // ---- quarter 4:1 ----
  for (auto c : cases) {
    int srcStride = align32(c.w), dstStride = align32(c.w/4);
    std::vector<uint8_t> src((size_t)srcStride*(c.h+8));
    for (auto& v : src) v = rnd();
    std::vector<uint8_t> d1((size_t)dstStride*(c.h/4+8), 0xCD), d2 = d1;
    WelsVP::DyadicBilinearQuarterDownsampler_c(d1.data(), dstStride, src.data(), srcStride, c.w, c.h);
    DyadicBilinearQuarterDownsampler_AArch64_neon(d2.data(), dstStride, src.data(), srcStride, c.w, c.h);
    char nm[128]; snprintf(nm, sizeof nm, "quarter %dx%d", c.w, c.h);
    bad += diff(d1, d2, dstStride, c.w/4, c.h/4, nm);
  }
  // ---- one third 3:1 (last arg is DST height) ----
  for (auto c : cases) {
    int srcStride = align32(c.w), dstH = c.h/3, dstStride = align32(c.w/3);
    std::vector<uint8_t> src((size_t)srcStride*(c.h+8));
    for (auto& v : src) v = rnd();
    std::vector<uint8_t> d1((size_t)dstStride*(dstH+8), 0xCD), d2 = d1;
    WelsVP::DyadicBilinearOneThirdDownsampler_c(d1.data(), dstStride, src.data(), srcStride, c.w, dstH);
    DyadicBilinearOneThirdDownsampler_AArch64_neon(d2.data(), dstStride, src.data(), srcStride, c.w, dstH);
    char nm[128]; snprintf(nm, sizeof nm, "onethird %dx%d", c.w, c.h);
    bad += diff(d1, d2, dstStride, c.w/3, dstH, nm);
  }
  // ---- general ratio: Accurate_c vs NEON wrap, and Fast_c vs NEON wrap ----
  struct { int sw, sh, dw, dh; } gcases[] = {{320,192,208,128},{1280,720,848,480},{640,360,424,240},{320,192,160,96}};
  for (auto g : gcases) {
    int srcStride = align32(g.sw), dstStride = align32(g.dw);
    std::vector<uint8_t> src((size_t)srcStride*(g.sh+8));
    for (auto& v : src) v = rnd();
    std::vector<uint8_t> d1((size_t)dstStride*(g.dh+8), 0xCD), d2 = d1, d3 = d1;
    WelsVP::GeneralBilinearAccurateDownsampler_c(d1.data(), dstStride, g.dw, g.dh, src.data(), srcStride, g.sw, g.sh);
    GeneralBilinearAccurateDownsamplerWrap_AArch64_neon(d2.data(), dstStride, g.dw, g.dh, src.data(), srcStride, g.sw, g.sh);
    WelsVP::GeneralBilinearFastDownsampler_c(d3.data(), dstStride, g.dw, g.dh, src.data(), srcStride, g.sw, g.sh);
    char nm[128];
    snprintf(nm, sizeof nm, "general ACC  %dx%d->%dx%d  c vs neon", g.sw,g.sh,g.dw,g.dh);
    bad += diff(d1, d2, dstStride, g.dw, g.dh, nm);
    snprintf(nm, sizeof nm, "general FAST_c vs ACC_c %dx%d->%dx%d (luma tbl gap)", g.sw,g.sh,g.dw,g.dh);
    diff(d3, d1, dstStride, g.dw, g.dh, nm);   // informational, not counted
  }
  printf("\n%s\n", bad ? "==> DIVERGENCE FOUND" : "==> ALL COMPARED KERNELS IDENTICAL");
  return bad != 0;
}
