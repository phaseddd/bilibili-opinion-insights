use std::time::Instant;

use crate::gui::motion::{
    CONTROL_IMPULSE_SECONDS, JellyMotionSnapshot, JellySwitchMotionSnapshot,
    JellySwitchMotionState, MIN_VISUAL_FRAME_DT, VISUAL_MOTION_DT,
};
use crate::gui::rendering::jelly_image_cache::JellyImageCache;

const MOTION_STATS_SMOOTHING: f32 = 0.12;

#[derive(Default)]
pub(crate) struct VisualState {
    pub(crate) motion_tick: u64,
    pub(crate) image_cache: JellyImageCache,
    motion_frame_clock: Option<Instant>,
    motion_frame_budget: MotionFrameBudget,
    impulses: Vec<MotionImpulse>,
    switches: Vec<(usize, JellySwitchMotionState)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ButtonMotionId {
    EnterWorkbench,
    QrLogin,
    RecheckAuth,
    Anonymous,
    HeaderClear,
    HeaderCancel,
    HeaderStart,
}

#[derive(Clone, Copy, Debug)]
struct MotionImpulse {
    id: ButtonMotionId,
    age_seconds: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MotionFrameStats {
    pub(crate) frames: u64,
    pub(crate) slow_frames: u64,
    pub(crate) last_dt: f32,
    pub(crate) max_dt: f32,
    pub(crate) ema_dt: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct MotionFrameBudget {
    stats: MotionFrameStats,
}

impl MotionFrameBudget {
    fn record(&mut self, dt: f32) -> f32 {
        let dt = if dt.is_finite() && dt > 0. {
            dt
        } else {
            VISUAL_MOTION_DT
        };

        self.stats.frames = self.stats.frames.saturating_add(1);
        self.stats.last_dt = dt;
        self.stats.max_dt = self.stats.max_dt.max(dt);
        if dt > MIN_VISUAL_FRAME_DT {
            self.stats.slow_frames = self.stats.slow_frames.saturating_add(1);
        }
        self.stats.ema_dt = if self.stats.frames == 1 {
            dt
        } else {
            self.stats.ema_dt * (1. - MOTION_STATS_SMOOTHING) + dt * MOTION_STATS_SMOOTHING
        };

        dt.clamp(0.001, 0.05)
    }
}

impl VisualState {
    pub(crate) fn motion_frame_stats(&self) -> MotionFrameStats {
        self.motion_frame_budget.stats
    }

    pub(crate) fn begin_motion_frame(&mut self) -> f32 {
        let now = Instant::now();
        let dt = self
            .motion_frame_clock
            .map(|last| now.duration_since(last).as_secs_f32())
            .unwrap_or(VISUAL_MOTION_DT);
        self.motion_frame_clock = Some(now);
        self.motion_frame_budget.record(dt)
    }

    pub(crate) fn stop_motion_frame_clock(&mut self) {
        self.motion_frame_clock = None;
    }

    pub(crate) fn trigger_button(&mut self, id: ButtonMotionId) {
        self.trigger(id);
    }

    pub(crate) fn tick_button_motion(&mut self, dt: f32) {
        let dt = if dt.is_finite() && dt > 0. {
            dt
        } else {
            VISUAL_MOTION_DT
        };
        for impulse in &mut self.impulses {
            impulse.age_seconds += dt;
        }
        self.impulses
            .retain(|impulse| impulse.age_seconds <= CONTROL_IMPULSE_SECONDS);
    }

    pub(crate) fn toggle_switch(&mut self, id_seed: usize, checked: bool) {
        if let Some((_, state)) = self
            .switches
            .iter_mut()
            .find(|(switch_id, _)| *switch_id == id_seed)
        {
            state.toggle_to(checked);
        } else {
            let mut state = JellySwitchMotionState::new(!checked);
            state.toggle_to(checked);
            self.switches.push((id_seed, state));
        }
    }

    pub(crate) fn button_motion(
        &self,
        id: ButtonMotionId,
        loading: bool,
        error: bool,
    ) -> JellyMotionSnapshot {
        JellyMotionSnapshot::from_impulse(self.age_for(id), self.motion_tick, loading, error)
    }

    pub(crate) fn switch_motion(
        &mut self,
        id_seed: usize,
        checked: bool,
        active: bool,
    ) -> JellySwitchMotionSnapshot {
        if let Some((_, state)) = self
            .switches
            .iter_mut()
            .find(|(switch_id, _)| *switch_id == id_seed)
        {
            if state.target() != checked {
                state.sync_target(checked);
            }
            return state.snapshot(self.motion_tick, active);
        }

        let state = JellySwitchMotionState::new(checked);
        let snapshot = state.snapshot(self.motion_tick, active);
        self.switches.push((id_seed, state));
        snapshot
    }

    pub(crate) fn tick_switch_motion(&mut self, dt: f32) -> bool {
        let dt = if dt.is_finite() && dt > 0. {
            dt
        } else {
            VISUAL_MOTION_DT
        };

        let mut active = false;
        for (_, state) in &mut self.switches {
            active |= state.tick(dt);
        }
        active
    }

    pub(crate) fn has_active_control_motion(&self) -> bool {
        !self.impulses.is_empty() || self.switches.iter().any(|(_, state)| state.is_active())
    }

    fn trigger(&mut self, id: ButtonMotionId) {
        self.impulses.retain(|impulse| impulse.id != id);
        self.impulses.push(MotionImpulse {
            id,
            age_seconds: 0.,
        });
    }

    fn age_for(&self, id: ButtonMotionId) -> Option<f32> {
        self.impulses
            .iter()
            .rev()
            .find(|impulse| impulse.id == id)
            .map(|impulse| impulse.age_seconds)
            .filter(|age| *age <= CONTROL_IMPULSE_SECONDS)
    }
}

#[cfg(test)]
mod tests {
    use super::{ButtonMotionId, MotionFrameBudget, VisualState};
    use crate::gui::motion::{CONTROL_IMPULSE_SECONDS, MIN_VISUAL_FRAME_DT, VISUAL_MOTION_DT};

    #[test]
    fn frame_budget_records_60fps_misses_without_clamping_stats() {
        let mut budget = MotionFrameBudget::default();

        let fast_dt = budget.record(VISUAL_MOTION_DT);
        let slow_dt = budget.record(MIN_VISUAL_FRAME_DT * 1.4);

        assert_eq!(budget.stats.frames, 2);
        assert_eq!(budget.stats.slow_frames, 1);
        assert!(budget.stats.max_dt > MIN_VISUAL_FRAME_DT);
        assert_eq!(fast_dt, VISUAL_MOTION_DT);
        assert!(slow_dt > MIN_VISUAL_FRAME_DT);
    }

    #[test]
    fn frame_budget_uses_reference_dt_for_invalid_samples() {
        let mut budget = MotionFrameBudget::default();

        let dt = budget.record(f32::NAN);

        assert_eq!(dt, VISUAL_MOTION_DT);
        assert_eq!(budget.stats.frames, 1);
        assert_eq!(budget.stats.slow_frames, 0);
    }

    #[test]
    fn visual_state_exposes_motion_frame_stats() {
        let mut visual = VisualState::default();

        let _ = visual.begin_motion_frame();

        let stats = visual.motion_frame_stats();
        assert_eq!(stats.frames, 1);
        assert!(stats.ema_dt > 0.);
    }

    #[test]
    fn button_impulse_duration_uses_elapsed_seconds() {
        let mut visual = VisualState::default();

        visual.trigger_button(ButtonMotionId::HeaderStart);
        visual.tick_button_motion(CONTROL_IMPULSE_SECONDS * 0.5);

        assert!(visual.has_active_control_motion());

        visual.tick_button_motion(CONTROL_IMPULSE_SECONDS * 0.6);

        assert!(!visual.has_active_control_motion());
    }
}
