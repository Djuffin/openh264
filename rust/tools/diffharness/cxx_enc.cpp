// Differential-oracle driver for the OpenH264 Rust encoder port.
//
// Sets a *fully explicit* SEncParamExt (no reliance on GetDefaultParams beyond
// the initial fill) so the Rust side can set byte-identical parameters and the
// only variable under test is the encoder itself.
//
// usage: cxx_enc <src.yuv> <w> <h> <frames> <qp> <cabac 0|1> <gop> <out.264>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "codec_api.h"
#include "codec_app_def.h"
#include "codec_def.h"

int main (int argc, char** argv) {
  if (argc < 9) {
    fprintf (stderr, "usage: %s <src.yuv> <w> <h> <frames> <qp> <cabac> <gop> <out.264> [rcmode] [baseinit 0|1|2] [slicemode] [slicenum] [threads]\n", argv[0]);
    return 1;
  }
  const char* kpSrc    = argv[1];
  const int   kiWidth  = atoi (argv[2]);
  const int   kiHeight = atoi (argv[3]);
  const int   kiFrames = atoi (argv[4]);
  const int   kiQp     = atoi (argv[5]);
  const int   kiCabac  = atoi (argv[6]);
  const int   kiGop    = atoi (argv[7]);
  const char* kpOut    = argv[8];
  // Optional 9th argument: iRCMode. Defaults to RC_OFF_MODE, the gate configuration.
  const int   kiRcMode = (argc > 9) ? atoi (argv[9]) : (int) RC_OFF_MODE;
  // Optional 10th argument: 1 selects Initialize(SEncParamBase) — the path
  // upstream's BaseEncoderTest::InitWithParam takes, and the one the SHA-1
  // parity test exercises. Everything else keeps FillDefault's values, so
  // scene-change detection, background detection, adaptive quantisation and
  // frame skip are all ON. 0 (default) is InitializeExt with the gate config.
  // 2 selects the GetDefaultParams + InitializeExt path: FillDefault's values
  // with only width/height/framerate/bitrate/threads set on top — the ordinary
  // API flow, and the one c_vs_rust_bench drives. qp/cabac/gop/rcmode/slice
  // arguments are ignored in this mode.
  const int   kiBaseInit = (argc > 10) ? atoi (argv[10]) : 0;
  // 11th/12th: uiSliceMode and uiSliceNum. SM_SINGLE_SLICE/1 is the gate.
  //   0 = SM_SINGLE_SLICE, 1 = SM_FIXEDSLCNUM_SLICE, 2 = SM_RASTER_SLICE,
  //   3 = SM_SIZELIMITED_SLICE (uiSliceNum is then the size constraint in bytes).
  const int   kiSliceMode = (argc > 11) ? atoi (argv[11]) : 0;
  const int   kiSliceNum  = (argc > 12) ? atoi (argv[12]) : 1;
  // 13th: iMultipleThreadIdc. 1 (default) is single-threaded.
  const int   kiThreads   = (argc > 13) ? atoi (argv[13]) : 1;

  ISVCEncoder* pEnc = NULL;
  if (WelsCreateSVCEncoder (&pEnc) != 0 || pEnc == NULL) {
    fprintf (stderr, "WelsCreateSVCEncoder failed\n");
    return 1;
  }

  SEncParamExt sParam;
  memset (&sParam, 0, sizeof (sParam));
  pEnc->GetDefaultParams (&sParam);

  if (kiBaseInit == 2) {
    // ---- defaults mode: exactly what c_vs_rust_bench's fill_params sets ----
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
  // ---- single spatial layer, single slice, RC off: the Phase-5 gate config ----
  sParam.iUsageType                 = CAMERA_VIDEO_REAL_TIME;
  sParam.iPicWidth                  = kiWidth;
  sParam.iPicHeight                 = kiHeight;
  sParam.iTargetBitrate             = 500000;
  sParam.iMaxBitrate                = UNSPECIFIED_BIT_RATE;
  sParam.iRCMode                    = (RC_MODES) kiRcMode;
  sParam.fMaxFrameRate              = 30.0f;
  sParam.iTemporalLayerNum          = 1;
  sParam.iSpatialLayerNum           = 1;
  sParam.iComplexityMode            = LOW_COMPLEXITY;
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
  sParam.bEnableLongTermReference   = false;
  sParam.iLTRRefNum                 = 0;
  sParam.iLtrMarkPeriod             = 30;
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

  // PRO_BASELINE forces CAVLC: ParamValidationExt (encoder_ext.cpp:655) resets
  // iEntropyCodingModeFlag to 0 for a baseline layer, so pinning it here made
  // `cabac 1` a silent no-op. Pick the profile from the flag instead.
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
    // uiSliceMbNum[0] rows per slice; 0 means one MB row per slice.
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
      fprintf (stderr, "Initialize failed\n");
      WelsDestroySVCEncoder (pEnc);
      return 1;
    }
  } else if (pEnc->InitializeExt (&sParam) != 0) {
    fprintf (stderr, "InitializeExt failed\n");
    WelsDestroySVCEncoder (pEnc);
    return 1;
  }

  FILE* fSrc = fopen (kpSrc, "rb");
  FILE* fOut = fopen (kpOut, "wb");
  if (!fSrc || !fOut) {
    fprintf (stderr, "file open failed\n");
    return 1;
  }

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
  for (int f = 0; f < kiFrames; ++f) {
    if ((int) fread (pBuf, 1, kiSize, fSrc) != kiSize)
      break;
    memset (&sInfo, 0, sizeof (sInfo));
    sPic.uiTimeStamp = (long long) (f * (1000 / 30.0));
    int iRet = pEnc->EncodeFrame (&sPic, &sInfo);
    if (iRet != cmResultSuccess) {
      fprintf (stderr, "EncodeFrame failed at %d: %d\n", f, iRet);
      break;
    }
    if (sInfo.eFrameType == videoFrameTypeSkip)
      continue;
    for (int l = 0; l < sInfo.iLayerNum; ++l) {
      SLayerBSInfo* p = &sInfo.sLayerInfo[l];
      int iLen = 0;
      for (int n = 0; n < p->iNalCount; ++n)
        iLen += p->pNalLengthInByte[n];
      fwrite (p->pBsBuf, 1, iLen, fOut);
    }
    fprintf (stderr, "frame %d: type=%d layers=%d bytes=%d\n",
             f, (int) sInfo.eFrameType, sInfo.iLayerNum, sInfo.iFrameSizeInBytes);
    ++iCoded;
  }

  fprintf (stderr, "coded %d frames\n", iCoded);
  delete[] pBuf;
  fclose (fSrc);
  fclose (fOut);
  pEnc->Uninitialize();
  WelsDestroySVCEncoder (pEnc);
  return 0;
}
