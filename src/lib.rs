use std::alloc::{GlobalAlloc, Layout};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicUsize};

use crate::count::Counts;
use crate::sample::Sample;

mod count;
pub mod dump;
pub mod elf;
mod sample;
mod state;

pub struct TraceAllocator<A> {
    allocator: A,
    counts: Counts,
}

impl<A> TraceAllocator<A> {
    pub const fn new(allocator: A) -> Self {
        let sample = Sample {
            bytes_to_sample: AtomicUsize::new(0),
        };
        let counts = Counts {
            acc_bytes: AtomicU64::new(0),
            alive_bytes: AtomicUsize::new(0),
            peak_alive_bytes: AtomicUsize::new(0),
            realloc_growth_count: AtomicU64::new(0),
            realloc_shrink_count: AtomicU64::new(0),
            sample,
        };
        Self { allocator, counts }
    }
}

unsafe impl<A> GlobalAlloc for TraceAllocator<A>
where
    A: GlobalAlloc,
{
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.allocator.alloc(layout) };
        if let Some(_gurad) = Tracing::disable()
            && !ptr.is_null()
        {
            self.counts.alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.allocator.dealloc(ptr, layout) }
        if let Some(_gurad) = Tracing::disable() {
            self.counts.dealloc(layout.size());
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.allocator.alloc_zeroed(layout) };
        if let Some(_gurad) = Tracing::disable()
            && !ptr.is_null()
        {
            self.counts.alloc(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = unsafe { self.allocator.realloc(ptr, layout, new_size) };
        if let Some(_gurad) = Tracing::disable()
            && !ptr.is_null()
        {
            let old_size = layout.size();
            if new_size > old_size {
                self.counts.alloc(new_size - old_size);
                self.counts.realloc_growth();
            } else {
                self.counts.dealloc(old_size - new_size);
                self.counts.realloc_shrink();
            }
        }
        ptr
    }
}

thread_local! {
    static TRACING_DISABLED: UnsafeCell<bool> = const { UnsafeCell::new(false) };
}

struct Tracing;

impl Tracing {
    pub fn disable() -> Option<Self> {
        let Ok(disabled) = TRACING_DISABLED.try_with(|it| it.get()) else {
            return None;
        };
        if unsafe { *disabled } {
            None
        } else {
            unsafe { *disabled = true };
            Some(Self)
        }
    }
}

impl Drop for Tracing {
    fn drop(&mut self) {
        let _ = TRACING_DISABLED.try_with(|it| {
            let disabled = it.get();
            unsafe { *disabled = false }
        });
    }
}
