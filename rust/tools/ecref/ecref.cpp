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

// --- SHA-1, so the digest is comparable with `tests/common/sha1.rs` ----------
struct Sha1 {
  uint32_t h[5] = {0x67452301u, 0xEFCDAB89u, 0x98BADCFEu, 0x10325476u, 0xC3D2E1F0u};
  uint64_t len = 0;
  uint8_t buf[64];
  size_t buflen = 0;

  static uint32_t rol(uint32_t v, int n) { return (v << n) | (v >> (32 - n)); }

  void block(const uint8_t* p) {
    uint32_t w[80];
    for (int i = 0; i < 16; i++)
      w[i] = (uint32_t(p[i * 4]) << 24) | (uint32_t(p[i * 4 + 1]) << 16) |
             (uint32_t(p[i * 4 + 2]) << 8) | uint32_t(p[i * 4 + 3]);
    for (int i = 16; i < 80; i++) w[i] = rol(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
    uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4];
    for (int i = 0; i < 80; i++) {
      uint32_t f, k;
      if (i < 20)      { f = (b & c) | (~b & d);          k = 0x5A827999u; }
      else if (i < 40) { f = b ^ c ^ d;                   k = 0x6ED9EBA1u; }
      else if (i < 60) { f = (b & c) | (b & d) | (c & d); k = 0x8F1BBCDCu; }
      else             { f = b ^ c ^ d;                   k = 0xCA62C1D6u; }
      uint32_t t = rol(a, 5) + f + e + k + w[i];
      e = d; d = c; c = rol(b, 30); b = a; a = t;
    }
    h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e;
  }

  void update(const uint8_t* p, size_t n) {
    len += n;
    while (n) {
      size_t take = 64 - buflen;
      if (take > n) take = n;
      memcpy(buf + buflen, p, take);
      buflen += take; p += take; n -= take;
      if (buflen == 64) { block(buf); buflen = 0; }
    }
  }

  std::string digest() {
    uint64_t bits = len * 8;
    uint8_t pad = 0x80;
    update(&pad, 1);
    uint8_t zero = 0;
    while (buflen != 56) update(&zero, 1);
    uint8_t tail[8];
    for (int i = 0; i < 8; i++) tail[i] = uint8_t(bits >> (56 - 8 * i));
    len -= 8;  // the length bytes are not part of the message
    update(tail, 8);
    char out[41];
    for (int i = 0; i < 5; i++) sprintf(out + i * 8, "%08x", h[i]);
    out[40] = 0;
    return std::string(out);
  }
};

// --- the annex-B split, byte for byte as `openh264_rs::split_annexb_units` ---
static std::vector<std::pair<size_t, size_t>> split_annexb(const std::vector<uint8_t>& b) {
  std::vector<size_t> starts;
  size_t i = 0, len = b.size();
  while (i + 2 < len) {
    if (b[i] == 0 && b[i + 1] == 0) {
      if (b[i + 2] == 1) { starts.push_back(i); i += 3; continue; }
      if (i + 3 < len && b[i + 2] == 0 && b[i + 3] == 1) { starts.push_back(i); i += 4; continue; }
    }
    size_t pos = std::string::npos;
    for (size_t j = i + 1; j < len; j++) if (b[j] == 0) { pos = j; break; }
    if (pos == std::string::npos) break;
    i = pos;
  }
  std::vector<std::pair<size_t, size_t>> units;
  for (size_t k = 0; k < starts.size(); k++)
    units.push_back({starts[k], (k + 1 < starts.size()) ? starts[k + 1] : len});
  return units;
}

static void hash_plane(Sha1& s, const uint8_t* p, int w, int h, int stride) {
  if (!p || w <= 0 || h <= 0 || stride <= 0) return;
  for (int y = 0; y < h; y++) s.update(p + size_t(y) * size_t(stride), size_t(w));
}

// One digest over a single emitted frame — `--frames`, the counterpart of
// `portref`'s PORTFRAME lines. The whole-run digest that this tool's row carries
// cannot answer "are these the same pictures in a different order", which is the
// question every ordering divergence in this corpus turns out to ask. Comparing
// the two per-frame lists as multisets does answer it.
static std::string frame_digest(uint8_t* const dst[3], int w, int h, int sy, int suv) {
  Sha1 s;
  hash_plane(s, dst[0], w, h, sy);
  hash_plane(s, dst[1], w / 2, h / 2, suv);
  hash_plane(s, dst[2], w / 2, h / 2, suv);
  return s.digest();
}

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
