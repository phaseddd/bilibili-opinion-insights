use std::sync::Arc;

use gpui::{RenderImage, Rgba};
use image::{Frame, ImageBuffer, RgbaImage};
use smallvec::SmallVec;

use crate::gui::materials::JellyMaterialToken;
use crate::gui::motion::JellyMotionSnapshot;

const BYTES_PER_PIXEL: usize = 4;

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellySurfaceBitmapRequest {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) pixel_size: f32,
    pub(crate) material: JellyMaterialToken,
    pub(crate) motion: JellyMotionSnapshot,
    pub(crate) density: JellySurfaceDensity,
    pub(crate) active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JellySurfaceDensity {
    Event,
    Result,
}

#[derive(Clone, Debug)]
pub(crate) struct JellySurfaceBitmap {
    pub(crate) width: usize,
    pub(crate) height: usize,
    rgba: Vec<u8>,
}

impl JellySurfaceBitmap {
    #[cfg(test)]
    pub(crate) fn rgba8(&self) -> &[u8] {
        &self.rgba
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

pub(crate) fn rasterize_surface_material_bitmap(
    request: JellySurfaceBitmapRequest,
) -> JellySurfaceBitmap {
    let pixel_size = request.pixel_size.max(0.5);
    let width = ((request.width / pixel_size).ceil() as usize).max(1);
    let height = ((request.height / pixel_size).ceil() as usize).max(1);
    let mut rgba = vec![0; width * height * BYTES_PER_PIXEL];
    let palette = SurfacePalette::from_token(request.material);
    let opacity = match request.density {
        JellySurfaceDensity::Event => 0.92,
        JellySurfaceDensity::Result => 0.96,
    };
    let pressure = request.motion.pressure.clamp(0., 1.);
    let rebound = request.motion.rebound.clamp(-1., 1.);
    let rim_pressure = request.motion.rim_pressure.clamp(0., 1.);
    let contact = request.motion.contact.clamp(0., 1.);
    let aura = request.motion.aura.clamp(0., 1.);
    let active_wave = if request.active {
        (request.motion.gloss_phase * std::f32::consts::TAU)
            .sin()
            .mul_add(0.5, 0.5)
    } else {
        0.
    };
    let inset = match request.density {
        JellySurfaceDensity::Event => 1.5,
        JellySurfaceDensity::Result => 2.0,
    };
    let corner = match request.density {
        JellySurfaceDensity::Event => 9.5,
        JellySurfaceDensity::Result => 12.0,
    };

    let shell_left = inset;
    let shell_top = inset + pressure * request.height * 0.025 - rebound.max(0.) * 0.8;
    let shell_right = request.width - inset;
    let shell_bottom = request.height - inset - pressure * request.height * 0.012;
    let shell_radius = corner + pressure * 1.5 + active_wave * 1.0;
    let inner_left = shell_left + request.width * 0.025;
    let inner_right = shell_right - request.width * 0.025;
    let inner_top = shell_top + request.height * 0.21;
    let inner_bottom = shell_bottom - request.height * 0.18;

    for row in 0..height {
        let y = (row as f32 + 0.5) * pixel_size;
        for col in 0..width {
            let x = (col as f32 + 0.5) * pixel_size;
            let mut pixel = SurfaceColor::transparent();

            let shadow = surface_shadow_alpha(SurfaceShadowParams {
                x,
                y,
                width: request.width,
                height: request.height,
                shell_bottom,
                contact,
                aura,
                density: request.density,
            });
            if shadow > 0.001 {
                pixel = blend_over(pixel, palette.contact_shadow.with_alpha(shadow * opacity));
            }

            let shell_dist = sd_round_rect(
                x,
                y,
                shell_left,
                shell_top,
                shell_right,
                shell_bottom,
                shell_radius,
            );
            let shell_coverage = coverage(shell_dist, pixel_size * 1.3);
            if shell_coverage > 0.001 {
                let x_t = (x / request.width).clamp(0., 1.);
                let y_t = (y / request.height).clamp(0., 1.);
                let rim = (1. - shell_dist.abs() / (pixel_size * 4.8))
                    .clamp(0., 1.)
                    .powf(1.25);
                let mut color = surface_gradient(&palette, x_t);
                color = color.overlay(palette.inner_glow, 0.12 + aura * 0.13 + active_wave * 0.06);
                color = color.overlay(
                    palette.contact_shadow,
                    y_t.powf(1.8) * (0.08 + contact * 0.08),
                );
                color = color.overlay(palette.specular, (1. - y_t).powf(2.1) * 0.1);
                color = color.overlay(palette.rim, rim * (0.22 + rim_pressure * 0.16));
                pixel = blend_over(
                    pixel,
                    color
                        .with_alpha(shell_coverage * (0.54 + palette.shell_alpha * 0.14) * opacity),
                );
            }

            let contact_band = surface_contact_band_alpha(SurfaceContactBandParams {
                x,
                y,
                width: request.width,
                height: request.height,
                contact,
                aura,
                density: request.density,
                active: request.active,
            });
            if contact_band > 0.001 {
                pixel = blend_over(
                    pixel,
                    palette.contact_shadow.with_alpha(contact_band * opacity),
                );
            }

            let inner_dist = sd_round_rect(
                x,
                y,
                inner_left,
                inner_top,
                inner_right,
                inner_bottom,
                (inner_bottom - inner_top).max(1.) * 0.5,
            );
            let inner_coverage = coverage(inner_dist, pixel_size * 1.1);
            if inner_coverage > 0.001 {
                let inner = palette
                    .core_top
                    .overlay(palette.core_bottom, 0.25 + pressure * 0.08)
                    .with_alpha(inner_coverage * (0.08 + palette.core_alpha * 0.04) * opacity);
                pixel = blend_over(pixel, inner);
            }

            let highlight = surface_highlight_alpha(
                x,
                y,
                request.width,
                request.height,
                pressure,
                active_wave,
                request.density,
            );
            if highlight > 0.001 {
                pixel = blend_over(
                    pixel,
                    palette
                        .specular
                        .with_alpha(highlight * (0.18 + rim_pressure * 0.12) * opacity),
                );
            }

            let offset = (row * width + col) * BYTES_PER_PIXEL;
            rgba[offset] = to_byte(pixel.r);
            rgba[offset + 1] = to_byte(pixel.g);
            rgba[offset + 2] = to_byte(pixel.b);
            rgba[offset + 3] = to_byte(pixel.a);
        }
    }

    JellySurfaceBitmap {
        width,
        height,
        rgba,
    }
}

fn surface_gradient(palette: &SurfacePalette, progress: f32) -> SurfaceColor {
    if progress < 0.5 {
        SurfaceColor::mix(palette.shell_start, palette.shell_mid, progress / 0.5)
    } else {
        SurfaceColor::mix(palette.shell_mid, palette.shell_end, (progress - 0.5) / 0.5)
    }
}

#[derive(Clone, Copy, Debug)]
struct SurfaceShadowParams {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    shell_bottom: f32,
    contact: f32,
    aura: f32,
    density: JellySurfaceDensity,
}

fn surface_shadow_alpha(params: SurfaceShadowParams) -> f32 {
    let density_scale = match params.density {
        JellySurfaceDensity::Event => 0.82,
        JellySurfaceDensity::Result => 1.0,
    };
    let rx = params.width * (0.46 + params.aura * 0.035);
    let ry = params.height * (0.3 + params.contact * 0.08);
    let dx = (params.x - params.width * 0.5) / rx.max(1.);
    let dy = (params.y - (params.shell_bottom + params.height * 0.035)) / ry.max(1.);
    let falloff = (1. - (dx * dx + dy * dy)).clamp(0., 1.);
    falloff.powf(2.2) * (0.08 + params.contact * 0.12 + params.aura * 0.05) * density_scale
}

fn surface_highlight_alpha(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    pressure: f32,
    active_wave: f32,
    density: JellySurfaceDensity,
) -> f32 {
    let density_scale = match density {
        JellySurfaceDensity::Event => 0.72,
        JellySurfaceDensity::Result => 0.9,
    };
    let left = width * (0.035 + active_wave * 0.02);
    let right = width * (0.76 - pressure * 0.035);
    let top = height * (0.14 + pressure * 0.03);
    let h = height * (0.16 - pressure * 0.025).max(0.07);
    if x < left || x > right {
        return 0.;
    }

    let y_t = (1. - ((y - top).abs() / h.max(1.))).clamp(0., 1.);
    let x_t = ((x - left) / (right - left).max(1.) * std::f32::consts::PI)
        .sin()
        .max(0.);
    y_t.powf(1.55) * x_t.powf(0.5) * density_scale
}

#[derive(Clone, Copy, Debug)]
struct SurfaceContactBandParams {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    contact: f32,
    aura: f32,
    density: JellySurfaceDensity,
    active: bool,
}

fn surface_contact_band_alpha(params: SurfaceContactBandParams) -> f32 {
    let density_scale = match params.density {
        JellySurfaceDensity::Event => 0.88,
        JellySurfaceDensity::Result => 1.0,
    };
    let center_y =
        params.height * (0.74 + params.contact * 0.025 - if params.active { 0.01 } else { 0. });
    let half_height = params.height * (0.2 + params.aura * 0.03);
    let band_y = (1. - ((params.y - center_y).abs() / half_height.max(1.))).clamp(0., 1.);
    if band_y <= 0. {
        return 0.;
    }

    let half_width = params.width * (0.46 + params.aura * 0.04);
    let band_x = (1. - ((params.x - params.width * 0.5).abs() / half_width.max(1.))).clamp(0., 1.);
    let strength = (0.1 + params.contact * 0.24 + params.aura * 0.08)
        * if params.active { 1.14 } else { 0.72 };

    band_y.powf(2.0) * band_x.powf(1.3) * strength * density_scale
}

fn coverage(distance: f32, edge: f32) -> f32 {
    (0.5 - distance / edge.max(0.25)).clamp(0., 1.)
}

fn sd_round_rect(x: f32, y: f32, left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> f32 {
    let center_x = (left + right) * 0.5;
    let center_y = (top + bottom) * 0.5;
    let half_x = ((right - left) * 0.5 - radius).max(0.);
    let half_y = ((bottom - top) * 0.5 - radius).max(0.);
    let qx = (x - center_x).abs() - half_x;
    let qy = (y - center_y).abs() - half_y;
    let outside_x = qx.max(0.);
    let outside_y = qy.max(0.);
    let outside = (outside_x * outside_x + outside_y * outside_y).sqrt();
    let inside = qx.max(qy).min(0.);
    outside + inside - radius
}

#[derive(Clone, Copy, Debug)]
struct SurfacePalette {
    shell_start: SurfaceColor,
    shell_mid: SurfaceColor,
    shell_end: SurfaceColor,
    shell_alpha: f32,
    core_top: SurfaceColor,
    core_bottom: SurfaceColor,
    core_alpha: f32,
    rim: SurfaceColor,
    specular: SurfaceColor,
    inner_glow: SurfaceColor,
    contact_shadow: SurfaceColor,
}

impl SurfacePalette {
    fn from_token(token: JellyMaterialToken) -> Self {
        Self {
            shell_start: SurfaceColor::from(token.shell_start.to_rgb()),
            shell_mid: SurfaceColor::from(token.shell_mid.to_rgb()),
            shell_end: SurfaceColor::from(token.shell_end.to_rgb()),
            shell_alpha: token.shell_alpha.clamp(0., 1.),
            core_top: SurfaceColor::from(token.core_top.to_rgb()),
            core_bottom: SurfaceColor::from(token.core_bottom.to_rgb()),
            core_alpha: token.core_alpha.clamp(0., 1.),
            rim: SurfaceColor::from(token.rim.to_rgb()),
            specular: SurfaceColor::from(token.specular.to_rgb()),
            inner_glow: SurfaceColor::from(token.inner_glow.to_rgb()),
            contact_shadow: SurfaceColor::from(token.contact_shadow.to_rgb()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SurfaceColor {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl SurfaceColor {
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

impl From<Rgba> for SurfaceColor {
    fn from(value: Rgba) -> Self {
        Self {
            r: value.r.clamp(0., 1.),
            g: value.g.clamp(0., 1.),
            b: value.b.clamp(0., 1.),
            a: value.a.clamp(0., 1.),
        }
    }
}

fn blend_over(base: SurfaceColor, layer: SurfaceColor) -> SurfaceColor {
    let alpha = layer.a + base.a * (1. - layer.a);
    if alpha <= 0.0001 {
        return SurfaceColor::transparent();
    }

    SurfaceColor {
        r: (layer.r * layer.a + base.r * base.a * (1. - layer.a)) / alpha,
        g: (layer.g * layer.a + base.g * base.a * (1. - layer.a)) / alpha,
        b: (layer.b * layer.a + base.b * base.a * (1. - layer.a)) / alpha,
        a: alpha,
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
    use crate::gui::motion::JellyMotionSnapshot;
    use crate::gui::theme::Palette;

    use super::{
        BYTES_PER_PIXEL, JellySurfaceBitmapRequest, JellySurfaceDensity,
        rasterize_surface_material_bitmap,
    };

    #[test]
    fn surface_bitmap_has_pixels_and_transparent_edges() {
        let bitmap = sample_bitmap(JellySurfaceDensity::Event, false);

        assert!(bitmap.width > 80);
        assert!(bitmap.height > 20);
        assert_eq!(
            bitmap.rgba8().len(),
            bitmap.width * bitmap.height * BYTES_PER_PIXEL
        );
        assert!(bitmap.rgba8().chunks_exact(4).any(|pixel| pixel[3] > 120));
        assert!(bitmap.rgba8().chunks_exact(4).any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn surface_bitmap_preserves_horizontal_material_gradient() {
        let bitmap = sample_bitmap(JellySurfaceDensity::Event, false);
        let left = average_covered_rgb(&bitmap, 0, bitmap.width / 3);
        let right = average_covered_rgb(&bitmap, bitmap.width * 2 / 3, bitmap.width);

        assert_ne!(left, right);
    }

    #[test]
    fn active_surface_bitmap_increases_lower_contact_alpha() {
        let idle = sample_bitmap(JellySurfaceDensity::Result, false);
        let active = sample_bitmap(JellySurfaceDensity::Result, true);
        let idle_bottom = average_alpha(&idle, idle.height * 2 / 3, idle.height);
        let active_bottom = average_alpha(&active, active.height * 2 / 3, active.height);

        assert!(active_bottom > idle_bottom);
    }

    #[test]
    fn surface_bitmap_can_create_gpui_render_image() {
        let bitmap = sample_bitmap(JellySurfaceDensity::Event, false);
        let image = bitmap
            .to_gpui_render_image()
            .expect("surface bitmap should fit into a GPUI RenderImage");
        let size = image.size(0);

        assert_eq!(u32::from(size.width), bitmap.width as u32);
        assert_eq!(u32::from(size.height), bitmap.height as u32);
    }

    fn sample_bitmap(density: JellySurfaceDensity, active: bool) -> super::JellySurfaceBitmap {
        rasterize_surface_material_bitmap(JellySurfaceBitmapRequest {
            width: 860.,
            height: if matches!(density, JellySurfaceDensity::Event) {
                42.
            } else {
                72.
            },
            pixel_size: 2.,
            material: JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default()),
            motion: JellyMotionSnapshot {
                pressure: if active { 0.22 } else { 0. },
                rebound: if active { 0.14 } else { 0. },
                squash_x: 0.,
                squash_y: 0.,
                rim_pressure: if active { 0.42 } else { 0.16 },
                gloss_phase: if active { 0.34 } else { 0. },
                inner_lag: 0.,
                contact: if active { 0.48 } else { 0.16 },
                aura: if active { 0.36 } else { 0.12 },
                error_shake: 0.,
            },
            density,
            active,
        })
    }

    fn average_covered_rgb(
        bitmap: &super::JellySurfaceBitmap,
        start_col: usize,
        end_col: usize,
    ) -> (u8, u8, u8) {
        let mut total = (0_u32, 0_u32, 0_u32);
        let mut count = 0_u32;
        for row in 0..bitmap.height {
            for col in start_col..end_col {
                let offset = (row * bitmap.width + col) * BYTES_PER_PIXEL;
                let pixel = &bitmap.rgba8()[offset..offset + BYTES_PER_PIXEL];
                if pixel[3] > 70 {
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

    fn average_alpha(bitmap: &super::JellySurfaceBitmap, start_row: usize, end_row: usize) -> f32 {
        let mut total = 0_u32;
        let mut count = 0_u32;
        for row in start_row..end_row {
            for col in 0..bitmap.width {
                let offset = (row * bitmap.width + col) * BYTES_PER_PIXEL;
                total += bitmap.rgba8()[offset + 3] as u32;
                count += 1;
            }
        }

        assert!(count > 0);
        total as f32 / count as f32
    }
}
