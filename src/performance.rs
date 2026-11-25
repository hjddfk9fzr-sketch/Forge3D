use serde_json::{json, Value};
use std::time::{Duration, Instant};

const WARMUP_FRAMES: usize = 30;

pub struct Benchmark {
    frames: usize,
    started: Instant,
    decode_time: Duration,
    first_frame: Option<Duration>,
    elapsed_frames: usize,
    frame_ms: Vec<f64>,
    submit_ms: Vec<f64>,
}

impl Benchmark {
    pub fn new(frames: usize, started: Instant, decode_time: Duration) -> Result<Self, String> {
        if !(30..=10_000).contains(&frames) {
            return Err("Use between 30 and 10000 benchmark frames.".into());
        }
        Ok(Self {
            frames,
            started,
            decode_time,
            first_frame: None,
            elapsed_frames: 0,
            frame_ms: Vec::with_capacity(frames),
            submit_ms: Vec::with_capacity(frames),
        })
    }

    /// 'submit' is CPU time through update and drawing command submission.
    /// 'frame' also includes the wait for the next frame.
    pub(crate) fn frame(&mut self, submit: Duration, frame: Duration) -> bool {
        self.elapsed_frames += 1;
        self.first_frame
            .get_or_insert_with(|| self.started.elapsed());
        if self.elapsed_frames > WARMUP_FRAMES {
            self.frame_ms.push(milliseconds(frame));
            self.submit_ms.push(milliseconds(submit));
        }
        self.frame_ms.len() == self.frames
    }

    pub(crate) fn report(&mut self, scene: &str, viewport: [u32; 2]) -> Value {
        json!({
            "scene":scene,
            "frames":self.frame_ms.len(),
            "warmup_frames":WARMUP_FRAMES,
            "viewport":viewport,
            "target_os":std::env::consts::OS,
            "target_arch":std::env::consts::ARCH,
            "debug_assertions":cfg!(debug_assertions),
            "decode_ms":milliseconds(self.decode_time),
            "first_frame_ms":self.first_frame.map(milliseconds),
            "frame_ms":percentiles(&mut self.frame_ms),
            "submit_ms":percentiles(&mut self.submit_ms),
            "peak_rss_bytes":peak_rss_bytes(),
        })
    }
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.
}

fn percentiles(samples: &mut [f64]) -> Value {
    samples.sort_unstable_by(f64::total_cmp);
    let at = |p: usize| {
        if samples.is_empty() {
            None
        } else {
            Some(samples[(samples.len() * p).div_ceil(100) - 1])
        }
    };
    json!({"p50":at(50),"p95":at(95),"max":samples.last()})
}

#[cfg(unix)]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage writes to the supplied storage on success.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    let resident = u64::try_from(unsafe { usage.assume_init() }.ru_maxrss).ok()?;
    if cfg!(target_os = "macos") {
        Some(resident)
    } else {
        resident.checked_mul(1024)
    }
}

#[cfg(windows)]
fn peak_rss_bytes() -> Option<u64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    let mut memory = PROCESS_MEMORY_COUNTERS {
        cb: size,
        ..Default::default()
    };
    // The pseudo-handle is valid for this process and needs no closing.
    let result = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut memory, size) };
    (result != 0).then_some(memory.PeakWorkingSetSize as u64)
}

#[cfg(not(any(unix, windows)))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_warms_up_and_reports_sorted_percentiles() {
        let mut benchmark = Benchmark::new(30, Instant::now(), Duration::from_millis(2)).unwrap();
        for _ in 0..WARMUP_FRAMES {
            assert!(!benchmark.frame(Duration::from_secs(1), Duration::from_secs(1)));
        }
        for sample in (1..=30).rev() {
            let elapsed = Duration::from_millis(sample);
            assert_eq!(benchmark.frame(elapsed / 2, elapsed), sample == 1);
        }
        let report = benchmark.report("workshop", [1280, 720]);
        assert_eq!(report["frames"], 30);
        assert_eq!(report["decode_ms"], 2.);
        assert_eq!(report["frame_ms"], json!({"p50":15., "p95":29., "max":30.}));
    }
}
