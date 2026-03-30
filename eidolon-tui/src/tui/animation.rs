// src/tui/animation.rs
use std::time::Instant;

/// Tracks animation state for the TUI render loop.
pub struct AnimationState {
    start_time: Instant,
    pub enabled: bool,
    pub fps: u32,
}

impl AnimationState {
    pub fn new(enabled: bool, fps: u32) -> Self {
        Self {
            start_time: Instant::now(),
            enabled,
            fps,
        }
    }

    /// Elapsed time in seconds since animation started.
    pub fn elapsed_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Returns a value oscillating between 0.0 and 1.0 on the given cycle period (seconds).
    pub fn pulse(&self, period_secs: f64) -> f64 {
        if !self.enabled {
            return 0.5;
        }
        let t = self.elapsed_secs() % period_secs;
        // Sine wave: 0->1->0 over the period
        (std::f64::consts::PI * t / period_secs).sin()
    }

    /// Interpolate between two RGB colors based on a 0.0-1.0 factor.
    pub fn lerp_color(
        start: (u8, u8, u8),
        end: (u8, u8, u8),
        factor: f64,
    ) -> (u8, u8, u8) {
        let f = factor.clamp(0.0, 1.0) as f32;
        let r = start.0 as f32 + (end.0 as f32 - start.0 as f32) * f;
        let g = start.1 as f32 + (end.1 as f32 - start.1 as f32) * f;
        let b = start.2 as f32 + (end.2 as f32 - start.2 as f32) * f;
        (r as u8, g as u8, b as u8)
    }

    /// Returns frame duration in milliseconds.
    pub fn frame_duration_ms(&self) -> u64 {
        1000 / self.fps.max(1) as u64
    }
}
