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
    fprintf (stderr, "usage: %s <src.yuv> <w> <h> <frames> <qp> <cabac> <gop> <out.264> [rcmode]\n", argv[0]);
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

  ISVCEncoder* pEnc = NULL;
  if (WelsCreateSVCEncoder (&pEnc) != 0 || pEnc == NULL) {
    fprintf (stderr, "WelsCreateSVCEncoder failed\n");
    return 1;
  }

  SEncParamExt sParam;
  memset (&sParam, 0, sizeof (sParam));
  pEnc->GetDefaultParams (&sParam);

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
  sParam.iMultipleThreadIdc         = 1;      // single-threaded: deterministic
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

  sParam.sSpatialLayers[0].uiProfileIdc        = PRO_BASELINE;
  sParam.sSpatialLayers[0].uiLevelIdc          = LEVEL_UNKNOWN;
  sParam.sSpatialLayers[0].iVideoWidth         = kiWidth;
  sParam.sSpatialLayers[0].iVideoHeight        = kiHeight;
  sParam.sSpatialLayers[0].fFrameRate          = 30.0f;
  sParam.sSpatialLayers[0].iSpatialBitrate     = 500000;
  sParam.sSpatialLayers[0].iMaxSpatialBitrate  = UNSPECIFIED_BIT_RATE;
  sParam.sSpatialLayers[0].iDLayerQp           = kiQp;
  sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_SINGLE_SLICE;
  sParam.sSpatialLayers[0].sSliceArgument.uiSliceNum  = 1;

  if (pEnc->InitializeExt (&sParam) != 0) {
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
