// abi_harness — the drop-in, tested as a drop-in (Phase 8 session C, T8.C5).
//
// A C++ program compiled against upstream's own `codec/api/wels` headers that
// `dlopen`s the Rust `cdylib`, resolves the seven exported symbols with `dlsym`, and
// drives the library through nothing but those seven and the vtables they hand back.
//
// **Why this exists at all.** Every other gate in this project links the crate as an
// `rlib` and calls it from Rust. That checks the code; it does not check the
// *artefact*. An rlib has no dynamic symbol table, no `dlopen` entry, and — crucially
// — its caller is compiled by the same rustc from the same declarations, so a struct
// whose Rust layout disagrees with the C header agrees with itself and every
// in-process hash matches. `api/abi_guard.rs`'s 51 pins close that by construction;
// this closes it by experiment, which is the other half.
//
// **It calls through the C++ abstract classes, not the C vtable structs**, because
// that is what a real consumer's code looks like — `cxx_enc.cpp`, upstream's own
// `test/api`, and every application that includes `codec_api.h` from C++. Under the
// Itanium C++ ABI a call through `ISVCDecoder*` loads the object's first word as a
// vtable address point and indexes the declared virtual functions in order; the Rust
// side builds exactly that — `CWelsDecoderImpl { base: ISVCDecoder { lpVtbl }, .. }`
// over a ten-slot table in declaration order. **That those two agree is a claim, and
// this program is the experiment that settles it.** If it ever stops being true the
// conformance part fails on the first asset.
//
//   modes (see run.sh, which drives all of them):
//     selftest                    resolve the seven, run the SHA-1 known-answer test
//     version                     part 3 — the version pair and the capability block
//     conformance <list>          part 1 — <asset> <hash> <concealed> per line
//     error <asset>               part 4 — the F77 stream returns a code, alive
//     enc <cxx_enc argv...>       part 2 — cxx_enc's driver, through the dylib
//
//   env: ABI_HARNESS_LIB   path to the cdylib (required)
//        ABI_HARNESS_RES   repo root for `conformance`/`error` asset paths
#include <dlfcn.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "codec_api.h"
#include "codec_app_def.h"
#include "codec_def.h"
// The version the *headers* declare, which is what a drop-in must report.
#include "codec_ver.h"

#include "../wels_ref_common.h"

// ---------------------------------------------------------------------------
// The seven, and nothing else.
// ---------------------------------------------------------------------------
typedef int  (*PFN_CreateEnc)  (ISVCEncoder**);
typedef void (*PFN_DestroyEnc) (ISVCEncoder*);
typedef long (*PFN_CreateDec)  (ISVCDecoder**);
typedef void (*PFN_DestroyDec) (ISVCDecoder*);
typedef int  (*PFN_DecCap)     (SDecoderCapability*);
typedef OpenH264Version (*PFN_Version) (void);
typedef void (*PFN_VersionEx)  (OpenH264Version*);

static void*          g_lib          = NULL;
static PFN_CreateEnc  g_CreateEnc    = NULL;
static PFN_DestroyEnc g_DestroyEnc   = NULL;
static PFN_CreateDec  g_CreateDec    = NULL;
static PFN_DestroyDec g_DestroyDec   = NULL;
static PFN_DecCap     g_DecCap       = NULL;
static PFN_Version    g_Version      = NULL;
static PFN_VersionEx  g_VersionEx    = NULL;

template <typename T>
static bool resolve (T& slot, const char* name) {
  dlerror();
  void* p = dlsym (g_lib, name);
  const char* err = dlerror();
  if (!p || err) {
    fprintf (stderr, "dlsym(%s) failed: %s\n", name, err ? err : "null");
    return false;
  }
  slot = (T) p;
  return true;
}

static bool load_library() {
  const char* path = getenv ("ABI_HARNESS_LIB");
  if (!path || !*path) {
    fprintf (stderr, "ABI_HARNESS_LIB is not set\n");
    return false;
  }
  // RTLD_LOCAL, deliberately: a drop-in consumer does not want the library's symbols
  // in the global namespace, and RTLD_GLOBAL here would let an accidental export
  // interpose on something in this process. The export gate says there are none; this
  // is the belt.
  g_lib = dlopen (path, RTLD_NOW | RTLD_LOCAL);
  if (!g_lib) {
    fprintf (stderr, "dlopen(%s) failed: %s\n", path, dlerror());
    return false;
  }
  return resolve (g_CreateEnc,  "WelsCreateSVCEncoder")
      && resolve (g_DestroyEnc, "WelsDestroySVCEncoder")
      && resolve (g_CreateDec,  "WelsCreateDecoder")
      && resolve (g_DestroyDec, "WelsDestroyDecoder")
      && resolve (g_DecCap,     "WelsGetDecoderCapability")
      && resolve (g_Version,    "WelsGetCodecVersion")
      && resolve (g_VersionEx,  "WelsGetCodecVersionEx");
}

static std::vector<uint8_t> read_file (const std::string& path, bool* ok) {
  std::vector<uint8_t> v;
  *ok = false;
  FILE* f = fopen (path.c_str(), "rb");
  if (!f) return v;
  fseek (f, 0, SEEK_END);
  long n = ftell (f);
  fseek (f, 0, SEEK_SET);
  if (n > 0) {
    v.resize ((size_t) n);
    if (fread (v.data(), 1, (size_t) n, f) != (size_t) n) { fclose (f); return v; }
  }
  fclose (f);
  *ok = true;
  return v;
}

static std::string res_root() {
  const char* r = getenv ("ABI_HARNESS_RES");
  return r && *r ? std::string (r) : std::string (".");
}

// ---------------------------------------------------------------------------
// Part 1 — decoder conformance through the dylib.
//
// The decode flow is `tests/decoder_conformance_test.rs`'s, statement for
// statement: annex-B split, ERROR_CON_SLICE_COPY, one NAL per DecodeFrame2, the
// end-of-stream drain, then FlushFrame for whatever GetOption reports remaining.
// `hash_concealed` selects the same two rules that file's two macros select.
// ---------------------------------------------------------------------------
static bool decode_asset (const std::string& path, bool hash_concealed,
                          std::string* digest, int* frames_out) {
  bool ok = false;
  std::vector<uint8_t> data = read_file (path, &ok);
  if (!ok || data.empty()) { fprintf (stderr, "cannot read %s\n", path.c_str()); return false; }

  ISVCDecoder* dec = NULL;
  if (g_CreateDec (&dec) != 0 || !dec) { fprintf (stderr, "WelsCreateDecoder failed\n"); return false; }

  SDecodingParam p;
  memset (&p, 0, sizeof (p));
  p.uiTargetDqLayer = UCHAR_MAX;
  p.eEcActiveIdc    = ERROR_CON_SLICE_COPY;
  p.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
  if (dec->Initialize (&p) != 0) { fprintf (stderr, "Initialize failed\n"); g_DestroyDec (dec); return false; }

  Sha1 sha;
  int frames = 0;
  std::vector<std::pair<size_t, size_t> > units = split_annexb (data);
  for (size_t u = 0; u < units.size(); ++u) {
    uint8_t* dst[3] = {NULL, NULL, NULL};
    SBufferInfo info;
    memset (&info, 0, sizeof (info));
    const uint8_t* src = data.data() + units[u].first;
    int len = (int) (units[u].second - units[u].first);
    DECODING_STATE st = dec->DecodeFrame2 (src, len, dst, &info);
    if ((hash_concealed || st == dsErrorFree) && info.iBufferStatus == 1) {
      SSysMEMBuffer& s = info.UsrData.sSystemBuffer;
      hash_plane (sha, dst[0], s.iWidth, s.iHeight, s.iStride[0]);
      hash_plane (sha, dst[1], s.iWidth / 2, s.iHeight / 2, s.iStride[1]);
      hash_plane (sha, dst[2], s.iWidth / 2, s.iHeight / 2, s.iStride[1]);
      ++frames;
    }
  }

  int eos = 1;
  dec->SetOption (DECODER_OPTION_END_OF_STREAM, &eos);
  {
    uint8_t* dst[3] = {NULL, NULL, NULL};
    SBufferInfo info;
    memset (&info, 0, sizeof (info));
    DECODING_STATE st = dec->DecodeFrame2 (NULL, 0, dst, &info);
    if ((hash_concealed || st == dsErrorFree) && info.iBufferStatus == 1) {
      SSysMEMBuffer& s = info.UsrData.sSystemBuffer;
      hash_plane (sha, dst[0], s.iWidth, s.iHeight, s.iStride[0]);
      hash_plane (sha, dst[1], s.iWidth / 2, s.iHeight / 2, s.iStride[1]);
      hash_plane (sha, dst[2], s.iWidth / 2, s.iHeight / 2, s.iStride[1]);
      ++frames;
    }
  }

  int remaining = 0;
  dec->GetOption (DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER, &remaining);
  for (int i = 0; i < remaining; ++i) {
    uint8_t* dst[3] = {NULL, NULL, NULL};
    SBufferInfo info;
    memset (&info, 0, sizeof (info));
    DECODING_STATE st = dec->FlushFrame (dst, &info);
    if ((hash_concealed || st == dsErrorFree) && info.iBufferStatus == 1) {
      SSysMEMBuffer& s = info.UsrData.sSystemBuffer;
      hash_plane (sha, dst[0], s.iWidth, s.iHeight, s.iStride[0]);
      hash_plane (sha, dst[1], s.iWidth / 2, s.iHeight / 2, s.iStride[1]);
      hash_plane (sha, dst[2], s.iWidth / 2, s.iHeight / 2, s.iStride[1]);
      ++frames;
    }
  }

  dec->Uninitialize();
  g_DestroyDec (dec);
  *digest = sha.digest();
  *frames_out = frames;
  return true;
}

static int mode_conformance (const char* list_path) {
  bool ok = false;
  std::vector<uint8_t> raw = read_file (list_path, &ok);
  if (!ok) { fprintf (stderr, "cannot read %s\n", list_path); return 2; }
  std::string text ((const char*) raw.data(), raw.size());

  int pass = 0, fail = 0;
  size_t pos = 0;
  while (pos <= text.size()) {
    size_t nl = text.find ('\n', pos);
    if (nl == std::string::npos) nl = text.size();
    std::string line = text.substr (pos, nl - pos);
    pos = nl + 1;
    if (line.empty() || line[0] == '#') { if (nl == text.size()) break; continue; }
    char asset[512] = {0}, hash[128] = {0};
    int concealed = 0;
    if (sscanf (line.c_str(), "%511s %127s %d", asset, hash, &concealed) != 3) continue;
    std::string digest;
    int frames = 0;
    if (!decode_asset (res_root() + "/res/" + asset, concealed != 0, &digest, &frames)) {
      printf ("  FAIL  %-48s decode failed\n", asset);
      ++fail;
    } else if (digest != hash) {
      printf ("  FAIL  %-48s %s != %s (%d frames)\n", asset, digest.c_str(), hash, frames);
      ++fail;
    } else {
      ++pass;
    }
    if (nl == text.size()) break;
  }
  printf ("conformance through the dylib: %d/%d assets bit-identical to the in-process goldens\n",
          pass, pass + fail);
  return fail == 0 ? 0 : 1;
}

// ---------------------------------------------------------------------------
// Part 3 — the version pair and the capability block.
// ---------------------------------------------------------------------------
static int mode_version() {
  int fail = 0;
  OpenH264Version v = g_Version();
  printf ("  WelsGetCodecVersion   -> %u.%u.%u (reserved %u)\n", v.uMajor, v.uMinor, v.uRevision, v.uReserved);
  if (v.uMajor != OPENH264_MAJOR || v.uMinor != OPENH264_MINOR || v.uRevision != OPENH264_REVISION) {
    printf ("  FAIL  expected %d.%d.%d from codec_ver.h\n", OPENH264_MAJOR, OPENH264_MINOR, OPENH264_REVISION);
    ++fail;
  }
  OpenH264Version vx;
  memset (&vx, 0xAB, sizeof (vx));
  g_VersionEx (&vx);
  if (memcmp (&v, &vx, sizeof (v)) != 0) {
    printf ("  FAIL  WelsGetCodecVersionEx wrote a different value than WelsGetCodecVersion returned\n");
    ++fail;
  }
  // The out-parameter form must tolerate null — the port's arm, and the reference's.
  g_VersionEx (NULL);

  SDecoderCapability cap;
  memset (&cap, 0xAB, sizeof (cap));
  int rc = g_DecCap (&cap);
  printf ("  WelsGetDecoderCapability -> rc=%d profile=%d iop=0x%X level=%d mbps=%d fs=%d cpb=%d dpb=%d br=%d red=%d\n",
          rc, cap.iProfileIdc, cap.iProfileIop, cap.iLevelIdc, cap.iMaxMbps, cap.iMaxFs,
          cap.iMaxCpb, cap.iMaxDpb, cap.iMaxBr, (int) cap.bRedPicCap);
  // `welsDecoderExt.cpp:1450` — the block upstream fills, value for value. It is
  // also `test_decoder_capability_query`'s, so the two sides of the ABI are
  // asserted against the same numbers.
  if (rc != cmResultSuccess || cap.iProfileIdc != 66 || cap.iProfileIop != 0xE0 ||
      cap.iLevelIdc != 32 || cap.iMaxMbps != 216000 || cap.iMaxFs != 5120 ||
      cap.iMaxCpb != 20000 || cap.iMaxDpb != 20480 || cap.iMaxBr != 20000 || cap.bRedPicCap) {
    printf ("  FAIL  capability block does not match welsDecoderExt.cpp's\n");
    ++fail;
  }
  printf ("version + capability: %s\n", fail ? "FAIL" : "OK");
  return fail ? 1 : 0;
}

// ---------------------------------------------------------------------------
// Part 4 — the F77 stream returns an error code, through the dylib, alive.
//
// `res/Error_I_P.264` aborted this library's *process* until T8.C1/T8.C2. Through a
// dylib the abort is the consumer's process, which is the whole reason P13 is a
// boundary rule. The assertion is the pair: a `dsDataErrorConcealed` in the code
// stream, and this program still running to print it.
// ---------------------------------------------------------------------------
static int mode_error (const char* asset) {
  bool ok = false;
  std::vector<uint8_t> data = read_file (res_root() + "/res/" + asset, &ok);
  if (!ok || data.empty()) { fprintf (stderr, "cannot read %s\n", asset); return 2; }

  ISVCDecoder* dec = NULL;
  if (g_CreateDec (&dec) != 0 || !dec) return 2;
  SDecodingParam p;
  memset (&p, 0, sizeof (p));
  p.uiTargetDqLayer = UCHAR_MAX;
  p.eEcActiveIdc    = ERROR_CON_SLICE_COPY;
  p.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
  if (dec->Initialize (&p) != 0) { g_DestroyDec (dec); return 2; }

  int union_bits = 0, frames = 0;
  std::vector<std::pair<size_t, size_t> > units = split_annexb (data);
  for (size_t u = 0; u < units.size(); ++u) {
    uint8_t* dst[3] = {NULL, NULL, NULL};
    SBufferInfo info;
    memset (&info, 0, sizeof (info));
    DECODING_STATE st = dec->DecodeFrame2 (data.data() + units[u].first,
                                           (int) (units[u].second - units[u].first), dst, &info);
    union_bits |= (int) st;
    if (info.iBufferStatus == 1) ++frames;
  }
  int eos = 1;
  dec->SetOption (DECODER_OPTION_END_OF_STREAM, &eos);
  {
    uint8_t* dst[3] = {NULL, NULL, NULL};
    SBufferInfo info;
    memset (&info, 0, sizeof (info));
    union_bits |= (int) dec->DecodeFrame2 (NULL, 0, dst, &info);
    if (info.iBufferStatus == 1) ++frames;
  }
  dec->Uninitialize();
  g_DestroyDec (dec);

  printf ("  %s through the dylib: %d frames, state union 0x%x, process alive\n", asset, frames, union_bits);
  // Five frames and `dsDataErrorConcealed` set — the C++ decoder's own answer for
  // this stream (`rust/tools/ecref/ecref res/Error_I_P.264 61251`).
  int fail = (frames != 5 || (union_bits & dsDataErrorConcealed) == 0);
  printf ("malformed stream: %s\n", fail ? "FAIL" : "OK");
  return fail ? 1 : 0;
}

// ---------------------------------------------------------------------------
// Part 2's driver — `cxx_enc.cpp`'s `main`, with the two factory calls going
// through `dlsym`ed pointers instead of the linker. Kept argument-compatible with
// `cxx_enc` and `rust_enc` so `run.sh` can hand all three the same line.
// ---------------------------------------------------------------------------
static int mode_enc (int argc, char** argv) {
  if (argc < 9) { fprintf (stderr, "usage: abi_harness enc <src.yuv> <w> <h> <frames> <qp> <cabac> <gop> <out.264> [...]\n"); return 2; }
  const char* kpSrc      = argv[1];
  const int   kiWidth    = atoi (argv[2]);
  const int   kiHeight   = atoi (argv[3]);
  const int   kiFrames   = atoi (argv[4]);
  const int   kiQp       = atoi (argv[5]);
  const int   kiCabac    = atoi (argv[6]);
  const int   kiGop      = atoi (argv[7]);
  const char* kpOut      = argv[8];
  const int   kiRcMode   = (argc >  9) ? atoi (argv[9])  : (int) RC_OFF_MODE;
  const int   kiBaseInit = (argc > 10) ? atoi (argv[10]) : 0;
  const int   kiSliceMode= (argc > 11) ? atoi (argv[11]) : 0;
  const int   kiSliceNum = (argc > 12) ? atoi (argv[12]) : 1;
  const int   kiThreads  = (argc > 13) ? atoi (argv[13]) : 1;
  const int   kiComplexity=(argc > 14) ? atoi (argv[14]) : 0;
  const int   kiLtrRefNum= (argc > 15) ? atoi (argv[15]) : 0;
  const int   kiLtrPeriod= (argc > 16) ? atoi (argv[16]) : 30;
  const int   kiLtrFb    = (argc > 17) ? atoi (argv[17]) : 0;

  ISVCEncoder* pEnc = NULL;
  if (g_CreateEnc (&pEnc) != 0 || pEnc == NULL) { fprintf (stderr, "WelsCreateSVCEncoder failed\n"); return 1; }

  SEncParamExt sParam;
  memset (&sParam, 0, sizeof (sParam));
  pEnc->GetDefaultParams (&sParam);

  if (kiBaseInit == 2) {
    sParam.iPicWidth                          = kiWidth;
    sParam.iPicHeight                         = kiHeight;
    sParam.fMaxFrameRate                      = 30.0f;
    sParam.iTargetBitrate                     = 2000000;
    sParam.iSpatialLayerNum                   = 1;
    sParam.iMultipleThreadIdc                 = kiThreads;
    sParam.sSpatialLayers[0].iVideoWidth      = kiWidth;
    sParam.sSpatialLayers[0].iVideoHeight     = kiHeight;
    sParam.sSpatialLayers[0].fFrameRate       = 30.0f;
    sParam.sSpatialLayers[0].iSpatialBitrate  = 2000000;
  } else {
    sParam.iUsageType                 = CAMERA_VIDEO_REAL_TIME;
    sParam.iPicWidth                  = kiWidth;
    sParam.iPicHeight                 = kiHeight;
    sParam.iTargetBitrate             = 500000;
    sParam.iMaxBitrate                = UNSPECIFIED_BIT_RATE;
    sParam.iRCMode                    = (RC_MODES) kiRcMode;
    sParam.fMaxFrameRate              = 30.0f;
    sParam.iTemporalLayerNum          = 1;
    sParam.iSpatialLayerNum           = 1;
    sParam.iComplexityMode            = (ECOMPLEXITY_MODE) kiComplexity;
    sParam.uiIntraPeriod              = (unsigned int) kiGop;
    sParam.iNumRefFrame               = AUTO_REF_PIC_COUNT;
    sParam.eSpsPpsIdStrategy          = CONSTANT_ID;
    sParam.bPrefixNalAddingCtrl       = false;
    sParam.bEnableSSEI                = false;
    sParam.bSimulcastAVC              = false;
    sParam.iPaddingFlag               = 0;
    sParam.iEntropyCodingModeFlag     = kiCabac;
    sParam.bEnableFrameSkip           = false;
    sParam.iMaxQp                     = 51;
    sParam.iMinQp                     = 0;
    sParam.uiMaxNalSize               = 0;
    sParam.bEnableLongTermReference   = (kiLtrRefNum > 0);
    sParam.iLTRRefNum                 = kiLtrRefNum;
    sParam.iLtrMarkPeriod             = kiLtrPeriod;
    sParam.iMultipleThreadIdc         = kiThreads;
    sParam.bUseLoadBalancing          = false;
    sParam.iLoopFilterDisableIdc      = 0;
    sParam.iLoopFilterAlphaC0Offset   = 0;
    sParam.iLoopFilterBetaOffset      = 0;
    sParam.bEnableDenoise             = false;
    sParam.bEnableBackgroundDetection = false;
    sParam.bEnableAdaptiveQuant       = false;
    sParam.bEnableFrameCroppingFlag   = true;
    sParam.bEnableSceneChangeDetect   = false;
    sParam.bIsLosslessLink            = false;
    sParam.bFixRCOverShoot            = false;
    sParam.iIdrBitrateRatio           = 400;
    sParam.bPsnrY = sParam.bPsnrU = sParam.bPsnrV = false;

    sParam.sSpatialLayers[0].uiProfileIdc        = kiCabac ? PRO_HIGH : PRO_BASELINE;
    sParam.sSpatialLayers[0].uiLevelIdc          = LEVEL_UNKNOWN;
    sParam.sSpatialLayers[0].iVideoWidth         = kiWidth;
    sParam.sSpatialLayers[0].iVideoHeight        = kiHeight;
    sParam.sSpatialLayers[0].fFrameRate          = 30.0f;
    sParam.sSpatialLayers[0].iSpatialBitrate     = 500000;
    sParam.sSpatialLayers[0].iMaxSpatialBitrate  = UNSPECIFIED_BIT_RATE;
    sParam.sSpatialLayers[0].iDLayerQp           = kiQp;
    switch (kiSliceMode) {
    case 1:
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_FIXEDSLCNUM_SLICE;
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceNum  = kiSliceNum;
      break;
    case 2:
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_RASTER_SLICE;
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceNum  = kiSliceNum;
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceMbNum[0] = kiSliceNum;
      break;
    case 3:
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_SIZELIMITED_SLICE;
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceSizeConstraint = kiSliceNum;
      break;
    default:
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_SINGLE_SLICE;
      sParam.sSpatialLayers[0].sSliceArgument.uiSliceNum  = 1;
      break;
    }
  }

  if (kiBaseInit == 1) {
    SEncParamBase sBase;
    memset (&sBase, 0, sizeof (sBase));
    sBase.iUsageType     = CAMERA_VIDEO_REAL_TIME;
    sBase.fMaxFrameRate  = 30.0f;
    sBase.iPicWidth      = kiWidth;
    sBase.iPicHeight     = kiHeight;
    sBase.iTargetBitrate = 5000000;
    if (pEnc->Initialize (&sBase) != 0) {
      fprintf (stderr, "Initialize failed\n"); g_DestroyEnc (pEnc); return 1;
    }
  } else if (pEnc->InitializeExt (&sParam) != 0) {
    fprintf (stderr, "InitializeExt failed\n"); g_DestroyEnc (pEnc); return 1;
  }

  FILE* fSrc = fopen (kpSrc, "rb");
  FILE* fOut = fopen (kpOut, "wb");
  if (!fSrc || !fOut) { fprintf (stderr, "file open failed\n"); return 1; }

  const int kiLuma = kiWidth * kiHeight;
  const int kiSize = kiLuma * 3 / 2;
  unsigned char* pBuf = new unsigned char[kiSize];

  SSourcePicture sPic;
  memset (&sPic, 0, sizeof (sPic));
  sPic.iColorFormat = videoFormatI420;
  sPic.iPicWidth  = kiWidth;
  sPic.iPicHeight = kiHeight;
  sPic.iStride[0] = kiWidth;
  sPic.iStride[1] = sPic.iStride[2] = kiWidth >> 1;
  sPic.pData[0] = pBuf;
  sPic.pData[1] = pBuf + kiLuma;
  sPic.pData[2] = pBuf + kiLuma + (kiLuma >> 2);

  SFrameBSInfo sInfo;
  int iCoded = 0;
  unsigned int uiIdrSeen = 0;
  for (int f = 0; f < kiFrames; ++f) {
    if ((int) fread (pBuf, 1, kiSize, fSrc) != kiSize) break;
    memset (&sInfo, 0, sizeof (sInfo));
    sPic.uiTimeStamp = (long long) (f * (1000 / 30.0));
    int iRet = pEnc->EncodeFrame (&sPic, &sInfo);
    if (iRet != cmResultSuccess) { fprintf (stderr, "EncodeFrame failed at %d: %d\n", f, iRet); break; }
    if (sInfo.eFrameType == videoFrameTypeSkip) continue;
    for (int l = 0; l < sInfo.iLayerNum; ++l) {
      SLayerBSInfo* p = &sInfo.sLayerInfo[l];
      int iLen = 0;
      for (int n = 0; n < p->iNalCount; ++n) iLen += p->pNalLengthInByte[n];
      fwrite (p->pBsBuf, 1, iLen, fOut);
    }
    ++iCoded;
    if (sInfo.eFrameType == videoFrameTypeIDR) ++uiIdrSeen;

    if ((kiLtrFb & 1) && f >= 2) {
      SLTRMarkingFeedback sFb;
      memset (&sFb, 0, sizeof (sFb));
      sFb.uiFeedbackType = LTR_MARKING_SUCCESS;
      sFb.uiIDRPicId     = uiIdrSeen;
      sFb.iLTRFrameNum   = f - 1;
      sFb.iLayerId       = 0;
      pEnc->SetOption (ENCODER_LTR_MARKING_FEEDBACK, &sFb);
    }
    if ((kiLtrFb & 2) && f > 0 && (f % 8) == 5) {
      SLTRRecoverRequest sRq;
      memset (&sRq, 0, sizeof (sRq));
      sRq.uiFeedbackType       = LTR_RECOVERY_REQUEST;
      sRq.uiIDRPicId           = uiIdrSeen;
      sRq.iLastCorrectFrameNum = f - 2;
      sRq.iCurrentFrameNum     = f;
      sRq.iLayerId             = 0;
      pEnc->SetOption (ENCODER_LTR_RECOVERY_REQUEST, &sRq);
    }
  }

  fprintf (stderr, "coded %d frames\n", iCoded);
  delete[] pBuf;
  fclose (fSrc);
  fclose (fOut);
  pEnc->Uninitialize();
  g_DestroyEnc (pEnc);
  return 0;
}

static int mode_selftest() {
  Sha1 s;
  s.update ((const uint8_t*) "abc", 3);
  std::string d = s.digest();
  printf ("  sha1(\"abc\") = %s\n", d.c_str());
  if (d != "a9993e364706816aba3e25717850c26c9cd0d89d") {
    printf ("  FAIL  the shared SHA-1 disagrees with tests/common/sha1.rs's known answer\n");
    return 1;
  }
  printf ("  the seven symbols resolved through dlsym\n");
  printf ("selftest: OK\n");
  return 0;
}

int main (int argc, char** argv) {
  if (argc < 2) {
    fprintf (stderr, "usage: %s <selftest|version|conformance <list>|error <asset>|enc ...>\n", argv[0]);
    return 2;
  }
  if (!load_library()) return 2;

  const char* mode = argv[1];
  if (!strcmp (mode, "selftest"))    return mode_selftest();
  if (!strcmp (mode, "version"))     return mode_version();
  if (!strcmp (mode, "conformance")) return argc >= 3 ? mode_conformance (argv[2]) : 2;
  if (!strcmp (mode, "error"))       return argc >= 3 ? mode_error (argv[2]) : 2;
  if (!strcmp (mode, "enc"))         return mode_enc (argc - 1, argv + 1);
  fprintf (stderr, "unknown mode %s\n", mode);
  return 2;
}
