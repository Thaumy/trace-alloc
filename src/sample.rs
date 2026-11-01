use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

use backtrace::Backtrace;

use crate::count::Counts;
use crate::state::STATE;

pub struct Sample {
    pub bytes_to_sample: AtomicUsize,
}

impl Sample {
    pub fn watch(&self, size: usize, counts: &Counts) {
        let Some(state) = STATE.as_ref() else { return };

        let mut left = self.bytes_to_sample.load(Ordering::Relaxed);
        while let Err(actual) = self.bytes_to_sample.compare_exchange_weak(
            left,
            if left > size {
                left - size
            } else {
                state.sample_period_bytes
            },
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            left = actual;
        }
        if left > size {
            return;
        }

        let (peak, acc, alive, growth, shrink) = counts.load();
        let mut dump = state.dump.lock().expect("failed to get dump lock");
        let _ = dump.write_all(b"c");
        let _ = dump.write_all(&peak.to_le_bytes());
        let _ = dump.write_all(&acc.to_le_bytes());
        let _ = dump.write_all(&alive.to_le_bytes());
        let _ = dump.write_all(&growth.to_le_bytes());
        let _ = dump.write_all(&shrink.to_le_bytes());

        let bt = Backtrace::new_unresolved();
        let frames = bt.frames();
        let _ = dump.write_all(b"a");
        let _ = dump.write_all(&frames.len().to_le_bytes());
        for addr in frames.iter().map(|f| f.ip().addr() as u64) {
            let _ = dump.write_all(&addr.to_le_bytes());
        }
    }
}
