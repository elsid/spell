use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct FpsMovingAverage {
    max_frames: usize,
    max_interval: Duration,
    times: VecDeque<Instant>,
    sum_duration: Duration,
}

impl FpsMovingAverage {
    pub fn new(max_frames: usize, max_interval: Duration) -> Self {
        assert!(max_frames >= 3);
        Self {
            max_frames,
            max_interval,
            times: VecDeque::new(),
            sum_duration: Duration::new(0, 0),
        }
    }

    pub fn add(&mut self, time: Instant) {
        if self.times.len() >= self.max_frames
            || (self.times.len() >= 3 && self.sum_duration >= self.max_interval) {
            if let Some(removed) = self.times.pop_front() {
                if let Some(first) = self.times.front() {
                    self.sum_duration -= *first - removed;
                }
            }
        }
        if let Some(last) = self.times.back() {
            self.sum_duration += time - *last;
        }
        self.times.push_back(time);
    }

    pub fn get(&self) -> f64 {
        if self.times.len() >= 2 {
            (self.times.len() - 1) as f64 / self.sum_duration.as_secs_f64()
        } else {
            0.0
        }
    }
}

pub struct DurationMovingAverage {
    max_frames: usize,
    max_interval: Duration,
    durations: VecDeque<Duration>,
    sum_duration: Duration,
}

impl DurationMovingAverage {
    pub fn new(max_frames: usize, max_interval: Duration) -> Self {
        assert!(max_frames >= 2);
        Self {
            max_frames,
            max_interval,
            durations: VecDeque::new(),
            sum_duration: Duration::new(0, 0),
        }
    }

    pub fn add(&mut self, duration: Duration) {
        if self.durations.len() >= self.max_frames
            || (self.durations.len() >= 2 && self.sum_duration >= self.max_interval) {
            if let Some(removed) = self.durations.pop_front() {
                self.sum_duration -= removed;
            }
        }
        self.durations.push_back(duration);
        self.sum_duration += duration;
    }

    pub fn get(&self) -> f64 {
        if self.durations.len() >= 1 {
            self.sum_duration.as_secs_f64() / self.durations.len() as f64
        } else {
            0.0
        }
    }
}
