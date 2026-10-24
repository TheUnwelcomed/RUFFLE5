//! PS5_RUFFLE_COVERS_V1 — a game's cover picture from one of its own frames: taken 8 seconds into
//! its first run when it has no picture (Settings > Automatic covers), or from the quick menu.
//!
//! The frame comes from the display (src/shell.rs `request_capture` / `poll_capture`), is cropped to
//! the movie's own area (the bars around a letterboxed movie are not part of it), shrunk to at most
//! 640 pixels wide and written as /data/ruffle5/covers/<game>.png on a thread of its own, so the game
//! does not hitch. A picture the player put beside the game (games/<name>.png) always wins over it.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::shell::Captured;

/// The movie's area in a `fw` x `fh` frame when it is zoomed to fit (Ruffle's default scale mode).
pub fn movie_area(fw: u32, fh: u32, movie_w: u32, movie_h: u32) -> (u32, u32, u32, u32) {
    if movie_w == 0 || movie_h == 0 {
        return (0, 0, fw, fh);
    }
    let s = (fw as f32 / movie_w as f32).min(fh as f32 / movie_h as f32);
    let (w, h) = (((movie_w as f32 * s) as u32).clamp(1, fw), ((movie_h as f32 * s) as u32).clamp(1, fh));
    ((fw - w) / 2, (fh - h) / 2, w, h)
}

/// Crop, shrink and encode; returns the PNG.
pub fn encode(frame: &Captured, area: (u32, u32, u32, u32)) -> Result<Vec<u8>, String> {
    let (x0, y0, w, h) = area;
    if x0 + w > frame.width || y0 + h > frame.height || w == 0 || h == 0 {
        return Err("crop outside the frame".into());
    }
    let mut crop = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let src = (((y0 + y) * frame.width + x0) * 4) as usize;
        let dst = (y * w * 4) as usize;
        crop[dst..dst + (w * 4) as usize].copy_from_slice(&frame.rgba[src..src + (w * 4) as usize]);
    }
    let (sw, sh, small) = crate::ui::shrink_rgba(w, h, &crop, 640);
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, sw, sh);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&small).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Save `frame` as the cover at `path`, off the game loop.
pub fn save_in_background(frame: Captured, area: (u32, u32, u32, u32), path: String) {
    let spawned = std::thread::Builder::new().name("cover".into()).spawn(move || match encode(&frame, area) {
        Ok(png) => match crate::fsutil::write_durably(&path, &png) {
            Ok(()) => crate::say(&format!("ok   cover saved: {path} ({} bytes)", png.len())),
            Err(e) => crate::say(&format!("warn cover {path}: {e}")),
        },
        Err(e) => crate::say(&format!("warn cover {path}: {e}")),
    });
    if let Err(e) = spawned {
        crate::say(&format!("warn cover thread: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_movie_area_is_centred() {
        assert_eq!(movie_area(1920, 1080, 550, 400), (217, 0, 1485, 1080));
        assert_eq!(movie_area(1920, 1080, 0, 0), (0, 0, 1920, 1080));
        assert_eq!(movie_area(3840, 2160, 1600, 900), (0, 0, 3840, 2160));
    }

    #[test]
    fn a_frame_encodes_to_a_png() {
        let frame = Captured { width: 64, height: 32, rgba: vec![200u8; 64 * 32 * 4] };
        let png = encode(&frame, (8, 0, 48, 32)).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let (w, h, _) = crate::ui::decode_png(&png).unwrap();
        assert_eq!((w, h), (48, 32));
    }
}
