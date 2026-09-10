//! Rich NAL-unit and bitstream divergence diagnostics.

use openh264_rs::split_annexb_units;

fn nal_type_name(nal_type: u8) -> &'static str {
    match nal_type {
        1 => "Non-IDR Slice (1)",
        2 => "Slice Data Partition A (2)",
        3 => "Slice Data Partition B (3)",
        4 => "Slice Data Partition C (4)",
        5 => "IDR Slice (5)",
        6 => "SEI (6)",
        7 => "SPS (7)",
        8 => "PPS (8)",
        9 => "Access Unit Delimiter (9)",
        10 => "End of Sequence (10)",
        11 => "End of Stream (11)",
        12 => "Filler Data (12)",
        14 => "Prefix NAL (14)",
        15 => "Subset SPS (15)",
        _ => "Unknown",
    }
}

/// Analyzes differences between Rust-generated bitstream and C++-generated bitstream
/// and formats a detailed, actionable divergence report.
pub fn format_divergence_report(
    config_label: &str,
    rust_stream: &[u8],
    cpp_stream: &[u8],
) -> String {
    let mut msg = String::new();
    msg.push_str(&format!(
        "\n================================================================================\n\
         BITSTREAM DIVERGENCE REPORT: {config_label}\n\
         ================================================================================\n"
    ));
    msg.push_str(&format!(
        "Stream Sizes: Rust = {} bytes, C++ = {} bytes (delta = {})\n",
        rust_stream.len(),
        cpp_stream.len(),
        rust_stream.len() as isize - cpp_stream.len() as isize
    ));

    // First raw byte difference
    let min_len = rust_stream.len().min(cpp_stream.len());
    let mut first_diff_byte = None;
    for i in 0..min_len {
        if rust_stream[i] != cpp_stream[i] {
            first_diff_byte = Some(i);
            break;
        }
    }
    if let Some(byte_idx) = first_diff_byte {
        msg.push_str(&format!("First byte discrepancy at global offset {byte_idx} (0x{byte_idx:X}):\n"));
        let start = byte_idx.saturating_sub(8);
        let end = (byte_idx + 16).min(min_len);
        msg.push_str(&format!(
            "  Rust: {:02X?}\n  C++ : {:02X?}\n",
            &rust_stream[start..end],
            &cpp_stream[start..end]
        ));
    } else if rust_stream.len() != cpp_stream.len() {
        msg.push_str(&format!(
            "Streams are identical for the first {min_len} bytes, but length differs.\n"
        ));
    }

    // NAL-level analysis
    let rust_nals = split_annexb_units(rust_stream);
    let cpp_nals = split_annexb_units(cpp_stream);

    msg.push_str(&format!(
        "\nNAL Unit Breakdown: Rust = {} NALs, C++ = {} NALs\n",
        rust_nals.len(),
        cpp_nals.len()
    ));

    let common_nals = rust_nals.len().min(cpp_nals.len());
    let mut divergent_nal_idx = None;

    for i in 0..common_nals {
        if rust_nals[i] != cpp_nals[i] {
            divergent_nal_idx = Some(i);
            break;
        }
    }

    if let Some(i) = divergent_nal_idx {
        let r_nal = rust_nals[i];
        let c_nal = cpp_nals[i];
        let r_type = if !r_nal.is_empty() { r_nal[0] & 0x1F } else { 0 };
        let c_type = if !c_nal.is_empty() { c_nal[0] & 0x1F } else { 0 };

        msg.push_str(&format!(
            "\nFirst Divergent NAL index: #{i}\n\
             - Rust NAL #{i}: length = {} bytes, type = {} ({})\n\
             - C++  NAL #{i}: length = {} bytes, type = {} ({})\n",
            r_nal.len(),
            r_type,
            nal_type_name(r_type),
            c_nal.len(),
            c_type,
            nal_type_name(c_type),
        ));

        let min_nal_len = r_nal.len().min(c_nal.len());
        for b in 0..min_nal_len {
            if r_nal[b] != c_nal[b] {
                msg.push_str(&format!(
                    "First payload difference at NAL offset {b} (0x{b:X}):\n\
                     - Rust byte: 0x{:02X} (binary {:08b})\n\
                     - C++  byte: 0x{:02X} (binary {:08b})\n",
                    r_nal[b], r_nal[b], c_nal[b], c_nal[b]
                ));
                let s = b.saturating_sub(6);
                let e = (b + 10).min(min_nal_len);
                msg.push_str(&format!(
                    "Context around offset {b}:\n  Rust: {:02X?}\n  C++ : {:02X?}\n",
                    &r_nal[s..e],
                    &c_nal[s..e]
                ));
                break;
            }
        }
    } else if rust_nals.len() != cpp_nals.len() {
        msg.push_str(&format!(
            "First {common_nals} NALs are identical, but total NAL count differs!\n"
        ));
    }

    msg.push_str("================================================================================\n");
    msg
}
