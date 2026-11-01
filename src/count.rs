use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::sample::Sample;

pub struct Counts {
    pub acc_bytes: AtomicU64,
    pub alive_bytes: AtomicUsize,
    pub peak_alive_bytes: AtomicUsize,
    pub realloc_growth_count: AtomicU64,
    pub realloc_shrink_count: AtomicU64,
    pub sample: Sample,
}

impl Counts {
    pub fn alloc(&self, size: usize) {
        self.acc_bytes.fetch_add(size as _, Ordering::Relaxed);
        let old_alive_bytes = self.alive_bytes.fetch_add(size as _, Ordering::Relaxed);
        self.peak_alive_bytes
            .fetch_max(old_alive_bytes + size, Ordering::Relaxed);
        self.sample.watch(size, self);
    }

    pub fn dealloc(&self, size: usize) {
        self.alive_bytes.fetch_sub(size as _, Ordering::Relaxed);
        self.sample.watch(size, self);
    }

    pub fn realloc_growth(&self) {
        self.realloc_growth_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn realloc_shrink(&self) {
        self.realloc_shrink_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn load(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.acc_bytes.load(Ordering::Relaxed),
            self.alive_bytes.load(Ordering::Relaxed) as _,
            self.peak_alive_bytes.load(Ordering::Relaxed) as _,
            self.realloc_growth_count.load(Ordering::Relaxed),
            self.realloc_shrink_count.load(Ordering::Relaxed),
        )
    }
}
