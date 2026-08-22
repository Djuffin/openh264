// wels_ref_common.h — the pieces every C++ referee in this tree needs, in one place.
//
// Phase 8 session C, T8.C5. `ecref.cpp` grew these first (Phase 5 session S) and the
// external-ABI harness needs the same three, computing the same numbers: a SHA-1 that
// agrees with `tests/common/sha1.rs`, the annex-B split that agrees with
// `openh264_rs::split_annexb_units`, and the plane hashing that agrees with
// `tests/decoder_conformance_test.rs`'s. A second copy of any of the three would be a
// referee that could drift from the thing it referees while both still passed — F21's
// shape, in the tools rather than the port.
//
// `abi_harness --selftest` runs the SHA-1 known-answer test the Rust side has
// (`"abc"` -> a9993e36...), so a drift in this file is caught by a value and not by a
// review.
#ifndef WELS_REF_COMMON_H
#define WELS_REF_COMMON_H

#include <cstdio>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>
#include <utility>

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

#endif  // WELS_REF_COMMON_H
