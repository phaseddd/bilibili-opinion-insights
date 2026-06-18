//! 离线 jelly-bake 烤的厚胶资产 → 运行时 GPUI `RenderImage` 贴图桥。
//!
//! 这是质感重构方案（`doc/claude-code/02`）运行时层的落点：离线 wgpu raymarch 烤出的
//! 透明背景横长厚胶 PNG，编译期嵌入二进制，首次使用时解码成 GPUI 期望的 BGRA 单帧
//! `RenderImage`，供按钮组件 `paint_image` 贴图。后续接入形变多帧 / sprite sheet 时在
//! 此扩展，最终取代 `rendering/jelly_image_cache.rs` 的 CPU 栅格化缓存。

use std::sync::{Arc, OnceLock};

use gpui::RenderImage;
use image::{Frame, ImageBuffer, ImageFormat, RgbaImage};
use smallvec::SmallVec;

// 各 tone 静止厚胶（横长透明背景），由 `tools/jelly-bake` 离线渲染、固化在
// `assets/jelly/`，编译期嵌入二进制，不依赖运行时工作目录。
const BUTTON_PRIMARY_REST_PNG: &[u8] =
    include_bytes!("../../../assets/jelly/button_primary_rest.png");
const BUTTON_CYAN_REST_PNG: &[u8] = include_bytes!("../../../assets/jelly/button_cyan_rest.png");
const BUTTON_WARNING_REST_PNG: &[u8] =
    include_bytes!("../../../assets/jelly/button_warning_rest.png");
const BUTTON_NEUTRAL_REST_PNG: &[u8] =
    include_bytes!("../../../assets/jelly/button_neutral_rest.png");

/// 按钮厚胶 tone（对应已烤的离线资产，判别值即资产数组下标）。
#[derive(Clone, Copy)]
pub enum ButtonAtlasTone {
    Primary = 0,
    Cyan = 1,
    Warning = 2,
    Neutral = 3,
}

/// 离线烤的横长厚胶按钮帧（已转 GPUI 期望的 BGRA 单帧 `RenderImage`）。
/// 进程级懒加载：首次取用时一次解码全部 tone，之后 `Arc::clone` 复用（解码失败的
/// tone 缓存为 `None`，调用方回退到原有渲染路径）。
pub fn button_rest(tone: ButtonAtlasTone) -> Option<Arc<RenderImage>> {
    fn cell() -> &'static OnceLock<[Option<Arc<RenderImage>>; 4]> {
        static CELL: OnceLock<[Option<Arc<RenderImage>>; 4]> = OnceLock::new();
        &CELL
    }
    let images = cell().get_or_init(|| {
        [
            decode_bgra_render_image(BUTTON_PRIMARY_REST_PNG),
            decode_bgra_render_image(BUTTON_CYAN_REST_PNG),
            decode_bgra_render_image(BUTTON_WARNING_REST_PNG),
            decode_bgra_render_image(BUTTON_NEUTRAL_REST_PNG),
        ]
    });
    images[tone as usize].clone()
}

/// PNG 字节 → RGBA8 → BGRA8（R/B swap，GPUI 内部按 BGRA 解读像素）→ 单帧 `RenderImage`。
fn decode_bgra_render_image(png_bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let rgba = image::load_from_memory_with_format(png_bytes, ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let (width, height) = rgba.dimensions();

    let mut bgra = rgba.into_raw();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let buffer: RgbaImage = ImageBuffer::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new(SmallVec::from_const([
        Frame::new(buffer),
    ]))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_button_tones_decode_to_render_image() {
        // 端到端闭环前提：内嵌的离线烤图资产必须能解码成 GPUI RenderImage。
        // 资产损坏、缺少 image 的 png feature 或 BGRA 构造回归都会让它失败。
        for tone in [
            ButtonAtlasTone::Primary,
            ButtonAtlasTone::Cyan,
            ButtonAtlasTone::Warning,
            ButtonAtlasTone::Neutral,
        ] {
            assert!(
                button_rest(tone).is_some(),
                "a baked jelly button tone failed to decode into a RenderImage"
            );
        }
    }
}
