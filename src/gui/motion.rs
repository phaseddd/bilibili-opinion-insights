use std::f32::consts::{PI, TAU};

pub const PROGRESS_CHAIN_POINTS: usize = 17;
pub const MIN_VISUAL_FPS: f32 = 60.;
pub const MIN_VISUAL_FRAME_DT: f32 = 1. / MIN_VISUAL_FPS;
pub const REFERENCE_VISUAL_FPS: f32 = 120.;
pub const VISUAL_MOTION_DT: f32 = 1. / REFERENCE_VISUAL_FPS;

type ProgressChainPoints = [(f32, f32); PROGRESS_CHAIN_POINTS];
type ChainGeometry = (ProgressChainPoints, ProgressChainPoints);

const REFERENCE_MOTION_DT: f32 = 0.072;
pub const CONTROL_IMPULSE_SECONDS: f32 = 0.52;
const SWITCH_ACCELERATION: f32 = 34.;
const SWITCH_MAX_VELOCITY: f32 = 5.8;
const SWITCH_ENDPOINT_EPS: f32 = 0.04;

#[derive(Clone, Copy, Debug)]
pub struct SpringToken {
    pub mass: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub max_velocity: f32,
}

impl SpringToken {
    pub const fn new(mass: f32, stiffness: f32, damping: f32, max_velocity: f32) -> Self {
        Self {
            mass,
            stiffness,
            damping,
            max_velocity,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SpringValue {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub token: SpringToken,
}

impl SpringValue {
    #[allow(dead_code)]
    pub fn new(target: f32, token: SpringToken) -> Self {
        Self {
            value: target,
            target,
            velocity: 0.,
            token,
        }
    }

    #[allow(dead_code)]
    pub fn tick(&mut self, dt: f32) {
        let dt = dt.clamp(0., 0.1);
        if dt <= 0. {
            return;
        }

        let spring = -self.token.stiffness * (self.value - self.target);
        let damp = -self.token.damping * self.velocity;
        let acceleration = (spring + damp) / self.token.mass.max(0.001);
        self.velocity += acceleration * dt;
        self.velocity = self
            .velocity
            .clamp(-self.token.max_velocity, self.token.max_velocity);
        self.value += self.velocity * dt;
    }
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct JellyMotionTokens {
    pub button_pressure: SpringToken,
    pub button_rebound: SpringToken,
    pub switch_progress: SpringToken,
    pub switch_squash: SpringToken,
    pub progress_follow: SpringToken,
    pub event_pulse_decay: f32,
    pub error_shake_decay: f32,
    pub loading_breath_rate: f32,
    pub reduced_motion: bool,
}

impl Default for JellyMotionTokens {
    fn default() -> Self {
        Self {
            button_pressure: SpringToken::new(1., 920., 22., 18.),
            button_rebound: SpringToken::new(1., 760., 18., 16.),
            switch_progress: SpringToken::new(1., 820., 20., 14.),
            switch_squash: SpringToken::new(1., 980., 18., 18.),
            progress_follow: SpringToken::new(1., 260., 30., 8.),
            event_pulse_decay: 0.84,
            error_shake_decay: 0.72,
            loading_breath_rate: 0.18,
            reduced_motion: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct JellyMotionSnapshot {
    pub pressure: f32,
    pub rebound: f32,
    pub squash_x: f32,
    pub squash_y: f32,
    pub rim_pressure: f32,
    pub gloss_phase: f32,
    pub inner_lag: f32,
    pub contact: f32,
    pub aura: f32,
    pub error_shake: f32,
}

impl JellyMotionSnapshot {
    pub fn from_impulse(
        age_seconds: Option<f32>,
        motion_tick: u64,
        loading: bool,
        error: bool,
    ) -> Self {
        let tokens = JellyMotionTokens::default();
        let breath = if loading {
            wave_between(motion_tick, tokens.loading_breath_rate, 0.08, 0.24)
        } else {
            0.
        };
        let error_shake = if error {
            let wave = (motion_tick as f32 * 1.25).sin();
            wave * tokens.error_shake_decay * 0.35
        } else {
            0.
        };

        let Some(age_seconds) = age_seconds else {
            return Self {
                gloss_phase: breath,
                aura: breath * 0.7,
                error_shake,
                ..Self::default()
            };
        };

        let t = (age_seconds / CONTROL_IMPULSE_SECONDS).clamp(0., 1.);
        let decay = (1. - t).powf(1.8);
        let oscillation = (t * TAU * 1.42).sin();
        let rebound = (oscillation * decay).clamp(-0.55, 1.);
        let positive = rebound.max(0.);
        let absolute = rebound.abs();
        let pressure = ((1. - t) * 0.32 + positive * 0.42).clamp(0., 1.);

        Self {
            pressure,
            rebound,
            squash_x: (positive * 0.86 + absolute * 0.18).clamp(0., 1.),
            squash_y: ((-rebound).max(0.) * 0.72 + pressure * 0.28).clamp(0., 1.),
            rim_pressure: (absolute * 0.9 + breath).clamp(0., 1.),
            gloss_phase: (breath + positive * 0.62).clamp(0., 1.),
            inner_lag: (decay * 0.38 + positive * 0.24).clamp(0., 1.),
            contact: (pressure * 0.72 + absolute * 0.28).clamp(0., 1.),
            aura: (breath * 0.6 + absolute * 0.42).clamp(0., 1.),
            error_shake,
        }
    }

    #[allow(dead_code)]
    pub fn settled(self) -> bool {
        self.pressure.abs() < 0.01
            && self.rebound.abs() < 0.01
            && self.squash_x.abs() < 0.01
            && self.squash_y.abs() < 0.01
            && self.error_shake.abs() < 0.01
    }
}

#[derive(Clone, Copy, Debug)]
pub struct JellySwitchMotionState {
    target: bool,
    progress: f32,
    velocity: f32,
    squash_x: SpringValue,
    squash_z: SpringValue,
    wiggle_x: SpringValue,
    pressed_impulse: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct JellySwitchMotionSnapshot {
    pub progress: f32,
    pub velocity: f32,
    pub pressure: f32,
    pub rebound: f32,
    pub squash_x: f32,
    pub squash_y: f32,
    pub rim_pressure: f32,
    pub gloss_phase: f32,
    pub inner_lag: f32,
    pub contact: f32,
    pub aura: f32,
    pub error_shake: f32,
    pub wiggle_x: f32,
}

impl JellySwitchMotionState {
    pub fn new(checked: bool) -> Self {
        let squash_token = JellyMotionTokens::default().switch_squash;
        Self {
            target: checked,
            progress: if checked { 1. } else { 0. },
            velocity: 0.,
            squash_x: SpringValue::new(0., squash_token),
            squash_z: SpringValue::new(0., squash_token),
            wiggle_x: SpringValue::new(0., squash_token),
            pressed_impulse: 0.,
        }
    }

    pub fn target(&self) -> bool {
        self.target
    }

    pub fn sync_target(&mut self, checked: bool) {
        self.target = checked;
    }

    pub fn toggle_to(&mut self, checked: bool) {
        if self.target != checked {
            self.target = checked;
            self.press();
            self.velocity += if checked { 0.86 } else { -0.86 };
        } else {
            self.press();
        }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let dt = dt.clamp(0.001, 0.05);
        let target = if self.target { 1. } else { 0. };
        let distance = target - self.progress;

        if distance.abs() > 0.002 {
            self.velocity += distance.signum() * SWITCH_ACCELERATION * dt;
            self.velocity = self
                .velocity
                .clamp(-SWITCH_MAX_VELOCITY, SWITCH_MAX_VELOCITY);
            self.wiggle_x.velocity =
                (self.wiggle_x.velocity + self.velocity * 0.18).clamp(-18., 18.);
        } else {
            self.velocity *= decay_for_dt(0.72, dt);
        }

        self.progress += self.velocity * dt;

        if self.progress >= 1. {
            self.progress = 1.;
            if self.velocity > SWITCH_ENDPOINT_EPS {
                self.inject_endpoint_collision(true);
            }
            self.velocity = 0.;
        } else if self.progress <= 0. {
            self.progress = 0.;
            if self.velocity < -SWITCH_ENDPOINT_EPS {
                self.inject_endpoint_collision(false);
            }
            self.velocity = 0.;
        }

        self.squash_x.tick(dt);
        self.squash_z.tick(dt);
        self.wiggle_x.tick(dt);
        self.pressed_impulse *= decay_for_dt(0.72, dt);

        self.is_active()
    }

    pub fn snapshot(self, motion_tick: u64, active: bool) -> JellySwitchMotionSnapshot {
        let travel_energy = (self.velocity.abs() / SWITCH_MAX_VELOCITY).clamp(0., 1.);
        let endpoint_contact = if self.progress < 0.04 || self.progress > 0.96 {
            travel_energy
        } else {
            0.
        };
        let active_wave = if active {
            wave_between(motion_tick, 0.2, 0., 1.)
        } else {
            0.
        };
        let squash_x = ((-self.squash_x.value).max(0.) * 0.38
            + self.squash_z.value.max(0.) * 0.22
            + travel_energy * 0.32
            + self.pressed_impulse * 0.28)
            .clamp(0., 1.);
        let squash_y = (self.squash_z.value.max(0.) * 0.28
            + self.pressed_impulse * 0.22
            + endpoint_contact * 0.16)
            .clamp(0., 1.);
        let rebound = (self.squash_z.value * 0.36 - self.squash_x.value * 0.16).clamp(-0.65, 0.9);
        let pressure =
            (self.pressed_impulse * 0.36 + travel_energy * 0.28 + endpoint_contact * 0.2)
                .clamp(0., 1.);
        let wiggle_x = self.wiggle_x.value.clamp(-1.2, 1.2);

        JellySwitchMotionSnapshot {
            progress: self.progress.clamp(0., 1.),
            velocity: self.velocity,
            pressure,
            rebound,
            squash_x,
            squash_y,
            rim_pressure: (travel_energy * 0.42
                + self.pressed_impulse * 0.2
                + active_wave * 0.18
                + endpoint_contact * 0.22)
                .clamp(0., 1.),
            gloss_phase: ((motion_tick as f32 * 0.14 + wiggle_x * 0.45)
                .sin()
                .mul_add(0.5, 0.5)),
            inner_lag: (self.squash_z.value.max(0.) * 0.32 + travel_energy * 0.18).clamp(0., 1.),
            contact: (pressure * 0.55 + endpoint_contact * 0.35 + active_wave * 0.08).clamp(0., 1.),
            aura: (active_wave * 0.22 + travel_energy * 0.24 + endpoint_contact * 0.22)
                .clamp(0., 1.),
            error_shake: 0.,
            wiggle_x,
        }
    }

    pub fn is_active(self) -> bool {
        let target = if self.target { 1. } else { 0. };
        (self.progress - target).abs() > 0.001
            || self.velocity.abs() > 0.006
            || self.pressed_impulse > 0.01
            || self.squash_x.value.abs() > 0.01
            || self.squash_x.velocity.abs() > 0.03
            || self.squash_z.value.abs() > 0.01
            || self.squash_z.velocity.abs() > 0.03
            || self.wiggle_x.value.abs() > 0.01
            || self.wiggle_x.velocity.abs() > 0.03
    }

    fn press(&mut self) {
        self.pressed_impulse = 1.;
        self.squash_x.velocity = (self.squash_x.velocity - 2.).clamp(-18., 18.);
        self.squash_z.velocity = (self.squash_z.velocity + 1.).clamp(-18., 18.);
        self.wiggle_x.velocity =
            (self.wiggle_x.velocity + (self.progress - 0.5).signum()).clamp(-18., 18.);
    }

    fn inject_endpoint_collision(&mut self, on: bool) {
        self.squash_x.velocity = (self.squash_x.velocity - 5.).clamp(-18., 18.);
        self.squash_z.velocity = (self.squash_z.velocity + 5.).clamp(-18., 18.);
        self.wiggle_x.velocity =
            (self.wiggle_x.velocity + if on { -10. } else { 10. }).clamp(-18., 18.);
    }
}

impl JellySwitchMotionSnapshot {
    pub fn layer_motion(self) -> JellyMotionSnapshot {
        JellyMotionSnapshot {
            pressure: self.pressure,
            rebound: self.rebound,
            squash_x: self.squash_x,
            squash_y: self.squash_y,
            rim_pressure: self.rim_pressure,
            gloss_phase: self.gloss_phase,
            inner_lag: self.inner_lag,
            contact: self.contact,
            aura: self.aura,
            error_shake: self.error_shake,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressMotionPhase {
    Idle,
    Validating,
    Running,
    Cancelling,
    Completed,
    Failed,
}

impl ProgressMotionPhase {
    fn is_live(self) -> bool {
        matches!(self, Self::Validating | Self::Running | Self::Cancelling)
    }

    fn follow_token(self) -> SpringToken {
        match self {
            Self::Completed => SpringToken::new(1., 300., 30., 3.8),
            Self::Cancelling => SpringToken::new(1., 120., 28., 1.5),
            Self::Failed => SpringToken::new(1., 150., 32., 1.8),
            Self::Idle => SpringToken::new(1., 180., 28., 2.6),
            Self::Validating | Self::Running => JellyMotionTokens::default().progress_follow,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct JellyProgressMotionState {
    follow: SpringValue,
    pulse: f32,
    compression: f32,
    cap_rebound: f32,
    phase_offset: f32,
    chain: JellyProgressPointChain,
}

#[derive(Clone, Copy, Debug)]
pub struct JellyProgressChainSnapshot {
    pub offsets: [f32; PROGRESS_CHAIN_POINTS],
    pub positions: [(f32, f32); PROGRESS_CHAIN_POINTS],
    pub normals: [(f32, f32); PROGRESS_CHAIN_POINTS],
    pub control_points: [(f32, f32); PROGRESS_CHAIN_POINTS],
}

impl JellyProgressChainSnapshot {
    #[cfg(test)]
    pub fn straight() -> Self {
        Self::from_offsets([0.; PROGRESS_CHAIN_POINTS])
    }

    #[cfg(test)]
    pub fn from_offsets(offsets: [f32; PROGRESS_CHAIN_POINTS]) -> Self {
        let mut positions = [(0., 0.); PROGRESS_CHAIN_POINTS];
        for idx in 0..PROGRESS_CHAIN_POINTS {
            let t = idx as f32 / (PROGRESS_CHAIN_POINTS - 1) as f32;
            positions[idx] = (t, offsets[idx].clamp(-0.56, 0.38));
        }
        let (normals, control_points) = chain_geometry_from_positions(positions);

        Self {
            offsets,
            positions,
            normals,
            control_points,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct JellyProgressPointChain {
    pos: [(f32, f32); PROGRESS_CHAIN_POINTS],
    prev: [(f32, f32); PROGRESS_CHAIN_POINTS],
    normal: [(f32, f32); PROGRESS_CHAIN_POINTS],
    control_points: [(f32, f32); PROGRESS_CHAIN_POINTS],
    inv_mass: [f32; PROGRESS_CHAIN_POINTS],
    initialized: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct JellyProgressMotionSnapshot {
    pub display_percent: f32,
    pub target_percent: f32,
    pub velocity: f32,
    pub pulse: f32,
    pub pressure: f32,
    pub rebound: f32,
    pub squash_x: f32,
    pub squash_y: f32,
    pub rim_pressure: f32,
    pub gloss_phase: f32,
    pub inner_lag: f32,
    pub contact: f32,
    pub aura: f32,
    pub error_shake: f32,
    pub chain: JellyProgressChainSnapshot,
}

impl Default for JellyProgressMotionState {
    fn default() -> Self {
        let token = JellyMotionTokens::default().progress_follow;
        Self {
            follow: SpringValue::new(0., token),
            pulse: 0.,
            compression: 0.,
            cap_rebound: 0.,
            phase_offset: 0.,
            chain: JellyProgressPointChain::default(),
        }
    }
}

impl JellyProgressMotionState {
    pub fn reset_to(&mut self, percent: f32) {
        let value = (percent / 100.).clamp(0., 1.);
        self.follow.value = value;
        self.follow.target = value;
        self.follow.velocity = 0.;
        self.pulse = 0.;
        self.compression = 0.;
        self.cap_rebound = 0.;
        self.phase_offset = 0.;
        self.chain.reset();
    }

    pub fn set_target_percent(&mut self, percent: f32) {
        let target = (percent / 100.).clamp(0., 1.);
        let delta = target - self.follow.target;
        if delta.abs() > 0.0005 {
            let impulse = (delta.abs() * 2.35).clamp(0.12, 0.88);
            self.pulse = self.pulse.max(impulse);
            self.compression = (self.compression + impulse * 0.42).clamp(0., 1.);
            self.cap_rebound =
                (self.cap_rebound + delta.signum() * impulse * 0.28).clamp(-0.52, 0.74);
            self.phase_offset += delta * TAU * 0.65;
            self.inject_chain_impulse(delta, impulse);
        }
        self.follow.target = target;
    }

    pub fn trigger_phase_pulse(&mut self, phase: ProgressMotionPhase) {
        match phase {
            ProgressMotionPhase::Completed => {
                self.pulse = self.pulse.max(0.72);
                self.compression = self.compression.max(0.28);
                self.cap_rebound = self.cap_rebound.max(0.36);
                self.inject_chain_settle(-0.16);
            }
            ProgressMotionPhase::Failed => {
                self.pulse = self.pulse.max(0.55);
                self.compression = self.compression.max(0.34);
                self.cap_rebound = self.cap_rebound.min(-0.28);
                self.inject_chain_settle(0.14);
            }
            ProgressMotionPhase::Cancelling => {
                self.pulse = self.pulse.max(0.42);
                self.compression = self.compression.max(0.24);
                self.cap_rebound = self.cap_rebound.min(-0.18);
                self.inject_chain_settle(0.08);
            }
            ProgressMotionPhase::Validating | ProgressMotionPhase::Running => {
                self.pulse = self.pulse.max(0.28);
                self.compression = self.compression.max(0.18);
                self.inject_chain_settle(-0.06);
            }
            ProgressMotionPhase::Idle => {}
        }
    }

    pub fn display_percent(self) -> f32 {
        (self.follow.value * 100.).clamp(0., 100.)
    }

    pub fn tick(&mut self, phase: ProgressMotionPhase, dt: f32) -> bool {
        let tokens = JellyMotionTokens::default();
        self.follow.token = phase.follow_token();
        let dt = dt.clamp(0.001, 0.05);

        let substeps = (dt / 0.012).ceil().clamp(1., 4.) as usize;
        let step_dt = dt / substeps as f32;
        for _ in 0..substeps {
            self.follow.tick(step_dt);
        }
        self.follow.value = self.follow.value.clamp(0., 1.);

        let velocity_energy =
            (self.follow.velocity.abs() / self.follow.token.max_velocity.max(0.001)).clamp(0., 1.);
        let pulse_decay = if phase.is_live() {
            tokens.event_pulse_decay
        } else {
            0.78
        };
        self.pulse *= decay_for_dt(pulse_decay, dt);
        self.compression =
            (self.compression * decay_for_dt(0.76, dt) + velocity_energy * 0.18).clamp(0., 1.);
        self.cap_rebound = (self.cap_rebound * decay_for_dt(0.8, dt)
            + self.follow.velocity * 0.018)
            .clamp(-0.52, 0.76);
        self.tick_chain(phase, velocity_energy, dt);

        self.is_active(phase)
    }

    pub fn snapshot(
        self,
        motion_tick: u64,
        phase: ProgressMotionPhase,
    ) -> JellyProgressMotionSnapshot {
        let active_wave = if phase.is_live() {
            wave_between(motion_tick, 0.17, 0., 1.)
        } else {
            0.
        };
        let velocity_energy =
            (self.follow.velocity.abs() / self.follow.token.max_velocity.max(0.001)).clamp(0., 1.);
        let pressure = (self.compression * 0.52
            + self.pulse * 0.26
            + active_wave * 0.12
            + if phase == ProgressMotionPhase::Cancelling {
                0.08
            } else {
                0.
            })
        .clamp(0., 1.);
        let rebound = (self.cap_rebound
            + if phase == ProgressMotionPhase::Completed {
                self.pulse * 0.1
            } else {
                0.
            }
            - if phase == ProgressMotionPhase::Cancelling {
                0.08 + active_wave * 0.05
            } else {
                0.
            })
        .clamp(-0.55, 0.82);
        let gloss_phase = ((motion_tick as f32 * 0.16 + self.phase_offset + self.pulse * 0.7)
            .sin()
            .mul_add(0.5, 0.5))
        .clamp(0., 1.);
        let error_shake = if phase == ProgressMotionPhase::Failed {
            (motion_tick as f32 * 1.14).sin() * (0.08 + self.pulse * 0.22)
        } else {
            0.
        };

        JellyProgressMotionSnapshot {
            display_percent: self.display_percent(),
            target_percent: (self.follow.target * 100.).clamp(0., 100.),
            velocity: self.follow.velocity,
            pulse: self.pulse,
            pressure,
            rebound,
            squash_x: (pressure * 0.48 + velocity_energy * 0.28 + rebound.max(0.) * 0.2)
                .clamp(0., 1.),
            squash_y: (pressure * 0.24 + (-rebound).max(0.) * 0.34).clamp(0., 1.),
            rim_pressure: (self.pulse * 0.5 + velocity_energy * 0.28 + active_wave * 0.18)
                .clamp(0., 1.),
            gloss_phase,
            inner_lag: (self.pulse * 0.24 + velocity_energy * 0.16).clamp(0., 1.),
            contact: (pressure * 0.62 + velocity_energy * 0.26 + self.pulse * 0.12).clamp(0., 1.),
            aura: (self.pulse * 0.38 + active_wave * 0.2 + velocity_energy * 0.2).clamp(0., 1.),
            error_shake,
            chain: self.chain.snapshot(),
        }
    }

    pub fn is_active(self, phase: ProgressMotionPhase) -> bool {
        phase.is_live()
            || (self.follow.target - self.follow.value).abs() > 0.001
            || self.follow.velocity.abs() > 0.006
            || self.pulse > 0.012
            || self.compression > 0.012
            || self.cap_rebound.abs() > 0.012
            || self.chain.is_active()
    }

    fn inject_chain_impulse(&mut self, delta: f32, impulse: f32) {
        self.chain.inject_progress_impulse(delta, impulse);
    }

    fn inject_chain_settle(&mut self, impulse: f32) {
        self.chain.inject_settle_impulse(impulse);
    }

    fn tick_chain(&mut self, phase: ProgressMotionPhase, velocity_energy: f32, dt: f32) {
        let drive = ChainDrive {
            phase,
            display: self.follow.value,
            velocity_energy,
            compression: self.compression,
            cap_rebound: self.cap_rebound,
            pulse: self.pulse,
            frame_scale: 1.,
        };
        self.chain.tick(drive, dt);
    }
}

impl Default for JellyProgressPointChain {
    fn default() -> Self {
        let mut chain = Self {
            pos: [(0., 0.); PROGRESS_CHAIN_POINTS],
            prev: [(0., 0.); PROGRESS_CHAIN_POINTS],
            normal: [(0., 1.); PROGRESS_CHAIN_POINTS],
            control_points: [(0., 0.); PROGRESS_CHAIN_POINTS],
            inv_mass: [1.; PROGRESS_CHAIN_POINTS],
            initialized: false,
        };
        chain.reset();
        chain
    }
}

impl JellyProgressPointChain {
    fn reset(&mut self) {
        for idx in 0..PROGRESS_CHAIN_POINTS {
            let t = idx as f32 / (PROGRESS_CHAIN_POINTS - 1) as f32;
            self.pos[idx] = (t, 0.);
            self.prev[idx] = self.pos[idx];
            self.inv_mass[idx] = if idx == 0 || idx == PROGRESS_CHAIN_POINTS - 1 {
                0.
            } else {
                1.
            };
        }
        self.recompute_geometry();
        self.initialized = true;
    }

    fn snapshot(self) -> JellyProgressChainSnapshot {
        let mut offsets = [0.; PROGRESS_CHAIN_POINTS];
        for (idx, offset) in offsets.iter_mut().enumerate() {
            *offset = self.pos[idx].1.clamp(-0.56, 0.38);
        }

        JellyProgressChainSnapshot {
            offsets,
            positions: self.pos,
            normals: self.normal,
            control_points: self.control_points,
        }
    }

    fn is_active(self) -> bool {
        self.pos.iter().zip(self.prev.iter()).any(|(pos, prev)| {
            pos.1.abs() > 0.006
                || (pos.0 - prev.0).abs() > 0.0004
                || (pos.1 - prev.1).abs() > 0.0004
        })
    }

    fn inject_progress_impulse(&mut self, delta: f32, impulse: f32) {
        self.ensure_initialized();
        let direction = delta.signum();
        for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
            let t = chain_t(idx);
            let arch = (PI * t).sin().max(0.);
            let tail = t.powf(1.35);
            let vertical = (-direction * impulse * arch * 0.082
                + direction * impulse * tail * 0.016)
                .clamp(-0.11, 0.11);
            let lateral = direction * impulse * arch * (t - 0.5) * 0.018;
            self.push_velocity(idx, lateral, vertical);
        }
    }

    fn inject_settle_impulse(&mut self, impulse: f32) {
        self.ensure_initialized();
        for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
            let t = chain_t(idx);
            let arch = (PI * t).sin().max(0.);
            self.push_velocity(idx, 0., (impulse * arch * 0.11).clamp(-0.09, 0.09));
        }
    }

    fn tick(&mut self, mut drive: ChainDrive, dt: f32) {
        self.ensure_initialized();
        let dt = dt.clamp(0.001, 0.05);
        let substeps = (dt / 0.006).ceil().clamp(1., 8.) as usize;
        let step_dt = dt / substeps as f32;
        drive.display = drive.display.clamp(0., 1.);
        drive.velocity_energy = drive.velocity_energy.clamp(0., 1.);
        drive.compression = drive.compression.clamp(0., 1.);
        drive.cap_rebound = drive.cap_rebound.clamp(-1., 1.);
        drive.pulse = drive.pulse.clamp(0., 1.);
        drive.frame_scale = (step_dt / VISUAL_MOTION_DT).clamp(0.35, 1.15);

        for _ in 0..substeps {
            self.integrate(drive);
            for _ in 0..5 {
                self.pin_endpoints();
                self.project_distance(0.72);
                self.project_bending(drive);
                self.flatten_ends(0.26);
            }
            self.pin_endpoints();
            self.clamp_shape();
        }

        self.recompute_geometry();
    }

    fn ensure_initialized(&mut self) {
        if !self.initialized {
            self.reset();
        }
    }

    fn integrate(&mut self, drive: ChainDrive) {
        let drag = match drive.phase {
            ProgressMotionPhase::Validating | ProgressMotionPhase::Running => 0.88,
            ProgressMotionPhase::Completed => 0.82,
            ProgressMotionPhase::Failed | ProgressMotionPhase::Cancelling => 0.8,
            ProgressMotionPhase::Idle => 0.78,
        };
        let spring_y = match drive.phase {
            ProgressMotionPhase::Validating | ProgressMotionPhase::Running => 0.27,
            ProgressMotionPhase::Completed => 0.21,
            ProgressMotionPhase::Failed | ProgressMotionPhase::Cancelling => 0.24,
            ProgressMotionPhase::Idle => 0.18,
        } * drive.frame_scale;
        let spring_x = 0.16 * drive.frame_scale;

        for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
            let t = chain_t(idx);
            let current = self.pos[idx];
            let previous = self.prev[idx];
            let target = drive.target_for_point(t);
            let vx = (current.0 - previous.0) * drag;
            let vy = (current.1 - previous.1) * drag;
            self.prev[idx] = current;
            self.pos[idx] = (
                current.0 + vx + (target.0 - current.0) * spring_x,
                current.1 + vy + (target.1 - current.1) * spring_y,
            );
        }
    }

    fn project_distance(&mut self, strength: f32) {
        let rest_len = 1. / (PROGRESS_CHAIN_POINTS - 1) as f32;
        for idx in 0..PROGRESS_CHAIN_POINTS - 1 {
            let a = self.pos[idx];
            let b = self.pos[idx + 1];
            let delta = (b.0 - a.0, b.1 - a.1);
            let len = (delta.0 * delta.0 + delta.1 * delta.1).sqrt();
            if len <= 0.0001 {
                continue;
            }
            let w_a = self.inv_mass[idx];
            let w_b = self.inv_mass[idx + 1];
            let weight_sum = w_a + w_b;
            if weight_sum <= 0. {
                continue;
            }
            let correction = ((len - rest_len) / len) * strength.clamp(0., 1.);
            let offset = (delta.0 * correction, delta.1 * correction);
            self.pos[idx].0 += offset.0 * (w_a / weight_sum);
            self.pos[idx].1 += offset.1 * (w_a / weight_sum);
            self.pos[idx + 1].0 -= offset.0 * (w_b / weight_sum);
            self.pos[idx + 1].1 -= offset.1 * (w_b / weight_sum);
        }
    }

    fn project_bending(&mut self, drive: ChainDrive) {
        let previous = self.pos;
        let strength = match drive.phase {
            ProgressMotionPhase::Validating | ProgressMotionPhase::Running => 0.055,
            ProgressMotionPhase::Completed => 0.075,
            ProgressMotionPhase::Failed | ProgressMotionPhase::Cancelling => 0.07,
            ProgressMotionPhase::Idle => 0.09,
        };
        for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
            let t = chain_t(idx);
            let arch = (PI * t).sin().max(0.);
            let average = (
                (previous[idx - 1].0 + previous[idx + 1].0) * 0.5,
                (previous[idx - 1].1 + previous[idx + 1].1) * 0.5,
            );
            let local_strength = (strength * (0.65 + arch * 0.55)).clamp(0., 0.14);
            self.pos[idx].0 = motion_lerp(self.pos[idx].0, average.0, local_strength * 0.45);
            self.pos[idx].1 = motion_lerp(self.pos[idx].1, average.1, local_strength);
        }
    }

    fn flatten_ends(&mut self, strength: f32) {
        let strength = strength.clamp(0., 1.);
        for idx in [1, 2, PROGRESS_CHAIN_POINTS - 3, PROGRESS_CHAIN_POINTS - 2] {
            let t = chain_t(idx);
            let edge = (1. - (2. * (t - 0.5).abs()).clamp(0., 1.)).clamp(0., 1.);
            let local = strength * (1. - edge).max(0.35);
            self.pos[idx].1 = motion_lerp(self.pos[idx].1, 0., local);
            self.pos[idx].0 = motion_lerp(self.pos[idx].0, t, local * 0.6);
        }
    }

    fn pin_endpoints(&mut self) {
        self.pos[0] = (0., 0.);
        self.pos[PROGRESS_CHAIN_POINTS - 1] = (1., 0.);
        self.prev[0] = self.pos[0];
        self.prev[PROGRESS_CHAIN_POINTS - 1] = self.pos[PROGRESS_CHAIN_POINTS - 1];
    }

    fn clamp_shape(&mut self) {
        for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
            let t = chain_t(idx);
            self.pos[idx].0 = self.pos[idx].0.clamp(t - 0.045, t + 0.045);
            self.pos[idx].1 = self.pos[idx].1.clamp(-0.56, 0.38);
        }
    }

    fn recompute_geometry(&mut self) {
        let (normal, control_points) = chain_geometry_from_positions(self.pos);
        self.normal = normal;
        self.control_points = control_points;
    }

    fn push_velocity(&mut self, idx: usize, dx: f32, dy: f32) {
        self.prev[idx].0 = (self.prev[idx].0 - dx).clamp(-0.08, 1.08);
        self.prev[idx].1 = (self.prev[idx].1 - dy).clamp(-0.62, 0.46);
    }
}

#[derive(Clone, Copy, Debug)]
struct ChainDrive {
    phase: ProgressMotionPhase,
    display: f32,
    velocity_energy: f32,
    compression: f32,
    cap_rebound: f32,
    pulse: f32,
    frame_scale: f32,
}

impl ChainDrive {
    fn target_for_point(self, t: f32) -> (f32, f32) {
        let arch = (PI * t).sin().max(0.);
        let center_bias = 1. - (2. * (t - 0.5).abs()).clamp(0., 1.);
        let live_factor = if self.phase.is_live() { 1. } else { 0.45 };
        let compression =
            (self.compression + self.pulse * 0.34 + self.velocity_energy * 0.28).clamp(0., 1.);
        let lift = match self.phase {
            ProgressMotionPhase::Failed => arch * (0.034 + compression * 0.072),
            ProgressMotionPhase::Cancelling => arch * (0.02 + compression * 0.055),
            _ => -arch * (0.052 + compression * 0.185) * live_factor,
        };
        let rebound = self.cap_rebound * center_bias * (0.028 + self.velocity_energy * 0.015);
        let cap_follow = (self.display - t).clamp(-0.18, 0.18) * arch * 0.015;
        let x_drag = self.cap_rebound * arch * (t - 0.5) * 0.018 - cap_follow;

        (
            (t + x_drag).clamp(0., 1.),
            (lift + rebound).clamp(-0.5, 0.34),
        )
    }
}

fn chain_geometry_from_positions(positions: ProgressChainPoints) -> ChainGeometry {
    let mut normals = [(0., 1.); PROGRESS_CHAIN_POINTS];
    let mut control_points = [(0., 0.); PROGRESS_CHAIN_POINTS];
    for idx in 0..PROGRESS_CHAIN_POINTS {
        let tangent = if idx == 0 {
            (
                positions[1].0 - positions[0].0,
                positions[1].1 - positions[0].1,
            )
        } else if idx == PROGRESS_CHAIN_POINTS - 1 {
            (
                positions[idx].0 - positions[idx - 1].0,
                positions[idx].1 - positions[idx - 1].1,
            )
        } else {
            (
                positions[idx + 1].0 - positions[idx - 1].0,
                positions[idx + 1].1 - positions[idx - 1].1,
            )
        };
        normals[idx] = normalize_2d((-tangent.1, tangent.0));
    }

    for idx in 0..PROGRESS_CHAIN_POINTS - 1 {
        let a = positions[idx];
        let b = positions[idx + 1];
        let tangent = (b.0 - a.0, b.1 - a.1);
        control_points[idx] = (
            (a.0 + b.0) * 0.5 + tangent.0 * 0.04,
            (a.1 + b.1) * 0.5 + tangent.1 * 0.04,
        );
    }
    control_points[PROGRESS_CHAIN_POINTS - 1] = positions[PROGRESS_CHAIN_POINTS - 1];

    (normals, control_points)
}

fn chain_t(idx: usize) -> f32 {
    idx as f32 / (PROGRESS_CHAIN_POINTS - 1) as f32
}

fn motion_lerp(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t.clamp(0., 1.)
}

fn normalize_2d(vector: (f32, f32)) -> (f32, f32) {
    let len = (vector.0 * vector.0 + vector.1 * vector.1).sqrt();
    if len > 0.0001 {
        (vector.0 / len, vector.1 / len)
    } else {
        (0., 1.)
    }
}

pub fn wave_01(motion_tick: u64, speed: f32) -> f32 {
    ((motion_tick as f32 * speed) % TAU).sin().mul_add(0.5, 0.5)
}

pub fn wave_between(motion_tick: u64, speed: f32, min: f32, max: f32) -> f32 {
    min + (max - min) * wave_01(motion_tick, speed)
}

fn decay_for_dt(base_decay: f32, dt: f32) -> f32 {
    base_decay.clamp(0., 1.).powf(dt / REFERENCE_MOTION_DT)
}

#[allow(dead_code)]
pub fn jelly_rebound(age_ticks: u64, duration_ticks: u64) -> f32 {
    if age_ticks > duration_ticks || duration_ticks == 0 {
        return 0.;
    }

    let t = age_ticks as f32 / duration_ticks as f32;
    ((t * TAU * 1.18).sin().abs() * (1.0 - t)).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::{
        JellyProgressMotionState, JellySwitchMotionState, PROGRESS_CHAIN_POINTS,
        ProgressMotionPhase, VISUAL_MOTION_DT,
    };

    #[test]
    fn progress_chain_keeps_endpoints_pinned_and_moves_middle() {
        let mut motion = JellyProgressMotionState::default();

        motion.set_target_percent(68.);
        for _ in 0..6 {
            motion.tick(ProgressMotionPhase::Running, VISUAL_MOTION_DT);
        }
        let snapshot = motion.snapshot(6, ProgressMotionPhase::Running);

        assert_eq!(snapshot.chain.offsets[0], 0.);
        assert_eq!(snapshot.chain.offsets[PROGRESS_CHAIN_POINTS - 1], 0.);
        assert!(snapshot.chain.offsets[PROGRESS_CHAIN_POINTS / 2].abs() > 0.01);
    }

    #[test]
    fn failed_progress_chain_preserves_target_without_completion() {
        let mut motion = JellyProgressMotionState::default();

        motion.set_target_percent(42.);
        for _ in 0..4 {
            motion.tick(ProgressMotionPhase::Running, VISUAL_MOTION_DT);
        }
        motion.trigger_phase_pulse(ProgressMotionPhase::Failed);
        motion.tick(ProgressMotionPhase::Failed, VISUAL_MOTION_DT);
        let snapshot = motion.snapshot(8, ProgressMotionPhase::Failed);

        assert_eq!(snapshot.target_percent, 42.);
        assert!(snapshot.display_percent < 42.);
        assert!(snapshot.chain.offsets.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn switch_motion_travels_between_endpoints_over_multiple_ticks() {
        let mut motion = JellySwitchMotionState::new(false);

        motion.toggle_to(true);
        motion.tick(VISUAL_MOTION_DT);
        let early = motion.snapshot(1, false);

        assert!(early.progress > 0.);
        assert!(early.progress < 1.);
        assert!(early.velocity > 0.);
    }

    #[test]
    fn switch_motion_endpoint_collision_leaves_squash_and_wiggle() {
        let mut motion = JellySwitchMotionState::new(false);

        motion.toggle_to(true);
        for _ in 0..60 {
            motion.tick(VISUAL_MOTION_DT);
        }
        let snapshot = motion.snapshot(60, true);

        assert_eq!(snapshot.progress, 1.);
        assert!(snapshot.squash_x > 0. || snapshot.squash_y > 0.);
        assert!(snapshot.wiggle_x.abs() > 0. || snapshot.rim_pressure > 0.);
    }
}
