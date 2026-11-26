//! An allocator for programs that run the prover.
//!
//! The prover allocates and frees many large tables. Apple's system allocator
//! keeps freed large blocks in a cache, and the cached pages still count
//! toward the process's footprint, the number iOS compares with the app's
//! limit. Giving each large allocation its own mapping returns its pages to
//! the kernel the moment it is freed, so the footprint follows the live data.
//!
//! Only Apple targets (macOS and iOS) take the mapped path: the cache is
//! Apple's and the effect was measured nowhere else. Everywhere else this is
//! the system allocator.
//!
//! The footprints reported for the prover (crates/crop-proof/HILLCLIMB.md)
//! assume `DirectLarge` is the global allocator of the final binary, the iOS
//! static library included. A program opts in with
//! `#[global_allocator] static A: crop_proof::alloc::DirectLarge = crop_proof::alloc::DirectLarge;`.

use std::alloc::{GlobalAlloc, Layout, System};

/// The system allocator, except that on Apple targets allocations of 1 MiB
/// or more are anonymous mappings of their own.
pub struct DirectLarge;

#[cfg(target_vendor = "apple")]
mod mapped {
    use std::alloc::Layout;

    /// Lowering it to 64 KiB took the reference case's footprint from 113 MB
    /// to 109 MB (medians of 8 interleaved probe runs on a loaded machine,
    /// prove times within the noise): too little to map every table that small.
    const THRESHOLD: usize = 1 << 20;
    /// Mappings start on a page boundary, which is at least this.
    const MAX_ALIGN: usize = 4096;

    pub fn is_direct(layout: Layout) -> bool {
        layout.size() >= THRESHOLD && layout.align() <= MAX_ALIGN
    }

    /// A fresh zeroed mapping of `size` bytes, or null.
    ///
    /// # Safety
    /// `size` is not zero.
    pub unsafe fn map(size: usize) -> *mut u8 {
        let pointer = libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        );
        if pointer == libc::MAP_FAILED {
            std::ptr::null_mut()
        } else {
            pointer.cast()
        }
    }

    /// # Safety
    /// `pointer` came from `map(size)` and is not used again.
    pub unsafe fn unmap(pointer: *mut u8, size: usize) {
        libc::munmap(pointer.cast(), size);
    }
}

// SAFETY: a layout takes the same path, by its size and alignment, when it is
// allocated and when it is freed. Small layouts go to the system allocator
// unchanged. A large one gets a mapping of its own: page-aligned, zeroed,
// and unmapped with the length it was mapped with.
#[cfg(target_vendor = "apple")]
unsafe impl GlobalAlloc for DirectLarge {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if mapped::is_direct(layout) {
            mapped::map(layout.size())
        } else {
            System.alloc(layout)
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if mapped::is_direct(layout) {
            mapped::map(layout.size())
        } else {
            System.alloc_zeroed(layout)
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if mapped::is_direct(layout) {
            mapped::unmap(pointer, layout.size());
        } else {
            System.dealloc(pointer, layout);
        }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller guarantees `new_size` is valid for this alignment.
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        if !mapped::is_direct(layout) && !mapped::is_direct(new_layout) {
            return System.realloc(pointer, layout, new_size);
        }
        let new = self.alloc(new_layout);
        if !new.is_null() {
            std::ptr::copy_nonoverlapping(pointer, new, layout.size().min(new_size));
            self.dealloc(pointer, layout);
        }
        new
    }
}

// SAFETY: every call goes to the system allocator unchanged.
#[cfg(not(target_vendor = "apple"))]
unsafe impl GlobalAlloc for DirectLarge {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout)
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        System.realloc(pointer, layout, new_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_blocks_are_zeroed_grow_and_shrink() {
        let allocator = DirectLarge;
        let small = Layout::from_size_align(1 << 10, 8).unwrap();
        let large = Layout::from_size_align(3 << 20, 8).unwrap();
        // SAFETY: each pointer is used within its layout and freed once with
        // the layout it currently has.
        unsafe {
            let pointer = allocator.alloc_zeroed(large);
            assert!(!pointer.is_null());
            let block = std::slice::from_raw_parts_mut(pointer, large.size());
            assert!(block.iter().all(|&byte| byte == 0));
            block[0] = 7;
            block[large.size() - 1] = 9;

            let grown = allocator.realloc(pointer, large, 5 << 20);
            assert_eq!(*grown, 7);
            assert_eq!(*grown.add(large.size() - 1), 9);
            let grown_layout = Layout::from_size_align(5 << 20, 8).unwrap();

            // Across the threshold, into the system allocator and back.
            let shrunk = allocator.realloc(grown, grown_layout, small.size());
            assert_eq!(*shrunk, 7);
            let back = allocator.realloc(shrunk, small, large.size());
            assert_eq!(*back, 7);
            allocator.dealloc(back, large);
        }
    }
}
