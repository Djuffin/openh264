#include <gtest/gtest.h>
#include "codec_api.h"
#include "openh264-rs/src/api/cxx_api.rs.h"
#include <stddef.h>
#include <string.h>
#include <vector>

static void CheckFunctionOrder (int expect, int actual, const char* name) {
  EXPECT_EQ (expect, actual) << "Wrong function order: " << name;
}

typedef void (*CheckFunc) (int, int, const char*);
extern "C" void CheckEncoderInterface (ISVCEncoder* p, CheckFunc);
extern "C" void CheckDecoderInterface (ISVCDecoder* p, CheckFunc);
extern "C" size_t GetBoolSize (void);
extern "C" size_t GetBoolOffset (void);
extern "C" size_t GetBoolStructSize (void);

// Store the 'this' pointer to verify 'this' is received as expected from C code.
static void* gThis;

/**
 * Return a unique number for each virtual function so that we are able to
 * check if the order of functions in the virtual table is as expected.
 */
struct SVCEncoderImpl : public ISVCEncoder {
  virtual ~SVCEncoderImpl() {}
  virtual int EXTAPI Initialize (const SEncParamBase* pParam) {
    EXPECT_TRUE (gThis == this);
    return 1;
  }
  virtual int EXTAPI InitializeExt (const SEncParamExt* pParam) {
    EXPECT_TRUE (gThis == this);
    return 2;
  }
  virtual int EXTAPI GetDefaultParams (SEncParamExt* pParam) {
    EXPECT_TRUE (gThis == this);
    return 3;
  }
  virtual int EXTAPI Uninitialize() {
    EXPECT_TRUE (gThis == this);
    return 4;
  }
  virtual int EXTAPI EncodeFrame (const SSourcePicture* kpSrcPic,
                                  SFrameBSInfo* pBsInfo) {
    EXPECT_TRUE (gThis == this);
    return 5;
  }
  virtual int EXTAPI EncodeParameterSets (SFrameBSInfo* pBsInfo) {
    EXPECT_TRUE (gThis == this);
    return 6;
  }
  virtual int EXTAPI ForceIntraFrame (bool bIDR, int iLayerId = -1) {
    EXPECT_TRUE (gThis == this);
    return 7;
  }
  virtual int EXTAPI SetOption (ENCODER_OPTION eOptionId, void* pOption) {
    EXPECT_TRUE (gThis == this);
    return 8;
  }
  virtual int EXTAPI GetOption (ENCODER_OPTION eOptionId, void* pOption) {
    EXPECT_TRUE (gThis == this);
    return 9;
  }
};

struct SVCDecoderImpl : public ISVCDecoder {
  virtual ~SVCDecoderImpl() {}
  virtual long EXTAPI Initialize (const SDecodingParam* pParam) {
    EXPECT_TRUE (gThis == this);
    return 1;
  }
  virtual long EXTAPI Uninitialize() {
    EXPECT_TRUE (gThis == this);
    return 2;
  }
  virtual DECODING_STATE EXTAPI DecodeFrame (const unsigned char* pSrc,
      const int iSrcLen, unsigned char** ppDst, int* pStride,
      int& iWidth, int& iHeight) {
    EXPECT_TRUE (gThis == this);
    return static_cast<DECODING_STATE> (3);
  }
  virtual DECODING_STATE EXTAPI DecodeFrameNoDelay (const unsigned char* pSrc,
      const int iSrcLen, unsigned char** ppDst, SBufferInfo* pDstInfo) {
    EXPECT_TRUE (gThis == this);
    return static_cast<DECODING_STATE> (4);
  }
  virtual DECODING_STATE EXTAPI DecodeFrame2 (const unsigned char* pSrc,
      const int iSrcLen, unsigned char** ppDst, SBufferInfo* pDstInfo) {
    EXPECT_TRUE (gThis == this);
    return static_cast<DECODING_STATE> (5);
  }
  virtual DECODING_STATE EXTAPI FlushFrame (unsigned char** ppDst, SBufferInfo* pDstInfo) {
    EXPECT_TRUE (gThis == this);
    return static_cast<DECODING_STATE> (10);
  }
  virtual DECODING_STATE EXTAPI DecodeFrameEx (const unsigned char* pSrc,
      const int iSrcLen, unsigned char* pDst, int iDstStride,
      int& iDstLen, int& iWidth, int& iHeight, int& iColorFormat) {
    EXPECT_TRUE (gThis == this);
    return static_cast<DECODING_STATE> (6);
  }
  virtual DECODING_STATE EXTAPI DecodeParser (const unsigned char* pSrc,
      const int iSrcLen, SParserBsInfo* pDstInfo) {
    EXPECT_TRUE (gThis == this);
    return static_cast<DECODING_STATE> (7);
  }
  virtual long EXTAPI SetOption (DECODER_OPTION eOptionId, void* pOption) {
    EXPECT_TRUE (gThis == this);
    return static_cast<DECODING_STATE> (8);
  }
  virtual long EXTAPI GetOption (DECODER_OPTION eOptionId, void* pOption) {
    EXPECT_TRUE (gThis == this);
    return static_cast<DECODING_STATE> (9);
  }
};

TEST (ISVCEncoderTest, CheckFunctionOrder) {
  SVCEncoderImpl* p = new SVCEncoderImpl;
  gThis = p;
  CheckEncoderInterface (p, CheckFunctionOrder);
  delete p;
}

TEST (ISVCDecoderTest, CheckFunctionOrder) {
  SVCDecoderImpl* p = new SVCDecoderImpl;
  gThis = p;
  CheckDecoderInterface (p, CheckFunctionOrder);
  delete p;
}

struct bool_test_struct {
  char c;
  bool b;
};

TEST (ISVCDecoderEncoderTest, CheckCAbi) {
  EXPECT_EQ (sizeof (bool), GetBoolSize()) << "Wrong size of bool type";
  EXPECT_EQ (offsetof (bool_test_struct, b), GetBoolOffset()) << "Wrong alignment of bool in a struct";
  EXPECT_EQ (sizeof (bool_test_struct), GetBoolStructSize()) << "Wrong size of struct with a bool";
}

// ---------------------------------------------------------------------------
// Rust `cxx` C++ Bridge Integration Tests (`namespace openh264rs`)
// ---------------------------------------------------------------------------

TEST (RustCxxBridgeTest, EncoderLifecycleAndOptions) {
  openh264rs::ISVCEncoder* encoder = nullptr;
  ASSERT_EQ (0, openh264rs::WelsCreateSVCEncoder (&encoder));
  ASSERT_NE (nullptr, encoder);

  openh264rs::SEncParamExt param;
  memset (&param, 0, sizeof (param));
  EXPECT_EQ (0, encoder->GetDefaultParams (&param));

  param.iPicWidth = 320;
  param.iPicHeight = 192;
  param.fMaxFrameRate = 30.0f;
  param.iTargetBitrate = 500000;
  param.sSpatialLayers[0].iVideoWidth = 320;
  param.sSpatialLayers[0].iVideoHeight = 192;
  param.sSpatialLayers[0].fFrameRate = 30.0f;
  param.sSpatialLayers[0].iSpatialBitrate = 500000;
  EXPECT_EQ (0, encoder->InitializeExt (&param));

  int trace_level = WELS_LOG_QUIET;
  EXPECT_EQ (0, encoder->SetOption (
      ENCODER_OPTION_TRACE_LEVEL,
      reinterpret_cast<openh264rs::c_void*> (&trace_level)));

  int idr_interval = 0;
  EXPECT_EQ (0, encoder->GetOption (
      ENCODER_OPTION_IDR_INTERVAL,
      reinterpret_cast<openh264rs::c_void*> (&idr_interval)));

  EXPECT_EQ (0, encoder->ForceIntraFrame (true, -1));
  EXPECT_EQ (0, encoder->Uninitialize ());

  openh264rs::WelsDestroySVCEncoder (encoder);
}

TEST (RustCxxBridgeTest, EncodeFrameViaCxxBindingsAndDecode) {
  openh264rs::ISVCEncoder* encoder = nullptr;
  ASSERT_EQ (0, openh264rs::WelsCreateSVCEncoder (&encoder));

  openh264rs::SEncParamExt param;
  memset (&param, 0, sizeof (param));
  ASSERT_EQ (0, encoder->GetDefaultParams (&param));

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

  std::vector<uint8_t> y_plane (width * height, 128);
  std::vector<uint8_t> u_plane ((width / 2) * (height / 2), 128);
  std::vector<uint8_t> v_plane ((width / 2) * (height / 2), 128);

  openh264rs::SSourcePicture src_pic;
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

  openh264rs::SFrameBSInfo bs_info;
  memset (&bs_info, 0, sizeof (bs_info));
  ASSERT_EQ (0, encoder->EncodeFrame (&src_pic, &bs_info));
  EXPECT_EQ (videoFrameTypeIDR, bs_info.eFrameType);
  EXPECT_GT (bs_info.iLayerNum, 0);

  // Concatenate encoded NAL layers from cxx SFrameBSInfo into Annex-B bitstream
  std::vector<uint8_t> bitstream;
  for (int i = 0; i < bs_info.iLayerNum; ++i) {
    const openh264rs::SLayerBSInfo& layer = bs_info.sLayerInfo[i];
    int layer_len = 0;
    for (int j = 0; j < layer.iNalCount; ++j) {
      layer_len += layer.pNalLengthInByte[j];
    }
    bitstream.insert (bitstream.end (), layer.pBsBuf, layer.pBsBuf + layer_len);
  }
  EXPECT_GT (bitstream.size (), 0u);

  // Decode the cxx-encoded bitstream via ISVCDecoder
  ISVCDecoder* decoder = nullptr;
  ASSERT_EQ (0L, WelsCreateDecoder (&decoder));

  SDecodingParam dec_param;
  memset (&dec_param, 0, sizeof (dec_param));
  dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
  ASSERT_EQ (0L, decoder->Initialize (&dec_param));

  unsigned char* dst[3] = { nullptr, nullptr, nullptr };
  SBufferInfo buf_info;
  memset (&buf_info, 0, sizeof (buf_info));

  DECODING_STATE state = decoder->DecodeFrameNoDelay (
      bitstream.data (), static_cast<int> (bitstream.size ()), dst, &buf_info);
  EXPECT_EQ (dsErrorFree, state);
  EXPECT_EQ (1, buf_info.iBufferStatus);
  EXPECT_EQ (width, buf_info.UsrData.sSystemBuffer.iWidth);
  EXPECT_EQ (height, buf_info.UsrData.sSystemBuffer.iHeight);

  EXPECT_EQ (0L, decoder->Uninitialize ());
  WelsDestroyDecoder (decoder);

  EXPECT_EQ (0, encoder->Uninitialize ());
  openh264rs::WelsDestroySVCEncoder (encoder);
}
