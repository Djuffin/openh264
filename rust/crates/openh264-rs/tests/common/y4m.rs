//! Minimal Y4M (YUV4MPEG2) parsing and gold-reference comparison.
//!
//! openh264-rs carries no dependencies beyond `libc`, so rather than pulling in
//! the `y4m` crate this module parses just enough of the container to compare a
//! decoded stream against a reference file: the stream header (for the frame
//! size) and the `FRAME`-delimited 4:2:0 planes that follow.
//!
//! Comparing parsed frames rather than raw bytes matters. The gold files in
//! `res/` were produced by ffmpeg and carry header fields our writer does not
//! emit -- `Ip A0:0 C420jpeg XYSCSS=420JPEG`, plus a per-stream frame rate --
//! so a plain byte comparison would fail on the header of every single stream.

#![allow(dead_code)]

/// Luma samples per macroblock edge. Used to report mismatches by macroblock,
/// which is the unit you actually debug a decoder in.
const MB_WIDTH: usize = 16;

pub struct Frame<'a> {
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
}

pub struct Y4mReader<'a> {
    data: &'a [u8],
    pos: usize,
    width: usize,
    height: usize,
}

impl<'a> Y4mReader<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, String> {
        let nl = data
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| "Y4M: stream header is not terminated (empty stream?)".to_string())?;
        let header = std::str::from_utf8(&data[..nl])
            .map_err(|_| "Y4M: stream header is not valid UTF-8".to_string())?;

        let mut fields = header.split(' ');
        if fields.next() != Some("YUV4MPEG2") {
            return Err(format!("Y4M: bad magic in stream header {header:?}"));
        }

        let (mut width, mut height) = (None, None);
        for field in fields {
            match field.as_bytes().first() {
                Some(b'W') => width = field[1..].parse::<usize>().ok(),
                Some(b'H') => height = field[1..].parse::<usize>().ok(),
                // C420, C420jpeg, C420mpeg2 and C420paldv differ only in
                // chroma siting, which does not affect the plane layout.
                Some(b'C') if !field.starts_with("C420") => {
                    return Err(format!(
                        "Y4M: colorspace {field:?} is not 4:2:0; only 4:2:0 is handled"
                    ));
                }
                _ => {}
            }
        }

        let width = width.ok_or_else(|| format!("Y4M: no width in header {header:?}"))?;
        let height = height.ok_or_else(|| format!("Y4M: no height in header {header:?}"))?;

        Ok(Self { data, pos: nl + 1, width, height })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    fn chroma_width(&self) -> usize {
        self.width.div_ceil(2)
    }

    fn chroma_height(&self) -> usize {
        self.height.div_ceil(2)
    }

    /// Reads the next frame, or `None` at end of stream.
    pub fn read_frame(&mut self) -> Result<Option<Frame<'a>>, String> {
        if self.pos >= self.data.len() {
            return Ok(None);
        }

        let rest = &self.data[self.pos..];
        if !rest.starts_with(b"FRAME") {
            return Err(format!(
                "Y4M: expected a FRAME marker at byte {}, found {:?}",
                self.pos,
                String::from_utf8_lossy(&rest[..rest.len().min(16)])
            ));
        }
        // The FRAME marker may carry its own parameters; they run to the newline.
        let nl = rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| format!("Y4M: unterminated FRAME marker at byte {}", self.pos))?;

        let y_size = self.width * self.height;
        let c_size = self.chroma_width() * self.chroma_height();
        let start = self.pos + nl + 1;
        let end = start + y_size + 2 * c_size;
        if end > self.data.len() {
            return Err(format!(
                "Y4M: truncated frame at byte {start}: need {} bytes, {} remain",
                y_size + 2 * c_size,
                self.data.len() - start
            ));
        }

        self.pos = end;
        Ok(Some(Frame {
            y: &self.data[start..start + y_size],
            u: &self.data[start + y_size..start + y_size + c_size],
            v: &self.data[start + y_size + c_size..end],
        }))
    }
}

/// Returns the coordinates and values of the first differing sample.
fn compare_plane(
    width: usize,
    height: usize,
    actual: &[u8],
    expected: &[u8],
) -> Option<(usize, usize, u8, u8)> {
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if actual[idx] != expected[idx] {
                return Some((x, y, actual[idx], expected[idx]));
            }
        }
    }
    None
}

fn compare_frames(width: usize, height: usize, actual: &Frame, expected: &Frame) -> String {
    let mut result = String::new();

    if let Some((x, y, a, e)) = compare_plane(width, height, actual.y, expected.y) {
        let mb_idx = x / MB_WIDTH + (y / MB_WIDTH) * (width / MB_WIDTH);
        result.push_str(&format!("Y-plane mismatch at {x},{y} (MB:{mb_idx}) : {a} != {e}\n"));
    }

    let chroma_mb_width = MB_WIDTH / 2;
    let chroma_width = width.div_ceil(2);
    let chroma_height = height.div_ceil(2);
    let width_in_mb = chroma_width / chroma_mb_width;

    if let Some((x, y, a, e)) = compare_plane(chroma_width, chroma_height, actual.u, expected.u) {
        let mb_idx = x / chroma_mb_width + (y / chroma_mb_width) * width_in_mb;
        result.push_str(&format!("U-plane mismatch at {x},{y} (MB:{mb_idx}) : {a} != {e}\n"));
    }
    if let Some((x, y, a, e)) = compare_plane(chroma_width, chroma_height, actual.v, expected.v) {
        let mb_idx = x / chroma_mb_width + (y / chroma_mb_width) * width_in_mb;
        result.push_str(&format!("V-plane mismatch at {x},{y} (MB:{mb_idx}) : {a} != {e}\n"));
    }

    result
}

/// Compares two Y4M streams frame by frame, ignoring header fields that do not
/// affect the pixels (frame rate, aspect ratio, chroma siting).
pub fn compare_y4m_buffers(actual_y4m_data: &[u8], expected_y4m_data: &[u8]) -> Result<(), String> {
    let mut actual = Y4mReader::new(actual_y4m_data).map_err(|e| format!("actual: {e}"))?;
    let mut expected = Y4mReader::new(expected_y4m_data).map_err(|e| format!("expected: {e}"))?;

    if (actual.width(), actual.height()) != (expected.width(), expected.height()) {
        return Err(format!(
            "Unexpected size of frames. {}x{} vs expected {}x{}",
            actual.width(),
            actual.height(),
            expected.width(),
            expected.height()
        ));
    }
    let (width, height) = (expected.width(), expected.height());

    let mut frame_idx = 0;
    loop {
        match (actual.read_frame()?, expected.read_frame()?) {
            (Some(actual_frame), Some(expected_frame)) => {
                let compare_result =
                    compare_frames(width, height, &actual_frame, &expected_frame);
                if !compare_result.is_empty() {
                    return Err(format!("Frame #{frame_idx} mismatch: {compare_result}"));
                }
                frame_idx += 1;
            }
            (None, None) => break,
            (Some(_), None) => {
                return Err(format!(
                    "Actual has more frames than expected. Expected {frame_idx} frames."
                ));
            }
            (None, Some(_)) => {
                return Err(format!(
                    "Expected has more frames than actual. Actual had {frame_idx} frames."
                ));
            }
        }
    }

    Ok(())
}
