use crate::gui::motion::{
    JellyMotionSnapshot, JellySwitchMotionSnapshot, JellySwitchMotionState, VISUAL_MOTION_DT,
};
use crate::gui::rendering::jelly_image_cache::JellyImageCache;

const CONTROL_IMPULSE_TICKS: u64 = 72;

#[derive(Default)]
pub(crate) struct VisualState {
    pub(crate) motion_tick: u64,
    pub(crate) motion_loop_running: bool,
    pub(crate) image_cache: JellyImageCache,
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
    pub(crate) started_tick: u64,
}

impl VisualState {
    pub(crate) fn trigger_button(&mut self, id: ButtonMotionId) {
        self.trigger(id);
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
        self.impulses.iter().any(|impulse| {
            self.motion_tick.saturating_sub(impulse.started_tick) <= CONTROL_IMPULSE_TICKS
        }) || self.switches.iter().any(|(_, state)| state.is_active())
    }

    fn trigger(&mut self, id: ButtonMotionId) {
        self.impulses.retain(|impulse| {
            impulse.id != id
                && self.motion_tick.saturating_sub(impulse.started_tick) <= CONTROL_IMPULSE_TICKS
        });
        self.impulses.push(MotionImpulse {
            id,
            started_tick: self.motion_tick,
        });
    }

    fn age_for(&self, id: ButtonMotionId) -> Option<u64> {
        self.impulses
            .iter()
            .rev()
            .find(|impulse| impulse.id == id)
            .map(|impulse| self.motion_tick.saturating_sub(impulse.started_tick))
            .filter(|age| *age <= CONTROL_IMPULSE_TICKS)
    }
}
