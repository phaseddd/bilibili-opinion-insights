#![allow(dead_code)]

use std::sync::Arc;

use gpui::RenderImage;
use gpui::Rgba;
use image::{Frame, ImageBuffer, RgbaImage};
use smallvec::SmallVec;

use crate::gui::materials::JellyMaterialToken;
use crate::gui::rendering::jelly_geometry::{
    JellyRibbonProfile, rasterize_ribbon_alpha_mask, sample_ribbon_sdf,
};

const BYTES_PER_PIXEL: usize = 4;

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellyRibbonBitmapConfig {
    pub(crate) pixel_size: f32,
    pub(crate) padding: f32,
    pub(crate) opacity: f32,
    pub(crate) rim_strength: f32,
    pub(crate) specular_strength: f32,
    pub(crate) inner_glow_strength: f32,
    pub(crate) contact_shadow_strength: f32,
}

impl Default for JellyRibbonBitmapConfig {
    fn default() -> Self {
        Self {
            pixel_size: 2.,
            padding: 12.,
            opacity: 1.,
            rim_strength: 0.58,
            specular_strength: 0.54,
            inner_glow_strength: 0.28,
            contact_shadow_strength: 0.32,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct JellyRibbonBitmap {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) origin: (f32, f32),
    pub(crate) pixel_size: f32,
    rgba: Vec<u8>,
}

impl JellyRibbonBitmap {
    pub(crate) fn rgba8(&self) -> &[u8] {
        &self.rgba
    }

    pub(crate) fn into_rgba8(self) -> Vec<u8> {
        self.rgba
    }

    pub(crate) fn to_bgra8_for_gpui(&self) -> Vec<u8> {
        let mut bgra = self.rgba.clone();
        for pixel in bgra.chunks_exact_mut(BYTES_PER_PIXEL) {
            pixel.swap(0, 2);
        }
        bgra
    }

    pub(crate) fn to_gpui_render_image(&self) -> Option<Arc<RenderImage>> {
        let width = u32::try_from(self.width).ok()?;
        let height = u32::try_from(self.height).ok()?;
        let bgra = self.to_bgra8_for_gpui();
        let image: RgbaImage = ImageBuffer::from_raw(width, height, bgra)?;
        let frame = Frame::new(image);

        Some(Arc::new(RenderImage::new(SmallVec::from_const([frame]))))
    }
}

pub(crate) fn rasterize_ribbon_material_bitmap(
    profile: &JellyRibbonProfile,
    material: JellyMaterialToken,
    config: JellyRibbonBitmapConfig,
) -> JellyRibbonBitmap {
    let mask = rasterize_ribbon_alpha_mask(profile, config.pixel_size, config.padding);
    let mut rgba = vec![0; mask.width * mask.height * BYTES_PER_PIXEL];
    let palette = BitmapMaterialPalette::from_token(material);
    let pixel_size = mask.pixel_size.max(0.25);
    let padding = config.padding.max(pixel_size);

    for row in 0..mask.height {
        let y = mask.origin.1 + (row as f32 + 0.5) * pixel_size;
        for col in 0..mask.width {
            let x = mask.origin.0 + (col as f32 + 0.5) * pixel_size;
            let idx = row * mask.width + col;
            let sample = sample_ribbon_sdf(profile, x, y);
            let coverage = mask.alpha[idx] as f32 / 255.;
            let progress = mask.progress[idx] as f32 / 255.;
            let pixel = shade_ribbon_pixel(
                BitmapShadeSample {
                    signed_distance: sample.signed_distance,
                    normal: sample.normal,
                    progress,
                    coverage,
                },
                BitmapShadeContext {
                    palette: &palette,
                    config,
                    padding,
                    pixel_size,
                },
            );
            let offset = idx * BYTES_PER_PIXEL;
            rgba[offset] = to_byte(pixel.r);
            rgba[offset + 1] = to_byte(pixel.g);
            rgba[offset + 2] = to_byte(pixel.b);
            rgba[offset + 3] = to_byte(pixel.a);
        }
    }

    JellyRibbonBitmap {
        width: mask.width,
        height: mask.height,
        origin: mask.origin,
        pixel_size: mask.pixel_size,
        rgba,
    }
}

#[derive(Clone, Copy, Debug)]
struct BitmapShadeSample {
    signed_distance: f32,
    normal: (f32, f32),
    progress: f32,
    coverage: f32,
}

#[derive(Clone, Copy, Debug)]
struct BitmapShadeContext<'a> {
    palette: &'a BitmapMaterialPalette,
    config: JellyRibbonBitmapConfig,
    padding: f32,
    pixel_size: f32,
}

fn shade_ribbon_pixel(sample: BitmapShadeSample, context: BitmapShadeContext<'_>) -> BitmapColor {
    let signed_distance = sample.signed_distance;
    let normal = sample.normal;
    let progress = sample.progress;
    let coverage = sample.coverage;
    let palette = context.palette;
    let config = context.config;
    let padding = context.padding;
    let pixel_size = context.pixel_size;
    let opacity = config.opacity.clamp(0., 1.);
    if coverage <= 0. && signed_distance >= padding {
        return BitmapColor::transparent();
    }

    let edge_width = (pixel_size * 2.8).max(1.);
    let edge = (1. - signed_distance.abs() / edge_width).clamp(0., 1.);
    let inner_depth = (-signed_distance / (edge_width * 2.5)).clamp(0., 1.);
    let top_light = (-normal.1).clamp(0., 1.);
    let lower_contact = normal.1.clamp(0., 1.);
    let side_light = normal.0.abs().clamp(0., 1.);
    let progress_bloom = (std::f32::consts::PI * progress.clamp(0., 1.))
        .sin()
        .max(0.);

    let base = if progress < 0.56 {
        BitmapColor::mix(palette.shell_start, palette.shell_mid, progress / 0.56)
    } else {
        BitmapColor::mix(
            palette.shell_mid,
            palette.shell_end,
            (progress - 0.56) / 0.44,
        )
    };
    let specular = top_light.powf(2.15)
        * (0.22 + progress_bloom * 0.2 + edge * 0.22)
        * config.specular_strength;
    let rim = edge * (0.24 + top_light * 0.42 + side_light * 0.18) * config.rim_strength;
    let glow = inner_depth * (1. - edge * 0.35) * config.inner_glow_strength;
    let lower_shadow = lower_contact * (0.18 + inner_depth * 0.1) * config.contact_shadow_strength;
    let outside_shadow = if coverage < 0.02 && signed_distance > 0. {
        ((padding - signed_distance) / padding)
            .clamp(0., 1.)
            .powf(2.1)
            * lower_contact
            * config.contact_shadow_strength
            * 0.68
    } else {
        0.
    };

    let mut color = base;
    color = color.overlay(palette.inner_glow, glow);
    color = color.overlay(palette.specular, specular);
    color = color.overlay(palette.rim, rim);
    color = color.overlay(palette.contact_shadow, lower_shadow);

    let shape_alpha = coverage
        * (0.62 + palette.shell_alpha * 0.3 + edge * 0.1 + progress_bloom * 0.04)
        * opacity;
    let shadow_alpha = outside_shadow * opacity;
    if shape_alpha <= 0.001 && shadow_alpha > 0. {
        return palette.contact_shadow.with_alpha(shadow_alpha);
    }

    color.with_alpha(shape_alpha.max(shadow_alpha).clamp(0., 1.))
}

#[derive(Clone, Copy, Debug)]
struct BitmapMaterialPalette {
    shell_start: BitmapColor,
    shell_mid: BitmapColor,
    shell_end: BitmapColor,
    shell_alpha: f32,
    rim: BitmapColor,
    specular: BitmapColor,
    inner_glow: BitmapColor,
    contact_shadow: BitmapColor,
}

impl BitmapMaterialPalette {
    fn from_token(token: JellyMaterialToken) -> Self {
        Self {
            shell_start: BitmapColor::from(token.shell_start.to_rgb()),
            shell_mid: BitmapColor::from(token.shell_mid.to_rgb()),
            shell_end: BitmapColor::from(token.shell_end.to_rgb()),
            shell_alpha: token.shell_alpha.clamp(0., 1.),
            rim: BitmapColor::from(token.rim.to_rgb()),
            specular: BitmapColor::from(token.specular.to_rgb()),
            inner_glow: BitmapColor::from(token.inner_glow.to_rgb()),
            contact_shadow: BitmapColor::from(token.contact_shadow.to_rgb()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct BitmapColor {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl BitmapColor {
    fn transparent() -> Self {
        Self {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.,
        }
    }

    fn mix(start: Self, end: Self, t: f32) -> Self {
        let t = t.clamp(0., 1.);
        Self {
            r: lerp(start.r, end.r, t),
            g: lerp(start.g, end.g, t),
            b: lerp(start.b, end.b, t),
            a: lerp(start.a, end.a, t),
        }
    }

    fn overlay(self, layer: Self, strength: f32) -> Self {
        let alpha = (layer.a * strength).clamp(0., 1.);
        Self {
            r: lerp(self.r, layer.r, alpha),
            g: lerp(self.g, layer.g, alpha),
            b: lerp(self.b, layer.b, alpha),
            a: self.a,
        }
    }

    fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: alpha.clamp(0., 1.),
            ..self
        }
    }
}

impl From<Rgba> for BitmapColor {
    fn from(value: Rgba) -> Self {
        Self {
            r: value.r.clamp(0., 1.),
            g: value.g.clamp(0., 1.),
            b: value.b.clamp(0., 1.),
            a: value.a.clamp(0., 1.),
        }
    }
}

fn to_byte(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}

fn lerp(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t
}

#[cfg(test)]
mod tests {
    use crate::gui::materials::{JellyMaterialToken, JellyTone};
    use crate::gui::motion::{JellyProgressChainSnapshot, PROGRESS_CHAIN_POINTS};
    use crate::gui::rendering::jelly_geometry::{
        JellyRibbonChainShape, JellyRibbonShape, jelly_ribbon_profile,
    };
    use crate::gui::theme::Palette;

    use super::{BYTES_PER_PIXEL, JellyRibbonBitmapConfig, rasterize_ribbon_material_bitmap};

    #[test]
    fn material_bitmap_has_rgba_pixels_and_transparent_padding() {
        let bitmap = sample_bitmap();

        assert!(bitmap.width > 16);
        assert!(bitmap.height > 8);
        assert_eq!(
            bitmap.rgba8().len(),
            bitmap.width * bitmap.height * BYTES_PER_PIXEL
        );
        assert!(bitmap.origin.0.is_finite());
        assert!(bitmap.origin.1.is_finite());
        assert!(bitmap.pixel_size > 0.);
        assert!(bitmap.rgba8().chunks_exact(4).any(|pixel| pixel[3] > 220));
        assert!(bitmap.rgba8().chunks_exact(4).any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn material_bitmap_preserves_progress_color_gradient() {
        let bitmap = sample_bitmap();
        let left = average_covered_rgb(&bitmap, 0, bitmap.width / 3);
        let right = average_covered_rgb(&bitmap, bitmap.width * 2 / 3, bitmap.width);

        assert_ne!(left, right);
        assert!(
            (left.0 as i16 - right.0 as i16).abs() > 4
                || (left.2 as i16 - right.2 as i16).abs() > 4
        );
    }

    #[test]
    fn material_bitmap_adds_top_specular_lift() {
        let bitmap = sample_bitmap();
        let top = average_covered_luma(&bitmap, 0, bitmap.height / 2);
        let bottom = average_covered_luma(&bitmap, bitmap.height / 2, bitmap.height);

        assert!(top > bottom);
    }

    #[test]
    fn material_bitmap_can_export_bgra_for_gpui_image_bridge() {
        let bitmap = sample_bitmap();
        let bgra = bitmap.to_bgra8_for_gpui();
        let rgba = bitmap.rgba8();
        let pixel_index = rgba
            .chunks_exact(4)
            .position(|pixel| pixel[3] > 0 && pixel[0] != pixel[2])
            .expect("sample should contain a colored non-gray pixel");
        let offset = pixel_index * 4;

        assert_eq!(bgra.len(), rgba.len());
        assert_eq!(bgra[offset], rgba[offset + 2]);
        assert_eq!(bgra[offset + 1], rgba[offset + 1]);
        assert_eq!(bgra[offset + 2], rgba[offset]);
        assert_eq!(bgra[offset + 3], rgba[offset + 3]);
    }

    #[test]
    fn material_bitmap_can_create_gpui_render_image() {
        let bitmap = sample_bitmap();
        let image = bitmap
            .to_gpui_render_image()
            .expect("bitmap dimensions should fit into a GPUI RenderImage");
        let size = image.size(0);

        assert_eq!(u32::from(size.width), bitmap.width as u32);
        assert_eq!(u32::from(size.height), bitmap.height as u32);
        assert_eq!(image.as_bytes(0).unwrap().len(), bitmap.rgba8().len());
    }

    fn sample_bitmap() -> super::JellyRibbonBitmap {
        let profile = jelly_ribbon_profile(JellyRibbonChainShape {
            shape: JellyRibbonShape {
                origin_x: 10.,
                origin_y: 10.,
                width: 420.,
                height: 42.,
                progress: 0.62,
                pressure: 0.5,
                rebound: 0.34,
                compression: 0.72,
                phase: 1.4,
            },
            chain: sample_chain(),
        });
        let material = JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default());

        rasterize_ribbon_material_bitmap(
            &profile,
            material,
            JellyRibbonBitmapConfig {
                pixel_size: 3.,
                padding: 12.,
                ..JellyRibbonBitmapConfig::default()
            },
        )
    }

    fn sample_chain() -> JellyProgressChainSnapshot {
        let mut offsets = [0.; PROGRESS_CHAIN_POINTS];
        for (idx, offset) in offsets.iter_mut().enumerate() {
            let t = idx as f32 / (PROGRESS_CHAIN_POINTS - 1) as f32;
            *offset = -0.18 * (std::f32::consts::PI * t).sin().max(0.);
        }
        JellyProgressChainSnapshot::from_offsets(offsets)
    }

    fn average_covered_rgb(
        bitmap: &super::JellyRibbonBitmap,
        start_col: usize,
        end_col: usize,
    ) -> (u8, u8, u8) {
        let mut total = (0_u32, 0_u32, 0_u32);
        let mut count = 0_u32;
        for row in 0..bitmap.height {
            for col in start_col..end_col {
                let offset = (row * bitmap.width + col) * BYTES_PER_PIXEL;
                let pixel = &bitmap.rgba8()[offset..offset + BYTES_PER_PIXEL];
                if pixel[3] > 128 {
                    total.0 += pixel[0] as u32;
                    total.1 += pixel[1] as u32;
                    total.2 += pixel[2] as u32;
                    count += 1;
                }
            }
        }

        assert!(count > 0);
        (
            (total.0 / count) as u8,
            (total.1 / count) as u8,
            (total.2 / count) as u8,
        )
    }

    fn average_covered_luma(
        bitmap: &super::JellyRibbonBitmap,
        start_row: usize,
        end_row: usize,
    ) -> f32 {
        let mut total = 0.;
        let mut count = 0.;
        for row in start_row..end_row {
            for col in 0..bitmap.width {
                let offset = (row * bitmap.width + col) * BYTES_PER_PIXEL;
                let pixel = &bitmap.rgba8()[offset..offset + BYTES_PER_PIXEL];
                if pixel[3] > 128 {
                    total += pixel[0] as f32 * 0.2126
                        + pixel[1] as f32 * 0.7152
                        + pixel[2] as f32 * 0.0722;
                    count += 1.;
                }
            }
        }

        assert!(count > 0.);
        total / count
    }
}
