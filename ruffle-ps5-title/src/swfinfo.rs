//! PS5_RUFFLE_SWFINFO_V1 — what a SWF's header says, read without decompressing the movie.
//!
//! The first eight bytes are plain (signature, version, length); the frame size, frame rate and
//! frame count follow them inside the (zlib-) compressed body, so for a CWS only the first few
//! dozen bytes are inflated. LZMA (ZWS) bodies are left alone: version and length only.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Read;

#[derive(Clone, Debug, Default)]
pub struct SwfInfo {
    pub version: u8,
    pub compression: &'static str,
    pub uncompressed_len: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f32,
    pub frames: u32,
}

/// Read the header of the SWF at `path`.
pub fn read(path: &str) -> Result<SwfInfo, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut head = vec![0u8; 4096];
    let mut got = 0;
    while got < head.len() {
        match file.read(&mut head[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => return Err(e.to_string()),
        }
    }
    head.truncate(got);
    if head.len() < 8 {
        return Err("shorter than a SWF header".into());
    }
    let compression = match &head[0..3] {
        b"FWS" => "FWS",
        b"CWS" => "CWS",
        b"ZWS" => "ZWS",
        _ => return Err("not a SWF signature".into()),
    };
    let mut info = SwfInfo {
        version: head[3],
        compression,
        uncompressed_len: u32::from_le_bytes([head[4], head[5], head[6], head[7]]),
        ..Default::default()
    };
    let body: Vec<u8> = match compression {
        "FWS" => head[8..].to_vec(),
        "CWS" => {
            let mut out = vec![0u8; 64];
            let mut dec = flate2::read::ZlibDecoder::new(&head[8..]);
            let mut n = 0;
            while n < out.len() {
                match dec.read(&mut out[n..]) {
                    Ok(0) => break,
                    Ok(k) => n += k,
                    Err(_) => break,
                }
            }
            out.truncate(n);
            out
        }
        _ => return Ok(info),
    };
    if let Some((w, h, rate, frames)) = parse_body(&body) {
        info.width = w;
        info.height = h;
        info.frame_rate = rate;
        info.frames = frames;
    }
    Ok(info)
}

/// RECT (frame size in twips), then frame rate (8.8 fixed, little-endian) and frame count.
fn parse_body(b: &[u8]) -> Option<(u32, u32, f32, u32)> {
    if b.is_empty() {
        return None;
    }
    let nbits = (b[0] >> 3) as usize;
    let mut bit = 5usize;
    let mut read = |n: usize| -> Option<i32> {
        if n == 0 || n > 31 {
            return Some(0);
        }
        let mut v: u32 = 0;
        for _ in 0..n {
            let byte = *b.get(bit / 8)?;
            let bitval = (byte >> (7 - bit % 8)) & 1;
            v = (v << 1) | bitval as u32;
            bit += 1;
        }
        // sign-extend
        let shift = 32 - n;
        Some(((v << shift) as i32) >> shift)
    };
    let xmin = read(nbits)?;
    let xmax = read(nbits)?;
    let ymin = read(nbits)?;
    let ymax = read(nbits)?;
    let at = (5 + 4 * nbits).div_ceil(8);
    let rate_lo = *b.get(at)?;
    let rate_hi = *b.get(at + 1)?;
    let frames = u16::from_le_bytes([*b.get(at + 2)?, *b.get(at + 3)?]) as u32;
    let rate = rate_hi as f32 + rate_lo as f32 / 256.0;
    Some((((xmax - xmin) / 20).max(0) as u32, ((ymax - ymin) / 20).max(0) as u32, rate, frames))
}
