//! An allocator for programs that run the prover.
//!
//! The prover allocates and frees many large tables. Apple's system allocator
//! keeps freed large blocks in a cache, and the cached pages still count
//! toward the process's footprint, the number iOS compares with the app's
//! limit. Giving each large allocation its own mapping returns its pages to
//! the kernel the moment it is freed, so the footprint follows the live data.
//!
//! Only Apple targets (macOS and iOS) take the mapped path, because the cache
//! is Apple's. Everywhere else this is the system allocator.
//!
//! The effect was measured on macOS only: at the reference case the footprint
//! peaked at 113 MB with `DirectLarge` and at 190 MB with the system
//! allocator (crates/crop-proof/HILLCLIMB.md, row 9a). No iOS device was
//! measured. The iPhone Simulator, a Mac process with the Simulator's system
//! libraries, showed no gain. There the app's footprint, up to the end of the
//! first proof of the process and before any proof was checked, peaked at
//! 162 to 173 MB with `DirectLarge` and at 156 to 166 MB with the system
//! allocator, three of those four runs at 156 MB (round 0 of
//! `testMeasureACropProof`, 4 runs each, 2026-11-26).
//!
//! The footprints reported there assume `DirectLarge` is the global allocator
//! of the final binary. A program opts in with
//! `#[global_allocator] static A: crop_proof::alloc::DirectLarge = crop_proof::alloc::DirectLarge;`.
//! `provenance` and `bench` do, and so does `crop-proof-ffi`, the Rust static
//! library of the iOS app.
//!
//! A static library can lose its allocator at link time. Every Rust static
//! library defines the allocator symbols (`__rust_alloc` and the rest). When
//! a program links two of them and strips dead code, as Xcode does, Apple's
//! linker keeps the first library's set and drops the other's with a warning
//! at most. So the app links one Rust static library, and
//! [`large_allocations_are_mapped`] lets a program check its own link when it
//! runs.

use std::alloc::{GlobalAlloc, Layout, System};

/// Whether `DirectLarge` is the global allocator of this program and maps
/// its large blocks. It allocates 2 MiB through the global allocator, frees
/// them, and says whether the mapped path served the allocation. The answer
/// is false on targets that are not Apple's, where nothing is mapped.
///
/// The count it reads rises only inside `DirectLarge`, so another allocator
/// cannot make the answer true.
pub fn large_allocations_are_mapped() -> bool {
    #[cfg(target_vendor = "apple")]
    {
        let before = mapped::made();
        // Without black_box the compiler may drop an allocation nobody reads.
        drop(std::hint::black_box(Vec::<u8>::with_capacity(2 << 20)));
        mapped::made() != before
    }
    #[cfg(not(target_vendor = "apple"))]
    false
}

/// The system allocator, except that on Apple targets allocations of 1 MiB
/// or more are anonymous mappings of their own.
pub struct DirectLarge;

#[cfg(target_vendor = "apple")]
mod mapped {
    use std::alloc::Layout;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mappings made since the program started. Only blocks of `THRESHOLD`
    /// bytes or more add to it, so it costs nothing that can be measured.
    static MADE: AtomicUsize = AtomicUsize::new(0);

    pub fn made() -> usize {
        MADE.load(Ordering::Relaxed)
    }

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
            MADE.fetch_add(1, Ordering::Relaxed);
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
    use std::sync::Mutex;

    use super::*;

    /// Held by the tests that map or read the count of mappings, so one
    /// cannot move the count while the other reads it.
    static MAPPING: Mutex<()> = Mutex::new(());

    /// This test binary runs on the system allocator, as an app does when
    /// the linker drops the allocator of the static library.
    #[test]
    fn a_program_without_the_allocator_maps_nothing() {
        let _alone = MAPPING.lock().unwrap();
        assert!(!large_allocations_are_mapped());
    }

    #[test]
    fn large_blocks_are_zeroed_grow_and_shrink() {
        let _alone = MAPPING.lock().unwrap();
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
