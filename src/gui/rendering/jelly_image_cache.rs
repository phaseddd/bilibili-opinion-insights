use std::collections::VecDeque;
use std::sync::Arc;

use gpui::RenderImage;

use crate::gui::materials::{JellyMaterialToken, JellyTone};
use crate::gui::motion::{JellyMotionSnapshot, JellyProgressMotionSnapshot};
use crate::gui::rendering::jelly_bitmap::{
    JellyRibbonBitmapConfig, rasterize_ribbon_material_bitmap,
};
use crate::gui::rendering::jelly_button_bitmap::{
    JellyButtonBitmapRequest, rasterize_button_material_bitmap,
};
use crate::gui::rendering::jelly_geometry::{
    JellyRibbonChainShape, JellyRibbonShape, jelly_ribbon_profile,
};
use crate::gui::rendering::jelly_switch_bitmap::{
    JellySwitchBitmapRequest, rasterize_switch_material_bitmap,
};

const MAX_CACHED_IMAGES: usize = 96;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JellyProgressImageQuality {
    Main,
    Lane,
}

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

#[derive(Clone)]
pub(crate) struct JellyButtonImage {
    pub(crate) image: Arc<RenderImage>,
}

#[derive(Clone)]
pub(crate) struct JellySwitchImage {
    pub(crate) image: Arc<RenderImage>,
}

#[derive(Clone, Copy)]
pub(crate) struct JellyProgressImageRequest {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) quality: JellyProgressImageQuality,
    pub(crate) motion: JellyProgressMotionSnapshot,
    pub(crate) phase: JellyProgressImagePhase,
    pub(crate) tone: JellyTone,
    pub(crate) material: JellyMaterialToken,
}

#[derive(Clone, Copy)]
pub(crate) struct JellyButtonImageRequest {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) motion: JellyMotionSnapshot,
    pub(crate) tone: JellyTone,
    pub(crate) material: JellyMaterialToken,
    pub(crate) enabled: bool,
    pub(crate) loading: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct JellySwitchImageRequest {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) motion: JellyMotionSnapshot,
    pub(crate) tone: JellyTone,
    pub(crate) material: JellyMaterialToken,
    pub(crate) checked: bool,
    pub(crate) enabled: bool,
    pub(crate) active: bool,
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
    tone: JellyTone,
    quality: JellyProgressImageQuality,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JellyButtonImageKey {
    width_px: u16,
    height_px: u16,
    pressure_bucket: u8,
    rebound_bucket: i8,
    squash_x_bucket: u8,
    squash_y_bucket: u8,
    rim_bucket: u8,
    gloss_bucket: u8,
    inner_bucket: u8,
    contact_bucket: u8,
    aura_bucket: u8,
    tone: JellyTone,
    enabled: bool,
    loading: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JellySwitchImageKey {
    width_px: u16,
    height_px: u16,
    pressure_bucket: u8,
    rebound_bucket: i8,
    squash_x_bucket: u8,
    squash_y_bucket: u8,
    rim_bucket: u8,
    gloss_bucket: u8,
    inner_bucket: u8,
    contact_bucket: u8,
    aura_bucket: u8,
    tone: JellyTone,
    checked: bool,
    enabled: bool,
    active: bool,
}

#[derive(Default)]
pub(crate) struct JellyImageCache {
    progress_entries: VecDeque<(JellyProgressImageKey, Arc<RenderImage>)>,
    button_entries: VecDeque<(JellyButtonImageKey, Arc<RenderImage>)>,
    switch_entries: VecDeque<(JellySwitchImageKey, Arc<RenderImage>)>,
}

impl JellyImageCache {
    pub(crate) fn progress_image(
        &mut self,
        request: JellyProgressImageRequest,
    ) -> Option<JellyProgressImage> {
        let key = JellyProgressImageKey::from_motion(request)?;
        if let Some(image) = lookup_image(&mut self.progress_entries, key) {
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
            chain: request.motion.chain,
        };
        let profile = jelly_ribbon_profile(shape);
        let render_config = request.quality.render_config(render_height);
        let bitmap = rasterize_ribbon_material_bitmap(
            &profile,
            request.material,
            JellyRibbonBitmapConfig {
                pixel_size: render_config.pixel_size,
                padding: render_config.padding,
                opacity: render_config.opacity,
                ..JellyRibbonBitmapConfig::default()
            },
        );
        let image = bitmap.to_gpui_render_image()?;

        insert_image(&mut self.progress_entries, key, image.clone());
        Some(JellyProgressImage { image })
    }

    pub(crate) fn button_image(
        &mut self,
        request: JellyButtonImageRequest,
    ) -> Option<JellyButtonImage> {
        let key = JellyButtonImageKey::from_motion(request)?;
        if let Some(image) = lookup_image(&mut self.button_entries, key) {
            return Some(JellyButtonImage { image });
        }

        let render_width = key.width_px as f32;
        let render_height = key.height_px as f32;
        let bitmap = rasterize_button_material_bitmap(JellyButtonBitmapRequest {
            width: render_width,
            height: render_height,
            pixel_size: if render_height >= 58. { 1.25 } else { 1.4 },
            material: request.material,
            motion: JellyMotionSnapshot {
                pressure: key.pressure_bucket as f32 / 10.,
                rebound: key.rebound_bucket as f32 / 10.,
                squash_x: key.squash_x_bucket as f32 / 10.,
                squash_y: key.squash_y_bucket as f32 / 10.,
                rim_pressure: key.rim_bucket as f32 / 10.,
                gloss_phase: key.gloss_bucket as f32 / 16.,
                inner_lag: key.inner_bucket as f32 / 10.,
                contact: key.contact_bucket as f32 / 10.,
                aura: key.aura_bucket as f32 / 10.,
                error_shake: request.motion.error_shake,
            },
            enabled: key.enabled,
            loading: key.loading,
        });
        let image = bitmap.to_gpui_render_image()?;

        insert_image(&mut self.button_entries, key, image.clone());
        Some(JellyButtonImage { image })
    }

    pub(crate) fn switch_image(
        &mut self,
        request: JellySwitchImageRequest,
    ) -> Option<JellySwitchImage> {
        let key = JellySwitchImageKey::from_motion(request)?;
        if let Some(image) = lookup_image(&mut self.switch_entries, key) {
            return Some(JellySwitchImage { image });
        }

        let render_width = key.width_px as f32;
        let render_height = key.height_px as f32;
        let bitmap = rasterize_switch_material_bitmap(JellySwitchBitmapRequest {
            width: render_width,
            height: render_height,
            pixel_size: if render_height >= 46. { 1.3 } else { 1.45 },
            material: request.material,
            motion: JellyMotionSnapshot {
                pressure: key.pressure_bucket as f32 / 10.,
                rebound: key.rebound_bucket as f32 / 10.,
                squash_x: key.squash_x_bucket as f32 / 10.,
                squash_y: key.squash_y_bucket as f32 / 10.,
                rim_pressure: key.rim_bucket as f32 / 10.,
                gloss_phase: key.gloss_bucket as f32 / 16.,
                inner_lag: key.inner_bucket as f32 / 10.,
                contact: key.contact_bucket as f32 / 10.,
                aura: key.aura_bucket as f32 / 10.,
                error_shake: request.motion.error_shake,
            },
            checked: key.checked,
            enabled: key.enabled,
            active: key.active,
        });
        let image = bitmap.to_gpui_render_image()?;

        insert_image(&mut self.switch_entries, key, image.clone());
        Some(JellySwitchImage { image })
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.progress_entries.len() + self.button_entries.len() + self.switch_entries.len()
    }
}

impl JellyProgressImageKey {
    fn from_motion(request: JellyProgressImageRequest) -> Option<Self> {
        let width_px = quantize_dimension(request.width, 4., 48., 1800.)?;
        let height_px = quantize_dimension(request.height, 2., 18., 120.)?;

        Some(Self {
            width_px,
            height_px,
            progress_bucket: quantize_unit(request.motion.display_percent / 100., 100) as u8,
            pressure_bucket: quantize_unit(request.motion.pressure, 8) as u8,
            rebound_bucket: quantize_signed(request.motion.rebound, 8),
            contact_bucket: quantize_unit(request.motion.contact, 8) as u8,
            phase_bucket: quantize_unit(request.motion.gloss_phase, 15) as u8,
            phase: request.phase,
            tone: request.tone,
            quality: request.quality,
        })
    }
}

impl JellyButtonImageKey {
    fn from_motion(request: JellyButtonImageRequest) -> Option<Self> {
        let width_px = quantize_dimension(request.width, 4., 64., 900.)?;
        let height_px = quantize_dimension(request.height, 2., 32., 96.)?;

        Some(Self {
            width_px,
            height_px,
            pressure_bucket: quantize_unit(request.motion.pressure, 10) as u8,
            rebound_bucket: quantize_signed(request.motion.rebound, 10),
            squash_x_bucket: quantize_unit(request.motion.squash_x, 10) as u8,
            squash_y_bucket: quantize_unit(request.motion.squash_y, 10) as u8,
            rim_bucket: quantize_unit(request.motion.rim_pressure, 10) as u8,
            gloss_bucket: quantize_unit(request.motion.gloss_phase, 16) as u8,
            inner_bucket: quantize_unit(request.motion.inner_lag, 10) as u8,
            contact_bucket: quantize_unit(request.motion.contact, 10) as u8,
            aura_bucket: quantize_unit(request.motion.aura, 10) as u8,
            tone: request.tone,
            enabled: request.enabled,
            loading: request.loading,
        })
    }
}

impl JellySwitchImageKey {
    fn from_motion(request: JellySwitchImageRequest) -> Option<Self> {
        let width_px = quantize_dimension(request.width, 2., 64., 320.)?;
        let height_px = quantize_dimension(request.height, 2., 28., 96.)?;

        Some(Self {
            width_px,
            height_px,
            pressure_bucket: quantize_unit(request.motion.pressure, 10) as u8,
            rebound_bucket: quantize_signed(request.motion.rebound, 10),
            squash_x_bucket: quantize_unit(request.motion.squash_x, 10) as u8,
            squash_y_bucket: quantize_unit(request.motion.squash_y, 10) as u8,
            rim_bucket: quantize_unit(request.motion.rim_pressure, 10) as u8,
            gloss_bucket: quantize_unit(request.motion.gloss_phase, 16) as u8,
            inner_bucket: quantize_unit(request.motion.inner_lag, 10) as u8,
            contact_bucket: quantize_unit(request.motion.contact, 10) as u8,
            aura_bucket: quantize_unit(request.motion.aura, 10) as u8,
            tone: request.tone,
            checked: request.checked,
            enabled: request.enabled,
            active: request.active,
        })
    }
}

fn lookup_image<K: Copy + Eq>(
    entries: &mut VecDeque<(K, Arc<RenderImage>)>,
    key: K,
) -> Option<Arc<RenderImage>> {
    let index = entries
        .iter()
        .position(|(entry_key, _)| *entry_key == key)?;
    let (_, image) = entries.remove(index)?;
    entries.push_back((key, image.clone()));
    Some(image)
}

fn insert_image<K>(entries: &mut VecDeque<(K, Arc<RenderImage>)>, key: K, image: Arc<RenderImage>) {
    if entries.len() >= MAX_CACHED_IMAGES {
        entries.pop_front();
    }
    entries.push_back((key, image));
}

#[derive(Clone, Copy, Debug)]
struct JellyImageRenderConfig {
    pixel_size: f32,
    padding: f32,
    opacity: f32,
}

impl JellyProgressImageQuality {
    fn render_config(self, render_height: f32) -> JellyImageRenderConfig {
        match self {
            Self::Main => JellyImageRenderConfig {
                pixel_size: 1.25,
                padding: render_height * 0.34,
                opacity: 0.98,
            },
            Self::Lane => JellyImageRenderConfig {
                pixel_size: 1.5,
                padding: render_height * 0.3,
                opacity: 0.96,
            },
        }
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
    use crate::gui::motion::{
        JellyMotionSnapshot, JellyProgressChainSnapshot, JellyProgressMotionSnapshot,
    };
    use crate::gui::theme::Palette;

    use super::{
        JellyButtonImageRequest, JellyImageCache, JellyProgressImagePhase,
        JellyProgressImageQuality, JellyProgressImageRequest, JellySwitchImageRequest,
    };

    #[test]
    fn progress_image_cache_reuses_quantized_motion() {
        let mut cache = JellyImageCache::default();
        let material = JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default());
        let first = cache
            .progress_image(sample_request(
                640.,
                46.,
                JellyProgressImageQuality::Main,
                sample_motion(37.2),
                JellyProgressImagePhase::Running,
                JellyTone::Primary,
                material,
            ))
            .expect("render image");
        let second = cache
            .progress_image(sample_request(
                641.2,
                46.4,
                JellyProgressImageQuality::Main,
                sample_motion(37.21),
                JellyProgressImagePhase::Running,
                JellyTone::Primary,
                material,
            ))
            .expect("render image");

        assert_eq!(cache.len(), 1);
        assert!(Arc::ptr_eq(&first.image, &second.image));
    }

    #[test]
    fn progress_image_cache_splits_progress_buckets() {
        let mut cache = JellyImageCache::default();
        let material = JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default());

        cache.progress_image(sample_request(
            640.,
            46.,
            JellyProgressImageQuality::Main,
            sample_motion(21.),
            JellyProgressImagePhase::Running,
            JellyTone::Primary,
            material,
        ));
        cache.progress_image(sample_request(
            640.,
            46.,
            JellyProgressImageQuality::Main,
            sample_motion(42.),
            JellyProgressImagePhase::Running,
            JellyTone::Primary,
            material,
        ));

        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn progress_image_cache_splits_material_tones() {
        let mut cache = JellyImageCache::default();
        let palette = Palette::default();

        cache.progress_image(sample_request(
            640.,
            46.,
            JellyProgressImageQuality::Main,
            sample_motion(37.),
            JellyProgressImagePhase::Running,
            JellyTone::Primary,
            JellyMaterialToken::for_tone(JellyTone::Primary, &palette),
        ));
        cache.progress_image(sample_request(
            640.,
            46.,
            JellyProgressImageQuality::Main,
            sample_motion(37.),
            JellyProgressImagePhase::Running,
            JellyTone::Cyan,
            JellyMaterialToken::for_tone(JellyTone::Cyan, &palette),
        ));

        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn progress_image_cache_splits_quality_profiles() {
        let mut cache = JellyImageCache::default();
        let material = JellyMaterialToken::for_tone(JellyTone::Primary, &Palette::default());

        cache.progress_image(sample_request(
            640.,
            46.,
            JellyProgressImageQuality::Main,
            sample_motion(37.),
            JellyProgressImagePhase::Running,
            JellyTone::Primary,
            material,
        ));
        cache.progress_image(sample_request(
            640.,
            46.,
            JellyProgressImageQuality::Lane,
            sample_motion(37.),
            JellyProgressImagePhase::Running,
            JellyTone::Primary,
            material,
        ));

        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn button_image_cache_reuses_quantized_motion() {
        let mut cache = JellyImageCache::default();
        let palette = Palette::default();
        let material = JellyMaterialToken::for_tone(JellyTone::Primary, &palette);
        let first = cache
            .button_image(sample_button_request(
                320.,
                66.,
                sample_button_motion(0.21),
                material,
            ))
            .expect("button image");
        let second = cache
            .button_image(sample_button_request(
                321.1,
                66.2,
                sample_button_motion(0.22),
                material,
            ))
            .expect("button image");

        assert_eq!(cache.len(), 1);
        assert!(Arc::ptr_eq(&first.image, &second.image));
    }

    #[test]
    fn button_image_cache_splits_tone_and_enabled_state() {
        let mut cache = JellyImageCache::default();
        let palette = Palette::default();

        cache.button_image(sample_button_request(
            320.,
            66.,
            sample_button_motion(0.2),
            JellyMaterialToken::for_tone(JellyTone::Primary, &palette),
        ));
        cache.button_image(JellyButtonImageRequest {
            tone: JellyTone::Cyan,
            material: JellyMaterialToken::for_tone(JellyTone::Cyan, &palette),
            ..sample_button_request(
                320.,
                66.,
                sample_button_motion(0.2),
                JellyMaterialToken::for_tone(JellyTone::Primary, &palette),
            )
        });
        cache.button_image(JellyButtonImageRequest {
            enabled: false,
            ..sample_button_request(
                320.,
                66.,
                sample_button_motion(0.2),
                JellyMaterialToken::for_tone(JellyTone::Primary, &palette),
            )
        });

        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn switch_image_cache_reuses_quantized_motion() {
        let mut cache = JellyImageCache::default();
        let palette = Palette::default();
        let material = JellyMaterialToken::for_tone(JellyTone::Primary, &palette);
        let first = cache
            .switch_image(sample_switch_request(
                142.,
                52.,
                sample_button_motion(0.21),
                material,
                true,
            ))
            .expect("switch image");
        let second = cache
            .switch_image(sample_switch_request(
                142.4,
                52.2,
                sample_button_motion(0.22),
                material,
                true,
            ))
            .expect("switch image");

        assert_eq!(cache.len(), 1);
        assert!(Arc::ptr_eq(&first.image, &second.image));
    }

    #[test]
    fn switch_image_cache_splits_checked_tone_and_enabled_state() {
        let mut cache = JellyImageCache::default();
        let palette = Palette::default();

        cache.switch_image(sample_switch_request(
            142.,
            52.,
            sample_button_motion(0.2),
            JellyMaterialToken::for_tone(JellyTone::Primary, &palette),
            true,
        ));
        cache.switch_image(JellySwitchImageRequest {
            checked: false,
            ..sample_switch_request(
                142.,
                52.,
                sample_button_motion(0.2),
                JellyMaterialToken::for_tone(JellyTone::Primary, &palette),
                true,
            )
        });
        cache.switch_image(JellySwitchImageRequest {
            tone: JellyTone::Cyan,
            material: JellyMaterialToken::for_tone(JellyTone::Cyan, &palette),
            ..sample_switch_request(
                142.,
                52.,
                sample_button_motion(0.2),
                JellyMaterialToken::for_tone(JellyTone::Primary, &palette),
                true,
            )
        });
        cache.switch_image(JellySwitchImageRequest {
            enabled: false,
            ..sample_switch_request(
                142.,
                52.,
                sample_button_motion(0.2),
                JellyMaterialToken::for_tone(JellyTone::Primary, &palette),
                true,
            )
        });

        assert_eq!(cache.len(), 4);
    }

    fn sample_request(
        width: f32,
        height: f32,
        quality: JellyProgressImageQuality,
        motion: JellyProgressMotionSnapshot,
        phase: JellyProgressImagePhase,
        tone: JellyTone,
        material: JellyMaterialToken,
    ) -> JellyProgressImageRequest {
        JellyProgressImageRequest {
            width,
            height,
            quality,
            motion,
            phase,
            tone,
            material,
        }
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

    fn sample_button_request(
        width: f32,
        height: f32,
        motion: JellyMotionSnapshot,
        material: JellyMaterialToken,
    ) -> JellyButtonImageRequest {
        JellyButtonImageRequest {
            width,
            height,
            motion,
            tone: JellyTone::Primary,
            material,
            enabled: true,
            loading: false,
        }
    }

    fn sample_switch_request(
        width: f32,
        height: f32,
        motion: JellyMotionSnapshot,
        material: JellyMaterialToken,
        checked: bool,
    ) -> JellySwitchImageRequest {
        JellySwitchImageRequest {
            width,
            height,
            motion,
            tone: JellyTone::Primary,
            material,
            checked,
            enabled: true,
            active: checked,
        }
    }

    fn sample_button_motion(pressure: f32) -> JellyMotionSnapshot {
        JellyMotionSnapshot {
            pressure,
            rebound: pressure * 0.2,
            squash_x: pressure * 0.4,
            squash_y: pressure * 0.3,
            rim_pressure: 0.25 + pressure * 0.2,
            gloss_phase: 0.36,
            inner_lag: pressure * 0.25,
            contact: 0.2 + pressure * 0.4,
            aura: 0.2,
            error_shake: 0.,
        }
    }
}
