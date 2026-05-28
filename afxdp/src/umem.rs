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
}

impl Umem {
    /// Allocate an anonymous memory region for frames.
    pub fn new(len: usize, frame_size: usize) -> Result<Self, UmemError> {
        // Use a simple anonymous mmap for the frame pool. This is portable and
        // sufficient for testing and for wiring into a real AF_XDP backing later.
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
            Ok(Umem {
                ptr: nn,
                len,
                frame_size,
                frames: AtomicUsize::new(0),
            })
        }
    }

    /// Number of frames available (simple counter for demo use)
    pub fn frame_count(&self) -> usize {
        self.frames.load(Ordering::SeqCst)
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

impl Drop for Umem {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.len);
        }
    }
}
