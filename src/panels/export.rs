//! Saving what the editor shows: data as CSV and parts of the window as PNG
//! images. On desktop a save dialog asks where; in the browser it downloads.

use egui::{ColorImage, Rect};

/// Save `bytes` as `name` (a file dialog on desktop, a download on the web).
/// Returns where it went, or `None` if the dialog was cancelled. With
/// `SHINE_EXPORT_DIR` set, desktop saves go straight into that folder (for
/// scripts and tests).
#[cfg(not(target_arch = "wasm32"))]
pub fn save(name: &str, kind: &str, extension: &str, bytes: &[u8]) -> Result<Option<String>, String> {
    let path = match std::env::var_os("SHINE_EXPORT_DIR") {
        Some(dir) => std::path::PathBuf::from(dir).join(name),
        None => match rfd::FileDialog::new().add_filter(kind, &[extension]).set_file_name(name).save_file() {
            Some(path) => path,
            None => return Ok(None),
        },
    };
    std::fs::write(&path, bytes).map_err(|e| format!("couldn't save {}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

#[cfg(target_arch = "wasm32")]
pub fn save(name: &str, _kind: &str, _extension: &str, bytes: &[u8]) -> Result<Option<String>, String> {
    crate::web::download(name, bytes).map(|()| Some(name.to_owned()))
}

/// The part of `image` (a screenshot) under `rect`, in points at
/// `pixels_per_point`.
pub fn crop(image: &ColorImage, rect: Rect, pixels_per_point: f32) -> ColorImage {
    let [w, h] = image.size;
    let clamp = |v: f32, max: usize| (v * pixels_per_point).round().clamp(0.0, max as f32) as usize;
    let (x0, x1) = (clamp(rect.min.x, w), clamp(rect.max.x, w));
    let (y0, y1) = (clamp(rect.min.y, h), clamp(rect.max.y, h));
    let (cw, ch) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let mut pixels = Vec::with_capacity(cw * ch);
    for y in y0..y1 {
        pixels.extend_from_slice(&image.pixels[y * w + x0..y * w + x1]);
    }
    ColorImage::new([cw, ch], pixels)
}

/// A PNG file of `image`.
pub fn png(image: &ColorImage) -> Result<Vec<u8>, String> {
    let [w, h] = image.size;
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        let data: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
        writer.write_image_data(&data).map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Color32, Pos2, Vec2};

    fn sample() -> ColorImage {
        // 4×3 pixels, each a distinct colour from its position.
        let pixels = (0..12).map(|k| Color32::from_rgb(k as u8 * 10, (k / 4) as u8, (k % 4) as u8)).collect();
        ColorImage::new([4, 3], pixels)
    }

    #[test]
    fn crops_in_points_at_the_pixel_scale() {
        let img = sample();
        // At 2 pixels per point, the point rect (0.5,0.5)-(1.5,1.5) is pixels (1,1)-(3,3).
        let c = crop(&img, Rect::from_min_size(Pos2::new(0.5, 0.5), Vec2::splat(1.0)), 2.0);
        assert_eq!(c.size, [2, 2]);
        assert_eq!(c.pixels, vec![img.pixels[5], img.pixels[6], img.pixels[9], img.pixels[10]]);
        // Outside the image is clipped, not a panic.
        assert_eq!(crop(&img, Rect::from_min_size(Pos2::new(3.0, 2.0), Vec2::splat(9.0)), 1.0).size, [1, 1]);
        assert_eq!(crop(&img, Rect::from_min_size(Pos2::new(9.0, 9.0), Vec2::splat(1.0)), 1.0).size, [0, 0]);
    }

    #[test]
    fn png_files_decode_to_the_same_pixels() {
        let img = sample();
        let bytes = png(&img).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (4, 3));
        assert_eq!(&buf[..4], &img.pixels[0].to_srgba_unmultiplied());
        assert_eq!(&buf[4 * 6..4 * 7], &img.pixels[6].to_srgba_unmultiplied());
    }
}
