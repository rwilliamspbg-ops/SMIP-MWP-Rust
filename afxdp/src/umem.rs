use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UmemError {
    #[error("mmap failed: {0}")]
    MmapFailed(String),
}

pub struct Umem {
    ptr: NonNull<u8>,
    len: usize,
    frame_size: usize,
    frames: AtomicUsize,
    /// Some only on targets where the region came from the global allocator.
    #[allow(dead_code)]
    layout: Option<std::alloc::Layout>,
}

impl Umem {
    /// Allocate an anonymous memory region for frames.
    ///
    /// On Unix the region is an anonymous `mmap`, which is what a real AF_XDP
    /// UMEM wants. `mmap` and friends are not available in `libc` on Windows,
    /// so that target falls back to a plain heap allocation: the frame
    /// accounting is identical and the crate still compiles and tests, it just
    /// cannot be wired to a kernel datapath there.
    #[cfg(unix)]
    pub fn new(len: usize, frame_size: usize) -> Result<Self, UmemError> {
        let prot = libc::PROT_READ | libc::PROT_WRITE;
        let flags = libc::MAP_ANONYMOUS | libc::MAP_PRIVATE;
        unsafe {
            let p = libc::mmap(std::ptr::null_mut(), len, prot, flags, -1, 0);
            if p == libc::MAP_FAILED {
                return Err(UmemError::MmapFailed(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
            let nn = NonNull::new_unchecked(p as *mut u8);
            let total_frames = if frame_size == 0 { 0 } else { len / frame_size };
            Ok(Umem {
                ptr: nn,
                len,
                frame_size,
                frames: AtomicUsize::new(total_frames),
                layout: None,
            })
        }
    }

    /// Windows fallback: heap-backed region with the same frame accounting.
    #[cfg(not(unix))]
    pub fn new(len: usize, frame_size: usize) -> Result<Self, UmemError> {
        let layout = std::alloc::Layout::from_size_align(len.max(1), 8)
            .map_err(|e| UmemError::MmapFailed(e.to_string()))?;
        // SAFETY: layout has non-zero size and we keep the pointer for Drop.
        let raw = unsafe { std::alloc::alloc(layout) };
        let nn = NonNull::new(raw).ok_or_else(|| UmemError::MmapFailed("alloc".into()))?;
        let total_frames = if frame_size == 0 { 0 } else { len / frame_size };
        Ok(Umem {
            ptr: nn,
            len,
            frame_size,
            frames: AtomicUsize::new(total_frames),
            layout: Some(layout),
        })
    }

    /// Number of frames available (simple counter for demo use)
    pub fn frame_count(&self) -> usize {
        self.frames.load(Ordering::SeqCst)
    }

    /// Return the total number of frames (computed from region length/frame size).
    pub fn total_frames(&self) -> usize {
        if self.frame_size == 0 {
            0
        } else {
            self.len / self.frame_size
        }
    }

    /// A pointer to the base of the UMEM region
    pub fn base_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// Return true if the UMEM region has zero length.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(unix)]
impl Drop for Umem {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.len);
        }
    }
}

#[cfg(not(unix))]
impl Drop for Umem {
    fn drop(&mut self) {
        if let Some(layout) = self.layout {
            // SAFETY: pointer came from alloc with this exact layout.
            unsafe { std::alloc::dealloc(self.ptr.as_ptr(), layout) };
        }
    }
}
