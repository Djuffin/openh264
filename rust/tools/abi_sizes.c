/* abi_sizes.c — the C++ side's own numbers for every type that crosses the ABI.
 *
 * Phase 8 session C, T8.C4. `api/abi_guard.rs`'s pins used to carry a comment
 * saying the numbers "were extracted from the C headers with a sizeof/offsetof
 * dump on darwin/arm64" — a dump that existed once, on someone's machine, and was
 * not in the tree. This is that dump, committed, with its output beside it
 * (`abi_sizes.txt`), so a pin that disagrees with the header can be settled by
 * running a program rather than by reading a struct twice.
 *
 *   build & run:  rust/tools/abi_sizes.sh          (writes rust/tools/abi_sizes.txt)
 *
 * **It is C and not C++, deliberately.** The contract is the C ABI, a C caller is
 * exactly who it is for, and `ISVCEncoderVtbl`/`ISVCDecoderVtbl` — the two structs
 * that *are* the vtable layout, slot order included — exist only in the header's
 * `#else` (non-C++) branch. `abi_sizes.sh` compiles this file a second time as C++
 * and diffs the two outputs for every type that exists in both, so the claim "the
 * numbers are the same either way" is checked rather than asserted.
 *
 * The list is every named typedef in the three public headers (`codec_api.h`,
 * `codec_app_def.h`, `codec_def.h` — the brief's "two headers" does not survive a
 * grep: `SBufferInfo`, `SSysMEMBuffer`, `EVideoFormatType`, `EVideoFrameType` and
 * `CM_RETURN` are all in the third) that a C caller can pass or receive. That is 51
 * of the 53 the headers declare. The two left out are `SliceInfo` and
 * `SRateThresholds`, which `codec_def.h` declares and **nothing in the API or the
 * library names at all** — not a parameter, not a return, not a field, not an option
 * payload; `grep -rn` over `codec/` finds only their own declarations. The port does
 * not declare them either, so there is nothing to pin and no divergence to hide.
 */
#include <stdio.h>
#include <stddef.h>
#include "codec_api.h"
#include "codec_app_def.h"
#include "codec_def.h"

#define SZ(T)      printf("size  %-26s %zu %zu\n", #T, sizeof(T), _Alignof(T))
#define OFF(T, F)  printf("off   %-26s %-30s %zu\n", #T, #F, offsetof(T, F))

int main(void) {
  SZ(EVideoFormatType);
  SZ(EVideoFrameType);
  SZ(CM_RETURN);
  SZ(SSysMEMBuffer);
  SZ(SBufferInfo);
  SZ(OpenH264Version);
  SZ(DECODING_STATE);
  SZ(ENCODER_OPTION);
  SZ(DECODER_OPTION);
  SZ(ERROR_CON_IDC);
  SZ(FEEDBACK_VCL_NAL_IN_AU);
  SZ(LAYER_TYPE);
  SZ(LAYER_NUM);
  SZ(VIDEO_BITSTREAM_TYPE);
  SZ(KEY_FRAME_REQUEST_TYPE);
  SZ(SLTRRecoverRequest);
  SZ(SLTRMarkingFeedback);
  SZ(SLTRConfig);
  SZ(RC_MODES);
  SZ(EProfileIdc);
  SZ(ELevelIdc);
  SZ(SliceModeEnum);
  SZ(SSliceArgument);
  SZ(EVideoFormatSPS);
  SZ(EColorPrimaries);
  SZ(ETransferCharacteristics);
  SZ(EColorMatrix);
  SZ(ESampleAspectRatio);
  SZ(SSpatialLayerConfig);
  SZ(EUsageType);
  SZ(ECOMPLEXITY_MODE);
  SZ(EParameterSetStrategy);
  SZ(SEncParamBase);
  SZ(SEncParamExt);
  SZ(SVideoProperty);
  SZ(SDecodingParam);
  SZ(SLayerBSInfo);
  SZ(SFrameBSInfo);
  SZ(SSourcePicture);
  SZ(SBitrateInfo);
  SZ(SDumpLayer);
  SZ(SProfileInfo);
  SZ(SLevelInfo);
  SZ(SDeliveryStatus);
  SZ(SDecoderCapability);
  SZ(SParserBsInfo);
  SZ(SEncoderStatistics);
  SZ(SDecoderStatistics);
  SZ(SVuiSarInfo);
#ifndef __cplusplus
  SZ(ISVCEncoderVtbl);
#endif
#ifndef __cplusplus
  SZ(ISVCDecoderVtbl);
#endif

  OFF(SSysMEMBuffer, iWidth);
  OFF(SSysMEMBuffer, iHeight);
  OFF(SSysMEMBuffer, iFormat);
  OFF(SSysMEMBuffer, iStride);
  OFF(SBufferInfo, iBufferStatus);
  OFF(SBufferInfo, uiInBsTimeStamp);
  OFF(SBufferInfo, uiOutYuvTimeStamp);
  OFF(SBufferInfo, UsrData);
  OFF(SBufferInfo, pDst);
  OFF(SVideoProperty, size);
  OFF(SVideoProperty, eVideoBsType);
  OFF(SDecoderCapability, iProfileIdc);
  OFF(SDecoderCapability, iProfileIop);
  OFF(SDecoderCapability, iLevelIdc);
  OFF(SDecoderCapability, iMaxMbps);
  OFF(SDecoderCapability, iMaxFs);
  OFF(SDecoderCapability, iMaxCpb);
  OFF(SDecoderCapability, iMaxDpb);
  OFF(SDecoderCapability, iMaxBr);
  OFF(SDecoderCapability, bRedPicCap);
  OFF(SParserBsInfo, iNalNum);
  OFF(SParserBsInfo, pNalLenInByte);
  OFF(SParserBsInfo, pDstBuff);
  OFF(SParserBsInfo, iSpsWidthInPixel);
  OFF(SParserBsInfo, iSpsHeightInPixel);
  OFF(SParserBsInfo, uiInBsTimeStamp);
  OFF(SParserBsInfo, uiOutBsTimeStamp);
  OFF(SVuiSarInfo, uiSarWidth);
  OFF(SVuiSarInfo, uiSarHeight);
  OFF(SVuiSarInfo, bOverscanAppropriateFlag);
  OFF(SDecoderStatistics, uiWidth);
  OFF(SDecoderStatistics, uiHeight);
  OFF(SDecoderStatistics, fAverageFrameSpeedInMs);
  OFF(SDecoderStatistics, fActualAverageFrameSpeedInMs);
  OFF(SDecoderStatistics, uiDecodedFrameCount);
  OFF(SDecoderStatistics, uiResolutionChangeTimes);
  OFF(SDecoderStatistics, uiIDRCorrectNum);
  OFF(SDecoderStatistics, uiAvgEcRatio);
  OFF(SDecoderStatistics, uiAvgEcPropRatio);
  OFF(SDecoderStatistics, uiEcIDRNum);
  OFF(SDecoderStatistics, uiEcFrameNum);
  OFF(SDecoderStatistics, uiIDRLostNum);
  OFF(SDecoderStatistics, uiFreezingIDRNum);
  OFF(SDecoderStatistics, uiFreezingNonIDRNum);
  OFF(SDecoderStatistics, iAvgLumaQp);
  OFF(SDecoderStatistics, iSpsReportErrorNum);
  OFF(SDecoderStatistics, iSubSpsReportErrorNum);
  OFF(SDecoderStatistics, iPpsReportErrorNum);
  OFF(SDecoderStatistics, iSpsNoExistNalNum);
  OFF(SDecoderStatistics, iSubSpsNoExistNalNum);
  OFF(SDecoderStatistics, iPpsNoExistNalNum);
  OFF(SDecoderStatistics, uiProfile);
  OFF(SDecoderStatistics, uiLevel);
  OFF(SDecoderStatistics, iCurrentActiveSpsId);
  OFF(SDecoderStatistics, iCurrentActivePpsId);
  OFF(SDecoderStatistics, iStatisticsLogInterval);
#ifndef __cplusplus
  OFF(ISVCEncoderVtbl, Initialize);
  OFF(ISVCEncoderVtbl, InitializeExt);
  OFF(ISVCEncoderVtbl, GetDefaultParams);
  OFF(ISVCEncoderVtbl, Uninitialize);
  OFF(ISVCEncoderVtbl, EncodeFrame);
  OFF(ISVCEncoderVtbl, EncodeParameterSets);
  OFF(ISVCEncoderVtbl, ForceIntraFrame);
  OFF(ISVCEncoderVtbl, SetOption);
  OFF(ISVCEncoderVtbl, GetOption);
#endif
#ifndef __cplusplus
  OFF(ISVCDecoderVtbl, Initialize);
  OFF(ISVCDecoderVtbl, Uninitialize);
  OFF(ISVCDecoderVtbl, DecodeFrame);
  OFF(ISVCDecoderVtbl, DecodeFrameNoDelay);
  OFF(ISVCDecoderVtbl, DecodeFrame2);
  OFF(ISVCDecoderVtbl, FlushFrame);
  OFF(ISVCDecoderVtbl, DecodeParser);
  OFF(ISVCDecoderVtbl, DecodeFrameEx);
  OFF(ISVCDecoderVtbl, SetOption);
  OFF(ISVCDecoderVtbl, GetOption);
#endif

  OFF(SEncParamExt, iTemporalLayerNum);
  OFF(SEncParamExt, sSpatialLayers);
  OFF(SEncParamExt, iComplexityMode);
  OFF(SEncParamExt, bEnableFrameSkip);
  OFF(SEncParamExt, bEnableLongTermReference);
  OFF(SEncParamExt, iMultipleThreadIdc);
  OFF(SEncParamExt, iLoopFilterDisableIdc);
  OFF(SEncParamExt, bEnableDenoise);
  OFF(SEncParamExt, bIsLosslessLink);
  OFF(SEncParamExt, bPsnrY);
  OFF(SSpatialLayerConfig, sSliceArgument);
  OFF(SSliceArgument, uiSliceMode);
  OFF(SSliceArgument, uiSliceNum);
  OFF(SSliceArgument, uiSliceMbNum);
  OFF(SSliceArgument, uiSliceSizeConstraint);
  OFF(SSourcePicture, iStride);
  OFF(SSourcePicture, pData);
  OFF(SSourcePicture, iPicWidth);
  OFF(SSourcePicture, iPicHeight);
  OFF(SSourcePicture, uiTimeStamp);
  OFF(SLayerBSInfo, eFrameType);
  OFF(SLayerBSInfo, uiLayerType);
  OFF(SLayerBSInfo, iSubSeqId);
  OFF(SLayerBSInfo, iNalCount);
  OFF(SLayerBSInfo, pNalLengthInByte);
  OFF(SLayerBSInfo, pBsBuf);
  OFF(SLayerBSInfo, rPsnr);
  OFF(SFrameBSInfo, iLayerNum);
  OFF(SFrameBSInfo, sLayerInfo);
  OFF(SFrameBSInfo, eFrameType);
  OFF(SFrameBSInfo, iFrameSizeInBytes);
  OFF(SFrameBSInfo, uiTimeStamp);
  OFF(SDecodingParam, pFileNameRestructed);
  OFF(SDecodingParam, uiCpuLoad);
  OFF(SDecodingParam, uiTargetDqLayer);
  OFF(SDecodingParam, eEcActiveIdc);
  OFF(SDecodingParam, bParseOnly);
  OFF(SDecodingParam, sVideoProperty);
  OFF(SEncoderStatistics, uiWidth);
  OFF(SEncoderStatistics, uiHeight);
  OFF(SEncoderStatistics, fAverageFrameSpeedInMs);
  OFF(SEncoderStatistics, fAverageFrameRate);
  OFF(SEncoderStatistics, fLatestFrameRate);
  OFF(SEncoderStatistics, uiBitRate);
  OFF(SEncoderStatistics, uiAverageFrameQP);
  OFF(SEncoderStatistics, uiInputFrameCount);
  OFF(SEncoderStatistics, uiSkippedFrameCount);
  OFF(SEncoderStatistics, uiResolutionChangeTimes);
  OFF(SEncoderStatistics, uiIDRReqNum);
  OFF(SEncoderStatistics, uiIDRSentNum);
  OFF(SEncoderStatistics, uiLTRSentNum);
  OFF(SEncoderStatistics, iStatisticsTs);
  OFF(SEncoderStatistics, iTotalEncodedBytes);
  OFF(SEncParamBase, iUsageType);
  OFF(SEncParamBase, iPicWidth);
  OFF(SEncParamBase, iPicHeight);
  OFF(SEncParamBase, iTargetBitrate);
  OFF(SEncParamBase, iRCMode);
  OFF(SEncParamBase, fMaxFrameRate);
  OFF(OpenH264Version, uMajor);
  OFF(OpenH264Version, uMinor);
  OFF(OpenH264Version, uRevision);
  OFF(OpenH264Version, uReserved);
  OFF(SBitrateInfo, iLayer);
  OFF(SBitrateInfo, iBitrate);
  return 0;
}
