use std::time::{Duration, Instant};

pub struct Timer {
    start: Instant,
    last_tick: Instant,
    delta: Duration,
}

impl Timer {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last_tick: now,
            delta: Duration::ZERO,
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.delta = now - self.last_tick;
        self.last_tick = now;
    }

    pub fn elapsed_secs(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }

    pub fn delta_secs(&self) -> f32 {
        self.delta.as_secs_f32()
    }

    pub fn fps(&self) -> f32 {
        let dt = self.delta_secs();
        if dt > 0.0 { 1.0 / dt } else { 0.0 }
    }
}
