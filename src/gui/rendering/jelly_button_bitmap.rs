use std::sync::Arc;

use gpui::{RenderImage, Rgba};
use image::{Frame, ImageBuffer, RgbaImage};
use smallvec::SmallVec;

use crate::gui::materials::JellyMaterialToken;
use crate::gui::motion::JellyMotionSnapshot;

const BYTES_PER_PIXEL: usize = 4;

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellyButtonBitmapRequest {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) pixel_size: f32,
    pub(crate) material: JellyMaterialToken,
    pub(crate) motion: JellyMotionSnapshot,
    pub(crate) enabled: bool,
    pub(crate) loading: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct JellyButtonBitmap {
    pub(crate) width: usize,
    pub(crate) height: usize,
    rgba: Vec<u8>,
}

impl JellyButtonBitmap {
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

pub(crate) fn rasterize_button_material_bitmap(
    request: JellyButtonBitmapRequest,
) -> JellyButtonBitmap {
    let pixel_size = request.pixel_size.max(0.5);
    let width = ((request.width / pixel_size).ceil() as usize).max(1);
    let height = ((request.height / pixel_size).ceil() as usize).max(1);
    let mut rgba = vec![0; width * height * BYTES_PER_PIXEL];
    let palette = ButtonPalette::from_token(request.material);
    let opacity = if request.enabled { 1. } else { 0.48 };
    let pressure = request.motion.pressure.clamp(0., 1.);
    let rebound = request.motion.rebound.clamp(-1., 1.);
    let squash_x = request.motion.squash_x.clamp(0., 1.);
    let squash_y = request.motion.squash_y.clamp(0., 1.);
    let contact = request.motion.contact.clamp(0., 1.);
    let aura = request.motion.aura.clamp(0., 1.);
    let loading = if request.loading {
        (request.motion.gloss_phase * std::f32::consts::TAU)
            .sin()
            .mul_add(0.5, 0.5)
    } else {
        0.
    };

    let shell_left = request.width * 0.025 - squash_x * request.width * 0.018;
    let shell_right = request.width * 0.975 + squash_x * request.width * 0.018;
    let shell_top = request.height * 0.08
        + pressure * request.height * 0.06
        + squash_y * request.height * 0.026
        - rebound.max(0.) * request.height * 0.035;
    let shell_bottom = request.height * 0.9 + pressure * request.height * 0.035
        - squash_y * request.height * 0.032;
    let shell_radius = (shell_bottom - shell_top) * (0.5 + rebound.max(0.) * 0.035);

    let trough_inset_x = request.width * (0.265 + pressure * 0.024);
    let trough_top = request.height * (0.335 + pressure * 0.035 + squash_y * 0.018);
    let trough_bottom = request.height * (0.695 + pressure * 0.025 - squash_y * 0.022);
    let core_inset_x = request.width * (0.365 + pressure * 0.028);
    let core_top = request.height * (0.438 + pressure * 0.038 + squash_y * 0.018);
    let core_bottom = request.height * (0.585 + pressure * 0.03 - squash_y * 0.018);

    for row in 0..height {
        let y = (row as f32 + 0.5) * pixel_size;
        for col in 0..width {
            let x = (col as f32 + 0.5) * pixel_size;
            let shell_dist = sd_round_rect(
                x,
                y,
                shell_left,
                shell_top,
                shell_right,
                shell_bottom,
                shell_radius,
            );
            let shell_coverage = coverage(shell_dist, pixel_size * 1.35);
            let mut pixel = ButtonColor::transparent();

            let shadow = contact_shadow_alpha(
                x,
                y,
                request.width,
                request.height,
                shell_bottom,
                contact,
                aura,
            ) * opacity;
            if shadow > 0.001 {
                pixel = blend_over(pixel, palette.contact_shadow.with_alpha(shadow));
            }

            if shell_coverage > 0.001 {
                let progress = (x / request.width).clamp(0., 1.);
                let vertical = (1. - y / request.height).clamp(0., 1.);
                let rim = (1. - shell_dist.abs() / (pixel_size * 5.5))
                    .clamp(0., 1.)
                    .powf(1.35);
                let top_light = vertical.powf(2.3) * (0.2 + loading * 0.14);
                let bottom_depth = (y / request.height).powf(2.1) * 0.2;
                let mut color = shell_gradient(&palette, progress);

                color = color.overlay(palette.inner_glow, 0.26 + aura * 0.2 + loading * 0.1);
                color = color.overlay(palette.contact_shadow, bottom_depth + pressure * 0.05);
                color = color.overlay(palette.specular, top_light);
                color = color.overlay(
                    palette.rim,
                    rim * (0.4 + request.motion.rim_pressure * 0.22),
                );

                pixel = blend_over(
                    pixel,
                    color
                        .with_alpha(shell_coverage * (0.84 + palette.shell_alpha * 0.16) * opacity),
                );
            }

            let trough_dist = sd_round_rect(
                x,
                y,
                trough_inset_x,
                trough_top,
                request.width - trough_inset_x,
                trough_bottom,
                (trough_bottom - trough_top) * 0.5,
            );
            let trough_coverage = coverage(trough_dist, pixel_size * 1.2);
            if trough_coverage > 0.001 {
                let trough = palette
                    .contact_shadow
                    .overlay(palette.shell_mid, 0.42 + aura * 0.14)
                    .overlay(palette.specular, 0.08 + request.motion.rim_pressure * 0.04)
                    .with_alpha(trough_coverage * (0.22 + pressure * 0.12) * opacity);
                pixel = blend_over(pixel, trough);
            }

            let core_dist = sd_round_rect(
                x,
                y,
                core_inset_x,
                core_top,
                request.width - core_inset_x,
                core_bottom,
                (core_bottom - core_top) * 0.5,
            );
            let core_coverage = coverage(core_dist, pixel_size);
            if core_coverage > 0.001 {
                let core_lift = (1. - y / request.height).clamp(0., 1.);
                let core = palette
                    .core_top
                    .overlay(palette.core_bottom, 0.42 + pressure * 0.18)
                    .overlay(palette.specular, core_lift.powf(2.8) * 0.1)
                    .with_alpha(core_coverage * (0.18 + palette.core_alpha * 0.08) * opacity);
                pixel = blend_over(pixel, core);
            }

            let highlight = top_highlight_alpha(x, y, request.width, request.height, pressure);
            if highlight > 0.001 {
                pixel = blend_over(
                    pixel,
                    palette.specular.with_alpha(
                        highlight * (0.26 + request.motion.rim_pressure * 0.12) * opacity,
                    ),
                );
            }

            let offset = (row * width + col) * BYTES_PER_PIXEL;
            rgba[offset] = to_byte(pixel.r);
            rgba[offset + 1] = to_byte(pixel.g);
            rgba[offset + 2] = to_byte(pixel.b);
            rgba[offset + 3] = to_byte(pixel.a);
        }
    }

    JellyButtonBitmap {
        width,
        height,
        rgba,
    }
}

fn shell_gradient(palette: &ButtonPalette, progress: f32) -> ButtonColor {
    if progress < 0.46 {
        ButtonColor::mix(palette.shell_start, palette.shell_mid, progress / 0.46)
    } else {
        ButtonColor::mix(
            palette.shell_mid,
            palette.shell_end,
            (progress - 0.46) / 0.54,
        )
    }
}

fn contact_shadow_alpha(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    shell_bottom: f32,
    contact: f32,
    aura: f32,
) -> f32 {
    let center_x = width * 0.5;
    let rx = width * (0.45 + contact * 0.04);
    let ry = height * (0.16 + contact * 0.05);
    let dx = (x - center_x) / rx.max(1.);
    let dy = (y - (shell_bottom + height * 0.03)) / ry.max(1.);
    let falloff = (1. - (dx * dx + dy * dy)).clamp(0., 1.);
    falloff.powf(2.3) * (0.18 + contact * 0.22 + aura * 0.08)
}

fn top_highlight_alpha(x: f32, y: f32, width: f32, height: f32, pressure: f32) -> f32 {
    let left = width * 0.22;
    let right = width * (0.72 - pressure * 0.04);
    let top = height * (0.13 + pressure * 0.04);
    let h = height * (0.045 - pressure * 0.012).max(0.026);
    if x < left || x > right {
        return 0.;
    }

    let y_t = (1. - ((y - top).abs() / h.max(1.))).clamp(0., 1.);
    let x_t = ((x - left) / (right - left).max(1.) * std::f32::consts::PI)
        .sin()
        .max(0.);
    y_t.powf(1.6) * x_t.powf(0.55)
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
struct ButtonPalette {
    shell_start: ButtonColor,
    shell_mid: ButtonColor,
    shell_end: ButtonColor,
    shell_alpha: f32,
    core_top: ButtonColor,
    core_bottom: ButtonColor,
    core_alpha: f32,
    rim: ButtonColor,
    specular: ButtonColor,
    inner_glow: ButtonColor,
    contact_shadow: ButtonColor,
}

impl ButtonPalette {
    fn from_token(token: JellyMaterialToken) -> Self {
        Self {
            shell_start: ButtonColor::from(token.shell_start.to_rgb()),
            shell_mid: ButtonColor::from(token.shell_mid.to_rgb()),
            shell_end: ButtonColor::from(token.shell_end.to_rgb()),
            shell_alpha: token.shell_alpha.clamp(0., 1.),
            core_top: ButtonColor::from(token.core_top.to_rgb()),
            core_bottom: ButtonColor::from(token.core_bottom.to_rgb()),
            core_alpha: token.core_alpha.clamp(0., 1.),
            rim: ButtonColor::from(token.rim.to_rgb()),
            specular: ButtonColor::from(token.specular.to_rgb()),
            inner_glow: ButtonColor::from(token.inner_glow.to_rgb()),
            contact_shadow: ButtonColor::from(token.contact_shadow.to_rgb()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ButtonColor {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl ButtonColor {
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

impl From<Rgba> for ButtonColor {
    fn from(value: Rgba) -> Self {
        Self {
            r: value.r.clamp(0., 1.),
            g: value.g.clamp(0., 1.),
            b: value.b.clamp(0., 1.),
            a: value.a.clamp(0., 1.),
        }
    }
}

fn blend_over(base: ButtonColor, layer: ButtonColor) -> ButtonColor {
    let alpha = layer.a + base.a * (1. - layer.a);
    if alpha <= 0.0001 {
        return ButtonColor::transparent();
    }

    ButtonColor {
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

    use super::{BYTES_PER_PIXEL, JellyButtonBitmapRequest, rasterize_button_material_bitmap};

    #[test]
    fn button_bitmap_has_pixels_and_transparent_edges() {
        let bitmap = sample_bitmap(false);

        assert!(bitmap.width > 32);
        assert!(bitmap.height > 16);
        assert_eq!(
            bitmap.rgba8().len(),
            bitmap.width * bitmap.height * BYTES_PER_PIXEL
        );
        assert!(bitmap.rgba8().chunks_exact(4).any(|pixel| pixel[3] > 210));
        assert!(bitmap.rgba8().chunks_exact(4).any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn button_bitmap_preserves_horizontal_material_gradient() {
        let bitmap = sample_bitmap(false);
        let left = average_covered_rgb(&bitmap, 0, bitmap.width / 3);
        let right = average_covered_rgb(&bitmap, bitmap.width * 2 / 3, bitmap.width);

        assert_ne!(left, right);
    }

    #[test]
    fn pressed_button_bitmap_changes_contact_shadow() {
        let idle = sample_bitmap(false);
        let pressed = sample_bitmap(true);
        let idle_bottom = average_alpha(&idle, idle.height * 2 / 3, idle.height);
        let pressed_bottom = average_alpha(&pressed, pressed.height * 2 / 3, pressed.height);

        assert!(pressed_bottom > idle_bottom);
    }

    #[test]
    fn squash_y_button_bitmap_compresses_covered_height() {
        let idle = sample_bitmap_with_motion(JellyMotionSnapshot {
            rim_pressure: 0.2,
            aura: 0.25,
            ..JellyMotionSnapshot::default()
        });
        let squashed = sample_bitmap_with_motion(JellyMotionSnapshot {
            squash_y: 0.9,
            rim_pressure: 0.2,
            aura: 0.25,
            ..JellyMotionSnapshot::default()
        });
        let idle_height = covered_height(&idle, 160);
        let squashed_height = covered_height(&squashed, 160);

        assert!(squashed_height < idle_height);
    }

    #[test]
    fn button_bitmap_keeps_inner_core_smaller_than_shell() {
        let bitmap = sample_bitmap(false);
        let shell_pixels = covered_pixel_count(&bitmap, 170);
        let core_pixels = pale_core_pixel_count(&bitmap);

        assert!(shell_pixels > 0);
        assert!(
            core_pixels as f32 / (shell_pixels as f32) < 0.08,
            "core coverage should stay visually subordinate to the outer jelly shell"
        );
    }

    #[test]
    fn button_bitmap_can_create_gpui_render_image() {
        let bitmap = sample_bitmap(false);
        let image = bitmap
            .to_gpui_render_image()
            .expect("button bitmap should fit into a GPUI RenderImage");
        let size = image.size(0);

        assert_eq!(u32::from(size.width), bitmap.width as u32);
        assert_eq!(u32::from(size.height), bitmap.height as u32);
    }

    fn sample_bitmap(pressed: bool) -> super::JellyButtonBitmap {
        sample_bitmap_with_motion(JellyMotionSnapshot {
            pressure: if pressed { 0.75 } else { 0. },
            rebound: if pressed { 0.2 } else { 0. },
            squash_x: if pressed { 0.45 } else { 0. },
            squash_y: if pressed { 0.35 } else { 0. },
            rim_pressure: if pressed { 0.5 } else { 0.2 },
            gloss_phase: 0.36,
            inner_lag: if pressed { 0.2 } else { 0. },
            contact: if pressed { 0.7 } else { 0.2 },
            aura: 0.25,
            error_shake: 0.,
        })
    }

    fn sample_bitmap_with_motion(motion: JellyMotionSnapshot) -> super::JellyButtonBitmap {
        rasterize_button_material_bitmap(JellyButtonBitmapRequest {
            width: 360.,
            height: 66.,
            pixel_size: 2.,
            material: JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default()),
            motion,
            enabled: true,
            loading: false,
        })
    }

    fn average_covered_rgb(
        bitmap: &super::JellyButtonBitmap,
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

    fn average_alpha(bitmap: &super::JellyButtonBitmap, start_row: usize, end_row: usize) -> f32 {
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

    fn covered_height(bitmap: &super::JellyButtonBitmap, alpha_threshold: u8) -> usize {
        let mut top = None;
        let mut bottom = None;
        for row in 0..bitmap.height {
            let covered = (0..bitmap.width).any(|col| {
                let offset = (row * bitmap.width + col) * BYTES_PER_PIXEL;
                bitmap.rgba8()[offset + 3] > alpha_threshold
            });
            if covered {
                top.get_or_insert(row);
                bottom = Some(row);
            }
        }

        let top = top.expect("bitmap should have covered pixels");
        let bottom = bottom.expect("bitmap should have covered pixels");
        bottom - top + 1
    }

    fn covered_pixel_count(bitmap: &super::JellyButtonBitmap, alpha_threshold: u8) -> usize {
        bitmap
            .rgba8()
            .chunks_exact(BYTES_PER_PIXEL)
            .filter(|pixel| pixel[3] > alpha_threshold)
            .count()
    }

    fn pale_core_pixel_count(bitmap: &super::JellyButtonBitmap) -> usize {
        bitmap
            .rgba8()
            .chunks_exact(BYTES_PER_PIXEL)
            .filter(|pixel| {
                pixel[3] > 54
                    && pixel[0] > 205
                    && pixel[1] > 215
                    && pixel[2] > 220
                    && pixel[0].abs_diff(pixel[1]) < 28
                    && pixel[1].abs_diff(pixel[2]) < 28
            })
            .count()
    }
}
