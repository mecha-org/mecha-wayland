//! Decoders into `Bitmap`: PNG always, SVG under the `svg` feature.

use crate::{Bitmap, Error, Format};

impl Bitmap {
    /// Decode a PNG of any colour type and bit depth to `Rgba8`, straight
    /// alpha, no colour management. Grey becomes three equal channels;
    /// missing alpha becomes 255.
    pub fn from_png(bytes: &[u8]) -> Result<Bitmap, Error> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(Error::Png)?;
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
        let info = reader.next_frame(&mut buf).map_err(Error::Png)?;
        let n = (info.width * info.height) as usize;
        let mut pixels = Vec::with_capacity(n * 4);
        match info.color_type {
            png::ColorType::Rgba => pixels.extend_from_slice(&buf[..n * 4]),
            png::ColorType::Rgb => {
                for p in buf[..n * 3].chunks_exact(3) {
                    pixels.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
            }
            png::ColorType::Grayscale => {
                for &g in &buf[..n] {
                    pixels.extend_from_slice(&[g, g, g, 255]);
                }
            }
            png::ColorType::GrayscaleAlpha => {
                for p in buf[..n * 2].chunks_exact(2) {
                    pixels.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
                }
            }
            png::ColorType::Indexed => {
                unreachable!("normalize_to_color8 expands a palette")
            }
        }
        Ok(Bitmap {
            width: info.width,
            height: info.height,
            format: Format::Rgba8,
            pixels,
        })
    }

    /// Render an SVG with resvg at `master` pixels on its longer side,
    /// the aspect kept, and keep the alpha channel as `R8` coverage: an
    /// icon is a shape to be tinted. `Error::Svg` if it does not parse.
    #[cfg(feature = "svg")]
    pub fn from_svg(bytes: &[u8], master: u32) -> Result<Bitmap, Error> {
        let pixmap = render_svg(bytes, master)?;
        let pixels = pixmap.data().chunks_exact(4).map(|p| p[3]).collect();
        Ok(Bitmap {
            width: pixmap.width(),
            height: pixmap.height(),
            format: Format::R8,
            pixels,
        })
    }

    /// Render an SVG as `from_svg` does, but keep its colour: `Rgba8`
    /// with straight alpha (tiny-skia's premultiplied pixels are divided
    /// back out). `Error::Svg` if it does not parse.
    #[cfg(feature = "svg")]
    pub fn from_svg_rgba(bytes: &[u8], master: u32) -> Result<Bitmap, Error> {
        let pixmap = render_svg(bytes, master)?;
        let mut pixels = Vec::with_capacity(pixmap.data().len());
        for p in pixmap.data().chunks_exact(4) {
            let a = p[3] as u32;
            if a == 0 {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                for &c in &p[..3] {
                    pixels.push(((c as u32 * 255 + a / 2) / a).min(255) as u8);
                }
                pixels.push(a as u8);
            }
        }
        Ok(Bitmap {
            width: pixmap.width(),
            height: pixmap.height(),
            format: Format::Rgba8,
            pixels,
        })
    }
}

/// Rasterize an SVG to a premultiplied pixmap, `master` pixels on its
/// longer side, the aspect kept.
#[cfg(feature = "svg")]
fn render_svg(bytes: &[u8], master: u32) -> Result<resvg::tiny_skia::Pixmap, Error> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(Error::Svg)?;
    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    let scale = master as f32 / sw.max(sh);
    let width = ((sw * scale).round() as u32).max(1);
    let height = ((sh * scale).round() as u32).max(1);
    let mut pixmap = tiny_skia::Pixmap::new(width, height).expect("a positive size");
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, width, height);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn rgb_decodes_to_rgba_with_full_alpha() {
        let bytes = encode(2, 1, png::ColorType::Rgb, &[10, 20, 30, 40, 50, 60]);
        let b = Bitmap::from_png(&bytes).unwrap();
        assert_eq!((b.width, b.height, b.format), (2, 1, Format::Rgba8));
        assert_eq!(b.pixels, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn grayscale_alpha_keeps_its_alpha() {
        let bytes = encode(1, 2, png::ColorType::GrayscaleAlpha, &[100, 200, 7, 0]);
        let b = Bitmap::from_png(&bytes).unwrap();
        assert_eq!(b.pixels, vec![100, 100, 100, 200, 7, 7, 7, 0]);
    }

    #[test]
    fn grayscale_and_rgba_pass_through() {
        let g = Bitmap::from_png(&encode(1, 1, png::ColorType::Grayscale, &[9])).unwrap();
        assert_eq!(g.pixels, vec![9, 9, 9, 255]);
        let c = Bitmap::from_png(&encode(1, 1, png::ColorType::Rgba, &[1, 2, 3, 4])).unwrap();
        assert_eq!(c.pixels, vec![1, 2, 3, 4]);
    }

    #[test]
    fn not_a_png_is_an_error() {
        assert!(matches!(Bitmap::from_png(b"nope"), Err(Error::Png(_))));
    }

    #[cfg(feature = "svg")]
    #[test]
    fn svg_renders_coverage_at_the_master_size() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50"><rect x="0" y="0" width="50" height="50" fill="black"/></svg>"#;
        let b = Bitmap::from_svg(svg, 128).unwrap();
        assert_eq!((b.width, b.height, b.format), (128, 64, Format::R8));
        assert_eq!(b.pixels[10 * 128 + 10], 255, "inside the rect");
        assert_eq!(b.pixels[10 * 128 + 100], 0, "outside it");
        assert!(matches!(Bitmap::from_svg(b"<svg", 16), Err(Error::Svg(_))));
    }

    #[cfg(feature = "svg")]
    #[test]
    fn svg_rgba_keeps_colour_straight_alpha() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><rect x="0" y="0" width="10" height="10" fill="#ff0000" fill-opacity="0.5"/></svg>"##;
        let b = Bitmap::from_svg_rgba(svg, 10).unwrap();
        assert_eq!((b.width, b.height, b.format), (10, 10, Format::Rgba8));
        let at = (5 * 10 + 5) * 4;
        for (got, want) in b.pixels[at..at + 4].iter().zip([255u8, 0, 0, 128]) {
            assert!(got.abs_diff(want) <= 1, "{:?}", &b.pixels[at..at + 4]);
        }
    }
}
