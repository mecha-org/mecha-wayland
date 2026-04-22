use anyhow::Result;
use glow::HasContext;
use renderer::{
    ImageSurface, Renderer,
    commands::{ClearColor, DrawQuad, DrawRect},
};
use std::{fs::File, path::PathBuf};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 256;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if let Err(e) = render() {
        // Non-fatal: emit a warning and let the build continue.
        println!("cargo:warning=build.rs render failed: {e}");
    }
}

fn render() -> Result<()> {
    let mut renderer = Renderer::new()?;

    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);

    let surface = renderer.create_surface::<ImageSurface>(WIDTH, HEIGHT)?;

    // DrawQuad::on_enqueue pushes opaque inner rects into the DrawRect queue,
    // so DrawRect must be initialised even though we never send one directly.
    renderer.init_command_queue::<ClearColor>();
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    unsafe {
        renderer.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(surface.fbo));
        renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
    }

    renderer.send_command(ClearColor { r: 0.1, g: 0.1, b: 0.1, a: 1.0 });
    renderer.send_command(DrawQuad {
        color: (0.2, 0.5, 1.0, 1.0),
        border_color: (1.0, 1.0, 1.0, 1.0),
        origin: (64.0, 64.0, 0.0),
        size: (128.0, 128.0),
        border_radius: 16.0,
        border_thickness: 2.0,
    });

    renderer.process_command_queue::<ClearColor>();
    renderer.process_command_queue::<DrawRect>();
    renderer.process_command_queue::<DrawQuad>();

    unsafe { renderer.gl.finish() };

    // Read pixels back to CPU. GL origin is bottom-left.
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    unsafe {
        renderer.gl.read_pixels(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );
    }

    // Flip vertically: GL bottom-left → PNG top-left origin.
    let row = (WIDTH * 4) as usize;
    for y in 0..HEIGHT as usize / 2 {
        let top = y * row;
        let bot = (HEIGHT as usize - 1 - y) * row;
        for x in 0..row {
            pixels.swap(top + x, bot + x);
        }
    }

    let out_dir = std::env::var("OUT_DIR")?;
    let path = PathBuf::from(&out_dir).join("rendered_quad.png");

    let file = File::create(&path)?;
    let mut encoder = png::Encoder::new(file, WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&pixels)?;

    println!("cargo:warning=Rendered quad saved to: {}", path.display());

    renderer.destroy_surface(surface);
    Ok(())
}
