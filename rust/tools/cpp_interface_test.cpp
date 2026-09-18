#include <gtest/gtest.h>
#include "openh264-rs/src/api/cxx_api.rs.h"
#include <stdint.h>
#include <string.h>
#include <vector>

TEST (CppInterfaceTest, VersionAndCapability) {
  OpenH264Version ver = WelsGetCodecVersion ();
  EXPECT_EQ (2u, ver.uMajor);
  EXPECT_EQ (6u, ver.uMinor);
  EXPECT_EQ (0u, ver.uRevision);

  OpenH264Version ver_ex;
  memset (&ver_ex, 0, sizeof (ver_ex));
  WelsGetCodecVersionEx (&ver_ex);
  EXPECT_EQ (2u, ver_ex.uMajor);
  EXPECT_EQ (6u, ver_ex.uMinor);
  EXPECT_EQ (0u, ver_ex.uRevision);

  SDecoderCapability cap;
  memset (&cap, 0, sizeof (cap));
  EXPECT_EQ (0, WelsGetDecoderCapability (&cap));
  EXPECT_EQ (66, cap.iProfileIdc);
  EXPECT_EQ (32, cap.iLevelIdc);
}

TEST (CppInterfaceTest, EncoderLifecycleAndMethods) {
  ISVCEncoder* encoder = nullptr;
  ASSERT_EQ (0, WelsCreateSVCEncoder (&encoder));
  ASSERT_NE (nullptr, encoder);

  SEncParamExt param;
  memset (&param, 0, sizeof (param));
  EXPECT_EQ (0, encoder->GetDefaultParams (&param));

  const int width = 320;
  const int height = 192;
  param.iPicWidth = width;
  param.iPicHeight = height;
  param.fMaxFrameRate = 30.0f;
  param.iTargetBitrate = 500000;
  param.sSpatialLayers[0].iVideoWidth = width;
  param.sSpatialLayers[0].iVideoHeight = height;
  param.sSpatialLayers[0].fFrameRate = 30.0f;
  param.sSpatialLayers[0].iSpatialBitrate = 500000;
  ASSERT_EQ (0, encoder->InitializeExt (&param));

  int trace_level = WELS_LOG_QUIET;
  EXPECT_EQ (0, encoder->SetOption (
      ENCODER_OPTION_TRACE_LEVEL,
      reinterpret_cast<c_void*> (&trace_level)));

  int idr_interval = -1;
  EXPECT_EQ (0, encoder->GetOption (
      ENCODER_OPTION_IDR_INTERVAL,
      reinterpret_cast<c_void*> (&idr_interval)));

  EXPECT_EQ (0, encoder->ForceIntraFrame (true, -1));

  SFrameBSInfo ps_info;
  memset (&ps_info, 0, sizeof (ps_info));
  EXPECT_EQ (0, encoder->EncodeParameterSets (&ps_info));
  EXPECT_GT (ps_info.iLayerNum, 0);

  std::vector<uint8_t> y_plane (width * height, 128);
  std::vector<uint8_t> u_plane ((width / 2) * (height / 2), 128);
  std::vector<uint8_t> v_plane ((width / 2) * (height / 2), 128);

  SSourcePicture src_pic;
  memset (&src_pic, 0, sizeof (src_pic));
  src_pic.iColorFormat = videoFormatI420;
  src_pic.iPicWidth = width;
  src_pic.iPicHeight = height;
  src_pic.iStride[0] = width;
  src_pic.iStride[1] = width / 2;
  src_pic.iStride[2] = width / 2;
  src_pic.pData[0] = y_plane.data ();
  src_pic.pData[1] = u_plane.data ();
  src_pic.pData[2] = v_plane.data ();

  SFrameBSInfo bs_info;
  memset (&bs_info, 0, sizeof (bs_info));
  EXPECT_EQ (0, encoder->EncodeFrame (&src_pic, &bs_info));
  EXPECT_EQ (videoFrameTypeIDR, bs_info.eFrameType);
  EXPECT_GT (bs_info.iLayerNum, 0);

  EXPECT_EQ (0, encoder->Uninitialize ());

  // Re-initialize using SEncParamBase via Initialize()
  SEncParamBase base_param;
  memset (&base_param, 0, sizeof (base_param));
  base_param.iUsageType = CAMERA_VIDEO_REAL_TIME;
  base_param.iPicWidth = width;
  base_param.iPicHeight = height;
  base_param.fMaxFrameRate = 30.0f;
  base_param.iTargetBitrate = 500000;
  encoder->SetOption (
      ENCODER_OPTION_TRACE_LEVEL,
      reinterpret_cast<c_void*> (&trace_level));
  EXPECT_EQ (0, encoder->Initialize (&base_param));
  EXPECT_EQ (0, encoder->Uninitialize ());

  WelsDestroySVCEncoder (encoder);
}

TEST (CppInterfaceTest, EncodeAndDecodeLoopback) {
  const int width = 320;
  const int height = 192;

  ISVCEncoder* encoder = nullptr;
  ASSERT_EQ (0, WelsCreateSVCEncoder (&encoder));

  SEncParamExt param;
  memset (&param, 0, sizeof (param));
  ASSERT_EQ (0, encoder->GetDefaultParams (&param));
  param.iPicWidth = width;
  param.iPicHeight = height;
  param.fMaxFrameRate = 30.0f;
  param.iTargetBitrate = 500000;
  param.sSpatialLayers[0].iVideoWidth = width;
  param.sSpatialLayers[0].iVideoHeight = height;
  param.sSpatialLayers[0].fFrameRate = 30.0f;
  param.sSpatialLayers[0].iSpatialBitrate = 500000;
  ASSERT_EQ (0, encoder->InitializeExt (&param));

  int trace_level = WELS_LOG_QUIET;
  encoder->SetOption (
      ENCODER_OPTION_TRACE_LEVEL,
      reinterpret_cast<c_void*> (&trace_level));

  std::vector<uint8_t> y_plane (width * height, 128);
  std::vector<uint8_t> u_plane ((width / 2) * (height / 2), 128);
  std::vector<uint8_t> v_plane ((width / 2) * (height / 2), 128);

  SSourcePicture src_pic;
  memset (&src_pic, 0, sizeof (src_pic));
  src_pic.iColorFormat = videoFormatI420;
  src_pic.iPicWidth = width;
  src_pic.iPicHeight = height;
  src_pic.iStride[0] = width;
  src_pic.iStride[1] = width / 2;
  src_pic.iStride[2] = width / 2;
  src_pic.pData[0] = y_plane.data ();
  src_pic.pData[1] = u_plane.data ();
  src_pic.pData[2] = v_plane.data ();

  std::vector<std::vector<uint8_t>> frames;
  for (int f = 0; f < 2; ++f) {
    src_pic.uiTimeStamp = f * 33;
    SFrameBSInfo bs_info;
    memset (&bs_info, 0, sizeof (bs_info));
    ASSERT_EQ (0, encoder->EncodeFrame (&src_pic, &bs_info));
    std::vector<uint8_t> bitstream;
    for (int i = 0; i < bs_info.iLayerNum; ++i) {
      const SLayerBSInfo& layer = bs_info.sLayerInfo[i];
      int layer_len = 0;
      for (int j = 0; j < layer.iNalCount; ++j) {
        layer_len += layer.pNalLengthInByte[j];
      }
      bitstream.insert (bitstream.end (), layer.pBsBuf, layer.pBsBuf + layer_len);
    }
    frames.push_back (bitstream);
  }

  encoder->Uninitialize ();
  WelsDestroySVCEncoder (encoder);

  // Decode via ISVCDecoder
  ISVCDecoder* decoder = nullptr;
  ASSERT_EQ (0, WelsCreateDecoder (&decoder));
  ASSERT_NE (nullptr, decoder);

  SDecodingParam dec_param;
  memset (&dec_param, 0, sizeof (dec_param));
  dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
  ASSERT_EQ (0, decoder->Initialize (&dec_param));

  EXPECT_EQ (0, decoder->SetOption (
      DECODER_OPTION_TRACE_LEVEL,
      reinterpret_cast<c_void*> (&trace_level)));

  int eos = 0;
  EXPECT_EQ (0, decoder->GetOption (
      DECODER_OPTION_END_OF_STREAM,
      reinterpret_cast<c_void*> (&eos)));
  EXPECT_EQ (0, eos);

  // Decode frame 0 via DecodeFrameNoDelay
  unsigned char* dst[3] = { nullptr, nullptr, nullptr };
  SBufferInfo buf_info;
  memset (&buf_info, 0, sizeof (buf_info));
  DECODING_STATE state = decoder->DecodeFrameNoDelay (
      frames[0].data (), static_cast<int> (frames[0].size ()), dst, &buf_info);
  EXPECT_EQ (dsErrorFree, state);
  EXPECT_EQ (1, buf_info.iBufferStatus);
  EXPECT_EQ (width, buf_info.UsrData.sSystemBuffer.iWidth);
  EXPECT_EQ (height, buf_info.UsrData.sSystemBuffer.iHeight);
  EXPECT_NE (nullptr, dst[0]);
  EXPECT_NE (nullptr, dst[1]);
  EXPECT_NE (nullptr, dst[2]);

  // Decode frame 1 via DecodeFrame2 + FlushFrame
  memset (&buf_info, 0, sizeof (buf_info));
  state = decoder->DecodeFrame2 (
      frames[1].data (), static_cast<int> (frames[1].size ()), dst, &buf_info);
  EXPECT_EQ (dsErrorFree, state);

  int end_of_stream = 1;
  decoder->SetOption (
      DECODER_OPTION_END_OF_STREAM,
      reinterpret_cast<c_void*> (&end_of_stream));
  state = decoder->FlushFrame (dst, &buf_info);
  EXPECT_EQ (dsErrorFree, state);

  EXPECT_EQ (0, decoder->Uninitialize ());
  WelsDestroyDecoder (decoder);
}

TEST (CppInterfaceTest, DecodeParserMode) {
  const int width = 320;
  const int height = 192;

  ISVCEncoder* encoder = nullptr;
  ASSERT_EQ (0, WelsCreateSVCEncoder (&encoder));

  SEncParamExt param;
  memset (&param, 0, sizeof (param));
  ASSERT_EQ (0, encoder->GetDefaultParams (&param));
  param.iPicWidth = width;
  param.iPicHeight = height;
  param.fMaxFrameRate = 30.0f;
  param.iTargetBitrate = 500000;
  param.sSpatialLayers[0].iVideoWidth = width;
  param.sSpatialLayers[0].iVideoHeight = height;
  param.sSpatialLayers[0].fFrameRate = 30.0f;
  param.sSpatialLayers[0].iSpatialBitrate = 500000;
  ASSERT_EQ (0, encoder->InitializeExt (&param));

  int trace_level = WELS_LOG_QUIET;
  encoder->SetOption (
      ENCODER_OPTION_TRACE_LEVEL,
      reinterpret_cast<c_void*> (&trace_level));

  std::vector<uint8_t> y_plane (width * height, 128);
  std::vector<uint8_t> u_plane ((width / 2) * (height / 2), 128);
  std::vector<uint8_t> v_plane ((width / 2) * (height / 2), 128);

  SSourcePicture src_pic;
  memset (&src_pic, 0, sizeof (src_pic));
  src_pic.iColorFormat = videoFormatI420;
  src_pic.iPicWidth = width;
  src_pic.iPicHeight = height;
  src_pic.iStride[0] = width;
  src_pic.iStride[1] = width / 2;
  src_pic.iStride[2] = width / 2;
  src_pic.pData[0] = y_plane.data ();
  src_pic.pData[1] = u_plane.data ();
  src_pic.pData[2] = v_plane.data ();

  SFrameBSInfo bs_info;
  memset (&bs_info, 0, sizeof (bs_info));
  ASSERT_EQ (0, encoder->EncodeFrame (&src_pic, &bs_info));

  std::vector<uint8_t> bitstream;
  for (int i = 0; i < bs_info.iLayerNum; ++i) {
    const SLayerBSInfo& layer = bs_info.sLayerInfo[i];
    int layer_len = 0;
    for (int j = 0; j < layer.iNalCount; ++j) {
      layer_len += layer.pNalLengthInByte[j];
    }
    bitstream.insert (bitstream.end (), layer.pBsBuf, layer.pBsBuf + layer_len);
  }

  encoder->Uninitialize ();
  WelsDestroySVCEncoder (encoder);

  ISVCDecoder* parser = nullptr;
  ASSERT_EQ (0, WelsCreateDecoder (&parser));
  SDecodingParam parse_param;
  memset (&parse_param, 0, sizeof (parse_param));
  parse_param.bParseOnly = true;
  parse_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
  ASSERT_EQ (0, parser->Initialize (&parse_param));
  parser->SetOption (
      DECODER_OPTION_TRACE_LEVEL,
      reinterpret_cast<c_void*> (&trace_level));

  SParserBsInfo parser_info;
  memset (&parser_info, 0, sizeof (parser_info));
  DECODING_STATE state = parser->DecodeParser (
      bitstream.data (), static_cast<int> (bitstream.size ()), &parser_info);
  EXPECT_EQ (dsErrorFree, state);

  memset (&parser_info, 0, sizeof (parser_info));
  state = parser->DecodeParser (nullptr, 0, &parser_info);
  EXPECT_EQ (dsErrorFree, state);
  EXPECT_GT (parser_info.iNalNum, 0);

  EXPECT_EQ (0, parser->Uninitialize ());
  WelsDestroyDecoder (parser);
}
