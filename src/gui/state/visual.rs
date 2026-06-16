use crate::gui::motion::JellyMotionSnapshot;
use crate::gui::rendering::jelly_image_cache::JellyImageCache;

const CONTROL_IMPULSE_TICKS: u64 = 24;

#[derive(Default)]
pub(crate) struct VisualState {
    pub(crate) motion_tick: u64,
    pub(crate) motion_loop_running: bool,
    pub(crate) image_cache: JellyImageCache,
    impulses: Vec<MotionImpulse>,
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
    id: MotionId,
    pub(crate) started_tick: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MotionId {
    Button(ButtonMotionId),
    Switch(usize),
}

impl VisualState {
    pub(crate) fn trigger_button(&mut self, id: ButtonMotionId) {
        self.trigger(MotionId::Button(id));
    }

    pub(crate) fn trigger_switch(&mut self, id_seed: usize) {
        self.trigger(MotionId::Switch(id_seed));
    }

    pub(crate) fn button_motion(
        &self,
        id: ButtonMotionId,
        loading: bool,
        error: bool,
    ) -> JellyMotionSnapshot {
        JellyMotionSnapshot::from_impulse(
            self.age_for(MotionId::Button(id)),
            self.motion_tick,
            loading,
            error,
        )
    }

    pub(crate) fn switch_motion(&self, id_seed: usize, active: bool) -> JellyMotionSnapshot {
        JellyMotionSnapshot::from_impulse(
            self.age_for(MotionId::Switch(id_seed)),
            self.motion_tick,
            active,
            false,
        )
    }

    pub(crate) fn has_active_control_motion(&self) -> bool {
        self.impulses.iter().any(|impulse| {
            self.motion_tick.saturating_sub(impulse.started_tick) <= CONTROL_IMPULSE_TICKS
        })
    }

    fn trigger(&mut self, id: MotionId) {
        self.impulses.retain(|impulse| {
            impulse.id != id
                && self.motion_tick.saturating_sub(impulse.started_tick) <= CONTROL_IMPULSE_TICKS
        });
        self.impulses.push(MotionImpulse {
            id,
            started_tick: self.motion_tick,
        });
    }

    fn age_for(&self, id: MotionId) -> Option<u64> {
        self.impulses
            .iter()
            .rev()
            .find(|impulse| impulse.id == id)
            .map(|impulse| self.motion_tick.saturating_sub(impulse.started_tick))
            .filter(|age| *age <= CONTROL_IMPULSE_TICKS)
    }
}
