//! How a component's box looks: its colour, outline and an optional picture.
//!
//! Pictures are kept small: a raster image is scaled to fit [`ICON_PX`] and
//! stored as (lossless) WebP; an SVG is stored as it is, since it is already
//! small and stays sharp at every zoom.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[cfg(feature = "gui")]
use egui::{Color32, Pos2, Rect, Shape, Stroke};
use serde::{Deserialize, Serialize};

/// Largest side of a stored raster picture, in pixels.
pub const ICON_PX: u32 = 128;
/// SVGs larger than this are refused: an icon shouldn't be a whole drawing.
pub const MAX_SVG_BYTES: usize = 512 * 1024;

/// The outline of a component's box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoxShape {
    #[default]
    Rectangle,
    Rounded,
    /// Fully rounded ends.
    Capsule,
    /// Cut corners.
    Chamfered,
}

impl BoxShape {
    pub const ALL: [BoxShape; 4] = [BoxShape::Rectangle, BoxShape::Rounded, BoxShape::Capsule, BoxShape::Chamfered];

    pub fn label(self) -> &'static str {
        match self {
            BoxShape::Rectangle => "Rectangle",
            BoxShape::Rounded => "Rounded",
            BoxShape::Capsule => "Capsule",
            BoxShape::Chamfered => "Cut corners",
        }
    }

    /// The outline filling `rect`, as a closed polygon (or a rounded rect).
    #[cfg(feature = "gui")]
    pub fn shape(self, rect: Rect, fill: Color32, stroke: Stroke) -> Shape {
        let short = rect.width().min(rect.height());
        match self {
            BoxShape::Rectangle => Shape::rect_filled(rect, (short * 0.12).min(6.0), fill),
            BoxShape::Rounded => Shape::rect_filled(rect, (short * 0.22).min(14.0), fill),
            BoxShape::Capsule => Shape::rect_filled(rect, short / 2.0, fill),
            BoxShape::Chamfered => {
                let c = (short * 0.25).min(16.0);
                let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
                let points = vec![
                    Pos2::new(l + c, t),
                    Pos2::new(r - c, t),
                    Pos2::new(r, t + c),
                    Pos2::new(r, b - c),
                    Pos2::new(r - c, b),
                    Pos2::new(l + c, b),
                    Pos2::new(l, b - c),
                    Pos2::new(l, t + c),
                ];
                Shape::convex_polygon(points, fill, stroke)
            }
        }
        .with_stroke(stroke)
    }
}

#[cfg(feature = "gui")]
trait WithStroke {
    fn with_stroke(self, stroke: Stroke) -> Shape;
}

#[cfg(feature = "gui")]
impl WithStroke for Shape {
    fn with_stroke(self, stroke: Stroke) -> Shape {
        match self {
            Shape::Rect(mut r) => {
                r.stroke = stroke;
                r.stroke_kind = egui::StrokeKind::Inside;
                Shape::Rect(r)
            }
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IconFormat {
    WebP,
    Svg,
}

/// A component's picture. The bytes are shared, so copying an icon is cheap.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "IconFile", into = "IconFile")]
pub struct Icon {
    pub format: IconFormat,
    pub bytes: Arc<[u8]>,
    /// Of the bytes: names the picture for egui's image cache.
    hash: u64,
}

#[derive(Serialize, Deserialize)]
struct IconFile {
    format: IconFormat,
    bytes: Vec<u8>,
}

impl From<IconFile> for Icon {
    fn from(f: IconFile) -> Self {
        Icon::new(f.format, f.bytes)
    }
}

impl From<Icon> for IconFile {
    fn from(i: Icon) -> Self {
        IconFile { format: i.format, bytes: i.bytes.to_vec() }
    }
}

impl Icon {
    pub fn new(format: IconFormat, bytes: Vec<u8>) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        format.hash(&mut hasher);
        Self { format, bytes: bytes.into(), hash: hasher.finish() }
    }

    /// Where egui's image loaders find it; the extension picks the decoder.
    pub fn uri(&self) -> String {
        let ext = match self.format {
            IconFormat::WebP => "webp",
            IconFormat::Svg => "svg",
        };
        format!("bytes://shine-icon/{:016x}.{ext}", self.hash)
    }
}

/// A component's colour, outline and picture. All optional: by default a box
/// is a rectangle in a colour derived from its name, showing its initials.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appearance {
    pub color: Option<[u8; 3]>,
    pub shape: BoxShape,
    pub icon: Option<Icon>,
}

#[cfg(feature = "gui")]
fn is_svg(name: &str, bytes: &[u8]) -> bool {
    if name.to_lowercase().ends_with(".svg") {
        return true;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_lowercase();
    let head = head.trim_start_matches('\u{feff}').trim_start();
    (head.starts_with("<?xml") || head.starts_with("<svg") || head.starts_with("<!--")) && head.contains("<svg")
}

/// An icon from a picture file: an SVG as it is, any other picture scaled
/// down to fit [`ICON_PX`] and saved as WebP.
#[cfg(feature = "gui")]
pub fn icon_from_file(name: &str, bytes: &[u8]) -> Result<Icon, String> {
    if is_svg(name, bytes) {
        if bytes.len() > MAX_SVG_BYTES {
            return Err(format!("the SVG is {} KB; icons can be up to {} KB", bytes.len() / 1024, MAX_SVG_BYTES / 1024));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "the SVG isn't text".to_owned())?;
        if !text.contains("<svg") {
            return Err("not an SVG picture".into());
        }
        return Ok(Icon::new(IconFormat::Svg, bytes.to_vec()));
    }
    let picture = image::load_from_memory(bytes).map_err(|e| format!("can't read the picture: {e}"))?;
    let picture = if picture.width() > ICON_PX || picture.height() > ICON_PX {
        picture.resize(ICON_PX, ICON_PX, image::imageops::FilterType::Lanczos3)
    } else {
        picture
    };
    let rgba = picture.to_rgba8();
    let mut webp = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
        .encode(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("can't save the picture: {e}"))?;
    Ok(Icon::new(IconFormat::WebP, webp))
}

/// Draw `icon` fitted inside `rect`. Returns false until the picture is
/// decoded (or if it can't be), so the caller can draw something else.
#[cfg(feature = "gui")]
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: &Icon) -> bool {
    let ctx = painter.ctx();
    let uri = icon.uri();
    let known = egui::Id::unique(("shine-icon", icon.hash));
    if !ctx.data(|d| d.get_temp::<bool>(known).unwrap_or(false)) {
        ctx.include_bytes(uri.clone(), egui::load::Bytes::Shared(icon.bytes.clone()));
        ctx.data_mut(|d| d.insert_temp(known, true));
    }
    // SVGs are drawn at a size: round it up to a power of two so zooming
    // doesn't make a new texture every frame.
    let pixels = (rect.width().max(rect.height()) * ctx.pixels_per_point()).clamp(16.0, 1024.0);
    let side = pixels.log2().ceil().exp2() as u32;
    let hint = egui::SizeHint::Size { width: side, height: side, maintain_aspect_ratio: true };
    match ctx.try_load_texture(&uri, egui::TextureOptions::LINEAR, hint) {
        Ok(egui::load::TexturePoll::Ready { texture }) => {
            let size = texture.size;
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let fitted = Rect::from_center_size(rect.center(), size * scale);
            painter.image(texture.id, fitted, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            true
        }
        _ => false,
    }
}

/// The fill colour of a component's box.
#[cfg(feature = "gui")]
pub fn fill(appearance: &Appearance, name: &str) -> Color32 {
    match appearance.color {
        Some([r, g, b]) => Color32::from_rgb(r, g, b),
        None => super::ui_kit::accent(name),
    }
}

/// White or near-black, whichever reads better on `fill`.
#[cfg(feature = "gui")]
pub fn text_on(fill: Color32) -> Color32 {
    let luminance = 0.2126 * fill.r() as f32 + 0.7152 * fill.g() as f32 + 0.0722 * fill.b() as f32;
    if luminance > 160.0 { Color32::from_gray(25) } else { Color32::WHITE }
}

/// A small badge for lists: the picture if there is one, else the initials
/// on the box's colour and shape.
#[cfg(feature = "gui")]
pub fn paint_badge(painter: &egui::Painter, rect: Rect, appearance: &Appearance, name: &str, initials: &str) {
    let fill = fill(appearance, name);
    if let Some(icon) = &appearance.icon {
        let r = rect.shrink(1.0);
        painter.add(appearance.shape.shape(r, Color32::TRANSPARENT, Stroke::new(1.0, fill)));
        if paint_icon(painter, r.shrink(2.0), icon) {
            return;
        }
    }
    painter.add(appearance.shape.shape(rect.shrink(1.0), fill, Stroke::NONE));
    let size = if initials.chars().count() > 2 { rect.height() * 0.36 } else { rect.height() * 0.46 };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, initials, egui::FontId::proportional(size.max(6.0)), text_on(fill));
}

/// Picture files the icon picker offers.
pub const PICTURE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "svg"];

/// Whether a file name looks like a picture.
pub fn is_picture(name: &str) -> bool {
    let lower = name.to_lowercase();
    PICTURE_EXTENSIONS.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn pictures_are_shrunk_to_webp() {
        let big = png(1000, 500);
        let icon = icon_from_file("photo.png", &big).unwrap();
        assert_eq!(icon.format, IconFormat::WebP);
        let back = image::load_from_memory_with_format(&icon.bytes, image::ImageFormat::WebP).unwrap();
        assert_eq!((back.width(), back.height()), (128, 64), "fits 128 px, keeps the aspect ratio");
        assert!(icon.bytes.len() < big.len() / 4, "{} → {} bytes", big.len(), icon.bytes.len());

        // Small pictures keep their size.
        let small = icon_from_file("dot.png", &png(16, 16)).unwrap();
        let back = image::load_from_memory(&small.bytes).unwrap();
        assert_eq!((back.width(), back.height()), (16, 16));
    }

    #[test]
    fn svgs_are_kept_as_they_are() {
        let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><circle cx="5" cy="5" r="4"/></svg>"#;
        let icon = icon_from_file("ring.svg", svg).unwrap();
        assert_eq!(icon.format, IconFormat::Svg);
        assert_eq!(&*icon.bytes, &svg[..]);
        assert!(icon.uri().ends_with(".svg"));
        // Recognised by content too.
        assert_eq!(icon_from_file("ring", svg).unwrap().format, IconFormat::Svg);
        assert!(icon_from_file("x.svg", &vec![b' '; MAX_SVG_BYTES + 1]).is_err());
        assert!(icon_from_file("notes.txt", b"hello").is_err());
    }

    #[test]
    fn icons_round_trip_and_keep_their_name() {
        let icon = icon_from_file("dot.png", &png(8, 8)).unwrap();
        let bytes = postcard::to_allocvec(&icon).unwrap();
        let back: Icon = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, icon);
        assert_eq!(back.uri(), icon.uri());
    }
}
