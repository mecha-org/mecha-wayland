use crate::asset_manager::Asset;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

/// A decoded RGBA8 image loaded from a PNG file.
///
/// Pass to [`Renderer::upload_image`](renderer::Renderer::upload_image) to create
/// a GPU texture, then use the returned [`GpuImage`](renderer::GpuImage) with the
/// [`Image`](renderer::Image) primitive.
pub struct ImageAsset {
    pub pixels: Vec<u8>, // RGBA8, row-major
    pub width:  u32,
    pub height: u32,
}

impl Asset for ImageAsset {
    type Params = PathBuf;

    fn load(path: Self::Params) -> Result<Self> {
        let file = std::fs::File::open(&path)
            .with_context(|| format!("failed to open image: {}", path.display()))?;
        let file = std::io::BufReader::new(file);

        let decoder = png::Decoder::new(file);
        let mut reader = decoder
            .read_info()
            .with_context(|| format!("failed to read PNG info: {}", path.display()))?;

        let mut pixels = vec![0u8; reader.output_buffer_size().unwrap()];
        let info = reader
            .next_frame(&mut pixels)
            .with_context(|| format!("failed to decode PNG frame: {}", path.display()))?;

        // Ensure we have RGBA8.
        let pixels = match info.color_type {
            png::ColorType::Rgba => {
                if info.bit_depth != png::BitDepth::Eight {
                    bail!("unsupported PNG bit depth: {:?}", info.bit_depth);
                }
                pixels[..info.buffer_size()].to_vec()
            }
            png::ColorType::Rgb => {
                // Expand RGB → RGBA by inserting full alpha.
                let rgb = &pixels[..info.buffer_size()];
                let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
                for chunk in rgb.chunks(3) {
                    rgba.extend_from_slice(chunk);
                    rgba.push(255);
                }
                rgba
            }
            other => bail!("unsupported PNG color type: {:?}", other),
        };

        Ok(Self { pixels, width: info.width, height: info.height })
    }
}
