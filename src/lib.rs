use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicU64, AtomicUsize};

use crate::count::Count;
use crate::sample::Sample;

mod count;
mod sample;

pub struct TraceAllocator<A> {
    allocator: A,
    count: Count,
}

impl<A> TraceAllocator<A> {
    pub const fn new(allocator: A) -> Self {
        let sample = Sample {
            bytes_to_sample: AtomicUsize::new(0),
        };
        let count = Count {
            acc_bytes: AtomicU64::new(0),
            alive_bytes: AtomicUsize::new(0),
            peak_alive_bytes: AtomicUsize::new(0),
            sample,
        };
        Self { allocator, count }
    }
}

unsafe impl<A> GlobalAlloc for TraceAllocator<A>
where
    A: GlobalAlloc,
{
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.allocator.alloc(layout) };
        if !ptr.is_null() {
            self.count.alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.allocator.dealloc(ptr, layout) }
        self.count.dealloc(layout.size());
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.allocator.alloc_zeroed(layout) };
        if !ptr.is_null() {
            self.count.alloc(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = unsafe { self.allocator.realloc(ptr, layout, new_size) };
        if !ptr.is_null() {
            let old_size = layout.size();
            if new_size > old_size {
                self.count.alloc(new_size - old_size);
            } else {
                self.count.dealloc(old_size - new_size);
            }
        }
        ptr
    }
}
