/*!
 * \copy
 *     Copyright (c)  2009-2013, Cisco Systems
 *     All rights reserved.
 *
 *     Redistribution and use in source and binary forms, with or without
 *     modification, are permitted provided that the following conditions
 *     are met:
 *
 *        * Redistributions of source code must retain the above copyright
 *          notice, this list of conditions and the following disclaimer.
 *
 *        * Redistributions in binary form must reproduce the above copyright
 *          notice, this list of conditions and the following disclaimer in
 *          the documentation and/or other materials provided with the
 *          distribution.
 *
 *     THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 *     "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 *     LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 *     FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 *     COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 *     INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 *     BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 *     LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 *     CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 *     LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 *     ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 *     POSSIBILITY OF SUCH DAMAGE.
 *
 *
 *  welsDecoderExt.cpp
 *
 *  Abstract
 *      Cisco OpenH264 decoder extension utilization
 *
 *  History
 *      3/12/2009 Created
 *
 *
 ************************************************************************/
//#include <assert.h>
#include "welsDecoderExt.h"
#include "welsCodecTrace.h"
#include "codec_def.h"
#include "typedefs.h"
#include "memory_align.h"
#include "utils.h"
#include "version.h"

//#include "macros.h"
#include "decoder.h"
#include "decoder_core.h"
#include "manage_dec_ref.h"
#include "error_concealment.h"

#include "measure_time.h"
extern "C" {
#include "decoder_core.h"
#include "manage_dec_ref.h"
}
#include "error_code.h"
#include "crt_util_safe_x.h" // Safe CRT routines like util for cross platforms
#include <time.h>
#if defined(_WIN32) /*&& defined(_DEBUG)*/

#include <windows.h>
#include <stdio.h>
#include <stdarg.h>
#include <sys/types.h>
#include <malloc.h>
#else
#include <sys/time.h>
#endif

namespace WelsDec {

//////////////////////////////////////////////////////////////////////
// Construction/Destruction
//////////////////////////////////////////////////////////////////////

/***************************************************************************
*   Description:
*       class CWelsDecoder constructor function, do initialization  and
*       alloc memory required
*
*   Input parameters: none
*
*   return: none
***************************************************************************/
DECLARE_PROCTHREAD (pThrProcInit, p) {
  SWelsDecThreadInfo* sThreadInfo = (SWelsDecThreadInfo*)p;
#if defined(WIN32)
  _alloca (WELS_DEC_MAX_THREAD_STACK_SIZE * (sThreadInfo->uiThrNum + 1));
#endif
  return sThreadInfo->pThrProcMain (p);
}

static DECODING_STATE  ConstructAccessUnit (CWelsDecoder* pWelsDecoder, PWelsDecoderThreadCTX pThrCtx) {
  int iRet = dsErrorFree;
  //WelsMutexLock (&pWelsDecoder->m_csDecoder);
  if (pThrCtx->pCtx->pLastThreadCtx != NULL) {
    PWelsDecoderThreadCTX pLastThreadCtx = (PWelsDecoderThreadCTX) (pThrCtx->pCtx->pLastThreadCtx);
    WAIT_EVENT (&pLastThreadCtx->sSliceDecodeStart, WELS_DEC_THREAD_WAIT_INFINITE);
    RESET_EVENT (&pLastThreadCtx->sSliceDecodeStart);
  }
  pThrCtx->pDec = NULL;
  if (GetThreadCount (pThrCtx->pCtx) > 1) {
    RESET_EVENT (&pThrCtx->sSliceDecodeFinish);
  }
  iRet |= pWelsDecoder->DecodeFrame2WithCtx (pThrCtx->pCtx, NULL, 0, pThrCtx->ppDst, &pThrCtx->sDstInfo);

  //WelsMutexUnlock (&pWelsDecoder->m_csDecoder);
  return (DECODING_STATE)iRet;
}

DECLARE_PROCTHREAD (pThrProcFrame, p) {
  SWelsDecoderThreadCTX* pThrCtx = (SWelsDecoderThreadCTX*)p;
  while (1) {
    RELEASE_SEMAPHORE (pThrCtx->sThreadInfo.sIsBusy);
    RELEASE_SEMAPHORE (&pThrCtx->sThreadInfo.sIsIdle);
    WAIT_SEMAPHORE (&pThrCtx->sThreadInfo.sIsActivated, WELS_DEC_THREAD_WAIT_INFINITE);
    if (pThrCtx->sThreadInfo.uiCommand == WELS_DEC_THREAD_COMMAND_RUN) {
      CWelsDecoder* pWelsDecoder = (CWelsDecoder*)pThrCtx->threadCtxOwner;
      ConstructAccessUnit (pWelsDecoder, pThrCtx);
    } else if (pThrCtx->sThreadInfo.uiCommand == WELS_DEC_THREAD_COMMAND_ABORT) {
      break;
    }
  }
  return 0;
}

CWelsDecoder::CWelsDecoder (void)
  : m_pWelsTrace (NULL),
    m_uiDecodeTimeStamp (0),
    m_bIsBaseline (false),
    m_iCpuCount (1),
    m_iThreadCount (0),
    m_iCtxCount (1),
    m_pPicBuff (NULL),
    m_bParamSetsLostFlag (false),
    m_bFreezeOutput (false),
    m_DecCtxActiveCount (0),
    m_pDecThrCtx (NULL),
    m_pLastDecThrCtx (NULL),
    m_iStreamSeqNum (0) {
#ifdef OUTPUT_BIT_STREAM
  char chFileName[1024] = { 0 };  //for .264
  int iBufUsed = 0;
  int iBufLeft = 1023;
  int iCurUsed;

  char chFileNameSize[1024] = { 0 }; //for .len
  int iBufUsedSize = 0;
  int iBufLeftSize = 1023;
  int iCurUsedSize;
#endif//OUTPUT_BIT_STREAM


  m_pWelsTrace = new welsCodecTrace();
  if (m_pWelsTrace != NULL) {
    m_pWelsTrace->SetCodecInstance (this);
    m_pWelsTrace->SetTraceLevel (WELS_LOG_ERROR);

    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO, "CWelsDecoder::CWelsDecoder() entry");
  }

  ResetReorderingPictureBuffers (&m_sReoderingStatus, m_sPictInfoList, true);

  m_iCpuCount = GetCPUCount();
  if (m_iCpuCount > WELS_DEC_MAX_NUM_CPU) {
    m_iCpuCount = WELS_DEC_MAX_NUM_CPU;
  }

  m_pDecThrCtx = new SWelsDecoderThreadCTX[m_iCtxCount];
  memset (m_pDecThrCtx, 0, sizeof (SWelsDecoderThreadCTX)*m_iCtxCount);
  for (int32_t i = 0; i < WELS_DEC_MAX_NUM_CPU; ++i) {
    m_pDecThrCtxActive[i] = NULL;
  }
#ifdef OUTPUT_BIT_STREAM
  SWelsTime sCurTime;

  WelsGetTimeOfDay (&sCurTime);

  iCurUsed = WelsSnprintf (chFileName, iBufLeft, "bs_0x%p_", (void*)this);
  iCurUsedSize = WelsSnprintf (chFileNameSize, iBufLeftSize, "size_0x%p_", (void*)this);

  iBufUsed += iCurUsed;
  iBufLeft -= iCurUsed;
  if (iBufLeft > 0) {
    iCurUsed = WelsStrftime (&chFileName[iBufUsed], iBufLeft, "%y%m%d%H%M%S", &sCurTime);
    iBufUsed += iCurUsed;
    iBufLeft -= iCurUsed;
  }

  iBufUsedSize += iCurUsedSize;
  iBufLeftSize -= iCurUsedSize;
  if (iBufLeftSize > 0) {
    iCurUsedSize = WelsStrftime (&chFileNameSize[iBufUsedSize], iBufLeftSize, "%y%m%d%H%M%S", &sCurTime);
    iBufUsedSize += iCurUsedSize;
    iBufLeftSize -= iCurUsedSize;
  }

  if (iBufLeft > 0) {
    iCurUsed = WelsSnprintf (&chFileName[iBufUsed], iBufLeft, ".%03.3u.264", WelsGetMillisecond (&sCurTime));
    iBufUsed += iCurUsed;
    iBufLeft -= iCurUsed;
  }

  if (iBufLeftSize > 0) {
    iCurUsedSize = WelsSnprintf (&chFileNameSize[iBufUsedSize], iBufLeftSize, ".%03.3u.len",
                                 WelsGetMillisecond (&sCurTime));
    iBufUsedSize += iCurUsedSize;
    iBufLeftSize -= iCurUsedSize;
  }


  m_pFBS = WelsFopen (chFileName, "wb");
  m_pFBSSize = WelsFopen (chFileNameSize, "wb");
#endif//OUTPUT_BIT_STREAM
}

/***************************************************************************
*   Description:
*       class CWelsDecoder destructor function, destroy allocced memory
*
*   Input parameters: none
*
*   return: none
***************************************************************************/
CWelsDecoder::~CWelsDecoder() {
  if (m_pWelsTrace != NULL) {
    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO, "CWelsDecoder::~CWelsDecoder()");
  }
  CloseDecoderThreads();
  UninitDecoder();

#ifdef OUTPUT_BIT_STREAM
  if (m_pFBS) {
    WelsFclose (m_pFBS);
    m_pFBS = NULL;
  }
  if (m_pFBSSize) {
    WelsFclose (m_pFBSSize);
    m_pFBSSize = NULL;
  }
#endif//OUTPUT_BIT_STREAM

  if (m_pWelsTrace != NULL) {
    delete m_pWelsTrace;
    m_pWelsTrace = NULL;
  }
  if (m_pDecThrCtx != NULL) {
    delete[] m_pDecThrCtx;
    m_pDecThrCtx = NULL;
  }
}

long CWelsDecoder::Initialize (const SDecodingParam* pParam) {
  int iRet = ERR_NONE;
  if (m_pWelsTrace == NULL) {
    return cmMallocMemeError;
  }

  if (pParam == NULL) {
    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_ERROR, "CWelsDecoder::Initialize(), invalid input argument.");
    return cmInitParaError;
  }

  // H.264 decoder initialization,including memory allocation,then open it ready to decode
  iRet = InitDecoder (pParam);
  if (iRet)
    return iRet;

  return cmResultSuccess;
}

long CWelsDecoder::Uninitialize() {
  UninitDecoder();

  return ERR_NONE;
}

void CWelsDecoder::UninitDecoder (void) {
  for (int32_t i = 0; i < m_iCtxCount; ++i) {
    if (m_pDecThrCtx[i].pCtx != NULL) {
      if (i > 0) {
        WelsResetRefPicWithoutUnRef (m_pDecThrCtx[i].pCtx);
      }
      UninitDecoderCtx (m_pDecThrCtx[i].pCtx);
    }
  }
}

void CWelsDecoder::OpenDecoderThreads() {
  if (m_iThreadCount >= 1) {
    m_uiDecodeTimeStamp = 0;
    CREATE_SEMAPHORE (&m_sIsBusy, m_iThreadCount, m_iThreadCount, NULL);
    WelsMutexInit (&m_csDecoder);
    CREATE_EVENT (&m_sBufferingEvent, 1, 0, NULL);
    SET_EVENT (&m_sBufferingEvent);
    CREATE_EVENT (&m_sReleaseBufferEvent, 1, 0, NULL);
    SET_EVENT (&m_sReleaseBufferEvent);
    for (int32_t i = 0; i < m_iThreadCount; ++i) {
      m_pDecThrCtx[i].sThreadInfo.uiThrMaxNum = m_iThreadCount;
      m_pDecThrCtx[i].sThreadInfo.uiThrNum = i;
      m_pDecThrCtx[i].sThreadInfo.uiThrStackSize = WELS_DEC_MAX_THREAD_STACK_SIZE;
      m_pDecThrCtx[i].sThreadInfo.pThrProcMain = pThrProcFrame;
      m_pDecThrCtx[i].sThreadInfo.sIsBusy = &m_sIsBusy;
      m_pDecThrCtx[i].sThreadInfo.uiCommand = WELS_DEC_THREAD_COMMAND_RUN;
      m_pDecThrCtx[i].threadCtxOwner = this;
      m_pDecThrCtx[i].kpSrc = NULL;
      m_pDecThrCtx[i].kiSrcLen = 0;
      m_pDecThrCtx[i].ppDst = NULL;
      m_pDecThrCtx[i].pDec = NULL;
      CREATE_EVENT (&m_pDecThrCtx[i].sImageReady, 1, 0, NULL);
      CREATE_EVENT (&m_pDecThrCtx[i].sSliceDecodeStart, 1, 0, NULL);
      CREATE_EVENT (&m_pDecThrCtx[i].sSliceDecodeFinish, 1, 0, NULL);
      CREATE_SEMAPHORE (&m_pDecThrCtx[i].sThreadInfo.sIsIdle, 0, 1, NULL);
      CREATE_SEMAPHORE (&m_pDecThrCtx[i].sThreadInfo.sIsActivated, 0, 1, NULL);
      CREATE_THREAD (&m_pDecThrCtx[i].sThreadInfo.sThrHandle, pThrProcInit, (void*) (& (m_pDecThrCtx[i])));
    }
  }
}
void CWelsDecoder::CloseDecoderThreads() {
  if (m_iThreadCount >= 1) {
    SET_EVENT (&m_sReleaseBufferEvent);
    for (int32_t i = 0; i < m_iThreadCount; i++) { //waiting the completion begun slices
      WAIT_SEMAPHORE (&m_pDecThrCtx[i].sThreadInfo.sIsIdle, WELS_DEC_THREAD_WAIT_INFINITE);
      m_pDecThrCtx[i].sThreadInfo.uiCommand = WELS_DEC_THREAD_COMMAND_ABORT;
      RELEASE_SEMAPHORE (&m_pDecThrCtx[i].sThreadInfo.sIsActivated);
      WAIT_THREAD (&m_pDecThrCtx[i].sThreadInfo.sThrHandle);
      CLOSE_EVENT (&m_pDecThrCtx[i].sImageReady);
      CLOSE_EVENT (&m_pDecThrCtx[i].sSliceDecodeStart);
      CLOSE_EVENT (&m_pDecThrCtx[i].sSliceDecodeFinish);
      CLOSE_SEMAPHORE (&m_pDecThrCtx[i].sThreadInfo.sIsIdle);
      CLOSE_SEMAPHORE (&m_pDecThrCtx[i].sThreadInfo.sIsActivated);
    }
    WelsMutexDestroy (&m_csDecoder);
    CLOSE_EVENT (&m_sBufferingEvent);
    CLOSE_EVENT (&m_sReleaseBufferEvent);
    CLOSE_SEMAPHORE (&m_sIsBusy);
  }
}

void CWelsDecoder::UninitDecoderCtx (PWelsDecoderContext& pCtx) {
  if (pCtx != NULL) {

    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO, "CWelsDecoder::UninitDecoderCtx(), openh264 codec version = %s.",
             VERSION_NUMBER);

    WelsEndDecoder (pCtx);

    if (pCtx->pMemAlign != NULL) {
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO,
               "CWelsDecoder::UninitDecoder(), verify memory usage (%d bytes) after free..",
               pCtx->pMemAlign->WelsGetMemoryUsage());
      delete pCtx->pMemAlign;
      pCtx->pMemAlign = NULL;
    }

    if (NULL != pCtx) {
      WelsFree (pCtx, "m_pDecContext");

      pCtx = NULL;
    }
    if (m_iCtxCount <= 1) m_pDecThrCtx[0].pCtx = NULL;
  }
}

// the return value of this function is not suitable, it need report failure info to upper layer.
int32_t CWelsDecoder::InitDecoder (const SDecodingParam* pParam) {

  WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO,
           "CWelsDecoder::init_decoder(), openh264 codec version = %s, ParseOnly = %d",
           VERSION_NUMBER, (int32_t)pParam->bParseOnly);
  if (m_iThreadCount >= 1 && pParam->bParseOnly) {
    m_iThreadCount = 0;
  }
  OpenDecoderThreads();
  //reset decoder context
  memset (&m_sDecoderStatistics, 0, sizeof (SDecoderStatistics));
  memset (&m_sLastDecPicInfo, 0, sizeof (SWelsLastDecPicInfo));
  memset (&m_sVlcTable, 0, sizeof (SVlcTable));
  UninitDecoder();
  WelsDecoderLastDecPicInfoDefaults (m_sLastDecPicInfo);
  for (int32_t i = 0; i < m_iCtxCount; ++i) {
    InitDecoderCtx (m_pDecThrCtx[i].pCtx, pParam);
    if (m_iThreadCount >= 1) {
      m_pDecThrCtx[i].pCtx->pThreadCtx = &m_pDecThrCtx[i];
    }
  }
  m_bParamSetsLostFlag = false;
  m_bFreezeOutput = false;
  return cmResultSuccess;
}

// the return value of this function is not suitable, it need report failure info to upper layer.
int32_t CWelsDecoder::InitDecoderCtx (PWelsDecoderContext& pCtx, const SDecodingParam* pParam) {

  WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO,
           "CWelsDecoder::init_decoder(), openh264 codec version = %s, ParseOnly = %d",
           VERSION_NUMBER, (int32_t)pParam->bParseOnly);

  //reset decoder context
  UninitDecoderCtx (pCtx);
  pCtx = (PWelsDecoderContext)WelsMallocz (sizeof (SWelsDecoderContext), "m_pDecContext");
  if (NULL == pCtx)
    return cmMallocMemeError;
  int32_t iCacheLineSize = 16;   // on chip cache line size in byte
  pCtx->pMemAlign = new CMemoryAlign (iCacheLineSize);
  WELS_VERIFY_RETURN_PROC_IF (cmMallocMemeError, (NULL == pCtx->pMemAlign), UninitDecoderCtx (pCtx))
  if (m_iCtxCount <= 1) m_pDecThrCtx[0].pCtx = pCtx;
  //fill in default value into context
  pCtx->pLastDecPicInfo = &m_sLastDecPicInfo;
  pCtx->pDecoderStatistics = &m_sDecoderStatistics;
  pCtx->pVlcTable = &m_sVlcTable;
  pCtx->pPictInfoList = m_sPictInfoList;
  pCtx->pPictReoderingStatus = &m_sReoderingStatus;
  pCtx->pCsDecoder = &m_csDecoder;
  pCtx->pStreamSeqNum = &m_iStreamSeqNum;
  WelsDecoderDefaults (pCtx, &m_pWelsTrace->m_sLogCtx);
  WelsDecoderSpsPpsDefaults (pCtx->sSpsPpsCtx);
  //check param and update decoder context
  pCtx->pParam = (SDecodingParam*)pCtx->pMemAlign->WelsMallocz (sizeof (SDecodingParam),
                 "SDecodingParam");
  WELS_VERIFY_RETURN_PROC_IF (cmMallocMemeError, (NULL == pCtx->pParam), UninitDecoderCtx (pCtx));
  int32_t iRet = DecoderConfigParam (pCtx, pParam);
  WELS_VERIFY_RETURN_IFNEQ (iRet, cmResultSuccess);

  //init decoder
  WELS_VERIFY_RETURN_PROC_IF (cmMallocMemeError, WelsInitDecoder (pCtx, &m_pWelsTrace->m_sLogCtx),
                              UninitDecoderCtx (pCtx))
  pCtx->pPicBuff = NULL;
  return cmResultSuccess;
}

int32_t CWelsDecoder::ResetDecoder (PWelsDecoderContext& pCtx) {
  // TBC: need to be modified when context and trace point are null
  if (m_iThreadCount >= 1) {
    ThreadResetDecoder (pCtx);
  } else {
    if (pCtx != NULL && m_pWelsTrace != NULL) {
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO, "ResetDecoder(), context error code is %d",
               pCtx->iErrorCode);
      SDecodingParam sPrevParam;
      memcpy (&sPrevParam, pCtx->pParam, sizeof (SDecodingParam));

      WELS_VERIFY_RETURN_PROC_IF (cmInitParaError, InitDecoderCtx (pCtx, &sPrevParam),
                                  UninitDecoderCtx (pCtx));
    } else if (m_pWelsTrace != NULL) {
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_ERROR, "ResetDecoder() failed as decoder context null");
    }
    ResetReorderingPictureBuffers (&m_sReoderingStatus, m_sPictInfoList, false);
  }
  return ERR_INFO_UNINIT;
}

int32_t CWelsDecoder::ThreadResetDecoder (PWelsDecoderContext& pCtx) {
  // TBC: need to be modified when context and trace point are null
  SDecodingParam sPrevParam;
  if (pCtx != NULL && m_pWelsTrace != NULL) {
    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO, "ResetDecoder(), context error code is %d", pCtx->iErrorCode);
    memcpy (&sPrevParam, pCtx->pParam, sizeof (SDecodingParam));
    ResetReorderingPictureBuffers (&m_sReoderingStatus, m_sPictInfoList, true);
    CloseDecoderThreads();
    UninitDecoder();
    InitDecoder (&sPrevParam);
  } else if (m_pWelsTrace != NULL) {
    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_ERROR, "ResetDecoder() failed as decoder context null");
  }
  return ERR_INFO_UNINIT;
}

/*
 * Set Option
 */
long CWelsDecoder::SetOption (DECODER_OPTION eOptID, void* pOption) {
  int iVal = 0;
  if (eOptID == DECODER_OPTION_NUM_OF_THREADS) {
    if (pOption != NULL) {
      int32_t threadCount = * ((int32_t*)pOption);
      if (threadCount < 0) threadCount = 0;
      if (threadCount > m_iCpuCount) {
        threadCount = m_iCpuCount;
      }
      if (threadCount > 3) {
        threadCount = 3;
      }
      if (threadCount != m_iThreadCount) {
        m_iThreadCount = threadCount;
        if (m_pDecThrCtx != NULL) {
          delete [] m_pDecThrCtx;
          m_iCtxCount = m_iThreadCount == 0 ? 1 : m_iThreadCount;
          m_pDecThrCtx = new SWelsDecoderThreadCTX[m_iCtxCount];
          memset (m_pDecThrCtx, 0, sizeof (SWelsDecoderThreadCTX)*m_iCtxCount);
        }
      }
    }
    return cmResultSuccess;
  }
  for (int32_t i = 0; i < m_iCtxCount; ++i) {
    PWelsDecoderContext pDecContext = m_pDecThrCtx[i].pCtx;
    if (pDecContext == NULL && eOptID != DECODER_OPTION_TRACE_LEVEL &&
        eOptID != DECODER_OPTION_TRACE_CALLBACK && eOptID != DECODER_OPTION_TRACE_CALLBACK_CONTEXT)
      return dsInitialOptExpected;
    if (eOptID == DECODER_OPTION_END_OF_STREAM) { // Indicate bit-stream of the final frame to be decoded
      if (pOption == NULL)
        return cmInitParaError;

      iVal = * ((int*)pOption); // boolean value for whether enabled End Of Stream flag

      if (pDecContext == NULL) return dsInitialOptExpected;

      pDecContext->bEndOfStreamFlag = iVal ? true : false;
      if (iVal && m_iThreadCount >= 1)
        SET_EVENT (&m_sReleaseBufferEvent);

      return cmResultSuccess;
    } else if (eOptID == DECODER_OPTION_ERROR_CON_IDC) { // Indicate error concealment status
      if (pOption == NULL)
        return cmInitParaError;

      if (pDecContext == NULL) return dsInitialOptExpected;

      iVal = * ((int*)pOption); // int value for error concealment idc
      iVal = WELS_CLIP3 (iVal, (int32_t)ERROR_CON_DISABLE, (int32_t)ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE);
      if ((pDecContext->pParam->bParseOnly) && (iVal != (int32_t)ERROR_CON_DISABLE)) {
        WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO,
                 "CWelsDecoder::SetOption for ERROR_CON_IDC = %d not allowd for parse only!.", iVal);
        return cmInitParaError;
      }

      pDecContext->pParam->eEcActiveIdc = (ERROR_CON_IDC)iVal;
      InitErrorCon (pDecContext);
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO,
               "CWelsDecoder::SetOption for ERROR_CON_IDC = %d.", iVal);

      return cmResultSuccess;
    } else if (eOptID == DECODER_OPTION_TRACE_LEVEL) {
      if (m_pWelsTrace) {
        uint32_t level = * ((uint32_t*)pOption);
        m_pWelsTrace->SetTraceLevel (level);
      }
      return cmResultSuccess;
    } else if (eOptID == DECODER_OPTION_TRACE_CALLBACK) {
      if (m_pWelsTrace) {
        WelsTraceCallback callback = * ((WelsTraceCallback*)pOption);
        m_pWelsTrace->SetTraceCallback (callback);
        WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO,
                 "CWelsDecoder::SetOption():DECODER_OPTION_TRACE_CALLBACK callback = %p.",
                 callback);
      }
      return cmResultSuccess;
    } else if (eOptID == DECODER_OPTION_TRACE_CALLBACK_CONTEXT) {
      if (m_pWelsTrace) {
        void* ctx = * ((void**)pOption);
        m_pWelsTrace->SetTraceCallbackContext (ctx);
      }
      return cmResultSuccess;
    } else if (eOptID == DECODER_OPTION_GET_STATISTICS) {
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_WARNING,
               "CWelsDecoder::SetOption():DECODER_OPTION_GET_STATISTICS: this option is get-only!");
      return cmInitParaError;
    } else if (eOptID == DECODER_OPTION_STATISTICS_LOG_INTERVAL) {
      if (pOption) {
        if (pDecContext == NULL) return dsInitialOptExpected;
        pDecContext->pDecoderStatistics->iStatisticsLogInterval = (* ((unsigned int*)pOption));
        return cmResultSuccess;
      }
    } else if (eOptID == DECODER_OPTION_GET_SAR_INFO) {
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_WARNING,
               "CWelsDecoder::SetOption():DECODER_OPTION_GET_SAR_INFO: this option is get-only!");
      return cmInitParaError;
    }
  }
  return cmInitParaError;
}

/*
 *  Get Option
 */
long CWelsDecoder::GetOption (DECODER_OPTION eOptID, void* pOption) {
  int iVal = 0;
  if (DECODER_OPTION_NUM_OF_THREADS == eOptID) {
    * ((int*)pOption) = m_iThreadCount;
    return cmResultSuccess;
  }
  PWelsDecoderContext pDecContext = m_pDecThrCtx[0].pCtx;
  if (pDecContext == NULL)
    return cmInitExpected;

  if (pOption == NULL)
    return cmInitParaError;

  if (DECODER_OPTION_END_OF_STREAM == eOptID) {
    iVal = pDecContext->bEndOfStreamFlag;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  }
#ifdef LONG_TERM_REF
  else if (DECODER_OPTION_IDR_PIC_ID == eOptID) {
    iVal = pDecContext->uiCurIdrPicId;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_FRAME_NUM == eOptID) {
    iVal = pDecContext->iFrameNum;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_LTR_MARKING_FLAG == eOptID) {
    iVal = pDecContext->bCurAuContainLtrMarkSeFlag;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_LTR_MARKED_FRAME_NUM == eOptID) {
    iVal = pDecContext->iFrameNumOfAuMarkedLtr;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  }
#endif
  else if (DECODER_OPTION_VCL_NAL == eOptID) { //feedback whether or not have VCL NAL in current AU
    iVal = pDecContext->iFeedbackVclNalInAu;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_TEMPORAL_ID == eOptID) { //if have VCL NAL in current AU, then feedback the temporal ID
    iVal = pDecContext->iFeedbackTidInAu;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_IS_REF_PIC == eOptID) {
    iVal = pDecContext->iFeedbackNalRefIdc;
    if (iVal > 0)
      iVal = 1;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_ERROR_CON_IDC == eOptID) {
    iVal = (int)pDecContext->pParam->eEcActiveIdc;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_GET_STATISTICS == eOptID) { // get decoder statistics info for real time debugging
    SDecoderStatistics* pDecoderStatistics = (static_cast<SDecoderStatistics*> (pOption));

    memcpy (pDecoderStatistics, pDecContext->pDecoderStatistics, sizeof (SDecoderStatistics));

    if (pDecContext->pDecoderStatistics->uiDecodedFrameCount != 0) { //not original status
      pDecoderStatistics->fAverageFrameSpeedInMs = (float) (pDecContext->dDecTime) /
          (pDecContext->pDecoderStatistics->uiDecodedFrameCount);
      pDecoderStatistics->fActualAverageFrameSpeedInMs = (float) (pDecContext->dDecTime) /
          (pDecContext->pDecoderStatistics->uiDecodedFrameCount + pDecContext->pDecoderStatistics->uiFreezingIDRNum +
           pDecContext->pDecoderStatistics->uiFreezingNonIDRNum);
    }
    return cmResultSuccess;
  } else if (eOptID == DECODER_OPTION_STATISTICS_LOG_INTERVAL) {
    if (pOption) {
      iVal = pDecContext->pDecoderStatistics->iStatisticsLogInterval;
      * ((unsigned int*)pOption) = iVal;
      return cmResultSuccess;
    }
  } else if (DECODER_OPTION_GET_SAR_INFO == eOptID) { //get decoder SAR info in VUI
    PVuiSarInfo pVuiSarInfo = (static_cast<PVuiSarInfo> (pOption));
    memset (pVuiSarInfo, 0, sizeof (SVuiSarInfo));
    if (!pDecContext->pSps) {
      return cmInitExpected;
    } else {
      pVuiSarInfo->uiSarWidth = pDecContext->pSps->sVui.uiSarWidth;
      pVuiSarInfo->uiSarHeight = pDecContext->pSps->sVui.uiSarHeight;
      pVuiSarInfo->bOverscanAppropriateFlag = pDecContext->pSps->sVui.bOverscanAppropriateFlag;
      return cmResultSuccess;
    }
  } else if (DECODER_OPTION_PROFILE == eOptID) {
    if (!pDecContext->pSps) {
      return cmInitExpected;
    }
    iVal = (int)pDecContext->pSps->uiProfileIdc;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_LEVEL == eOptID) {
    if (!pDecContext->pSps) {
      return cmInitExpected;
    }
    iVal = (int)pDecContext->pSps->uiLevelIdc;
    * ((int*)pOption) = iVal;
    return cmResultSuccess;
  } else if (DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER == eOptID) {
    for (int32_t activeThread = 0; activeThread < m_DecCtxActiveCount; ++activeThread) {
      WAIT_SEMAPHORE (&m_pDecThrCtxActive[activeThread]->sThreadInfo.sIsIdle, WELS_DEC_THREAD_WAIT_INFINITE);
      RELEASE_SEMAPHORE (&m_pDecThrCtxActive[activeThread]->sThreadInfo.sIsIdle);
    }
    * ((int*)pOption) = m_sReoderingStatus.iNumOfPicts;
    return cmResultSuccess;
  }

  return cmInitParaError;
}

DECODING_STATE CWelsDecoder::DecodeFrameNoDelay (const unsigned char* kpSrc,
    const int kiSrcLen,
    unsigned char** ppDst,
    SBufferInfo* pDstInfo) {
  int iRet = dsErrorFree;
  if (m_iThreadCount >= 1) {
    SET_EVENT (&m_sReleaseBufferEvent);
    iRet = ThreadDecodeFrameInternal (kpSrc, kiSrcLen, ppDst, pDstInfo);
    if (m_sReoderingStatus.iNumOfPicts) {
      WAIT_EVENT (&m_sBufferingEvent, WELS_DEC_THREAD_WAIT_INFINITE);
      RESET_EVENT (&m_sBufferingEvent);
      RESET_EVENT (&m_sReleaseBufferEvent);
      ReleaseBufferedReadyPictureReorder (NULL, ppDst, pDstInfo);
    }
    return (DECODING_STATE)iRet;
  }
  //SBufferInfo sTmpBufferInfo;
  //unsigned char* ppTmpDst[3] = {NULL, NULL, NULL};
  iRet = (int)DecodeFrame2 (kpSrc, kiSrcLen, ppDst, pDstInfo);
  //memcpy (&sTmpBufferInfo, pDstInfo, sizeof (SBufferInfo));
  //ppTmpDst[0] = ppDst[0];
  //ppTmpDst[1] = ppDst[1];
  //ppTmpDst[2] = ppDst[2];
  iRet |= DecodeFrame2 (NULL, 0, ppDst, pDstInfo);
  //if ((pDstInfo->iBufferStatus == 0) && (sTmpBufferInfo.iBufferStatus == 1)) {
  //memcpy (pDstInfo, &sTmpBufferInfo, sizeof (SBufferInfo));
  //ppDst[0] = ppTmpDst[0];
  //ppDst[1] = ppTmpDst[1];
  //ppDst[2] = ppTmpDst[2];
  //}
  return (DECODING_STATE)iRet;
}

DECODING_STATE CWelsDecoder::DecodeFrame2WithCtx (PWelsDecoderContext pDecContext, const unsigned char* kpSrc,
    const int kiSrcLen,
    unsigned char** ppDst,
    SBufferInfo* pDstInfo) {
  if (pDecContext == NULL || pDecContext->pParam == NULL) {
    if (m_pWelsTrace != NULL) {
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_ERROR, "Call DecodeFrame2 without Initialize.\n");
    }
    return dsInitialOptExpected;
  }

  if (pDecContext->pParam->bParseOnly) {
    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_ERROR, "bParseOnly should be false for this API calling! \n");
    pDecContext->iErrorCode |= dsInvalidArgument;
    return dsInvalidArgument;
  }
  if (CheckBsBuffer (pDecContext, kiSrcLen)) {
    if (ResetDecoder(pDecContext)) {
      if (pDstInfo) pDstInfo->iBufferStatus = 0;
      return dsOutOfMemory;
    }
    return dsErrorFree;
  }
  if (kiSrcLen > 0 && kpSrc != NULL) {
#ifdef OUTPUT_BIT_STREAM
    if (m_pFBS) {
      WelsFwrite (kpSrc, sizeof (unsigned char), kiSrcLen, m_pFBS);
      WelsFflush (m_pFBS);
    }
    if (m_pFBSSize) {
      WelsFwrite (&kiSrcLen, sizeof (int), 1, m_pFBSSize);
      WelsFflush (m_pFBSSize);
    }
#endif//OUTPUT_BIT_STREAM
    pDecContext->bEndOfStreamFlag = false;
    if (GetThreadCount (pDecContext) <= 0) {
      pDecContext->uiDecodingTimeStamp = ++m_uiDecodeTimeStamp;
    }
  } else {
    //For application MODE, the error detection should be added for safe.
    //But for CONSOLE MODE, when decoding LAST AU, kiSrcLen==0 && kpSrc==NULL.
    pDecContext->bEndOfStreamFlag = true;
    pDecContext->bInstantDecFlag = true;
  }

  int64_t iStart, iEnd;
  iStart = WelsTime();

  if (GetThreadCount (pDecContext) <= 1) {
    ppDst[0] = ppDst[1] = ppDst[2] = NULL;
  }
  pDecContext->iErrorCode = dsErrorFree; //initialize at the starting of AU decoding.
  pDecContext->iFeedbackVclNalInAu = FEEDBACK_UNKNOWN_NAL; //initialize
  unsigned long long uiInBsTimeStamp = pDstInfo->uiInBsTimeStamp;
  if (GetThreadCount (pDecContext) <= 1) {
    memset (pDstInfo, 0, sizeof (SBufferInfo));
  }
  pDstInfo->uiInBsTimeStamp = uiInBsTimeStamp;
#ifdef LONG_TERM_REF
  pDecContext->bReferenceLostAtT0Flag = false; //initialize for LTR
  pDecContext->bCurAuContainLtrMarkSeFlag = false;
  pDecContext->iFrameNumOfAuMarkedLtr = 0;
  pDecContext->iFrameNum = -1; //initialize
#endif

  if (GetThreadCount (pDecContext) >= 1) {
    WAIT_EVENT (&m_sReleaseBufferEvent, WELS_DEC_THREAD_WAIT_INFINITE);
  }

  pDecContext->iFeedbackTidInAu = -1; //initialize
  pDecContext->iFeedbackNalRefIdc = -1; //initialize
  if (pDstInfo) {
    pDstInfo->uiOutYuvTimeStamp = 0;
    pDecContext->uiTimeStamp = pDstInfo->uiInBsTimeStamp;
  } else {
    pDecContext->uiTimeStamp = 0;
  }
  WelsDecodeBs (pDecContext, kpSrc, kiSrcLen, ppDst,
                pDstInfo, NULL); //iErrorCode has been modified in this function
  pDecContext->bInstantDecFlag = false; //reset no-delay flag
  if (pDecContext->iErrorCode) {
    EWelsNalUnitType eNalType =
      NAL_UNIT_UNSPEC_0; //for NBR, IDR frames are expected to decode as followed if error decoding an IDR currently

    eNalType = pDecContext->sCurNalHead.eNalUnitType;
    if (pDecContext->iErrorCode & dsOutOfMemory) {
      if (ResetDecoder (pDecContext)) {
        if (pDstInfo) pDstInfo->iBufferStatus = 0;
        return dsOutOfMemory;
      }
      return dsErrorFree;
    }
    if (pDecContext->iErrorCode & dsRefListNullPtrs) {
      if (ResetDecoder (pDecContext)) {
        if (pDstInfo) pDstInfo->iBufferStatus = 0;
        return dsRefListNullPtrs;
      }
      return dsErrorFree;
    }
    //for AVC bitstream (excluding AVC with temporal scalability, including TP), as long as error occur, SHOULD notify upper layer key frame loss.
    if ((IS_PARAM_SETS_NALS (eNalType) || NAL_UNIT_CODED_SLICE_IDR == eNalType) ||
        (VIDEO_BITSTREAM_AVC == pDecContext->eVideoType)) {
      if (pDecContext->pParam->eEcActiveIdc == ERROR_CON_DISABLE) {
#ifdef LONG_TERM_REF
        pDecContext->bParamSetsLostFlag = true;
#else
        pDecContext->bReferenceLostAtT0Flag = true;
#endif
      }
    }

    if (pDecContext->bPrintFrameErrorTraceFlag) {
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO, "decode failed, failure type:%d \n",
               pDecContext->iErrorCode);
      pDecContext->bPrintFrameErrorTraceFlag = false;
    } else {
      pDecContext->iIgnoredErrorInfoPacketCount++;
      if (pDecContext->iIgnoredErrorInfoPacketCount == INT_MAX) {
        WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_WARNING, "continuous error reached INT_MAX! Restart as 0.");
        pDecContext->iIgnoredErrorInfoPacketCount = 0;
      }
    }
    if ((pDecContext->pParam->eEcActiveIdc != ERROR_CON_DISABLE) && (pDstInfo->iBufferStatus == 1)) {
      //TODO after dec status updated
      pDecContext->iErrorCode |= dsDataErrorConcealed;

      pDecContext->pDecoderStatistics->uiDecodedFrameCount++;
      if (pDecContext->pDecoderStatistics->uiDecodedFrameCount == 0) { //exceed max value of uint32_t
        ResetDecStatNums (pDecContext->pDecoderStatistics);
        pDecContext->pDecoderStatistics->uiDecodedFrameCount++;
      }
      int32_t iMbConcealedNum = pDecContext->iMbEcedNum + pDecContext->iMbEcedPropNum;
      pDecContext->pDecoderStatistics->uiAvgEcRatio = pDecContext->iMbNum == 0 ?
          (pDecContext->pDecoderStatistics->uiAvgEcRatio * pDecContext->pDecoderStatistics->uiEcFrameNum) : ((
                pDecContext->pDecoderStatistics->uiAvgEcRatio * pDecContext->pDecoderStatistics->uiEcFrameNum) + ((
                      iMbConcealedNum * 100) / pDecContext->iMbNum));
      pDecContext->pDecoderStatistics->uiAvgEcPropRatio = pDecContext->iMbNum == 0 ?
          (pDecContext->pDecoderStatistics->uiAvgEcPropRatio * pDecContext->pDecoderStatistics->uiEcFrameNum) : ((
                pDecContext->pDecoderStatistics->uiAvgEcPropRatio * pDecContext->pDecoderStatistics->uiEcFrameNum) + ((
                      pDecContext->iMbEcedPropNum * 100) / pDecContext->iMbNum));
      pDecContext->pDecoderStatistics->uiEcFrameNum += (iMbConcealedNum == 0 ? 0 : 1);
      pDecContext->pDecoderStatistics->uiAvgEcRatio = pDecContext->pDecoderStatistics->uiEcFrameNum == 0 ? 0 :
          pDecContext->pDecoderStatistics->uiAvgEcRatio / pDecContext->pDecoderStatistics->uiEcFrameNum;
      pDecContext->pDecoderStatistics->uiAvgEcPropRatio = pDecContext->pDecoderStatistics->uiEcFrameNum == 0 ? 0 :
          pDecContext->pDecoderStatistics->uiAvgEcPropRatio / pDecContext->pDecoderStatistics->uiEcFrameNum;
    }
    iEnd = WelsTime();
    pDecContext->dDecTime += (iEnd - iStart) / 1e3;

    OutputStatisticsLog (*pDecContext->pDecoderStatistics);
    if (GetThreadCount (pDecContext) >= 1) {
      BufferingReadyPicture (pDecContext, ppDst, pDstInfo);
      SET_EVENT (&m_sBufferingEvent);
    } else {
      ReorderPicturesInDisplay (pDecContext, ppDst, pDstInfo);
    }

    return (DECODING_STATE)pDecContext->iErrorCode;
  }
  // else Error free, the current codec works well

  if (pDstInfo->iBufferStatus == 1) {

    pDecContext->pDecoderStatistics->uiDecodedFrameCount++;
    if (pDecContext->pDecoderStatistics->uiDecodedFrameCount == 0) { //exceed max value of uint32_t
      ResetDecStatNums (pDecContext->pDecoderStatistics);
      pDecContext->pDecoderStatistics->uiDecodedFrameCount++;
    }

    OutputStatisticsLog (*pDecContext->pDecoderStatistics);
  }
  iEnd = WelsTime();
  pDecContext->dDecTime += (iEnd - iStart) / 1e3;

  if (GetThreadCount (pDecContext) >= 1) {
    BufferingReadyPicture (pDecContext, ppDst, pDstInfo);
    SET_EVENT (&m_sBufferingEvent);
  } else {
    ReorderPicturesInDisplay (pDecContext, ppDst, pDstInfo);
  }
  return dsErrorFree;
}

DECODING_STATE CWelsDecoder::DecodeFrame2 (const unsigned char* kpSrc,
    const int kiSrcLen,
    unsigned char** ppDst,
    SBufferInfo* pDstInfo) {
  PWelsDecoderContext pDecContext = m_pDecThrCtx[0].pCtx;
  return DecodeFrame2WithCtx (pDecContext, kpSrc, kiSrcLen, ppDst, pDstInfo);
}

DECODING_STATE CWelsDecoder::FlushFrame (unsigned char** ppDst,
    SBufferInfo* pDstInfo) {
  bool bEndOfStreamFlag = true;
  if (m_iThreadCount <= 1) {
    for (int32_t j = 0; j < m_iCtxCount; ++j) {
      if (!m_pDecThrCtx[j].pCtx->bEndOfStreamFlag) {
        bEndOfStreamFlag = false;
      }
    }
  }
  if (bEndOfStreamFlag && m_sReoderingStatus.iNumOfPicts > 0) {
    ReleaseBufferedReadyPictureReorder (NULL, ppDst, pDstInfo, true);
  }
  return dsErrorFree;
}

void CWelsDecoder::OutputStatisticsLog (SDecoderStatistics& sDecoderStatistics) {
  if ((sDecoderStatistics.uiDecodedFrameCount > 0) && (sDecoderStatistics.iStatisticsLogInterval > 0)
      && ((sDecoderStatistics.uiDecodedFrameCount % sDecoderStatistics.iStatisticsLogInterval) == 0)) {
    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO,
             "DecoderStatistics: uiWidth=%d, uiHeight=%d, fAverageFrameSpeedInMs=%.1f, fActualAverageFrameSpeedInMs=%.1f, \
              uiDecodedFrameCount=%d, uiResolutionChangeTimes=%d, uiIDRCorrectNum=%d, \
              uiAvgEcRatio=%d, uiAvgEcPropRatio=%d, uiEcIDRNum=%d, uiEcFrameNum=%d, \
              uiIDRLostNum=%d, uiFreezingIDRNum=%d, uiFreezingNonIDRNum=%d, iAvgLumaQp=%d, \
              iSpsReportErrorNum=%d, iSubSpsReportErrorNum=%d, iPpsReportErrorNum=%d, iSpsNoExistNalNum=%d, iSubSpsNoExistNalNum=%d, iPpsNoExistNalNum=%d, \
              uiProfile=%d, uiLevel=%d, \
              iCurrentActiveSpsId=%d, iCurrentActivePpsId=%d,",
             sDecoderStatistics.uiWidth,
             sDecoderStatistics.uiHeight,
             sDecoderStatistics.fAverageFrameSpeedInMs,
             sDecoderStatistics.fActualAverageFrameSpeedInMs,

             sDecoderStatistics.uiDecodedFrameCount,
             sDecoderStatistics.uiResolutionChangeTimes,
             sDecoderStatistics.uiIDRCorrectNum,

             sDecoderStatistics.uiAvgEcRatio,
             sDecoderStatistics.uiAvgEcPropRatio,
             sDecoderStatistics.uiEcIDRNum,
             sDecoderStatistics.uiEcFrameNum,

             sDecoderStatistics.uiIDRLostNum,
             sDecoderStatistics.uiFreezingIDRNum,
             sDecoderStatistics.uiFreezingNonIDRNum,
             sDecoderStatistics.iAvgLumaQp,

             sDecoderStatistics.iSpsReportErrorNum,
             sDecoderStatistics.iSubSpsReportErrorNum,
             sDecoderStatistics.iPpsReportErrorNum,
             sDecoderStatistics.iSpsNoExistNalNum,
             sDecoderStatistics.iSubSpsNoExistNalNum,
             sDecoderStatistics.iPpsNoExistNalNum,

             sDecoderStatistics.uiProfile,
             sDecoderStatistics.uiLevel,

             sDecoderStatistics.iCurrentActiveSpsId,
             sDecoderStatistics.iCurrentActivePpsId);
  }
}

/*!
 * \brief  Refreshes, from the active SPS, the three things the display layer needs
 *         to put pictures into output order: whether this stream reorders at all,
 *         how many frames its DPB holds, and what its VUI promises about reordering.
 */
void CWelsDecoder::UpdateReorderingParameters (PWelsDecoderContext pCtx) {
  const PSps kpSps = pCtx->pSps;
  m_bIsBaseline = kpSps->uiProfileIdc == 66 || kpSps->uiProfileIdc == 83;
  m_sReoderingStatus.bReorderPictures = NeedsPictureReordering (kpSps);
  m_sReoderingStatus.iDpbSize = GetDpbSize (kpSps);
  m_sReoderingStatus.iMaxNumReorderFrames = kpSps->sVui.bBitstreamRestrictionFlag
      ? (int32_t) kpSps->sVui.uiMaxNumReorderFrames : -1;
}

/*!
 * \brief  DPB fullness in frame buffers, C.4.1: |R| + |W \ R|, where R is the set
 *         of pictures the core holds as reference right now and W the set waiting
 *         here for output. A picture that is both is one frame buffer, not two.
 *
 * Marking for the current picture has already run when this is reached --
 * DecodeCurrentAccessUnit calls DecodeFrameConstruction and then WelsMarkAsRef
 * before returning to this layer -- so R already holds the current picture if it is
 * a reference, and W holds it because it has just been buffered. The count is
 * therefore one more than the fullness C.4.5.3 tests before storing the current
 * picture, which is why the caller's test is "> iDpbSize" and not ">=".
 */
int32_t CWelsDecoder::GetDpbFullness (PWelsDecoderContext pCtx, PPicBuff pPicBuff) {
  PPicture pRefs[2 * MAX_DPB_COUNT];
  int32_t iRefs = 0;
  if (pCtx != NULL) {
    for (int32_t iList = 0; iList < 2; ++iList) {
      PPicture* pList = (iList == 0) ? pCtx->sRefPic.pShortRefList[LIST_0] : pCtx->sRefPic.pLongRefList[LIST_0];
      const uint32_t kuiCount = (iList == 0) ? pCtx->sRefPic.uiShortRefCount[LIST_0] :
                                pCtx->sRefPic.uiLongRefCount[LIST_0];
      for (uint32_t i = 0; i < kuiCount && i < MAX_DPB_COUNT; ++i) {
        if (pList[i] == NULL)
          continue;
        bool bSeen = false;
        for (int32_t j = 0; j < iRefs; ++j) {
          if (pRefs[j] == pList[i]) {
            bSeen = true;
            break;
          }
        }
        if (!bSeen && iRefs < (int32_t) (sizeof (pRefs) / sizeof (pRefs[0])))
          pRefs[iRefs++] = pList[i];
      }
    }
  }
  int32_t iWaiting = 0;
  for (int32_t i = 0; i <= m_sReoderingStatus.iLargestBufferedPicIndex; ++i) {
    if (m_sPictInfoList[i].iPOC == IMinInt32)
      continue;
    PPicture pPic = NULL;
    const int32_t kiPicBuffIdx = m_sPictInfoList[i].iPicBuffIdx;
    if (pPicBuff != NULL && kiPicBuffIdx >= 0 && kiPicBuffIdx < pPicBuff->iCapacity)
      pPic = pPicBuff->ppPic[kiPicBuffIdx];
    bool bIsRef = false;
    if (pPic != NULL) {
      for (int32_t j = 0; j < iRefs; ++j) {
        if (pRefs[j] == pPic) {
          bIsRef = true;
          break;
        }
      }
    }
    if (!bIsRef)
      ++iWaiting;
  }
  return iRefs + iWaiting;
}

/*!
 * \brief  Moves the current picture into the first free slot of the picture list,
 *         and takes the DPB reference that keeps its planes alive until it is
 *         emitted. Leaves iBufferStatus alone -- i.e. the picture where it is -- if
 *         the list is full; see EmitOnFullPictInfoList.
 */
void CWelsDecoder::StoreReadyPicture (PWelsDecoderContext pCtx, SBufferInfo* pDstInfo) {
  for (int32_t i = 0; i < PICT_INFO_LIST_SIZE; ++i) {
    if (m_sPictInfoList[i].iPOC == IMinInt32) {
      memcpy (&m_sPictInfoList[i].sBufferInfo, pDstInfo, sizeof (SBufferInfo));
      //The decoded picture's own POC, not the slice header's: the two agree except
      //after an MMCO 5, where marking zeroes the picture's POC as 8.2.1 requires
      //and the slice header still carries what was coded.
      m_sPictInfoList[i].iPOC = pCtx->pLastDecPicInfo->pPreviousDecodedPictureInDpb != NULL
                               ? pCtx->pLastDecPicInfo->pPreviousDecodedPictureInDpb->iFramePoc
                               : pCtx->pSliceHeader->iPicOrderCntLsb;
      m_sPictInfoList[i].iSeqNum = m_sReoderingStatus.iOutputSeqNum;
      m_sPictInfoList[i].uiDecodingTimeStamp = pCtx->uiDecodingTimeStamp;
      if (pCtx->pLastDecPicInfo->pPreviousDecodedPictureInDpb != NULL) {
        m_sPictInfoList[i].iPicBuffIdx = pCtx->pLastDecPicInfo->pPreviousDecodedPictureInDpb->iPicBuffIdx;
        if (GetThreadCount (pCtx) <= 1) ++pCtx->pLastDecPicInfo->pPreviousDecodedPictureInDpb->iRefCount;
      }
      pDstInfo->iBufferStatus = 0;
      ++m_sReoderingStatus.iNumOfPicts;
      if (i > m_sReoderingStatus.iLargestBufferedPicIndex) {
        m_sReoderingStatus.iLargestBufferedPicIndex = i;
      }
      break;
    }
  }
}

/*!
 * \brief  The picture list is full: emit whichever of the current picture and the
 *         smallest waiting picture comes first in output order, so that the output
 *         order still holds and neither is dropped.
 *
 * PICT_INFO_LIST_SIZE is twice the largest DPB Table A-1 allows, and the backlog
 * this layer can build is bounded below that (see GetTargetRefListSize), so this is
 * a guarantee that a picture is never dropped rather than a path a conforming
 * stream reaches.
 */
void CWelsDecoder::EmitOnFullPictInfoList (PWelsDecoderContext pCtx, unsigned char** ppDst,
    SBufferInfo* pDstInfo) {
  const int32_t kiCurPoc = pCtx->pLastDecPicInfo->pPreviousDecodedPictureInDpb != NULL
                           ? pCtx->pLastDecPicInfo->pPreviousDecodedPictureInDpb->iFramePoc
                           : pCtx->pSliceHeader->iPicOrderCntLsb;
  int32_t iMinIdx = -1;
  for (int32_t i = 0; i <= m_sReoderingStatus.iLargestBufferedPicIndex; ++i) {
    if (m_sPictInfoList[i].iPOC == IMinInt32) continue;
    if (iMinIdx < 0
        || ((m_sPictInfoList[i].iSeqNum == m_sPictInfoList[iMinIdx].iSeqNum)
            ? (m_sPictInfoList[i].iPOC < m_sPictInfoList[iMinIdx].iPOC)
            : (m_sPictInfoList[i].iSeqNum - m_sPictInfoList[iMinIdx].iSeqNum < 0))) {
      iMinIdx = i;
    }
  }
  const bool kbCurrentIsFirst = (iMinIdx < 0)
                                || ((m_sPictInfoList[iMinIdx].iSeqNum == m_sReoderingStatus.iOutputSeqNum)
                                    ? (kiCurPoc <= m_sPictInfoList[iMinIdx].iPOC)
                                    : (m_sReoderingStatus.iOutputSeqNum - m_sPictInfoList[iMinIdx].iSeqNum < 0));
  if (kbCurrentIsFirst) {
    //Straight out, the way C.4.5.2 outputs a picture no waiting one precedes.
    ppDst[0] = pDstInfo->pDst[0];
    ppDst[1] = pDstInfo->pDst[1];
    ppDst[2] = pDstInfo->pDst[2];
    return;
  }
  //Force the smallest waiting picture out, which frees its slot, buffer the current
  //picture into it, and hand the freed picture to the caller.
  SBufferInfo sCurrent;
  memcpy (&sCurrent, pDstInfo, sizeof (SBufferInfo));
  ReleaseBufferedReadyPictureReorder (pCtx, ppDst, pDstInfo, true);
  SBufferInfo sEmitted;
  memcpy (&sEmitted, pDstInfo, sizeof (SBufferInfo));
  unsigned char* pEmitted[3] = { ppDst[0], ppDst[1], ppDst[2] };
  memcpy (pDstInfo, &sCurrent, sizeof (SBufferInfo));
  StoreReadyPicture (pCtx, pDstInfo);
  memcpy (pDstInfo, &sEmitted, sizeof (SBufferInfo));
  ppDst[0] = pEmitted[0];
  ppDst[1] = pEmitted[1];
  ppDst[2] = pEmitted[2];
}

void CWelsDecoder::BufferingReadyPicture (PWelsDecoderContext pCtx, unsigned char** ppDst,
    SBufferInfo* pDstInfo) {
  if (pDstInfo->iBufferStatus == 0) {
    return;
  }
  UpdateReorderingParameters (pCtx);
  //A coded video sequence ends at an IDR or an SPS change -- which is what the
  //core's own iSeqNum counts -- and at a memory_management_control_operation equal
  //to 5, which 8.2.1 makes the current picture's POC zero and C.4.4 makes a point
  //past which nothing earlier may still be waiting. Marking has already run, so
  //bLastHasMmco5 is this picture's flag.
  const bool kbNewSequence = (m_sReoderingStatus.iPrevCoreSeqNum != pCtx->iSeqNum)
                             || pCtx->pLastDecPicInfo->bLastHasMmco5;
  if (kbNewSequence) {
    ++m_sReoderingStatus.iOutputSeqNum;
    m_sReoderingStatus.iPrevCoreSeqNum = pCtx->iSeqNum;
    //C.4.4: an IDR that asks for it discards the pictures of the sequence before it
    //instead of outputting them. No stream in this tree's test material sets the
    //flag on anything but its very first picture, where there is nothing to discard,
    //so this arm is written to the specification and is not exercised here.
    if (pCtx->pSliceHeader != NULL && pCtx->pSliceHeader->bIdrFlag
        && pCtx->pSliceHeader->sRefMarking.bNoOutputOfPriorPicsFlag) {
      for (int32_t i = 0; i <= m_sReoderingStatus.iLargestBufferedPicIndex; ++i) {
        if (m_sPictInfoList[i].iPOC == IMinInt32)
          continue;
        m_sPictInfoList[i].iPOC = IMinInt32;
        const int32_t kiPicBuffIdx = m_sPictInfoList[i].iPicBuffIdx;
        if (pCtx->pPicBuff != NULL && kiPicBuffIdx >= 0 && kiPicBuffIdx < pCtx->pPicBuff->iCapacity) {
          PPicture pPic = pCtx->pPicBuff->ppPic[kiPicBuffIdx];
          --pPic->iRefCount;
          if (pPic->iRefCount <= 0 && pPic->pSetUnRef)
            pPic->pSetUnRef (pPic);
        }
        --m_sReoderingStatus.iNumOfPicts;
      }
    }
  }
  StoreReadyPicture (pCtx, pDstInfo);
}

/*!
 * \brief  The bumping process of C.4.5.3, one output per completed picture.
 *
 * Picks the buffered picture with the smallest (sequence, POC) -- the next one in
 * output order, since output order is POC order within a coded video sequence and
 * sequences follow one another in decoding order -- and emits it when the DPB says
 * nothing still to come can precede it:
 *
 *  - isFlush: end of stream, everything left goes out in order;
 *  - the picture is from an older sequence than the one being decoded. C.4.4 empties
 *    the DPB across a sequence boundary, so nothing that follows can precede it;
 *  - the DPB has no empty frame buffer (C.4.5.3). GetDpbFullness() counts one more
 *    than the fullness the specification tests, because the current picture is
 *    already stored here and already marked there, hence "> iDpbSize";
 *  - the VUI carries max_num_reorder_frames and more than that many pictures are
 *    waiting (E.2.1): the smallest of them cannot be preceded by a picture that has
 *    not been decoded yet. This is the term that keeps the latency of a stream with
 *    a VUI down to what its encoder promised.
 */
void CWelsDecoder::ReleaseBufferedReadyPictureReorder (PWelsDecoderContext pCtx, unsigned char** ppDst,
    SBufferInfo* pDstInfo, bool isFlush) {
  PPicBuff pPicBuff = pCtx ? pCtx->pPicBuff : m_pPicBuff;
  if (pCtx == NULL && m_iThreadCount <= 1) {
    pCtx = m_pDecThrCtx[0].pCtx;
  }
  //The reference lists this reads live in a decoding context, and the threaded
  //callers have none to pass: the last thread to finish a picture owns the lists
  //that describe the DPB now. With neither, R is empty and the fullness test falls
  //back to the number of pictures waiting, which is still an upper bound on when to
  //emit and never emits one early.
  PWelsDecoderContext pRefCtx = pCtx;
  if (pRefCtx == NULL && m_pLastDecThrCtx != NULL) {
    pRefCtx = m_pLastDecThrCtx->pCtx;
  }
  if (m_sReoderingStatus.iNumOfPicts > 0) {
    m_sReoderingStatus.iMinPOC = IMinInt32;
    int32_t firstValidIdx = -1;
    for (int32_t i = 0; i <= m_sReoderingStatus.iLargestBufferedPicIndex; ++i) {
      if (m_sReoderingStatus.iMinPOC == IMinInt32 && m_sPictInfoList[i].iPOC > IMinInt32) {
        m_sReoderingStatus.iMinPOC = m_sPictInfoList[i].iPOC;
        m_sReoderingStatus.iMinSeqNum = m_sPictInfoList[i].iSeqNum;
        m_sReoderingStatus.iPictInfoIndex = i;
        firstValidIdx = i;
        break;
      }
    }
    for (int32_t i = 0; i <= m_sReoderingStatus.iLargestBufferedPicIndex; ++i) {
      if (i == firstValidIdx) continue;
      if (m_sPictInfoList[i].iPOC > IMinInt32
          && ((m_sPictInfoList[i].iSeqNum == m_sReoderingStatus.iMinSeqNum) ? (m_sPictInfoList[i].iPOC < m_sReoderingStatus.iMinPOC) : (m_sPictInfoList[i].iSeqNum - m_sReoderingStatus.iMinSeqNum < 0))) {
        m_sReoderingStatus.iMinPOC = m_sPictInfoList[i].iPOC;
        m_sReoderingStatus.iMinSeqNum = m_sPictInfoList[i].iSeqNum;
        m_sReoderingStatus.iPictInfoIndex = i;
      }
    }
  }
  if (m_sReoderingStatus.iMinPOC > IMinInt32) {
    bool isReady = true;
    if (!isFlush) {
      isReady = (m_sReoderingStatus.iMinSeqNum - m_sReoderingStatus.iOutputSeqNum < 0)
                || (GetDpbFullness (pRefCtx, pPicBuff) > m_sReoderingStatus.iDpbSize)
                || (m_sReoderingStatus.iMaxNumReorderFrames >= 0
                    && m_sReoderingStatus.iNumOfPicts > m_sReoderingStatus.iMaxNumReorderFrames);
    }
    if (isReady) {
      m_sReoderingStatus.iLastWrittenPOC = m_sReoderingStatus.iMinPOC;
      m_sReoderingStatus.iLastWrittenSeqNum = m_sReoderingStatus.iMinSeqNum;
#if defined (_DEBUG)
#ifdef _MOTION_VECTOR_DUMP_
      fprintf (stderr, "Output POC: #%d uiDecodingTimeStamp=%d\n", m_sReoderingStatus.iLastWrittenPOC,
               m_sPictInfoList[m_sReoderingStatus.iPictInfoIndex].uiDecodingTimeStamp);
#endif
#endif
      memcpy (pDstInfo, &m_sPictInfoList[m_sReoderingStatus.iPictInfoIndex].sBufferInfo, sizeof (SBufferInfo));
      ppDst[0] = pDstInfo->pDst[0];
      ppDst[1] = pDstInfo->pDst[1];
      ppDst[2] = pDstInfo->pDst[2];
      m_sPictInfoList[m_sReoderingStatus.iPictInfoIndex].iPOC = IMinInt32;
      int32_t iPicBuffIdx = m_sPictInfoList[m_sReoderingStatus.iPictInfoIndex].iPicBuffIdx;
      if (pPicBuff != NULL) {
        if (iPicBuffIdx >= 0 && iPicBuffIdx < pPicBuff->iCapacity)
        {
            PPicture pPic = pPicBuff->ppPic[iPicBuffIdx];
            --pPic->iRefCount;
            if (pPic->iRefCount <= 0 && pPic->pSetUnRef)
              pPic->pSetUnRef(pPic);
        }
      }
      m_sReoderingStatus.iMinPOC = IMinInt32;
      --m_sReoderingStatus.iNumOfPicts;
    }
  }
}

DECODING_STATE CWelsDecoder::ReorderPicturesInDisplay(PWelsDecoderContext pDecContext, unsigned char** ppDst,
  SBufferInfo* pDstInfo) {
  DECODING_STATE iRet = dsErrorFree;
  if (pDecContext->pSps != NULL) {
    UpdateReorderingParameters (pDecContext);
    //A stream that cannot reorder is handed its picture the moment it is decoded,
    //which is what this layer always did for baseline. Everything else goes into
    //the buffer and comes out by the bumping process.
    if (m_sReoderingStatus.bReorderPictures && pDstInfo->iBufferStatus == 1) {
      BufferingReadyPicture (pDecContext, ppDst, pDstInfo);
      if (pDstInfo->iBufferStatus == 1) {
        //The picture list was full, so the picture is still here. It is sized so
        //that this cannot happen; the valve is what makes that a guarantee.
        EmitOnFullPictInfoList (pDecContext, ppDst, pDstInfo);
      } else {
        ReleaseBufferedReadyPictureReorder (pDecContext, ppDst, pDstInfo);
      }
    }
  }
  return iRet;
}

DECODING_STATE CWelsDecoder::DecodeParser (const unsigned char* kpSrc, const int kiSrcLen, SParserBsInfo* pDstInfo) {
  PWelsDecoderContext pDecContext = m_pDecThrCtx[0].pCtx;

  if (pDecContext == NULL || pDecContext->pParam == NULL) {
    if (m_pWelsTrace != NULL) {
      WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_ERROR, "Call DecodeParser without Initialize.\n");
    }
    return dsInitialOptExpected;
  }

  if (!pDecContext->pParam->bParseOnly) {
    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_ERROR, "bParseOnly should be true for this API calling! \n");
    pDecContext->iErrorCode |= dsInvalidArgument;
    return dsInvalidArgument;
  }
  int64_t iEnd, iStart = WelsTime();
  if (CheckBsBuffer (pDecContext, kiSrcLen)) {
    if (ResetDecoder (pDecContext))
      return dsOutOfMemory;

    return dsErrorFree;
  }
  if (kiSrcLen > 0 && kpSrc != NULL) {
#ifdef OUTPUT_BITSTREAM
    if (m_pFBS) {
      WelsFwrite (kpSrc, sizeof (unsigned char), kiSrcLen, m_pFBS);
      WelsFflush (m_pFBS);
    }
#endif//OUTPUT_BIT_STREAM
    pDecContext->bEndOfStreamFlag = false;
  } else {
    //For application MODE, the error detection should be added for safe.
    //But for CONSOLE MODE, when decoding LAST AU, kiSrcLen==0 && kpSrc==NULL.
    pDecContext->bEndOfStreamFlag = true;
    pDecContext->bInstantDecFlag = true;
  }

  pDecContext->iErrorCode = dsErrorFree; //initialize at the starting of AU decoding.
  pDecContext->pParam->eEcActiveIdc = ERROR_CON_DISABLE; //add protection to disable EC here.
  pDecContext->iFeedbackNalRefIdc = -1; //initialize
  if (!pDecContext->bFramePending) { //frame complete
    pDecContext->pParserBsInfo->iNalNum = 0;
    memset (pDecContext->pParserBsInfo->pNalLenInByte, 0, MAX_NAL_UNITS_IN_LAYER);
  }
  pDstInfo->iNalNum = 0;
  pDstInfo->iSpsWidthInPixel = pDstInfo->iSpsHeightInPixel = 0;
  if (pDstInfo) {
    pDecContext->uiTimeStamp = pDstInfo->uiInBsTimeStamp;
    pDstInfo->uiOutBsTimeStamp = 0;
  } else {
    pDecContext->uiTimeStamp = 0;
  }
  WelsDecodeBs (pDecContext, kpSrc, kiSrcLen, NULL, NULL, pDstInfo);
  if (pDecContext->iErrorCode & dsOutOfMemory) {
    if (ResetDecoder (pDecContext))
      return dsOutOfMemory;
    return dsErrorFree;
  }

  if (!pDecContext->bFramePending && pDecContext->pParserBsInfo->iNalNum) {
    memcpy (pDstInfo, pDecContext->pParserBsInfo, sizeof (SParserBsInfo));

    if (pDecContext->iErrorCode == ERR_NONE) { //update statistics: decoding frame count
      pDecContext->pDecoderStatistics->uiDecodedFrameCount++;
      if (pDecContext->pDecoderStatistics->uiDecodedFrameCount == 0) { //exceed max value of uint32_t
        ResetDecStatNums (pDecContext->pDecoderStatistics);
        pDecContext->pDecoderStatistics->uiDecodedFrameCount++;
      }
    }
  }

  pDecContext->bInstantDecFlag = false; //reset no-delay flag

  if (pDecContext->iErrorCode && pDecContext->bPrintFrameErrorTraceFlag) {
    WelsLog (&m_pWelsTrace->m_sLogCtx, WELS_LOG_INFO, "decode failed, failure type:%d \n", pDecContext->iErrorCode);
    pDecContext->bPrintFrameErrorTraceFlag = false;
  }
  iEnd = WelsTime();
  pDecContext->dDecTime += (iEnd - iStart) / 1e3;
  return (DECODING_STATE)pDecContext->iErrorCode;
}

DECODING_STATE CWelsDecoder::DecodeFrame (const unsigned char* kpSrc,
    const int kiSrcLen,
    unsigned char** ppDst,
    int* pStride,
    int& iWidth,
    int& iHeight) {
  DECODING_STATE eDecState = dsErrorFree;
  SBufferInfo    DstInfo;

  memset (&DstInfo, 0, sizeof (SBufferInfo));
  DstInfo.UsrData.sSystemBuffer.iStride[0] = pStride[0];
  DstInfo.UsrData.sSystemBuffer.iStride[1] = pStride[1];
  DstInfo.UsrData.sSystemBuffer.iWidth = iWidth;
  DstInfo.UsrData.sSystemBuffer.iHeight = iHeight;

  eDecState = DecodeFrame2 (kpSrc, kiSrcLen, ppDst, &DstInfo);
  if (eDecState == dsErrorFree) {
    pStride[0] = DstInfo.UsrData.sSystemBuffer.iStride[0];
    pStride[1] = DstInfo.UsrData.sSystemBuffer.iStride[1];
    iWidth     = DstInfo.UsrData.sSystemBuffer.iWidth;
    iHeight    = DstInfo.UsrData.sSystemBuffer.iHeight;
  }

  return eDecState;
}

DECODING_STATE CWelsDecoder::DecodeFrameEx (const unsigned char* kpSrc,
    const int kiSrcLen,
    unsigned char* pDst,
    int iDstStride,
    int& iDstLen,
    int& iWidth,
    int& iHeight,
    int& iColorFormat) {
  DECODING_STATE state = dsErrorFree;

  return state;
}

DECODING_STATE CWelsDecoder::ParseAccessUnit (SWelsDecoderThreadCTX& sThreadCtx) {
  sThreadCtx.pCtx->bHasNewSps = false;
  sThreadCtx.pCtx->bParamSetsLostFlag = m_bParamSetsLostFlag;
  sThreadCtx.pCtx->bFreezeOutput = m_bFreezeOutput;
  sThreadCtx.pCtx->uiDecodingTimeStamp = ++m_uiDecodeTimeStamp;
  bool bPicBuffChanged = false;
  if (m_pLastDecThrCtx != NULL && sThreadCtx.pCtx->sSpsPpsCtx.iSeqId < m_pLastDecThrCtx->pCtx->sSpsPpsCtx.iSeqId) {
    CopySpsPps (m_pLastDecThrCtx->pCtx, sThreadCtx.pCtx);
    sThreadCtx.pCtx->iPicQueueNumber = m_pLastDecThrCtx->pCtx->iPicQueueNumber;
    if (sThreadCtx.pCtx->pPicBuff != m_pPicBuff) {
      bPicBuffChanged = true;
      sThreadCtx.pCtx->pPicBuff = m_pPicBuff;
      sThreadCtx.pCtx->bHaveGotMemory = m_pPicBuff != NULL;
      sThreadCtx.pCtx->iImgWidthInPixel = m_pLastDecThrCtx->pCtx->iImgWidthInPixel;
      sThreadCtx.pCtx->iImgHeightInPixel = m_pLastDecThrCtx->pCtx->iImgHeightInPixel;
    }
  }

  //if threadCount > 1, then each thread must contain exact one complete frame.
  if (GetThreadCount (sThreadCtx.pCtx) > 1) {
    sThreadCtx.pCtx->pAccessUnitList->uiAvailUnitsNum = 0;
    sThreadCtx.pCtx->pAccessUnitList->uiActualUnitsNum = 0;
  }

  int32_t iRet = DecodeFrame2WithCtx (sThreadCtx.pCtx, sThreadCtx.kpSrc, sThreadCtx.kiSrcLen, sThreadCtx.ppDst,
                                      &sThreadCtx.sDstInfo);

  int32_t iErr = InitConstructAccessUnit (sThreadCtx.pCtx, &sThreadCtx.sDstInfo);
  if (ERR_NONE != iErr) {
    return (DECODING_STATE) (iRet | iErr);
  }
  if (sThreadCtx.pCtx->bNewSeqBegin) {
    m_pPicBuff = sThreadCtx.pCtx->pPicBuff;
  } else if (bPicBuffChanged) {
    InitialDqLayersContext (sThreadCtx.pCtx, sThreadCtx.pCtx->pSps->iMbWidth << 4, sThreadCtx.pCtx->pSps->iMbHeight << 4);
  }
  if (!sThreadCtx.pCtx->bNewSeqBegin && m_pLastDecThrCtx != NULL) {
    sThreadCtx.pCtx->sFrameCrop = m_pLastDecThrCtx->pCtx->pSps->sFrameCrop;
  }
  m_bParamSetsLostFlag = sThreadCtx.pCtx->bNewSeqBegin ? false : sThreadCtx.pCtx->bParamSetsLostFlag;
  m_bFreezeOutput = sThreadCtx.pCtx->bNewSeqBegin ? false : sThreadCtx.pCtx->bFreezeOutput;
  return (DECODING_STATE)iErr;
}
/*
* Run decoding picture in separate thread.
*/

int CWelsDecoder::ThreadDecodeFrameInternal (const unsigned char* kpSrc, const int kiSrcLen, unsigned char** ppDst,
    SBufferInfo* pDstInfo) {
  int state = dsErrorFree;
  int32_t i, j;
  int32_t signal = 0;

  //serial using of threads
  if (m_DecCtxActiveCount < m_iThreadCount) {
    signal = m_DecCtxActiveCount;
  } else {
    signal = m_pDecThrCtxActive[0]->sThreadInfo.uiThrNum;
  }

  WAIT_SEMAPHORE (&m_pDecThrCtx[signal].sThreadInfo.sIsIdle, WELS_DEC_THREAD_WAIT_INFINITE);

  for (i = 0; i < m_DecCtxActiveCount; ++i) {
    if (m_pDecThrCtxActive[i] == &m_pDecThrCtx[signal]) {
      m_pDecThrCtxActive[i] = NULL;
      for (j = i; j < m_DecCtxActiveCount - 1; j++) {
        m_pDecThrCtxActive[j] = m_pDecThrCtxActive[j + 1];
        m_pDecThrCtxActive[j + 1] = NULL;
      }
      --m_DecCtxActiveCount;
      break;
    }
  }

  m_pDecThrCtxActive[m_DecCtxActiveCount++] = &m_pDecThrCtx[signal];
  if (m_pLastDecThrCtx != NULL) {
    m_pDecThrCtx[signal].pCtx->pLastThreadCtx = m_pLastDecThrCtx;
  }
  m_pDecThrCtx[signal].kpSrc = const_cast<uint8_t*> (kpSrc);
  m_pDecThrCtx[signal].kiSrcLen = kiSrcLen;
  m_pDecThrCtx[signal].ppDst = ppDst;
  memcpy (&m_pDecThrCtx[signal].sDstInfo, pDstInfo, sizeof (SBufferInfo));

  ParseAccessUnit (m_pDecThrCtx[signal]);
  if (m_iThreadCount > 1) {
    m_pLastDecThrCtx = &m_pDecThrCtx[signal];
  }
  m_pDecThrCtx[signal].sThreadInfo.uiCommand = WELS_DEC_THREAD_COMMAND_RUN;
  RELEASE_SEMAPHORE (&m_pDecThrCtx[signal].sThreadInfo.sIsActivated);

  // wait early picture
  if (m_DecCtxActiveCount >= m_iThreadCount) {
    WAIT_SEMAPHORE (&m_pDecThrCtxActive[0]->sThreadInfo.sIsIdle, WELS_DEC_THREAD_WAIT_INFINITE);
    RELEASE_SEMAPHORE (&m_pDecThrCtxActive[0]->sThreadInfo.sIsIdle);
  }
  return state;
}

} // namespace WelsDec


using namespace WelsDec;
/*
*       WelsGetDecoderCapability
*       @return: DecCapability information
*/
int WelsGetDecoderCapability (SDecoderCapability* pDecCapability) {
  memset (pDecCapability, 0, sizeof (SDecoderCapability));
  pDecCapability->iProfileIdc = 66; //Baseline
  pDecCapability->iProfileIop = 0xE0; //11100000b
  pDecCapability->iLevelIdc = 32; //level_idc = 3.2
  pDecCapability->iMaxMbps = 216000; //from level_idc = 3.2
  pDecCapability->iMaxFs = 5120; //from level_idc = 3.2
  pDecCapability->iMaxCpb = 20000; //from level_idc = 3.2
  pDecCapability->iMaxDpb = 20480; //from level_idc = 3.2
  pDecCapability->iMaxBr = 20000; //from level_idc = 3.2
  pDecCapability->bRedPicCap = 0; //not support redundant pic

  return ERR_NONE;
}
/* WINAPI is indeed in prefix due to sync to application layer callings!! */

/*
*   WelsCreateDecoder
*   @return:    success in return 0, otherwise failed.
*/
long WelsCreateDecoder (ISVCDecoder** ppDecoder) {

  if (NULL == ppDecoder) {
    return ERR_INVALID_PARAMETERS;
  }

  *ppDecoder = new CWelsDecoder();

  if (NULL == *ppDecoder) {
    return ERR_MALLOC_FAILED;
  }

  return ERR_NONE;
}

/*
*   WelsDestroyDecoder
*/
void WelsDestroyDecoder (ISVCDecoder* pDecoder) {
  if (NULL != pDecoder) {
    delete (CWelsDecoder*)pDecoder;
  }
}
