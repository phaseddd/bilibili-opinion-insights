use std::sync::Arc;

use gpui::{RenderImage, Rgba};
use image::{Frame, ImageBuffer, RgbaImage};
use smallvec::SmallVec;

use crate::gui::materials::JellyMaterialToken;
use crate::gui::motion::JellyMotionSnapshot;

const BYTES_PER_PIXEL: usize = 4;

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellySwitchBitmapRequest {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) pixel_size: f32,
    pub(crate) material: JellyMaterialToken,
    pub(crate) motion: JellyMotionSnapshot,
    pub(crate) checked: bool,
    pub(crate) enabled: bool,
    pub(crate) active: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct JellySwitchBitmap {
    pub(crate) width: usize,
    pub(crate) height: usize,
    rgba: Vec<u8>,
}

impl JellySwitchBitmap {
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

pub(crate) fn rasterize_switch_material_bitmap(
    request: JellySwitchBitmapRequest,
) -> JellySwitchBitmap {
    let pixel_size = request.pixel_size.max(0.5);
    let width = ((request.width / pixel_size).ceil() as usize).max(1);
    let height = ((request.height / pixel_size).ceil() as usize).max(1);
    let mut rgba = vec![0; width * height * BYTES_PER_PIXEL];
    let palette = SwitchPalette::from_token(request.material);
    let opacity = if request.enabled { 1. } else { 0.45 };
    let pressure = request.motion.pressure.clamp(0., 1.);
    let rebound = request.motion.rebound.clamp(-1., 1.);
    let squash_x = request.motion.squash_x.clamp(0., 1.);
    let squash_z = request.motion.inner_lag.clamp(0., 1.);
    let rim_pressure = request.motion.rim_pressure.clamp(0., 1.);
    let aura = request.motion.aura.clamp(0., 1.);
    let active_wave = if request.active {
        (request.motion.gloss_phase * std::f32::consts::TAU)
            .sin()
            .mul_add(0.5, 0.5)
    } else {
        0.
    };

    let track_left = request.width * 0.04;
    let track_right = request.width * 0.96;
    let track_top = request.height * 0.18 + pressure * request.height * 0.04;
    let track_bottom = request.height * 0.82 - pressure * request.height * 0.03;
    let track_radius = (track_bottom - track_top) * 0.5;
    let thumb_diameter = request.height * 0.82 - pressure * request.height * 0.12 + squash_z * 2.;
    let thumb_radius = thumb_diameter * 0.5;
    let travel = (track_right - track_left - thumb_diameter).max(1.);
    let progress = if request.checked { 1. } else { 0. };
    let endpoint = if request.checked { 1. } else { -1. };
    let thumb_left =
        track_left + travel * progress + endpoint * (squash_x * 4. + rebound.max(0.) * 3.);
    let thumb_top = (request.height - thumb_diameter) * 0.5 + pressure * 1.2 - rebound * 0.8;

    for row in 0..height {
        let y = (row as f32 + 0.5) * pixel_size;
        for col in 0..width {
            let x = (col as f32 + 0.5) * pixel_size;
            let mut pixel = SwitchColor::transparent();

            let track_dist = sd_round_rect(
                x,
                y,
                track_left,
                track_top,
                track_right,
                track_bottom,
                track_radius,
            );
            let track_coverage = coverage(track_dist, pixel_size * 1.4);
            if track_coverage > 0.001 {
                let progress_t =
                    ((x - track_left) / (track_right - track_left).max(1.)).clamp(0., 1.);
                let vertical = (1. - y / request.height).clamp(0., 1.);
                let mut color = track_gradient(&palette, progress_t);
                color = color.overlay(palette.inner_glow, 0.18 + aura * 0.14 + active_wave * 0.08);
                color = color.overlay(
                    palette.contact_shadow,
                    (y / request.height).powf(1.9) * 0.12,
                );
                color = color.overlay(palette.specular, vertical.powf(2.4) * 0.18);
                color = color.overlay(
                    palette.rim,
                    (1. - track_dist.abs() / (pixel_size * 4.8))
                        .clamp(0., 1.)
                        .powf(1.3)
                        * (0.22 + rim_pressure * 0.16),
                );
                pixel = blend_over(
                    pixel,
                    color
                        .with_alpha(track_coverage * (0.82 + palette.track_alpha * 0.18) * opacity),
                );
            }

            let groove_dist = sd_round_rect(
                x,
                y,
                track_left + request.width * 0.05,
                track_top + request.height * 0.15,
                track_right - request.width * 0.05,
                track_bottom - request.height * 0.15,
                track_radius * 0.68,
            );
            let groove_coverage = coverage(groove_dist, pixel_size * 1.15);
            if groove_coverage > 0.001 {
                let groove = palette
                    .contact_shadow
                    .overlay(palette.track_mid, 0.24 + aura * 0.08)
                    .with_alpha(groove_coverage * 0.14 * opacity);
                pixel = blend_over(pixel, groove);
            }

            let thumb_dist = sd_round_rect(
                x,
                y,
                thumb_left,
                thumb_top,
                thumb_left + thumb_diameter,
                thumb_top + thumb_diameter,
                thumb_radius,
            );
            let thumb_coverage = coverage(thumb_dist, pixel_size * 1.15);
            if thumb_coverage > 0.001 {
                let progress_x = ((x - thumb_left) / thumb_diameter.max(1.)).clamp(0., 1.);
                let progress_y = ((y - thumb_top) / thumb_diameter.max(1.)).clamp(0., 1.);
                let rim = (1. - thumb_dist.abs() / (pixel_size * 5.4))
                    .clamp(0., 1.)
                    .powf(1.4);
                let mut color = thumb_gradient(&palette, progress_x, progress_y);
                color = color.overlay(palette.inner_glow, 0.18 + aura * 0.12 + active_wave * 0.06);
                color = color.overlay(palette.contact_shadow, progress_y.powf(1.9) * 0.16);
                color = color.overlay(palette.specular, (1. - progress_y).powf(2.6) * 0.16);
                color = color.overlay(palette.rim, rim * (0.38 + rim_pressure * 0.18));
                pixel = blend_over(
                    pixel,
                    color
                        .with_alpha(thumb_coverage * (0.86 + palette.thumb_alpha * 0.14) * opacity),
                );
            }

            let thumb_highlight = top_highlight_alpha(
                x,
                y,
                thumb_left,
                thumb_top,
                thumb_diameter,
                pressure,
                active_wave,
            );
            if thumb_highlight > 0.001 {
                pixel = blend_over(
                    pixel,
                    palette
                        .specular
                        .with_alpha(thumb_highlight * (0.25 + rim_pressure * 0.12) * opacity),
                );
            }

            let thumb_shadow = thumb_contact_shadow_alpha(
                x,
                y,
                ThumbShadowShape {
                    width: request.width,
                    height: request.height,
                    thumb_left,
                    thumb_top,
                    thumb_diameter,
                    contact: request.motion.contact,
                    aura: request.motion.aura,
                },
            ) * opacity;
            if thumb_shadow > 0.001 {
                pixel = blend_over(pixel, palette.contact_shadow.with_alpha(thumb_shadow));
            }

            let offset = (row * width + col) * BYTES_PER_PIXEL;
            rgba[offset] = to_byte(pixel.r);
            rgba[offset + 1] = to_byte(pixel.g);
            rgba[offset + 2] = to_byte(pixel.b);
            rgba[offset + 3] = to_byte(pixel.a);
        }
    }

    JellySwitchBitmap {
        width,
        height,
        rgba,
    }
}

fn track_gradient(palette: &SwitchPalette, progress: f32) -> SwitchColor {
    if progress < 0.5 {
        SwitchColor::mix(palette.track_start, palette.track_mid, progress / 0.5)
    } else {
        SwitchColor::mix(palette.track_mid, palette.track_end, (progress - 0.5) / 0.5)
    }
}

fn thumb_gradient(palette: &SwitchPalette, progress_x: f32, progress_y: f32) -> SwitchColor {
    let side_bias = (progress_x - 0.5).abs() * 2.;
    let vertical = 1. - progress_y;
    let mut color = SwitchColor::mix(
        palette.thumb_top,
        palette.thumb_mid,
        progress_y.clamp(0., 1.),
    );
    color = color.overlay(palette.thumb_end, side_bias * 0.22);
    color = color.overlay(palette.specular, vertical.powf(2.5) * 0.12);
    color
}

fn top_highlight_alpha(
    x: f32,
    y: f32,
    thumb_left: f32,
    thumb_top: f32,
    thumb_diameter: f32,
    pressure: f32,
    active_wave: f32,
) -> f32 {
    let left = thumb_left + thumb_diameter * 0.12;
    let right = thumb_left + thumb_diameter * (0.82 - pressure * 0.04);
    let top = thumb_top + thumb_diameter * (0.1 + active_wave * 0.03);
    let height = thumb_diameter * (0.16 - pressure * 0.03).max(0.08);
    if x < left || x > right {
        return 0.;
    }

    let y_t = (1. - ((y - top).abs() / height.max(1.))).clamp(0., 1.);
    let x_t = ((x - left) / (right - left).max(1.) * std::f32::consts::PI)
        .sin()
        .max(0.);
    y_t.powf(1.5) * x_t.powf(0.72)
}

#[derive(Clone, Copy, Debug)]
struct ThumbShadowShape {
    width: f32,
    height: f32,
    thumb_left: f32,
    thumb_top: f32,
    thumb_diameter: f32,
    contact: f32,
    aura: f32,
}

fn thumb_contact_shadow_alpha(x: f32, y: f32, shape: ThumbShadowShape) -> f32 {
    let center_x = shape.thumb_left + shape.thumb_diameter * 0.5;
    let rx = shape.width * (0.085 + shape.contact * 0.02);
    let ry = shape.height * (0.12 + shape.contact * 0.04);
    let dx = (x - center_x) / rx.max(1.);
    let dy = (y - (shape.thumb_top + shape.thumb_diameter + shape.height * 0.025)) / ry.max(1.);
    let falloff = (1. - (dx * dx + dy * dy)).clamp(0., 1.);
    falloff.powf(2.4) * (0.16 + shape.contact * 0.16 + shape.aura * 0.06)
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
struct SwitchPalette {
    track_start: SwitchColor,
    track_mid: SwitchColor,
    track_end: SwitchColor,
    track_alpha: f32,
    thumb_top: SwitchColor,
    thumb_mid: SwitchColor,
    thumb_end: SwitchColor,
    thumb_alpha: f32,
    rim: SwitchColor,
    specular: SwitchColor,
    inner_glow: SwitchColor,
    contact_shadow: SwitchColor,
}

impl SwitchPalette {
    fn from_token(token: JellyMaterialToken) -> Self {
        Self {
            track_start: SwitchColor::from(token.shell_start.to_rgb()),
            track_mid: SwitchColor::from(token.shell_mid.to_rgb()),
            track_end: SwitchColor::from(token.shell_end.to_rgb()),
            track_alpha: token.shell_alpha.clamp(0., 1.),
            thumb_top: SwitchColor::from(token.core_top.to_rgb()),
            thumb_mid: SwitchColor::from(token.core_bottom.to_rgb()),
            thumb_end: SwitchColor::from(token.shell_end.to_rgb()),
            thumb_alpha: token.core_alpha.clamp(0., 1.),
            rim: SwitchColor::from(token.rim.to_rgb()),
            specular: SwitchColor::from(token.specular.to_rgb()),
            inner_glow: SwitchColor::from(token.inner_glow.to_rgb()),
            contact_shadow: SwitchColor::from(token.contact_shadow.to_rgb()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SwitchColor {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl SwitchColor {
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

impl From<Rgba> for SwitchColor {
    fn from(value: Rgba) -> Self {
        Self {
            r: value.r.clamp(0., 1.),
            g: value.g.clamp(0., 1.),
            b: value.b.clamp(0., 1.),
            a: value.a.clamp(0., 1.),
        }
    }
}

fn blend_over(base: SwitchColor, layer: SwitchColor) -> SwitchColor {
    let alpha = layer.a + base.a * (1. - layer.a);
    if alpha <= 0.0001 {
        return SwitchColor::transparent();
    }

    SwitchColor {
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

    use super::{BYTES_PER_PIXEL, JellySwitchBitmapRequest, rasterize_switch_material_bitmap};

    #[test]
    fn switch_bitmap_has_pixels_and_transparent_edges() {
        let bitmap = sample_bitmap(false);

        assert!(bitmap.width > 20);
        assert!(bitmap.height > 16);
        assert_eq!(
            bitmap.rgba8().len(),
            bitmap.width * bitmap.height * BYTES_PER_PIXEL
        );
        assert!(bitmap.rgba8().chunks_exact(4).any(|pixel| pixel[3] > 200));
        assert!(bitmap.rgba8().chunks_exact(4).any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn switch_bitmap_changes_thumb_position_with_checked_state() {
        let off = sample_bitmap(false);
        let on = sample_bitmap(true);
        let off_center = thumb_center_of_mass(&off);
        let on_center = thumb_center_of_mass(&on);

        assert!(on_center > off_center);
    }

    #[test]
    fn pressed_switch_bitmap_changes_contact_shadow() {
        let idle = sample_bitmap_with_motion(false, false, JellyMotionSnapshot::default());
        let active = sample_bitmap_with_motion(
            true,
            true,
            JellyMotionSnapshot {
                pressure: 0.72,
                rebound: 0.2,
                squash_x: 0.45,
                squash_y: 0.3,
                rim_pressure: 0.52,
                gloss_phase: 0.38,
                inner_lag: 0.28,
                contact: 0.68,
                aura: 0.24,
                error_shake: 0.,
            },
        );
        let idle_bottom = average_alpha(&idle, idle.height * 2 / 3, idle.height);
        let active_bottom = average_alpha(&active, active.height * 2 / 3, active.height);

        assert!(active_bottom > idle_bottom);
    }

    #[test]
    fn switch_bitmap_can_create_gpui_render_image() {
        let bitmap = sample_bitmap(false);
        let image = bitmap
            .to_gpui_render_image()
            .expect("switch bitmap should fit into a GPUI RenderImage");
        let size = image.size(0);

        assert_eq!(u32::from(size.width), bitmap.width as u32);
        assert_eq!(u32::from(size.height), bitmap.height as u32);
    }

    fn sample_bitmap(checked: bool) -> super::JellySwitchBitmap {
        sample_bitmap_with_motion(
            checked,
            true,
            JellyMotionSnapshot {
                pressure: if checked { 0.72 } else { 0.12 },
                rebound: if checked { 0.18 } else { -0.08 },
                squash_x: if checked { 0.42 } else { 0.12 },
                squash_y: if checked { 0.28 } else { 0.08 },
                rim_pressure: if checked { 0.5 } else { 0.22 },
                gloss_phase: 0.36,
                inner_lag: if checked { 0.28 } else { 0.12 },
                contact: if checked { 0.68 } else { 0.2 },
                aura: 0.24,
                error_shake: 0.,
            },
        )
    }

    fn sample_bitmap_with_motion(
        checked: bool,
        enabled: bool,
        motion: JellyMotionSnapshot,
    ) -> super::JellySwitchBitmap {
        rasterize_switch_material_bitmap(JellySwitchBitmapRequest {
            width: 142.,
            height: 52.,
            pixel_size: 1.6,
            material: JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default()),
            motion,
            checked,
            enabled,
            active: checked,
        })
    }

    fn thumb_center_of_mass(bitmap: &super::JellySwitchBitmap) -> f32 {
        let mut total = 0.0;
        let mut alpha = 0.0;
        for row in 0..bitmap.height {
            for col in 0..bitmap.width {
                let offset = (row * bitmap.width + col) * BYTES_PER_PIXEL;
                let pixel = &bitmap.rgba8()[offset..offset + BYTES_PER_PIXEL];
                let a = pixel[3] as f32 / 255.;
                if a > 0.3 {
                    total += col as f32 * a;
                    alpha += a;
                }
            }
        }

        assert!(alpha > 0.);
        total / alpha
    }

    fn average_alpha(bitmap: &super::JellySwitchBitmap, start_row: usize, end_row: usize) -> f32 {
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
