// F80's asset: a stream the reference encoder produces that drives
// `WelsRequestMem`'s **third** arm — same picture size, changed `num_ref_frames`
// (`decoder.cpp:493-509`). Phase 8b session A, T8b.A6.
//
//   build: c++ -std=c++11 -I codec/api/wels -o mk rust/tools/make_numref_asset.cpp \
//              "$PWD/libopenh264.dylib"
//   run:   DYLD_LIBRARY_PATH=$PWD ./mk res/num_ref_change_320x192.264
//   check: DYLD_LIBRARY_PATH=$PWD rust/tools/ecref/ecref <asset> 99999999 --sps
//
// **`SetOption(ENCODER_OPTION_NUMBER_REF)` cannot make this stream.**
// `welsEncoderExt.cpp:1142` only calls `CheckReferenceNumSetting` on the live
// parameter block; nothing regenerates the SPS, so the bitstream keeps the value it
// was initialised with. Reconfiguring the encoder — `Uninitialize` +
// `InitializeExt` at the same size with a different `iNumRefFrame` — does, and that
// is what an application doing the same thing produces.
//
// No stream in `res/` reaches this arm: `ecref --sps` over all 63 shows every
// multi-SPS stream repeating one `num_ref_frames`, and the only one that changes
// anything (`Error_I_P.264`) changes the *resolution*, which is the second arm.
//
// **The asset lives in `res/` since T8b.C3**, the commit that ported
// `IncreasePicBuff`/`DecreasePicBuff`. It was held under `tests/data/f80/` until then
// because `decoder_reachability_sweep.rs` globs `res/` and this stream made the port
// return `dsOutOfMemory`, so the sweep would have been red for a defect nobody had
// fixed yet. Its referee is `tests/decoder_numref_change_test.rs`. See F80/F87.
#include <cstdio>
#include <cstring>
#include <climits>
#include <vector>
#include "codec_api.h"
#include "codec_app_def.h"
int main(int argc, char** argv) {
  const int W = 320, H = 192, FRAMES = 24;
  ISVCEncoder* enc = nullptr;
  if (WelsCreateSVCEncoder(&enc) || !enc) return 2;
  SEncParamExt p; memset(&p, 0, sizeof(p));
  enc->GetDefaultParams(&p);
  p.iUsageType = CAMERA_VIDEO_REAL_TIME;
  p.iPicWidth = W; p.iPicHeight = H; p.fMaxFrameRate = 30.0f;
  p.iTargetBitrate = 500000; p.iRCMode = RC_OFF_MODE;
  p.iMultipleThreadIdc = 1; p.iSpatialLayerNum = 1;
  p.iNumRefFrame = 1;
  p.uiIntraPeriod = 0;
  p.sSpatialLayers[0].iVideoWidth = W;
  p.sSpatialLayers[0].iVideoHeight = H;
  p.sSpatialLayers[0].fFrameRate = 30.0f;
  p.sSpatialLayers[0].iSpatialBitrate = 500000;
  if (enc->InitializeExt(&p) != 0) { fprintf(stderr, "InitializeExt\n"); return 2; }
  int quiet = WELS_LOG_QUIET; enc->SetOption(ENCODER_OPTION_TRACE_LEVEL, &quiet);
  int strategy = INCREASING_ID; enc->SetOption(ENCODER_OPTION_SPS_PPS_ID_STRATEGY, &strategy);

  std::vector<unsigned char> buf(size_t(W) * H * 3 / 2, 0x40);
  SSourcePicture pic; memset(&pic, 0, sizeof(pic));
  pic.iColorFormat = videoFormatI420;
  pic.iPicWidth = W; pic.iPicHeight = H;
  pic.iStride[0] = W; pic.iStride[1] = W / 2; pic.iStride[2] = W / 2;
  pic.pData[0] = buf.data();
  pic.pData[1] = buf.data() + size_t(W) * H;
  pic.pData[2] = buf.data() + size_t(W) * H * 5 / 4;

  FILE* out = fopen(argv[1], "wb");
  if (!out) return 2;
  // Two configurations at the same picture size, differing only in
  // `iNumRefFrame`, in one stream — an application reconfiguring its encoder.
  // `SetOption(ENCODER_OPTION_NUMBER_REF)` cannot do it: `welsEncoderExt.cpp:1142`
  // only calls `CheckReferenceNumSetting` on the live parameter block and never
  // regenerates the SPS, so the bitstream keeps the original value.
  for (int pass = 0; pass < 2; pass++) {
    if (pass == 1) {
      enc->Uninitialize();
      p.iNumRefFrame = 4;
      if (enc->InitializeExt(&p) != 0) { fprintf(stderr, "re-InitializeExt\n"); return 2; }
      enc->SetOption(ENCODER_OPTION_TRACE_LEVEL, &quiet);
      enc->SetOption(ENCODER_OPTION_SPS_PPS_ID_STRATEGY, &strategy);
    }
    for (int i = 0; i < FRAMES; i++) {
      for (size_t k = 0; k < buf.size(); k++) buf[k] = (unsigned char)((k + (pass * FRAMES + i) * 7) & 0xff);
      SFrameBSInfo info; memset(&info, 0, sizeof(info));
      long rv = enc->EncodeFrame(&pic, &info);
      if (rv != 0) { fprintf(stderr, "EncodeFrame %d -> %ld\n", i, rv); break; }
      if (info.eFrameType == videoFrameTypeSkip) continue;
      for (int l = 0; l < info.iLayerNum; l++) {
        int len = 0;
        for (int n = 0; n < info.sLayerInfo[l].iNalCount; n++) len += info.sLayerInfo[l].pNalLengthInByte[n];
        fwrite(info.sLayerInfo[l].pBsBuf, 1, size_t(len), out);
      }
    }
  }
  fclose(out);
  enc->Uninitialize(); WelsDestroySVCEncoder(enc);
  return 0;
}
