use std::cell::UnsafeCell;
use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

use backtrace::Backtrace;

use crate::count::Count;

struct Config {
    pub dump: Mutex<BufWriter<File>>,
    pub sample_period_bytes: usize,
}

static CFG: LazyLock<Option<Config>> = LazyLock::new(|| {
    Some(Config {
        dump: {
            let cap = env::var("TRACE_ALLOC_SAMPLE_DUMP_BUF_SIZE_BYTES").ok()?;
            let cap = str::parse::<usize>(&cap).ok()?;

            let path = env::var("TRACE_ALLOC_SAMPLE_DUMP_PATH").ok()?;
            let path = Path::new(&path);
            let file = File::create(path).ok()?;

            Mutex::new(BufWriter::with_capacity(cap, file))
        },
        sample_period_bytes: {
            let bytes = env::var("TRACE_ALLOC_SAMPLE_PERIOD_BYTES").ok()?;
            str::parse::<usize>(&bytes).ok()?
        },
    })
});

thread_local! {
    static SAMPLING: UnsafeCell<bool> = const { UnsafeCell::new(false) };
}

struct SamplingGuard;

impl SamplingGuard {
    pub fn take() -> Option<Self> {
        let Ok(sampling) = SAMPLING.try_with(|it| it.get()) else {
            return None;
        };
        if unsafe { *sampling } {
            None
        } else {
            unsafe { *sampling = true };
            Some(Self)
        }
    }
}

impl Drop for SamplingGuard {
    fn drop(&mut self) {
        let _ = SAMPLING.try_with(|it| {
            let sampling = it.get();
            unsafe { *sampling = false }
        });
    }
}

pub struct Sample {
    pub bytes_to_sample: AtomicUsize,
}

impl Sample {
    pub fn watch(&self, size: usize, count: &Count) {
        let Some(_gurad) = SamplingGuard::take() else {
            return;
        };

        let Some(cfg) = CFG.as_ref() else { return };
        let left = self.bytes_to_sample.fetch_sub(size, Ordering::Relaxed);
        if left <= size {
            self.bytes_to_sample
                .store(cfg.sample_period_bytes, Ordering::Relaxed);
            let (peak, acc, alive, growth, shink) = count.load();
            let mut dump = cfg.dump.lock().expect("failed to get dump lock");
            let _ = writeln!(dump, "@{peak} {acc} {alive} {growth} {shink}");

            let bt = Backtrace::new_unresolved();
            let addrs = bt.frames().iter().map(|f| f.ip().addr());
            let _ = writeln!(dump, "!{peak} {acc} {alive} {growth} {shink}");
        }
    }
}
