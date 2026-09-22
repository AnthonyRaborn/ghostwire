//! The rig's own CPU use, shown as "neural load".

use std::time::{Duration, Instant};

const SAMPLE_EVERY: Duration = Duration::from_secs(2);

pub struct CpuMeter {
    last_wall: Instant,
    last_cpu: Duration,
    percent: f32,
}

impl CpuMeter {
    pub fn new() -> Self {
        Self {
            last_wall: Instant::now(),
            last_cpu: process_cpu_time(),
            percent: 0.0,
        }
    }

    pub fn percent(&self) -> f32 {
        self.percent
    }

    /// Cheap to call every frame; only resamples every couple of seconds.
    pub fn sample(&mut self) {
        let wall = self.last_wall.elapsed();
        if wall < SAMPLE_EVERY {
            return;
        }
        let cpu = process_cpu_time();
        self.percent = cpu.saturating_sub(self.last_cpu).as_secs_f32() / wall.as_secs_f32() * 100.0;
        self.last_wall = Instant::now();
        self.last_cpu = cpu;
    }
}

/// User + system CPU time for the whole process, across all threads.
#[cfg(unix)]
fn process_cpu_time() -> Duration {
    // SAFETY: getrusage only writes into the struct we hand it.
    let usage = unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) != 0 {
            return Duration::ZERO;
        }
        usage
    };
    let tv = |t: libc::timeval| Duration::new(t.tv_sec as u64, t.tv_usec as u32 * 1_000);
    tv(usage.ru_utime) + tv(usage.ru_stime)
}

#[cfg(not(unix))]
fn process_cpu_time() -> Duration {
    Duration::ZERO
}
