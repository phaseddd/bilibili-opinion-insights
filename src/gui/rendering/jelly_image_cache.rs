use std::collections::VecDeque;
use std::sync::Arc;

use gpui::RenderImage;

use crate::gui::materials::JellyMaterialToken;
use crate::gui::motion::JellyProgressMotionSnapshot;
use crate::gui::rendering::jelly_bitmap::{
    JellyRibbonBitmapConfig, rasterize_ribbon_material_bitmap,
};
use crate::gui::rendering::jelly_geometry::{
    JellyRibbonChainShape, JellyRibbonShape, jelly_ribbon_profile,
};

const MAX_CACHED_IMAGES: usize = 96;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JellyProgressImagePhase {
    Idle,
    Validating,
    Running,
    Cancelling,
    Completed,
    Failed,
}

#[derive(Clone)]
pub(crate) struct JellyProgressImage {
    pub(crate) image: Arc<RenderImage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JellyProgressImageKey {
    width_px: u16,
    height_px: u16,
    progress_bucket: u8,
    pressure_bucket: u8,
    rebound_bucket: i8,
    contact_bucket: u8,
    phase_bucket: u8,
    phase: JellyProgressImagePhase,
}

#[derive(Default)]
pub(crate) struct JellyImageCache {
    entries: VecDeque<(JellyProgressImageKey, Arc<RenderImage>)>,
}

impl JellyImageCache {
    pub(crate) fn progress_image(
        &mut self,
        width: f32,
        height: f32,
        motion: JellyProgressMotionSnapshot,
        phase: JellyProgressImagePhase,
        material: JellyMaterialToken,
    ) -> Option<JellyProgressImage> {
        let key = JellyProgressImageKey::from_motion(width, height, motion, phase)?;
        if let Some(image) = self.lookup(key) {
            return Some(JellyProgressImage { image });
        }

        let render_width = key.width_px as f32;
        let render_height = key.height_px as f32;
        let fill = (key.progress_bucket as f32 / 100.).clamp(0.02, 1.);
        let shape = JellyRibbonChainShape {
            shape: JellyRibbonShape {
                origin_x: 0.,
                origin_y: 0.,
                width: render_width,
                height: render_height,
                progress: fill,
                pressure: key.pressure_bucket as f32 / 8.,
                rebound: key.rebound_bucket as f32 / 8.,
                compression: key.contact_bucket as f32 / 8.,
                phase: key.phase_bucket as f32 / 15. * std::f32::consts::TAU,
            },
            chain: motion.chain,
        };
        let profile = jelly_ribbon_profile(shape);
        let bitmap = rasterize_ribbon_material_bitmap(
            &profile,
            material,
            JellyRibbonBitmapConfig {
                pixel_size: 2.,
                padding: render_height * 0.28,
                opacity: 0.96,
                ..JellyRibbonBitmapConfig::default()
            },
        );
        let image = bitmap.to_gpui_render_image()?;

        self.insert(key, image.clone());
        Some(JellyProgressImage { image })
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn lookup(&mut self, key: JellyProgressImageKey) -> Option<Arc<RenderImage>> {
        let index = self
            .entries
            .iter()
            .position(|(entry_key, _)| *entry_key == key)?;
        let (_, image) = self.entries.remove(index)?;
        self.entries.push_back((key, image.clone()));
        Some(image)
    }

    fn insert(&mut self, key: JellyProgressImageKey, image: Arc<RenderImage>) {
        if self.entries.len() >= MAX_CACHED_IMAGES {
            self.entries.pop_front();
        }
        self.entries.push_back((key, image));
    }
}

impl JellyProgressImageKey {
    fn from_motion(
        width: f32,
        height: f32,
        motion: JellyProgressMotionSnapshot,
        phase: JellyProgressImagePhase,
    ) -> Option<Self> {
        let width_px = quantize_dimension(width, 4., 48., 1800.)?;
        let height_px = quantize_dimension(height, 2., 18., 120.)?;

        Some(Self {
            width_px,
            height_px,
            progress_bucket: quantize_unit(motion.display_percent / 100., 100) as u8,
            pressure_bucket: quantize_unit(motion.pressure, 8) as u8,
            rebound_bucket: quantize_signed(motion.rebound, 8),
            contact_bucket: quantize_unit(motion.contact, 8) as u8,
            phase_bucket: quantize_unit(motion.gloss_phase, 15) as u8,
            phase,
        })
    }
}

fn quantize_dimension(value: f32, step: f32, min: f32, max: f32) -> Option<u16> {
    if !value.is_finite() {
        return None;
    }

    let value = (value / step).round() * step;
    Some(value.clamp(min, max) as u16)
}

fn quantize_unit(value: f32, steps: u8) -> u16 {
    (value.clamp(0., 1.) * steps as f32).round() as u16
}

fn quantize_signed(value: f32, steps: i8) -> i8 {
    (value.clamp(-1., 1.) * steps as f32).round() as i8
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::gui::materials::{JellyMaterialToken, JellyTone};
    use crate::gui::motion::{JellyProgressChainSnapshot, JellyProgressMotionSnapshot};
    use crate::gui::theme::Palette;

    use super::{JellyImageCache, JellyProgressImagePhase};

    #[test]
    fn progress_image_cache_reuses_quantized_motion() {
        let mut cache = JellyImageCache::default();
        let material = JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default());
        let first = cache
            .progress_image(
                640.,
                46.,
                sample_motion(37.2),
                JellyProgressImagePhase::Running,
                material,
            )
            .expect("render image");
        let second = cache
            .progress_image(
                641.2,
                46.4,
                sample_motion(37.21),
                JellyProgressImagePhase::Running,
                material,
            )
            .expect("render image");

        assert_eq!(cache.len(), 1);
        assert!(Arc::ptr_eq(&first.image, &second.image));
    }

    #[test]
    fn progress_image_cache_splits_progress_buckets() {
        let mut cache = JellyImageCache::default();
        let material = JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default());

        cache.progress_image(
            640.,
            46.,
            sample_motion(21.),
            JellyProgressImagePhase::Running,
            material,
        );
        cache.progress_image(
            640.,
            46.,
            sample_motion(42.),
            JellyProgressImagePhase::Running,
            material,
        );

        assert_eq!(cache.len(), 2);
    }

    fn sample_motion(display_percent: f32) -> JellyProgressMotionSnapshot {
        JellyProgressMotionSnapshot {
            display_percent,
            target_percent: display_percent,
            velocity: 0.15,
            pulse: 0.2,
            pressure: 0.25,
            rebound: 0.1,
            squash_x: 0.1,
            squash_y: 0.05,
            rim_pressure: 0.2,
            gloss_phase: 0.44,
            inner_lag: 0.1,
            contact: 0.28,
            aura: 0.2,
            error_shake: 0.,
            chain: JellyProgressChainSnapshot { offsets: [0.; 9] },
        }
    }
}
