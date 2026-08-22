// ecref — the C++ decoder's answer for one malformed-corpus entry.
//
// Phase 5 session S, F43. The port's malformed-stream parity table
// (`tests/data/malformed_parity/*.txt`) pins the *port's* behaviour; it is not a
// live comparison against the C++. When F43's fix made error concealment run for
// the first time, 22 of its rows moved, and "the port changed" is not evidence
// either way — the question output equivalence actually asks is **what does the
// C++ decoder do on the same bytes**.
//
// This is that reference. It replicates `malformed_stream_parity.rs`'s
// `decode_case` exactly — same annex-B split, same `ERROR_CON_SLICE_COPY`, same
// per-NAL feed, same EOS + drain, same plane hashing in emission order — against
// `libopenh264.dylib`, and prints the row in the same shape the table stores.
//
//   build: rust/tools/ecref/build.sh
//   usage: ecref <stream.264> <truncate-to-bytes>   file, truncated, annex-B fed
//          ecref --stdin [--raw]                    bytes on stdin
//          ... [--nodelay]                          feed via DecodeFrameNoDelay
//   out:   <frames> <WxH|-> <sha1|-> <calls> <bufstatus>
//
// **The stdin form is what gives every corpus row a referee** (Phase 5 session
// U, T5.U2). A prefix truncation is expressible as `<file> <length>`, so the
// `trunc.*` rows always had one; the other families — header corruption,
// synthetic tails, and the degenerate NALs — *build* their bytes inside the Rust
// harness, and a positional interface cannot name them. 389 of the corpus's 2707
// rows were therefore pinned against the port's own previous output, which is
// precisely the blindness F43–F46 lived behind. With the harness dumping its
// corpus (`MALFORMED_DUMP_DIR`) and this reading a blob, the referee is the same
// for every row and the two families stop being different kinds of evidence.
//
// `--raw` selects the single-`DecodeFrame2`-call feed that `Feed::Raw` uses (the
// degenerate table's `raw.` rows), where start-code detection is the decoder's
// job rather than the harness's. Without it the blob is annex-B split first,
// which is every other row.
//
// Keep it: the same question arrives whenever a damaged-input path goes live.
#include <cstdio>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "codec_api.h"
#include "codec_app_def.h"

// SHA-1, the annex-B split and the plane hashing — shared with the external-ABI
// harness since T8.C5, because two copies of a referee can drift apart while both
// keep passing.
#include "../wels_ref_common.h"

int main(int argc, char** argv) {
  std::vector<uint8_t> data;
  bool raw_feed = false;
  bool want_frames = false;
  for (int i = 1; i < argc; i++) if (strcmp(argv[i], "--frames") == 0) want_frames = true;
  // `--nodelay` feeds each unit through `DecodeFrameNoDelay` instead of
  // `DecodeFrame2` (T8.C8, F82). The two are different entry points with different
  // emission timing — the reference's `DecodeFrameNoDelay` is `DecodeFrame2` twice —
  // so a port that implements one in terms of the other needs a referee for the
  // other, and this is it.
  bool nodelay = false;
  for (int i = 1; i < argc; i++) if (strcmp(argv[i], "--nodelay") == 0) nodelay = true;
  // `--options` prints every get-able scalar `DECODER_OPTION` after every decode
  // call — the referee for `GetOption` (Phase 8b session A, T8b.A3). Before it the
  // decoder had *no* instrument that read an option value back: the corpus reads
  // frames and codes, the conformance suite reads bytes, and `GetOption` returned
  // `cmResultSuccess` with nothing written for twelve of its sixteen ids while every
  // gate stayed green. Both the return code and the value are printed, because half
  // the reference's arms answer `cmInitExpected` rather than writing.
  bool want_options = false;
  for (int i = 1; i < argc; i++) if (strcmp(argv[i], "--options") == 0) want_options = true;
  // `--sps` prints the activated SPS's dimensions and `num_ref_frames` per call
  // (F80: is `WelsRequestMem`'s third arm reachable at all?).
  bool want_sps = false;
  for (int i = 1; i < argc; i++) if (strcmp(argv[i], "--sps") == 0) want_sps = true;
  // `--parse-only` drives `ISVCDecoder::DecodeParser` instead of `DecodeFrame2`
  // (Phase 8b session B, T8b.B2). It is a different entry point with a different
  // output — an annex-B bitstream, not planes — and it had no referee at all: the
  // port's slot was a stub that returned `dsErrorFree` and wrote nothing, which is
  // exactly what an unrefereed entry point looks like from every gate this project
  // owns. Prints one row per call, and a per-frame SHA-1 over the composed bytes.
  bool parse_only = false;
  for (int i = 1; i < argc; i++) if (strcmp(argv[i], "--parse-only") == 0) parse_only = true;

  if (argc >= 2 && strcmp(argv[1], "--stdin") == 0) {
    for (int i = 2; i < argc; i++) {
      if (strcmp(argv[i], "--raw") == 0) raw_feed = true;
      else if (strcmp(argv[i], "--annexb") == 0) raw_feed = false;
      else if (strcmp(argv[i], "--frames") == 0) { /* handled above */ }
      else if (strcmp(argv[i], "--nodelay") == 0) { /* handled above */ }
      else if (strcmp(argv[i], "--options") == 0) { /* handled above */ }
      else if (strcmp(argv[i], "--sps") == 0) { /* handled above */ }
      else if (strcmp(argv[i], "--parse-only") == 0) { /* handled above */ }
      else { fprintf(stderr, "unknown flag %s\n", argv[i]); return 2; }
    }
    // Read to EOF. An empty blob is a legal corpus entry (`raw.empty`), so a
    // zero-length read is a result and not an error.
    uint8_t chunk[65536];
    size_t n;
    while ((n = fread(chunk, 1, sizeof(chunk), stdin)) > 0)
      data.insert(data.end(), chunk, chunk + n);
    if (ferror(stdin)) { fprintf(stderr, "stdin read error\n"); return 2; }
  } else if (argc >= 3) {
    FILE* f = fopen(argv[1], "rb");
    if (!f) { fprintf(stderr, "cannot open %s\n", argv[1]); return 2; }
    fseek(f, 0, SEEK_END);
    long total = ftell(f);
    fseek(f, 0, SEEK_SET);
    data.assign(size_t(total), 0);
    if (total > 0 && fread(data.data(), 1, size_t(total), f) != size_t(total)) {
      fprintf(stderr, "short read\n"); fclose(f); return 2;
    }
    fclose(f);
    long want = atol(argv[2]);
    if (want >= 0 && want < total) data.resize(size_t(want));
  } else {
    fprintf(stderr, "usage: ecref <stream> <truncate-to-bytes>\n");
    fprintf(stderr, "       ecref --stdin [--raw|--annexb]\n");
    return 2;
  }

  // --- `--sps`: every SPS in the stream, by its own syntax (F80, T8b.A6) -----
  //
  // The question F80 asks is whether `WelsRequestMem`'s third arm — same picture
  // size, *changed* `num_ref_frames` — is reachable at all, and no `GetOption`
  // reports `num_ref_frames`. So the bits are read here rather than asked for.
  // Annex E's SPS syntax, up to `max_num_ref_frames`; scaling lists are skipped
  // properly because `test_scalinglist_jm.264` has them and a wrong skip would
  // silently misreport every field after.
  //
  // Subset SPS (NAL 15) is not read: it carries the SVC extension and this port's
  // corpus reaches the third arm, if at all, through the base layer.
  if (want_sps) {
    auto ue = [](const std::vector<uint8_t>& b, size_t& bit) -> uint32_t {
      int lead = 0;
      while (bit < b.size() * 8) {
        int v = (b[bit >> 3] >> (7 - (bit & 7))) & 1;
        bit++;
        if (v) break;
        lead++;
        if (lead > 31) return 0;
      }
      uint32_t val = 0;
      for (int i = 0; i < lead; i++) {
        if (bit >= b.size() * 8) return 0;
        val = (val << 1) | ((b[bit >> 3] >> (7 - (bit & 7))) & 1);
        bit++;
      }
      return (1u << lead) - 1 + val;
    };
    auto u1 = [](const std::vector<uint8_t>& b, size_t& bit) -> int {
      if (bit >= b.size() * 8) return 0;
      int v = (b[bit >> 3] >> (7 - (bit & 7))) & 1;
      bit++;
      return v;
    };
    auto se = [&](const std::vector<uint8_t>& b, size_t& bit) -> int32_t {
      uint32_t k = ue(b, bit);
      return (k & 1) ? int32_t((k + 1) >> 1) : -int32_t(k >> 1);
    };
    int nsps = 0;
    for (auto& u : split_annexb(data)) {
      const uint8_t* nal = data.data() + u.first;
      size_t len = u.second - u.first;
      // Skip the start code, then the NAL header byte.
      size_t off = 0;
      while (off + 2 < len && !(nal[off] == 0 && nal[off + 1] == 0 && nal[off + 2] == 1)) off++;
      if (off + 3 >= len) continue;
      off += 3;
      if ((nal[off] & 0x1f) != 7) continue;          // SPS only
      // De-emulate.
      std::vector<uint8_t> rbsp;
      int zeros = 0;
      for (size_t i = off + 1; i < len; i++) {
        if (zeros == 2 && nal[i] == 3) { zeros = 0; continue; }
        rbsp.push_back(nal[i]);
        zeros = (nal[i] == 0) ? zeros + 1 : 0;
      }
      if (rbsp.size() < 4) continue;
      size_t bit = 0;
      uint32_t profile_idc = 0;
      for (int i = 0; i < 8; i++) profile_idc = (profile_idc << 1) | u1(rbsp, bit);
      bit += 8;                                       // constraint flags + reserved
      uint32_t level_idc = 0;
      for (int i = 0; i < 8; i++) level_idc = (level_idc << 1) | u1(rbsp, bit);
      uint32_t sps_id = ue(rbsp, bit);
      uint32_t chroma_format_idc = 1;
      if (profile_idc == 100 || profile_idc == 110 || profile_idc == 122 || profile_idc == 244 ||
          profile_idc == 44  || profile_idc == 83  || profile_idc == 86  || profile_idc == 118 ||
          profile_idc == 128 || profile_idc == 138 || profile_idc == 139 || profile_idc == 134 ||
          profile_idc == 135) {
        chroma_format_idc = ue(rbsp, bit);
        if (chroma_format_idc == 3) u1(rbsp, bit);    // separate_colour_plane_flag
        ue(rbsp, bit);                                // bit_depth_luma_minus8
        ue(rbsp, bit);                                // bit_depth_chroma_minus8
        u1(rbsp, bit);                                // qpprime_y_zero_transform_bypass
        if (u1(rbsp, bit)) {                          // seq_scaling_matrix_present
          int lists = (chroma_format_idc != 3) ? 8 : 12;
          for (int i = 0; i < lists; i++) {
            if (!u1(rbsp, bit)) continue;
            int size = (i < 6) ? 16 : 64;
            int last = 8, next = 8;
            for (int j = 0; j < size; j++) {
              if (next != 0) { int d = se(rbsp, bit); next = (last + d + 256) % 256; }
              last = (next == 0) ? last : next;
            }
          }
        }
      }
      ue(rbsp, bit);                                  // log2_max_frame_num_minus4
      uint32_t poc_type = ue(rbsp, bit);
      if (poc_type == 0) {
        ue(rbsp, bit);                                // log2_max_poc_lsb_minus4
      } else if (poc_type == 1) {
        u1(rbsp, bit);                                // delta_pic_order_always_zero
        se(rbsp, bit);                                // offset_for_non_ref_pic
        se(rbsp, bit);                                // offset_for_top_to_bottom_field
        uint32_t n = ue(rbsp, bit);
        for (uint32_t i = 0; i < n && i < 256; i++) se(rbsp, bit);
      }
      uint32_t num_ref_frames = ue(rbsp, bit);
      u1(rbsp, bit);                                  // gaps_in_frame_num_allowed
      uint32_t w_mbs = ue(rbsp, bit) + 1;
      uint32_t h_map = ue(rbsp, bit) + 1;
      int frame_mbs_only = u1(rbsp, bit);
      printf("SPS %d id=%u profile=%u level=%u %ux%u num_ref_frames=%u\n",
             nsps++, sps_id, profile_idc, level_idc,
             w_mbs * 16, h_map * 16 * (frame_mbs_only ? 1 : 2), num_ref_frames);
    }
    if (nsps == 0) printf("SPS none\n");
    return 0;
  }

  // --- `--parse-only`: the `DecodeParser` referee (T8b.B2) -------------------
  //
  // One annex-B NAL per call, the way the corpus harness feeds `DecodeFrame2`, then
  // the trailing `(NULL, 0)` that `welsDecoderExt.cpp:1210` treats as end of stream.
  // The access unit closes when the parser sees the first NAL of the next one, so
  // output appears on the call *after* a frame's last slice — which is a fact about
  // the reference and therefore part of what the goldens pin.
  //
  // `uiInBsTimeStamp` is the call index plus one rather than zero, so the timestamp
  // plumbing is visible in the rows: `uiOutBsTimeStamp` should come back as the
  // timestamp of the *first NAL of the emitted access unit*, and `uiInBsTimeStamp`
  // comes back **zero** on every emitting call, because the reference's copy-out is
  // one `memcpy` of a struct whose input timestamp nothing ever writes.
  if (parse_only) {
    ISVCDecoder* pdec = nullptr;
    if (WelsCreateDecoder(&pdec) != 0 || !pdec) { fprintf(stderr, "WelsCreateDecoder\n"); return 2; }
    SDecodingParam pp;
    memset(&pp, 0, sizeof(pp));
    pp.uiTargetDqLayer = UCHAR_MAX;
    pp.eEcActiveIdc = ERROR_CON_SLICE_COPY;
    pp.bParseOnly = true;
    pp.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
    if (pdec->Initialize(&pp) != 0) { fprintf(stderr, "Initialize\n"); return 2; }

    SParserBsInfo bs;
    memset(&bs, 0, sizeof(bs));
    Sha1 all;
    int call = 0, emitted = 0;
    auto one = [&](const uint8_t* buf, int len) {
      bs.uiInBsTimeStamp = uint64_t(call) + 1;
      int rv = int(pdec->DecodeParser(buf, len, &bs));
      printf("PARSE %d rv=0x%x nal=%d lens=[", call, rv, bs.iNalNum);
      int total = 0;
      for (int i = 0; i < bs.iNalNum; i++) {
        printf("%s%d", i ? "," : "", bs.pNalLenInByte[i]);
        total += bs.pNalLenInByte[i];
      }
      Sha1 one_sha;
      if (bs.iNalNum > 0 && bs.pDstBuff) {
        one_sha.update(bs.pDstBuff, size_t(total));
        all.update(bs.pDstBuff, size_t(total));
        emitted++;
      }
      printf("] sps=%dx%d in=%llu out=%llu sha1=%s\n",
             bs.iSpsWidthInPixel, bs.iSpsHeightInPixel,
             (unsigned long long) bs.uiInBsTimeStamp,
             (unsigned long long) bs.uiOutBsTimeStamp,
             bs.iNalNum > 0 ? one_sha.digest().c_str() : "-");
      call++;
    };
    if (raw_feed) {
      one(data.empty() ? nullptr : data.data(), int(data.size()));
    } else {
      for (auto& u : split_annexb(data)) one(data.data() + u.first, int(u.second - u.first));
    }
    one(nullptr, 0);
    printf("PARSEONLY %d %d %s\n", call, emitted, emitted ? all.digest().c_str() : "-");
    pdec->Uninitialize();
    WelsDestroyDecoder(pdec);
    return 0;
  }

  ISVCDecoder* dec = nullptr;
  if (WelsCreateDecoder(&dec) != 0 || !dec) { fprintf(stderr, "WelsCreateDecoder\n"); return 2; }

  SDecodingParam p;
  memset(&p, 0, sizeof(p));
  p.uiTargetDqLayer = UCHAR_MAX;
  p.eEcActiveIdc = ERROR_CON_SLICE_COPY;
  p.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
  if (dec->Initialize(&p) != 0) { fprintf(stderr, "Initialize\n"); return 2; }

  // The eleven get-able scalar options, in `codec_app_def.h` order. `GET_STATISTICS`
  // and `GET_SAR_INFO` are struct-valued and are not here; `NUM_OF_THREADS` is the
  // object's field and does not change per call.
  struct OptId { const char* name; DECODER_OPTION id; };
  static const OptId kOpts[] = {
    {"EOS",   DECODER_OPTION_END_OF_STREAM},
    {"VCL",   DECODER_OPTION_VCL_NAL},
    {"TID",   DECODER_OPTION_TEMPORAL_ID},
    {"FN",    DECODER_OPTION_FRAME_NUM},
    {"IDR",   DECODER_OPTION_IDR_PIC_ID},
    {"LTRF",  DECODER_OPTION_LTR_MARKING_FLAG},
    {"LTRN",  DECODER_OPTION_LTR_MARKED_FRAME_NUM},
    {"EC",    DECODER_OPTION_ERROR_CON_IDC},
    {"PROF",  DECODER_OPTION_PROFILE},
    {"LEVEL", DECODER_OPTION_LEVEL},
    {"REF",   DECODER_OPTION_IS_REF_PIC},
    {"REM",   DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER},
  };
  int opt_call = 0;
  auto dump_options = [&](const char* what) {
    if (!want_options) return;
    printf("OPT %d %s", opt_call++, what);
    for (size_t i = 0; i < sizeof(kOpts) / sizeof(kOpts[0]); i++) {
      // Deliberately *not* initialized: the caller's garbage is exactly what a
      // silent no-write arm hands back, and pinning that would pin noise. A
      // sentinel makes "did not write" visible and reproducible instead.
      int v = 0x5EED5EED;
      long rc = dec->GetOption(kOpts[i].id, &v);
      printf(" %s=%ld/%d", kOpts[i].name, rc, v);
    }
    printf("\n");
  };

  Sha1 sha;
  int frames = 0, w0 = 0, h0 = 0;
  std::string calls, bufs;
  std::vector<std::string> frame_hashes;   // --frames: one digest per emitted frame

  auto feed = [&](const uint8_t* buf, int len) {
    uint8_t* dst[3] = {nullptr, nullptr, nullptr};
    SBufferInfo info;
    memset(&info, 0, sizeof(info));
    int ret = nodelay ? int(dec->DecodeFrameNoDelay(buf, len, dst, &info))
                      : int(dec->DecodeFrame2(buf, len, dst, &info));
    char tmp[32];
    sprintf(tmp, "%s0x%x", calls.empty() ? "" : ",", ret);
    calls += tmp;
    sprintf(tmp, "%s%d", bufs.empty() ? "" : ",", info.iBufferStatus);
    bufs += tmp;
    dump_options("decode");
    if (info.iBufferStatus != 1) return;
    int w = info.UsrData.sSystemBuffer.iWidth, h = info.UsrData.sSystemBuffer.iHeight;
    int sy = info.UsrData.sSystemBuffer.iStride[0], suv = info.UsrData.sSystemBuffer.iStride[1];
    frames++;
    if (!w0) { w0 = w; h0 = h; }
    if (want_frames) frame_hashes.push_back(frame_digest(dst, w, h, sy, suv));
    hash_plane(sha, dst[0], w, h, sy);
    hash_plane(sha, dst[1], w / 2, h / 2, suv);
    hash_plane(sha, dst[2], w / 2, h / 2, suv);
  };

  // `Feed::Raw` hands the decoder the whole blob in one call; `Feed::AnnexB`
  // splits first. `feed`'s null-for-empty matches the harness's `decode_case`,
  // where an empty unit passes a null `src` rather than a dangling one.
  if (raw_feed) {
    feed(data.empty() ? nullptr : data.data(), int(data.size()));
  } else {
    for (auto& u : split_annexb(data)) feed(data.data() + u.first, int(u.second - u.first));
  }

  int eos = 1;
  dec->SetOption(DECODER_OPTION_END_OF_STREAM, &eos);
  feed(nullptr, 0);

  int remaining = 0;
  dec->GetOption(DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER, &remaining);
  int drain = remaining;          // MAX_DRAIN in malformed_stream_parity.rs
  if (drain > 24) drain = 24;
  if (drain < 0) drain = 0;
  for (int i = 0; i < drain; i++) {
    uint8_t* dst[3] = {nullptr, nullptr, nullptr};
    SBufferInfo info;
    memset(&info, 0, sizeof(info));
    int ret = int(dec->FlushFrame(dst, &info));
    char tmp[32];
    sprintf(tmp, ",0x%x", ret);
    calls += tmp;
    sprintf(tmp, ",%d", info.iBufferStatus);
    bufs += tmp;
    dump_options("flush");
    if (info.iBufferStatus != 1) continue;
    int w = info.UsrData.sSystemBuffer.iWidth, h = info.UsrData.sSystemBuffer.iHeight;
    int sy = info.UsrData.sSystemBuffer.iStride[0], suv = info.UsrData.sSystemBuffer.iStride[1];
    frames++;
    if (!w0) { w0 = w; h0 = h; }
    if (want_frames) frame_hashes.push_back(frame_digest(dst, w, h, sy, suv) + " (flush)");
    hash_plane(sha, dst[0], w, h, sy);
    hash_plane(sha, dst[1], w / 2, h / 2, suv);
    hash_plane(sha, dst[2], w / 2, h / 2, suv);
  }

  dec->Uninitialize();
  WelsDestroyDecoder(dec);

  printf("%d ", frames);
  if (w0) printf("%dx%d ", w0, h0); else printf("- ");
  printf("%s ", frames ? sha.digest().c_str() : "-");
  printf("%s %s\n", calls.c_str(), bufs.c_str());
  for (size_t i = 0; i < frame_hashes.size(); i++)
    fprintf(stderr, "CPPFRAME %zu %s\n", i, frame_hashes[i].c_str());
  return 0;
}
