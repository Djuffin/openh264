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

// ---------------------------------------------------------------------------
// The log referee's capture side (T9.X2, F100).
//
// `OH264_TRACE_LOG=<path>` installs a trace callback that writes every delivered
// message to <path> as `<level>|<text>`, and raises the trace level to INFO so
// the INFO-level parameter and statistics blocks are actually delivered (the
// default is WELS_LOG_WARNING, and a filter that never passes looks exactly like
// a codec that never logs). Unset — which is every sweep run — this is inert and
// the driver behaves as it always did.
//
// The sink is a file rather than stdout because the drivers already write the
// bitstream and their own diagnostics there.
// ---------------------------------------------------------------------------
static FILE* g_pTraceLog = NULL;

static void TraceSink (void* pCtx, int iLevel, const char* kpString) {
  (void) pCtx;
  if (g_pTraceLog != NULL && kpString != NULL) {
    fprintf (g_pTraceLog, "%d|%s\n", iLevel, kpString);
  }
}

static void InstallTraceCapture (ISVCEncoder* pEnc) {
  const char* kpPath = getenv ("OH264_TRACE_LOG");
  if (kpPath == NULL || *kpPath == '\0') {
    return;
  }
  g_pTraceLog = fopen (kpPath, "wb");
  if (g_pTraceLog == NULL) {
    fprintf (stderr, "cxx_enc: cannot open OH264_TRACE_LOG=%s\n", kpPath);
    return;
  }
  WelsTraceCallback pfCb = TraceSink;
  pEnc->SetOption (ENCODER_OPTION_TRACE_CALLBACK, &pfCb);
  void* pTraceCtx = (void*) g_pTraceLog;
  pEnc->SetOption (ENCODER_OPTION_TRACE_CALLBACK_CONTEXT, &pTraceCtx);
  int iLevel = WELS_LOG_INFO;
  pEnc->SetOption (ENCODER_OPTION_TRACE_LEVEL, &iLevel);
}

int main (int argc, char** argv) {
  if (argc < 9) {
    fprintf (stderr, "usage: %s <src.yuv> <w> <h> <frames> <qp> <cabac> <gop> <out.264> [rcmode] [baseinit 0|1|2] [slicemode] [slicenum] [threads] [complexity] [ltr] [ltrperiod] [ltrfb] [psstrategy] [dlayers] [denoise] [bgd] [setoptext] [usage 0|1] [lossless 0|1]\n", argv[0]);
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
  // 14th: iComplexityMode. 0 LOW (default, and what every sweep preset runs),
  // 1 MEDIUM, 2 HIGH. Anything but LOW turns `bFastMode` off in the encoder and
  // selects the fine mode-decision family.
  const int   kiComplexity = (argc > 14) ? atoi (argv[14]) : 0;
  // 15th: iLTRRefNum. 0 (default) leaves long-term reference OFF, which is what
  // every preset before `ltr` ran; N > 0 turns bEnableLongTermReference on and asks
  // for N long-term slots. It also grows the spatial picture pool
  // (`kuiRefNumInTemporal = kuiLayerInTemporal + iLTRRefNum`).
  const int   kiLtrRefNum = (argc > 15) ? atoi (argv[15]) : 0;
  // 16th: iLtrMarkPeriod. 30 is FillDefault's. Small values mark far more often.
  const int   kiLtrPeriod = (argc > 16) ? atoi (argv[16]) : 30;
  // 17th: LTR feedback bitmask. 1 = send ENCODER_LTR_MARKING_FEEDBACK
  // (LTR_MARKING_SUCCESS) after every frame from the third on; 2 = send
  // ENCODER_LTR_RECOVERY_REQUEST every eighth frame. Both are what a real
  // application relays from its decoder; without them `bLTRMarkingFlag` and
  // `bReceivedT0LostFlag` are never set, so `HandleLTRMarkFeedback`'s marking arms,
  // `DeleteLTRFromLongList` and `WelsBuildRefList`'s long-reference arm are
  // unreachable. The values below are a fixed schedule, identical on both sides,
  // which is all a differential test needs.
  const int   kiLtrFb     = (argc > 17) ? atoi (argv[17]) : 0;
  // 18th: eSpsPpsIdStrategy, as the enum's own value (Phase 8b session B, T8b.B3).
  //   0 CONSTANT_ID, 1 INCREASING_ID, 2 SPS_LISTING, 3 SPS_LISTING_AND_PPS_INCREASING,
  //   6 SPS_PPS_LISTING — `codec_app_def.h:514-518`, and not a dense range.
  // Before T8b.B3 the last three refused at `InitializeExt` in the port, so this knob
  // is what refereed them: it is the only way to ask the two encoders for the same
  // listing configuration and compare the bytes.
  const int   kiPsStrategy = (argc > 18) ? atoi (argv[18]) : (int) CONSTANT_ID;

  // 19th/20th: iSpatialLayerNum and bEnableDenoise (Phase 8b session C, T8b.C1/C2).
  // These are the two axes `METHOD_DOWNSAMPLE` and `METHOD_DENOISE` sit behind, and
  // until this session the port refused both at `InitializeExt` (S48), so neither
  // had any byte coverage. Layer geometry follows `BaseEncoderTest`'s own rule
  // (`test/api/BaseEncoderTest.cpp:43`): layer i is the input halved
  // `iSpatialLayerNum - 1 - i` times, and the target bitrate is multiplied by the
  // layer count. See the `dl` preset in sweep.sh.
  const int   kiDLayers = (argc > 19) ? atoi (argv[19]) : 1;
  const int   kiDenoise = (argc > 20) ? atoi (argv[20]) : 0;

  // 21st: bEnableBackgroundDetection (Phase 9 session B4, D-ref-1). Off in every
  // driver before this session, which is why `WelsMdBackgroundMbEnc`,
  // `VaaBackgroundMbDataUpdate` and the analyzer's `BackgroundDetection` had no byte
  // coverage at all — a probe read 0 entries across five sweep configurations
  // (F117/T9.B27). `FillDefault` leaves the flag ON, so an ordinary application runs
  // this family and the harness never did. See the `bg` preset in sweep.sh.
  const int   kiBgd = (argc > 21) ? atoi (argv[21]) : 0;
  // 23rd: **the log referee's reach into `SetOption` (T9.X2)**. N > 0 re-applies
  // the *same* SEncParamExt through
  // `SetOption(ENCODER_OPTION_SVC_ENCODE_PARAM_EXT)` after frame N-1. It exists
  // because `CWelsH264SVCEncoder::LogStatistics` has exactly two callers and
  // neither is reachable from this driver otherwise: the `UpdateStatistics` path
  // needs `kiDeltaFrames > fMaxFrameRate * 2` plus a log interval (tens of
  // frames), and the two `SetOption` arms were never exercised here at all. With
  // the parameters unchanged the option is a no-op for the encode; what it is
  // NOT a no-op for is the trace, which is the point.
  //
  // 0 (default) in every sweep row, so `sweep.sh` is untouched.
  const int   kiSetOptExt = (argc > 22) ? atoi (argv[22]) : 0;
  // 23rd: iUsageType — 0 CAMERA_VIDEO_REAL_TIME (the default, and every preset
  // before P10.1), 1 SCREEN_CONTENT_REAL_TIME. 24th: bIsLosslessLink, which the
  // encoder reads only under screen usage: ParamValidationExt turns long-term
  // reference off without it (encoder_ext.cpp:415-419).
  //
  // Under screen usage the encoder's own ParamValidation forces
  // bEnableSceneChangeDetect ON and bEnableAdaptiveQuant / bEnableBackgroundDetection
  // OFF (encoder_ext.cpp:274-290) whatever the three pins below say; the port does
  // the same (wels_encoder_ext.rs), and the two trace logs show the same forcing —
  // which is part of what a screen row compares. See the `scc` preset in sweep.sh.
  const int   kiUsage    = (argc > 23) ? atoi (argv[23]) : 0;
  const int   kiLossless = (argc > 24) ? atoi (argv[24]) : 0;

  ISVCEncoder* pEnc = NULL;
  if (WelsCreateSVCEncoder (&pEnc) != 0 || pEnc == NULL) {
    fprintf (stderr, "WelsCreateSVCEncoder failed\n");
    return 1;
  }

  // Before any Initialize: the parameter block is logged from inside it.
  InstallTraceCapture (pEnc);

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
  sParam.iUsageType                 = kiUsage ? SCREEN_CONTENT_REAL_TIME : CAMERA_VIDEO_REAL_TIME;
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
  sParam.eSpsPpsIdStrategy          = (EParameterSetStrategy) kiPsStrategy;
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
  sParam.bIsLosslessLink            = (kiLossless != 0);
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

  sParam.bEnableDenoise = (kiDenoise != 0);
  sParam.bEnableBackgroundDetection = (kiBgd != 0);
  if (kiDLayers > 1) {
    const SSpatialLayerConfig kTemplate = sParam.sSpatialLayers[0];
    sParam.iSpatialLayerNum = kiDLayers;
    for (int i = 0; i < kiDLayers; i++) {
      sParam.sSpatialLayers[i] = kTemplate;
      sParam.sSpatialLayers[i].iVideoWidth  = kiWidth  >> (kiDLayers - 1 - i);
      sParam.sSpatialLayers[i].iVideoHeight = kiHeight >> (kiDLayers - 1 - i);
      sParam.sSpatialLayers[i].fFrameRate   = 30.0f;
      sParam.sSpatialLayers[i].iSpatialBitrate    = sParam.iTargetBitrate;
      sParam.sSpatialLayers[i].iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
    }
    // *After* the per-layer assignment, as `BaseEncoderTest` does it: each layer
    // carries the base rate and only the overall target scales. The other order
    // (each layer at n x base) is refused by `WelsBitRateVerification` under any
    // rc mode but RC_OFF, which would make the `dl` preset fail on both sides and
    // measure nothing.
    sParam.iTargetBitrate *= kiDLayers;
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
  // The encoder's own `uiIdrPicId` counts coded IDRs, and the LTR feedback below has
  // to quote it back or `FilterLTRMarkingFeedback` drops the packet. A real
  // application learns it from the stream; both drivers count the same frame types,
  // so counting is deterministic and identical on both sides.
  unsigned int uiIdrSeen = 0;
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
    // The referee's SetOption reach — after this frame, before the next.
    if (kiSetOptExt > 0 && f == kiSetOptExt - 1) {
      int iOptRet = pEnc->SetOption (ENCODER_OPTION_SVC_ENCODE_PARAM_EXT, &sParam);
      if (iOptRet != cmResultSuccess) {
        fprintf (stderr, "SetOption(SVC_ENCODE_PARAM_EXT) failed at %d: %d\n", f, iOptRet);
        break;
      }
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
    if (sInfo.eFrameType == videoFrameTypeIDR)
      ++uiIdrSeen;

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
  WelsDestroySVCEncoder (pEnc);
  return 0;
}
