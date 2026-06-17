use std::f32::consts::TAU;

pub const PROGRESS_CHAIN_POINTS: usize = 9;
pub const VISUAL_MOTION_TICK_MS: u64 = 8;
pub const VISUAL_MOTION_DT: f32 = VISUAL_MOTION_TICK_MS as f32 / 1000.;

const REFERENCE_MOTION_DT: f32 = 0.072;
const CONTROL_IMPULSE_SECONDS: f32 = 0.52;
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
        age_ticks: Option<u64>,
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

        let Some(age_ticks) = age_ticks else {
            return Self {
                gloss_phase: breath,
                aura: breath * 0.7,
                error_shake,
                ..Self::default()
            };
        };

        let impulse_ticks = (CONTROL_IMPULSE_SECONDS / VISUAL_MOTION_DT).max(1.);
        let t = (age_ticks as f32 / impulse_ticks).clamp(0., 1.);
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
    chain_offsets: [f32; PROGRESS_CHAIN_POINTS],
    chain_velocity: [f32; PROGRESS_CHAIN_POINTS],
}

#[derive(Clone, Copy, Debug)]
pub struct JellyProgressChainSnapshot {
    pub offsets: [f32; PROGRESS_CHAIN_POINTS],
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
            chain_offsets: [0.; PROGRESS_CHAIN_POINTS],
            chain_velocity: [0.; PROGRESS_CHAIN_POINTS],
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
        self.chain_offsets = [0.; PROGRESS_CHAIN_POINTS];
        self.chain_velocity = [0.; PROGRESS_CHAIN_POINTS];
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
            chain: JellyProgressChainSnapshot {
                offsets: self.chain_offsets,
            },
        }
    }

    pub fn is_active(self, phase: ProgressMotionPhase) -> bool {
        phase.is_live()
            || (self.follow.target - self.follow.value).abs() > 0.001
            || self.follow.velocity.abs() > 0.006
            || self.pulse > 0.012
            || self.compression > 0.012
            || self.cap_rebound.abs() > 0.012
            || self
                .chain_offsets
                .iter()
                .chain(self.chain_velocity.iter())
                .any(|value| value.abs() > 0.006)
    }

    fn inject_chain_impulse(&mut self, delta: f32, impulse: f32) {
        let direction = delta.signum();
        for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
            let t = idx as f32 / (PROGRESS_CHAIN_POINTS - 1) as f32;
            let arch = (std::f32::consts::PI * t).sin().max(0.);
            let tail = t.powf(1.4);
            self.chain_velocity[idx] += (-direction * impulse * arch * 0.92
                + direction * impulse * tail * 0.18)
                .clamp(-0.9, 0.9);
        }
    }

    fn inject_chain_settle(&mut self, impulse: f32) {
        for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
            let t = idx as f32 / (PROGRESS_CHAIN_POINTS - 1) as f32;
            let arch = (std::f32::consts::PI * t).sin().max(0.);
            self.chain_velocity[idx] = (self.chain_velocity[idx] + impulse * arch).clamp(-0.8, 0.8);
        }
    }

    fn tick_chain(&mut self, phase: ProgressMotionPhase, velocity_energy: f32, dt: f32) {
        let live_factor = if phase.is_live() { 1. } else { 0.45 };
        let damping = match phase {
            ProgressMotionPhase::Completed => 8.6,
            ProgressMotionPhase::Failed => 9.2,
            ProgressMotionPhase::Cancelling => 9.8,
            ProgressMotionPhase::Idle => 10.,
            ProgressMotionPhase::Validating | ProgressMotionPhase::Running => 7.4,
        };
        let compression =
            (self.compression + self.pulse * 0.36 + velocity_energy * 0.28).clamp(0., 1.);

        for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
            let t = idx as f32 / (PROGRESS_CHAIN_POINTS - 1) as f32;
            let arch = (std::f32::consts::PI * t).sin().max(0.);
            let center_bias = 1. - (2. * (t - 0.5).abs()).clamp(0., 1.);
            let target = match phase {
                ProgressMotionPhase::Failed => arch * (0.035 + compression * 0.045),
                ProgressMotionPhase::Cancelling => arch * (0.02 + compression * 0.035),
                _ => -arch * (0.045 + compression * 0.15) * live_factor,
            } + self.cap_rebound * center_bias * 0.025;
            let spring = (target - self.chain_offsets[idx]) * 24.;
            let damp = -self.chain_velocity[idx] * damping;
            self.chain_velocity[idx] =
                (self.chain_velocity[idx] + (spring + damp) * dt).clamp(-1.2, 1.2);
            self.chain_offsets[idx] =
                (self.chain_offsets[idx] + self.chain_velocity[idx] * dt).clamp(-0.44, 0.28);
        }

        for _ in 0..2 {
            let previous = self.chain_offsets;
            for idx in 1..PROGRESS_CHAIN_POINTS - 1 {
                let neighbor_average = (previous[idx - 1] + previous[idx + 1]) * 0.5;
                self.chain_offsets[idx] =
                    (self.chain_offsets[idx] * 0.78 + neighbor_average * 0.22).clamp(-0.44, 0.28);
            }
        }

        self.chain_offsets[0] = 0.;
        self.chain_offsets[PROGRESS_CHAIN_POINTS - 1] = 0.;
        self.chain_velocity[0] = 0.;
        self.chain_velocity[PROGRESS_CHAIN_POINTS - 1] = 0.;
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
