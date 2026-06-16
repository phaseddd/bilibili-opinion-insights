use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct SpringToken {
    pub mass: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl SpringToken {
    #[allow(dead_code)]
    pub const fn new(mass: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            mass,
            stiffness,
            damping,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
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
        self.value += self.velocity * dt;
    }
}

pub fn wave_01(motion_tick: u64, speed: f32) -> f32 {
    ((motion_tick as f32 * speed) % TAU).sin().mul_add(0.5, 0.5)
}

pub fn wave_between(motion_tick: u64, speed: f32, min: f32, max: f32) -> f32 {
    min + (max - min) * wave_01(motion_tick, speed)
}

pub fn jelly_rebound(age_ticks: u64, duration_ticks: u64) -> f32 {
    if age_ticks > duration_ticks || duration_ticks == 0 {
        return 0.;
    }

    let t = age_ticks as f32 / duration_ticks as f32;
    ((t * TAU * 1.18).sin().abs() * (1.0 - t)).clamp(0., 1.)
}
