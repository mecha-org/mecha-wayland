use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TextureId(pub(crate) u32);

pub enum TextureFormat {
    R8,
}

pub(crate) struct GpuTexture {
    pub handle: glow::NativeTexture,
    pub width: u32,
    pub height: u32,
}

pub(crate) type TextureStore = HashMap<TextureId, GpuTexture>;
