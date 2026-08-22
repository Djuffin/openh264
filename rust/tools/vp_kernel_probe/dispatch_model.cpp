// Step 0c: a faithful plain-C++ model of the path CDownsampling::Process really
// takes on this host (sample buffer allocated => the multi-pass arm), validated
// plane-by-plane against the real class.  This model is what gets ported to Rust.
#include "downsample.h"
#include <cstdio>
#include <cstring>
#include <vector>
using namespace WelsVP;

static uint32_t s = 4242;
static uint8_t rnd() { s = s * 1103515245u + 12345u; return (uint8_t)((s >> 16) & 0xff); }
static int ALIGN(int v, int a){ return (v + a - 1) & ~(a - 1); }

// ---- model kernels (straight transcriptions of the _c sources) -------------
static void half_c(uint8_t* dst, int dstStride, const uint8_t* src, int srcStride, int srcW, int srcH) {
  int dw = srcW >> 1, dh = srcH >> 1;
  for (int j = 0; j < dh; j++) {
    for (int i = 0; i < dw; i++) {
      int x = i << 1;
      int r1 = (src[x] + src[x+1] + 1) >> 1;
      int r2 = (src[x+srcStride] + src[x+srcStride+1] + 1) >> 1;
      dst[i] = (uint8_t)((r1 + r2 + 1) >> 1);
    }
    dst += dstStride; src += srcStride << 1;
  }
}
static void gen_acc_c(uint8_t* dst, int dstStride, int dw, int dh,
                      const uint8_t* src, int srcStride, int sw, int sh) {
  const int kBit = 15, kScale = 1 << kBit;
  int sx = (int)(( (float)sw / (float)dw * kScale) + 0.5f);
  int sy = (int)(( (float)sh / (float)dh * kScale) + 0.5f);
  uint8_t* lineDst = dst;
  int yInv = 1 << (kBit - 1);
  for (int i = 0; i < dh - 1; i++) {
    int yy = yInv >> kBit, fv = yInv & (kScale - 1);
    const uint8_t* bySrc = src + (size_t)yy * srcStride;
    uint8_t* byDst = lineDst;
    int xInv = 1 << (kBit - 1);
    for (int j = 0; j < dw - 1; j++) {
      int xx = xInv >> kBit, fu = xInv & (kScale - 1);
      const uint8_t* p = bySrc + xx;
      long long a = p[0], b = p[1], c = p[srcStride], d = p[srcStride + 1];
      long long x = ((long long)(kScale-1-fu)) * (kScale-1-fv) * a
                  + ((long long)fu) * (kScale-1-fv) * b
                  + ((long long)(kScale-1-fu)) * fv * c
                  + ((long long)fu) * fv * d
                  + (long long)(1 << (2*kBit - 1));
      x >>= (2 * kBit);
      if (x < 0) x = 0; if (x > 255) x = 255;
      *byDst++ = (uint8_t)x;
      xInv += sx;
    }
    *byDst = *(bySrc + (xInv >> kBit));
    lineDst += dstStride; yInv += sy;
  }
  { int yy = yInv >> kBit; const uint8_t* bySrc = src + (size_t)yy * srcStride;
    uint8_t* byDst = lineDst; int xInv = 1 << (kBit - 1);
    for (int j = 0; j < dw; j++) { *byDst++ = *(bySrc + (xInv >> kBit)); xInv += sx; } }
}
// CDownsampling::DownsampleHalfAverage — the alignment branch decides the width passed
static void half_avg(uint8_t* dst, int dstStride, const uint8_t* src, int srcStride, int srcW, int srcH) {
  if ((srcStride & 31) == 0) half_c(dst, dstStride, src, srcStride, ALIGN(srcW & ~1, 32), srcH);
  else                       half_c(dst, dstStride, src, srcStride, ALIGN(srcW & ~1, 16), srcH);
}

#define MAX_SAMPLE_WIDTH 1920
#define MAX_SAMPLE_HEIGHT 1088

struct Map { uint8_t* p[3]; int stride[3]; int w, h; };
static std::vector<uint8_t> scratch[2][3];

static int model_process (const Map& S, Map& D) {
  int sWY = S.w, sHY = S.h, dWY = D.w, dHY = D.h;
  int sWUV = sWY >> 1, sHUV = sHY >> 1, dWUV = dWY >> 1, dHUV = dHY >> 1;
  if (sWY <= dWY || sHY <= dHY) return 1; // RET_INVALIDPARAM
  // m_bNoSampleBuffer is false on this host => only the size test can pick arm 1
  if ((sWY >> 1) > MAX_SAMPLE_WIDTH || (sHY >> 1) > MAX_SAMPLE_HEIGHT) {
    return 2; // arm 1: not exercised here
  }
  int idx = 0;
  int halfW = sWY >> 1, halfH = sHY >> 1;
  const uint8_t* srcY = S.p[0]; const uint8_t* srcU = S.p[1]; const uint8_t* srcV = S.p[2];
  int stY = S.stride[0], stU = S.stride[1], stV = S.stride[2];
  uint8_t* dY = scratch[idx][0].data(); uint8_t* dU = scratch[idx][1].data(); uint8_t* dV = scratch[idx][2].data();
  idx++;
  for (;;) {
    if (halfW == dWY && halfH == dHY) {
      half_avg(D.p[0], D.stride[0], srcY, stY, sWY, sHY);
      half_avg(D.p[1], D.stride[1], srcU, stU, sWUV, sHUV);
      half_avg(D.p[2], D.stride[2], srcV, stV, sWUV, sHUV);
      break;
    } else if (halfW > dWY && halfH > dHY) {
      int dstStY = ALIGN(halfW, 32), dstStU = ALIGN(halfW >> 1, 32), dstStV = ALIGN(halfW >> 1, 32);
      half_avg(dY, dstStY, srcY, stY, sWY, sHY);
      half_avg(dU, dstStU, srcU, stU, sWUV, sHUV);
      half_avg(dV, dstStV, srcV, stV, sWUV, sHUV);
      srcY = dY; srcU = dU; srcV = dV;
      sWY = halfW; sWUV = halfW >> 1; sHY = halfH; sHUV = halfH >> 1;
      stY = dstStY; stU = dstStU; stV = dstStV;
      halfW >>= 1; halfH >>= 1;
      idx = idx % 2;
      dY = scratch[idx][0].data(); dU = scratch[idx][1].data(); dV = scratch[idx][2].data();
      idx++;
    } else {
      gen_acc_c(D.p[0], D.stride[0], dWY, dHY, srcY, stY, sWY, sHY);   // NEON table: luma = ACCURATE
      gen_acc_c(D.p[1], D.stride[1], dWUV, dHUV, srcU, stU, sWUV, sHUV);
      gen_acc_c(D.p[2], D.stride[2], dWUV, dHUV, srcV, stV, sWUV, sHUV);
      break;
    }
  }
  return 0;
}

static int al32(int v){ return ALIGN(v, 32); }

int main() {
  for (int i = 0; i < 2; i++) {
    scratch[i][0].assign((size_t)MAX_SAMPLE_WIDTH*MAX_SAMPLE_HEIGHT, 0);
    scratch[i][1].assign((size_t)MAX_SAMPLE_WIDTH*MAX_SAMPLE_HEIGHT/4, 0);
    scratch[i][2].assign((size_t)MAX_SAMPLE_WIDTH*MAX_SAMPLE_HEIGHT/4, 0);
  }
  struct { int sw, sh, dw, dh; } cs[] = {
    {320,192,160,96},{1280,720,640,360},{640,360,320,180},{320,180,160,90},
    {640,384,160,96},{1280,720,320,180},{1280,720,160,90},
    {960,576,320,192},{320,192,208,128},{1280,720,848,480},{352,288,176,144},
    {176,144,88,72},{640,480,320,240},{1920,1080,960,540},{1920,1088,480,272},
    {720,576,352,288},{1024,768,512,384},{800,600,400,300},{640,360,424,240},
  };
  CDownsampling ds (0x000004 /* WELS_CPU_NEON */);
  int bad = 0;
  for (auto c : cs) {
    int ssY=al32(c.sw), ssUV=al32(c.sw/2), dsY=al32(c.dw), dsUV=al32(c.dw/2);
    std::vector<uint8_t> sY((size_t)ssY*(c.sh+32)), sU((size_t)ssUV*(c.sh/2+32)), sV((size_t)ssUV*(c.sh/2+32));
    for (auto& v : sY) v = rnd();
    for (auto& v : sU) v = rnd();
    for (auto& v : sV) v = rnd();
    size_t nY=(size_t)dsY*(c.dh+32), nUV=(size_t)dsUV*(c.dh/2+32);
    std::vector<uint8_t> rY(nY,0xCD), rU(nUV,0xCD), rV(nUV,0xCD);
    std::vector<uint8_t> mY(nY,0xCD), mU(nUV,0xCD), mV(nUV,0xCD);

    SPixMap src, dst; memset(&src,0,sizeof src); memset(&dst,0,sizeof dst);
    src.pPixel[0]=sY.data(); src.pPixel[1]=sU.data(); src.pPixel[2]=sV.data();
    src.iStride[0]=ssY; src.iStride[1]=ssUV; src.iStride[2]=ssUV;
    src.sRect.iRectWidth=c.sw; src.sRect.iRectHeight=c.sh; src.eFormat=VIDEO_FORMAT_I420; src.iSizeInBits=8;
    dst.pPixel[0]=rY.data(); dst.pPixel[1]=rU.data(); dst.pPixel[2]=rV.data();
    dst.iStride[0]=dsY; dst.iStride[1]=dsUV; dst.iStride[2]=dsUV;
    dst.sRect.iRectWidth=c.dw; dst.sRect.iRectHeight=c.dh; dst.eFormat=VIDEO_FORMAT_I420; dst.iSizeInBits=8;
    ds.Process (METHOD_DOWNSAMPLE, &src, &dst);

    Map S, D;
    S.p[0]=sY.data(); S.p[1]=sU.data(); S.p[2]=sV.data();
    S.stride[0]=ssY; S.stride[1]=ssUV; S.stride[2]=ssUV; S.w=c.sw; S.h=c.sh;
    D.p[0]=mY.data(); D.p[1]=mU.data(); D.p[2]=mV.data();
    D.stride[0]=dsY; D.stride[1]=dsUV; D.stride[2]=dsUV; D.w=c.dw; D.h=c.dh;
    int arm = model_process(S, D);

    auto cmp=[&](std::vector<uint8_t>&a, std::vector<uint8_t>&b, int st, int w, int h){
      int n=0; for(int j=0;j<h;j++) for(int i=0;i<w;i++) if(a[(size_t)j*st+i]!=b[(size_t)j*st+i]) n++; return n; };
    int ny=cmp(rY,mY,dsY,c.dw,c.dh), nu=cmp(rU,mU,dsUV,c.dw/2,c.dh/2), nv=cmp(rV,mV,dsUV,c.dw/2,c.dh/2);
    bool ok = (arm==0) && !ny && !nu && !nv;
    if (!ok) bad++;
    printf("%4dx%-4d -> %4dx%-4d  %-9s  Y%-6s U%-6s V%-6s\n", c.sw,c.sh,c.dw,c.dh,
           arm==0?"modelled":(arm==1?"INVALID":"ARM1"),
           ny?"DIFF":"ok", nu?"DIFF":"ok", nv?"DIFF":"ok");
  }
  printf("\n%s\n", bad ? "==> MODEL WRONG" : "==> MODEL MATCHES THE REFERENCE ON EVERY CASE");
  return bad != 0;
}
