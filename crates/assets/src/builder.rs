use anyhow::{Context, Result, bail};
use png::{BitDepth, ColorType};
use resvg::{tiny_skia, usvg};
use serde::Deserialize;
use std::{fs::File, io::BufWriter, path::Path};

#[derive(Deserialize)]
struct AtlasConfig {
    atlas: Vec<AtlasEntry>,
}

#[derive(Deserialize)]
struct AtlasEntry {
    name: String,
    #[serde(default)]
    sprite: Vec<SpriteEntry>,
    #[serde(default)]
    font: Vec<FontEntry>,
}

#[derive(Deserialize)]
struct SpriteEntry {
    name: String,
    path: String,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Deserialize)]
struct FontEntry {
    name: String,
    path: String,
    sizes: Vec<u32>,
}

struct SpriteImage {
    name: String,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

struct GlyphImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    bearing_x: f32,
    bearing_y: f32,
    advance: f32,
}

// Parallel to the crunch::Item<usize> list — tells us what each slot is.
enum Slot {
    Sprite(usize), // index into sprites
    Glyph(usize),  // index into glyphs
}

pub fn pack_atlas(toml_path: &Path, out_dir: &Path) -> Result<()> {
    let content = std::fs::read_to_string(toml_path)
        .with_context(|| format!("reading {}", toml_path.display()))?;

    println!("cargo:rerun-if-changed={}", toml_path.display());

    let config: AtlasConfig =
        toml::from_str(&content).with_context(|| format!("parsing {}", toml_path.display()))?;

    let base_dir = toml_path.parent().unwrap_or(Path::new("."));

    for (idx, atlas) in config.atlas.iter().enumerate() {
        pack_one_atlas(atlas, base_dir, out_dir, idx as u32)?;
    }

    Ok(())
}

fn pack_one_atlas(
    atlas: &AtlasEntry,
    base_dir: &Path,
    out_dir: &Path,
    atlas_id: u32,
) -> Result<()> {
    // ── Load sprites ──────────────────────────────────────────────────────────
    let mut sprites: Vec<SpriteImage> = Vec::new();
    for entry in &atlas.sprite {
        let path = base_dir.join(&entry.path);
        println!("cargo:rerun-if-changed={}", path.display());
        let mut img = load_sprite(&path, entry.width, entry.height)
            .with_context(|| format!("loading sprite '{}'", path.display()))?;
        img.name = entry.name.clone();
        sprites.push(img);
    }

    // ── Rasterize fonts ───────────────────────────────────────────────────────
    // `glyph_runs[i]` = flat array of 95 GlyphImages for font_entry[fi].sizes[si],
    //                   in the same iteration order as the nested loop below.
    let mut glyph_runs: Vec<Vec<GlyphImage>> = Vec::new(); // one entry per (font, size) pair
    let mut font_objs: Vec<fontdue::Font> = Vec::new();

    for font_entry in &atlas.font {
        let path = base_dir.join(&font_entry.path);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes =
            std::fs::read(&path).with_context(|| format!("reading font '{}'", path.display()))?;
        let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!("fontdue: {e}"))?;

        for &px in &font_entry.sizes {
            let mut run: Vec<GlyphImage> = Vec::with_capacity(95);
            for char_idx in 0u8..95 {
                let ch = (char_idx + 32) as char;
                let (metrics, pixels) = font.rasterize(ch, px as f32);
                run.push(GlyphImage {
                    width: metrics.width as u32,
                    height: metrics.height as u32,
                    pixels,
                    bearing_x: metrics.xmin as f32,
                    bearing_y: metrics.ymin as f32,
                    advance: metrics.advance_width,
                });
            }
            glyph_runs.push(run);
        }

        font_objs.push(font);
    }

    // ── Build item list for crunch ────────────────────────────────────────────
    let mut slots: Vec<Slot> = Vec::new();
    let mut pack_items: Vec<crunch::Item<usize>> = Vec::new();

    for (si, s) in sprites.iter().enumerate() {
        let idx = slots.len();
        slots.push(Slot::Sprite(si));
        pack_items.push(crunch::Item::new(
            idx,
            s.width as usize + 1,
            s.height as usize + 1,
            crunch::Rotation::None,
        ));
    }

    for (run_idx, run) in glyph_runs.iter().enumerate() {
        for (char_idx, g) in run.iter().enumerate() {
            let (w, h) = if g.width == 0 || g.height == 0 {
                (1, 1)
            } else {
                (g.width as usize, g.height as usize)
            };
            let idx = slots.len();
            slots.push(Slot::Glyph(run_idx * 95 + char_idx));
            pack_items.push(crunch::Item::new(idx, w + 1, h + 1, crunch::Rotation::None));
        }
    }

    // Flat glyph list for position lookup.
    let flat_glyphs: Vec<&GlyphImage> = glyph_runs.iter().flat_map(|r| r.iter()).collect();

    // ── Pack ──────────────────────────────────────────────────────────────────
    let mut atlas_size = 0usize;
    let mut packed = None;
    for &size in &[128usize, 256, 512, 1024, 2048, 4096] {
        if let Ok(p) = crunch::pack(crunch::Rect::of_size(size, size), pack_items.clone()) {
            atlas_size = size;
            packed = Some(p);
            break;
        }
    }
    let packed = packed
        .with_context(|| format!("atlas '{}': items don't fit within 4096×4096", atlas.name))?;

    // ── Blit pixels ───────────────────────────────────────────────────────────
    let mut atlas_pixels = vec![0u8; atlas_size * atlas_size];
    let mut sprite_rects = vec![(0u32, 0u32, 0u32, 0u32); sprites.len()];
    let mut glyph_pos = vec![(0u32, 0u32); flat_glyphs.len()]; // (atlas_x, atlas_y)

    for item in &packed {
        let rx = item.rect.x as usize;
        let ry = item.rect.y as usize;
        match &slots[item.data] {
            Slot::Sprite(si) => {
                let s = &sprites[*si];
                for row in 0..s.height as usize {
                    for col in 0..s.width as usize {
                        atlas_pixels[(ry + row) * atlas_size + (rx + col)] =
                            s.pixels[row * s.width as usize + col];
                    }
                }
                sprite_rects[*si] = (rx as u32, ry as u32, s.width, s.height);
            }
            Slot::Glyph(gi) => {
                let g = flat_glyphs[*gi];
                if g.width > 0 && g.height > 0 {
                    for row in 0..g.height as usize {
                        for col in 0..g.width as usize {
                            atlas_pixels[(ry + row) * atlas_size + (rx + col)] =
                                g.pixels[row * g.width as usize + col];
                        }
                    }
                }
                glyph_pos[*gi] = (rx as u32, ry as u32);
            }
        }
    }

    // ── Write PNG ─────────────────────────────────────────────────────────────
    let atlas_name = &atlas.name;
    let png_path = out_dir.join(format!("{atlas_name}.png"));
    {
        let w = BufWriter::new(File::create(&png_path)?);
        let mut enc = png::Encoder::new(w, atlas_size as u32, atlas_size as u32);
        enc.set_color(ColorType::Grayscale);
        enc.set_depth(BitDepth::Eight);
        enc.write_header()?.write_image_data(&atlas_pixels)?;
    }

    // ── Generate Rust source ──────────────────────────────────────────────────
    let rs_path = out_dir.join(format!("{atlas_name}_gen.rs"));
    let mut rs = String::from("// generated by assets::builder — do not edit\n");

    let atlas_upper = to_const_name(atlas_name);
    rs.push_str(&format!(
        "pub const {atlas_upper}: ::assets::AtlasData = ::assets::AtlasData {{\n    \
         id: ::assets::AtlasId({atlas_id}),\n    \
         png_bytes: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{atlas_name}.png\")),\n    \
         width: {atlas_size},\n    \
         height: {atlas_size},\n}};\n"
    ));

    for (i, sprite) in sprites.iter().enumerate() {
        let (x, y, w, h) = sprite_rects[i];
        let sprite_upper = format!("{atlas_upper}_{}", to_const_name(&sprite.name));
        rs.push_str(&format!(
            "pub const {sprite_upper}: ::assets::SpriteRegion = \
             ::assets::SpriteRegion {{ x: {x}.0, y: {y}.0, w: {w}.0, h: {h}.0 }};\n"
        ));
    }

    // One BakedFont const per (font_entry, size) pair.
    let mut run_idx = 0usize;
    let mut font_obj_idx = 0usize;
    for font_entry in &atlas.font {
        let font = &font_objs[font_obj_idx];
        for &px in &font_entry.sizes {
            let hm = font.horizontal_line_metrics(px as f32);
            let line_height = hm.map(|m| m.new_line_size).unwrap_or(px as f32 * 1.2);
            let ascent = hm.map(|m| m.ascent).unwrap_or(px as f32 * 0.8);
            let const_name = format!(
                "{atlas_upper}_FONT_{}_{px}",
                to_const_name(&font_entry.name)
            );

            let mut glyph_entries = String::new();
            for char_idx in 0..95usize {
                let gi = run_idx * 95 + char_idx;
                let g = flat_glyphs[gi];
                let (ax, ay) = glyph_pos[gi];
                glyph_entries.push_str(&format!(
                    "        ::assets::GlyphInfo {{ x: {ax}.0, y: {ay}.0, w: {w}, h: {h}, \
                     bearing_x: {bx}, bearing_y: {by}, advance: {adv} }},\n",
                    w = f32_lit(g.width as f32),
                    h = f32_lit(g.height as f32),
                    bx = f32_lit(g.bearing_x),
                    by = f32_lit(g.bearing_y),
                    adv = f32_lit(g.advance),
                ));
            }

            rs.push_str(&format!(
                "pub const {const_name}: ::assets::BakedFont = ::assets::BakedFont {{\n    \
                 size: {px}.0,\n    \
                 line_height: {lh},\n    \
                 ascent: {asc},\n    \
                 glyphs: [\n{glyph_entries}    ],\n}};\n",
                lh = f32_lit(line_height),
                asc = f32_lit(ascent),
            ));

            run_idx += 1;
        }
        font_obj_idx += 1;
    }

    std::fs::write(&rs_path, &rs)?;
    println!("cargo:warning=atlas '{atlas_name}' packed at {atlas_size}×{atlas_size}");

    Ok(())
}

fn to_const_name(s: &str) -> String {
    s.to_uppercase().replace('-', "_").replace(' ', "_")
}

fn f32_lit(v: f32) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

fn load_sprite(path: &Path, width: Option<u32>, height: Option<u32>) -> Result<SpriteImage> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "svg" => {
            let w = width.context("SVG sprite requires `width` in TOML")?;
            let h = height.context("SVG sprite requires `height` in TOML")?;
            let pixels = rasterize_svg(path, w, h)?;
            Ok(SpriteImage {
                name: String::new(),
                width: w,
                height: h,
                pixels,
            })
        }
        "png" => load_png(path),
        _ => bail!("unsupported sprite extension '{ext}'"),
    }
}

fn rasterize_svg(path: &Path, width: u32, height: u32) -> Result<Vec<u8>> {
    let data = std::fs::read_to_string(path)?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(&data, &opt)?;
    let size = tree.size();
    let scale_x = width as f32 / size.width();
    let scale_y = height as f32 / size.height();
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("failed to create pixmap {width}×{height}"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.pixels().iter().map(|p| p.alpha()).collect())
}

fn load_png(path: &Path) -> Result<SpriteImage> {
    let file = std::io::BufReader::new(File::open(path)?);
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .context("PNG has unknown size")?
    ];
    let info = reader.next_frame(&mut buf)?;
    let bytes = &buf[..info.buffer_size()];

    let pixels: Vec<u8> = match (info.color_type, info.bit_depth) {
        (ColorType::Grayscale, BitDepth::Eight) => bytes.to_vec(),
        (ColorType::GrayscaleAlpha, BitDepth::Eight) => bytes.chunks(2).map(|c| c[1]).collect(),
        (ColorType::Rgba, BitDepth::Eight) => bytes.chunks(4).map(|c| c[3]).collect(),
        (ColorType::Rgb, BitDepth::Eight) => bytes
            .chunks(3)
            .map(|c| (0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32) as u8)
            .collect(),
        _ => bail!(
            "unsupported PNG format {:?}/{:?}",
            info.color_type,
            info.bit_depth
        ),
    };

    Ok(SpriteImage {
        name: String::new(),
        width: info.width,
        height: info.height,
        pixels,
    })
}
