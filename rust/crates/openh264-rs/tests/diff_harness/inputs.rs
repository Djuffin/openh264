//! In-memory YUV input loader, looper, and synthetic screen clip generator.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub struct YuvClip {
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub frames_yuv: Vec<Vec<u8>>, // Each element is a full I420 frame (Y + U + V)
}

impl YuvClip {
    pub fn frame_size(&self) -> usize {
        (self.width * self.height * 3 / 2) as usize
    }

    pub fn num_frames(&self) -> usize {
        self.frames_yuv.len()
    }
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("../../../"),
        manifest_dir.join("../../"),
        manifest_dir.join("../"),
    ];
    for c in candidates {
        if c.join("res").exists() {
            if let Ok(canon) = c.canonicalize() {
                return canon;
            }
            return c;
        }
    }
    manifest_dir
}

static FILE_CACHE: OnceLock<Mutex<HashMap<PathBuf, Vec<u8>>>> = OnceLock::new();

fn read_file_cached(path: &Path) -> Vec<u8> {
    let cache = FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = cache.lock().unwrap();
    if let Some(data) = map.get(path) {
        return data.clone();
    }
    let data = std::fs::read(path).unwrap_or_else(|e| {
        panic!("Failed to read asset file at {}: {e}", path.display());
    });
    map.insert(path.to_path_buf(), data.clone());
    data
}

/// Loads a `.yuv` file from `res/` and loops it in memory to at least `want_frames`.
pub fn load_looped_res(rel_path: &str, width: i32, height: i32, want_frames: usize) -> YuvClip {
    let root = repo_root();
    let full_path = root.join(rel_path);
    let raw = read_file_cached(&full_path);
    let frame_size = (width * height * 3 / 2) as usize;
    assert!(
        frame_size > 0 && raw.len() >= frame_size,
        "Asset {} is too small for {}x{} frames",
        rel_path,
        width,
        height
    );

    let nsrc = raw.len() / frame_size;
    let reps = want_frames.div_ceil(nsrc);
    let total_frames = nsrc * reps;

    let mut frames_yuv = Vec::with_capacity(total_frames);
    for _ in 0..reps {
        for s in 0..nsrc {
            let offset = s * frame_size;
            frames_yuv.push(raw[offset..offset + frame_size].to_vec());
        }
    }

    let stem = Path::new(rel_path)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    YuvClip {
        name: format!("{}_loop{}", stem, total_frames),
        width,
        height,
        frames_yuv,
    }
}

// ============================================================================
// Synthetic Screen Content Clip Generator (Pure Rust replacement for gen_screen_clip.py)
// ============================================================================

const PAPER: u8 = 235;
const RULE: u8 = 200;
const INKS: [u8; 4] = [16, 48, 96, 144];
const GLYPH_W: usize = 7;
const GLYPH_H: usize = 10;
const GLYPH_MIN_BITS: u32 = 12;
const FIRST_LINE_ROW: usize = 4;
const LINE_PITCH: usize = 12;
const CELL_PITCH: usize = 8;
const LEFT_MARGIN: usize = 8;
const PAGE_TAIL: usize = 32;

struct Lcg {
    state: u32,
}

impl Lcg {
    fn new(seed: u32) -> Self {
        Self {
            state: seed & 0x7FFF_FFFF,
        }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = (self.state.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7FFF_FFFF;
        self.state >> 8
    }
}

fn draw_glyphs(rng: &mut Lcg) -> Vec<[u8; GLYPH_H]> {
    let mut glyphs = Vec::with_capacity(16);
    glyphs.push([0u8; GLYPH_H]); // Glyph 0 is blank
    for _ in 1..16 {
        let mut rows = [0u8; GLYPH_H];
        loop {
            let mut bit_count = 0;
            for r in 0..GLYPH_H {
                rows[r] = (rng.next_u32() & 0x7F) as u8;
                bit_count += rows[r].count_ones();
            }
            if bit_count >= GLYPH_MIN_BITS {
                break;
            }
        }
        glyphs.push(rows);
    }
    glyphs
}

fn draw_page(width: usize, height: usize, seed: u32) -> Vec<u8> {
    let mut rng = Lcg::new(seed);
    let glyphs = draw_glyphs(&mut rng);
    let mut page = vec![PAPER; width * height];
    let cells = (width.saturating_sub(16)) / 8;
    let mut line = 0usize;
    let mut top = FIRST_LINE_ROW;

    while top + GLYPH_H <= height {
        if line % 5 == 4 {
            let y = top + GLYPH_H / 2;
            page[y * width..(y + 1) * width].fill(RULE);
        } else {
            let ink_base = (rng.next_u32() % 4) as usize;
            let mut c = 0usize;
            let mut word = 0usize;
            while c < cells {
                let word_len = (3 + rng.next_u32() % 5) as usize;
                let ink = INKS[(ink_base + word) % 4];
                for _ in 0..word_len {
                    if c >= cells {
                        break;
                    }
                    let glyph_idx = (rng.next_u32() % 16) as usize;
                    let glyph = &glyphs[glyph_idx];
                    let x0 = LEFT_MARGIN + CELL_PITCH * c;
                    for r in 0..GLYPH_H {
                        let bits = glyph[r];
                        let row = (top + r) * width;
                        for b in 0..GLYPH_W {
                            if ((bits >> (GLYPH_W - 1 - b)) & 1) != 0 {
                                page[row + x0 + b] = ink;
                            }
                        }
                    }
                    c += 1;
                }
                word += 1;
            }
        }
        line += 1;
        top += LINE_PITCH;
    }
    page
}

/// Pure Rust generator for synthetic scrolling screen content clips.
pub fn generate_screen_clip(
    name: &str,
    width: i32,
    height: i32,
    frames: usize,
    scroll: usize,
    cut_every: usize,
    hold_every: usize,
    seed: u32,
) -> YuvClip {
    let (w, h, n, k, c, d) = (width as usize, height as usize, frames, scroll, cut_every, hold_every);
    let page_h = h + k * n + PAGE_TAIL;
    let mut pages: HashMap<usize, Vec<u8>> = HashMap::new();

    let chroma_size = (w * h) / 2;
    let chroma = vec![128u8; chroma_size];

    let mut frames_yuv = Vec::with_capacity(n);
    let mut prev_luma: Option<Vec<u8>> = None;

    for f in 0..n {
        let luma = if d > 0 && (f % d == d - 1) && prev_luma.is_some() {
            prev_luma.clone().unwrap()
        } else {
            let o = if c > 0 { (f % c) * k } else { f * k };
            let page_idx = if c > 0 { f / c } else { 0 };
            let page = pages.entry(page_idx).or_insert_with(|| {
                draw_page(w, page_h, seed + page_idx as u32)
            });
            page[o * w..(o + h) * w].to_vec()
        };

        let mut frame = Vec::with_capacity(w * h * 3 / 2);
        frame.extend_from_slice(&luma);
        frame.extend_from_slice(&chroma);
        prev_luma = Some(luma);
        frames_yuv.push(frame);
    }

    YuvClip {
        name: name.to_string(),
        width,
        height,
        frames_yuv,
    }
}
