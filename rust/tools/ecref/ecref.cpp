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

  if (argc >= 2 && strcmp(argv[1], "--stdin") == 0) {
    for (int i = 2; i < argc; i++) {
      if (strcmp(argv[i], "--raw") == 0) raw_feed = true;
      else if (strcmp(argv[i], "--annexb") == 0) raw_feed = false;
      else if (strcmp(argv[i], "--frames") == 0) { /* handled above */ }
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

  ISVCDecoder* dec = nullptr;
  if (WelsCreateDecoder(&dec) != 0 || !dec) { fprintf(stderr, "WelsCreateDecoder\n"); return 2; }

  SDecodingParam p;
  memset(&p, 0, sizeof(p));
  p.uiTargetDqLayer = UCHAR_MAX;
  p.eEcActiveIdc = ERROR_CON_SLICE_COPY;
  p.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
  if (dec->Initialize(&p) != 0) { fprintf(stderr, "Initialize\n"); return 2; }

  Sha1 sha;
  int frames = 0, w0 = 0, h0 = 0;
  std::string calls, bufs;
  std::vector<std::string> frame_hashes;   // --frames: one digest per emitted frame

  auto feed = [&](const uint8_t* buf, int len) {
    uint8_t* dst[3] = {nullptr, nullptr, nullptr};
    SBufferInfo info;
    memset(&info, 0, sizeof(info));
    int ret = int(dec->DecodeFrame2(buf, len, dst, &info));
    char tmp[32];
    sprintf(tmp, "%s0x%x", calls.empty() ? "" : ",", ret);
    calls += tmp;
    sprintf(tmp, "%s%d", bufs.empty() ? "" : ",", info.iBufferStatus);
    bufs += tmp;
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
