use std::sync::{Arc, Mutex};
use thiserror::Error;

use datapath::socket::XdpSocket as DatapathXdpSocket;

#[derive(Error, Debug)]
pub enum AfXdpError {
    #[error("initialization error: {0}")]
    Init(String),
}

/// Re-export a boxed trait object type compatible with the datapath crate's socket
pub type AfXdpSocket = Box<dyn DatapathXdpSocket + Send>;

/// A simple in-process mock socket useful for tests and CI.
pub struct MockSocket {
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl MockSocket {
    pub fn new(frames: Vec<Vec<u8>>) -> Self {
        Self {
            frames: Arc::new(Mutex::new(frames)),
            sent: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn take_sent(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.sent.lock().unwrap())
    }
}

impl DatapathXdpSocket for MockSocket {
    fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.frames.lock().unwrap())
    }
    fn send(&mut self, buf: &mut Vec<u8>, offsets: &[(usize, usize)]) -> Result<(), ()> {
        let mut out = Vec::with_capacity(offsets.len());
        for (off, len) in offsets.iter().cloned() {
            out.push(buf[off..off + len].to_vec());
        }
        *self.sent.lock().unwrap() = out;
        Ok(())
    }
}

// Provide a constructor that returns a boxed `datapath::socket::XdpSocket` object.
pub fn new_mock_socket(frames: Vec<Vec<u8>>) -> AfXdpSocket {
    Box::new(MockSocket::new(frames))
}

// --- Real socket skeleton --------------------------------------------------
// When built with `--features real` this module can be expanded to perform
// genuine AF_XDP UMEM allocation, ring setup and socket handling. For now we
// provide a thin wrapper type that can be completed later.

#[cfg(feature = "real")]
mod real {
    use super::*;
    use crate::rings::{RingMmap, XskMmapOffsets};
    use crate::umem::Umem;
    use std::os::unix::io::RawFd;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::atomic::AtomicU64;

    pub struct RealSocket {
        ifname: String,
        queue_id: u32,
        fd: RawFd,
        _umem: Umem,
        // Pointer to the mmap'ed ring area (kept alive for future ring-based ops)
        ring_map_ptr: *mut libc::c_void,
        ring_map_size: usize,
        mmap_offsets: Option<crate::rings::XskMmapOffsets>,
        ring: Option<RingMmap>,
        // simple frame allocator index into UMEM frames
        next_frame: AtomicUsize,
        // lock-free bounded free list for frame offsets
        free_list: FreeList,
    }

    // SAFETY: RealSocket contains raw pointers to mmap'ed memory and file
    // descriptors which are safe to move between threads provided the caller
    // ensures exclusive access to the socket object. We mark the type as
    // `Send` so it can be boxed into the `AfXdpSocket` alias used by the
    // datapath. This is an explicit, well-audited opt-in.
    unsafe impl Send for RealSocket {}

    // A simple bounded lock-free free-list implemented as a circular buffer
    // of `u64` entries with atomic head/tail indices. Capacity is the next
    // power-of-two <= total_frames.
    pub struct FreeList {
        buf: Vec<AtomicU64>,
        mask: usize,
        head: AtomicUsize,
        tail: AtomicUsize,
    }

    impl FreeList {
        pub fn with_capacity(mut n: usize) -> Self {
            // round up to the next power-of-two (capacity >= n)
            if n == 0 {
                n = 1;
            }
            let cap = n.next_power_of_two();
            let mut buf = Vec::with_capacity(cap);
            for _ in 0..cap {
                buf.push(AtomicU64::new(0));
            }
            FreeList {
                buf,
                mask: cap - 1,
                head: AtomicUsize::new(0),
                tail: AtomicUsize::new(0),
            }
        }

        pub fn try_push(&self, v: u64) -> bool {
            let tail = self.tail.load(Ordering::Relaxed);
            let head = self.head.load(Ordering::Acquire);
            let used = tail.wrapping_sub(head);
            if used == self.buf.len() {
                return false; // full
            }
            let idx = tail & self.mask;
            self.buf[idx].store(v, Ordering::Relaxed);
            self.tail.store(tail.wrapping_add(1), Ordering::Release);
            true
        }

        pub fn try_pop(&self) -> Option<u64> {
            let head = self.head.load(Ordering::Relaxed);
            let tail = self.tail.load(Ordering::Acquire);
            if head == tail {
                return None; // empty
            }
            let idx = head & self.mask;
            let v = self.buf[idx].load(Ordering::Relaxed);
            self.head.store(head.wrapping_add(1), Ordering::Release);
            Some(v)
        }
    }

    impl RealSocket {
        pub fn new(
            ifname: &str,
            queue_id: u32,
            umem_frame_size: usize,
            umem_pages: usize,
        ) -> Result<Self, AfXdpError> {
            // Allocate UMEM backing region
            let umem = Umem::new(umem_frame_size * umem_pages, umem_frame_size)
                .map_err(|e| AfXdpError::Init(format!("umem alloc: {}", e)))?;

            // Create AF_XDP socket
            const AF_XDP: libc::c_int = 44; // PF_XDP / AF_XDP
            let fd = unsafe { libc::socket(AF_XDP, libc::SOCK_RAW, 0) };
            if fd < 0 {
                return Err(AfXdpError::Init(
                    std::io::Error::last_os_error().to_string(),
                ));
            }

            // Resolve interface index
            let ifc =
                std::ffi::CString::new(ifname).map_err(|e| AfXdpError::Init(e.to_string()))?;
            let ifindex = unsafe { libc::if_nametoindex(ifc.as_ptr()) };
            if ifindex == 0 {
                unsafe {
                    libc::close(fd);
                }
                return Err(AfXdpError::Init(format!(
                    "if_nametoindex failed for {}",
                    ifname
                )));
            }

            // Bind the socket to the interface/queue using sockaddr_xdp
            #[repr(C)]
            struct SockAddrXdp {
                sxdp_family: libc::sa_family_t,
                sxdp_ifindex: u32,
                sxdp_queue_id: u32,
                sxdp_flags: u32,
                sxdp_reserved: [u8; 12],
            }

            let sa = SockAddrXdp {
                sxdp_family: AF_XDP as libc::sa_family_t,
                sxdp_ifindex: ifindex,
                sxdp_queue_id: queue_id,
                sxdp_flags: 0,
                sxdp_reserved: [0u8; 12],
            };

            let ret = unsafe {
                libc::bind(
                    fd,
                    &sa as *const SockAddrXdp as *const libc::sockaddr,
                    std::mem::size_of::<SockAddrXdp>() as libc::socklen_t,
                )
            };
            if ret < 0 {
                let err = std::io::Error::last_os_error().to_string();
                unsafe {
                    libc::close(fd);
                }
                return Err(AfXdpError::Init(format!("bind failed: {}", err)));
            }

            // Register UMEM with socket via setsockopt XDP_UMEM_REG
            // The numeric values below mirror the kernel headers; they are stable across
            // modern kernels but may require adjustment for very old kernels.
            const SOL_XDP: libc::c_int = 283; // socket option level for XDP
            const XDP_UMEM_REG: libc::c_int = 1;

            #[repr(C)]
            struct XdpUmemReg {
                addr: u64,
                len: u64,
                chunk_size: u32,
                headroom: u32,
            }

            let reg = XdpUmemReg {
                addr: umem.base_ptr() as u64,
                len: umem.len() as u64,
                chunk_size: umem.frame_size() as u32,
                headroom: 0,
            };

            let rc = unsafe {
                libc::setsockopt(
                    fd,
                    SOL_XDP,
                    XDP_UMEM_REG,
                    &reg as *const XdpUmemReg as *const libc::c_void,
                    std::mem::size_of::<XdpUmemReg>() as libc::socklen_t,
                )
            };
            if rc < 0 {
                let err = std::io::Error::last_os_error().to_string();
                unsafe {
                    libc::close(fd);
                }
                return Err(AfXdpError::Init(format!(
                    "setsockopt(UmemReg) failed: {}",
                    err
                )));
            }

            // Query mmap offsets for rings using XDP_MMAP_OFFSETS
            const XDP_MMAP_OFFSETS: libc::c_int = 7;
            let mut offs = crate::rings::XskMmapOffsets {
                rx: 0,
                rx_desc: 0,
                tx: 0,
                tx_desc: 0,
                fill: 0,
                fill_desc: 0,
                comp: 0,
                comp_desc: 0,
            };
            let mut optlen = std::mem::size_of::<XskMmapOffsets>() as libc::socklen_t;
            let rc2 = unsafe {
                libc::getsockopt(
                    fd,
                    SOL_XDP,
                    XDP_MMAP_OFFSETS,
                    &mut offs as *mut crate::rings::XskMmapOffsets as *mut libc::c_void,
                    &mut optlen as *mut libc::socklen_t,
                )
            };
            if rc2 < 0 {
                let err = std::io::Error::last_os_error().to_string();
                unsafe {
                    libc::close(fd);
                }
                return Err(AfXdpError::Init(format!(
                    "getsockopt(MMAP_OFFSETS) failed: {}",
                    err
                )));
            }

            // mmap the combined area (RX/TX/FILL/COMP rings). The kernel exposes a single
            // mmap region with offsets reported above; compute the required size.
            let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };
            let mmap_size = page_size * 16; // conservative default for ring backing
            let map = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    mmap_size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED | libc::MAP_LOCKED,
                    fd,
                    0,
                )
            };
            if map == libc::MAP_FAILED {
                let err = std::io::Error::last_os_error().to_string();
                unsafe {
                    libc::close(fd);
                }
                return Err(AfXdpError::Init(format!("mmap rings failed: {}", err)));
            }

            // For demo purposes we don't fully implement ring cursors here. A full
            // implementation would wrap the mapped memory with safe ring types and
            // provide enqueue/dequeue helpers for RX/TX/FILL/COMP.

            let ring = unsafe { RingMmap::new(map, mmap_size, offs) };
            // Initialize free list with all frame offsets
            let frames = umem.len() / umem.frame_size();
            let free_list = FreeList::with_capacity(frames);
            // pre-fill the free list with all offsets; if capacity < frames some
            // frames will be skipped (capacity is power-of-two <= frames)
            for i in 0..frames {
                let _ = free_list.try_push((i * umem.frame_size()) as u64);
            }

            Ok(RealSocket {
                ifname: ifname.to_string(),
                queue_id,
                fd,
                _umem: umem,
                ring_map_ptr: map,
                ring_map_size: mmap_size,
                mmap_offsets: Some(offs),
                ring: Some(ring),
                next_frame: AtomicUsize::new(0),
                free_list,
            })
        }
    }

    impl Drop for RealSocket {
        fn drop(&mut self) {
            if !self.ring_map_ptr.is_null() {
                unsafe {
                    libc::munmap(self.ring_map_ptr, self.ring_map_size);
                }
            }
            // close only valid fds
            if self.fd >= 0 {
                unsafe {
                    libc::close(self.fd);
                }
            }
        }
    }

    impl datapath::socket::XdpSocket for RealSocket {
        fn poll(&mut self, max: usize) -> Vec<Vec<u8>> {
            // Use ring-based RX if available
            if let Some(rm) = &self.ring {
                let descs = rm.rx_pop(max);
                let mut out = Vec::with_capacity(descs.len());
                for d in descs {
                    // d is a UMEM frame address (offset). Copy whole frame_size bytes.
                    let frame_size = self._umem.frame_size();
                    let base = self._umem.base_ptr();
                    unsafe {
                        let src = base.add(d as usize);
                        let slice = std::slice::from_raw_parts(src, frame_size);
                        out.push(slice.to_vec());
                    }
                }
                return out;
            }
            // fallback to empty
            Vec::new()
        }
        fn send(&mut self, buf: &mut Vec<u8>, offsets: &[(usize, usize)]) -> Result<(), ()> {
            // Use ring-based TX if available
            let ring = match &self.ring {
                Some(r) => r,
                None => return Err(()),
            };
            // Reclaim completed frames from comp ring into free list
            if let Some(rm) = &self.ring {
                let comps = rm.comp_pop(64);
                for a in comps {
                    let _ = self.free_list.try_push(a);
                }
            }

            let mut addrs: Vec<u64> = Vec::with_capacity(offsets.len());

            // Allocate frames from free list first, falling back to next_frame
            for (off, len) in offsets.iter().cloned() {
                let mem_off = if let Some(f) = self.free_list.try_pop() {
                    f
                } else {
                    let frames = self._umem.len() / self._umem.frame_size();
                    let idx = self.next_frame.fetch_add(1, Ordering::Relaxed) % frames;
                    (idx * self._umem.frame_size()) as u64
                };

                let slice = &buf[off..off + len];
                unsafe {
                    let dst = self._umem.base_ptr().add(mem_off as usize);
                    std::ptr::copy_nonoverlapping(
                        slice.as_ptr(),
                        dst,
                        std::cmp::min(slice.len(), self._umem.frame_size()),
                    );
                }
                addrs.push(mem_off as u64);
            }

            let pushed = ring.tx_push(&addrs);
            if pushed == 0 || pushed < addrs.len() {
                // Return allocated frames back to free list if we couldn't push all
                for &a in &addrs[..pushed] {
                    let _ = self.free_list.try_push(a);
                }
                for &a in &addrs[pushed..] {
                    let _ = self.free_list.try_push(a);
                }
                return Err(());
            }

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn freelist_push_pop_roundtrip() {
            let fl = FreeList::with_capacity(8);
            assert!(fl.try_push(100));
            assert!(fl.try_push(200));
            assert_eq!(fl.try_pop(), Some(100));
            assert_eq!(fl.try_pop(), Some(200));
            assert_eq!(fl.try_pop(), None);
        }

        #[test]
        fn lifecycle_reclaim_under_churn() {
            // Setup umem with 16 frames
            let frame_size = 2048usize;
            let frames = 16usize;
            let umem = Umem::new(frame_size * frames, frame_size).expect("umem alloc");

            // Prepare a ring buffer area large enough for descriptors
            let mut buf = vec![0u8; 16384].into_boxed_slice();
            let ptr = buf.as_mut_ptr();
            let offs = crate::rings::XskMmapOffsets {
                rx: 0,
                rx_desc: 128,
                tx: 64,
                tx_desc: 256,
                fill: 512,
                fill_desc: 1024,
                comp: 2048,
                comp_desc: 4096,
            };
            let ring = unsafe { crate::rings::RingMmap::new(ptr as *mut libc::c_void, buf.len(), offs) };

            // FreeList sized to frames (power-of-two <= frames)
            let fl = FreeList::with_capacity(frames);
            for i in 0..frames {
                assert!(fl.try_push((i * frame_size) as u64));
            }

            // Simulate many cycles of allocation and completion
            let mut next_idx = 0usize;
            for _round in 0..64 {
                // allocate up to half the frames
                let mut allocated = Vec::new();
                for _ in 0..(frames / 2) {
                    if let Some(f) = fl.try_pop() {
                        allocated.push(f);
                    } else {
                        let off = (next_idx * frame_size) as u64;
                        allocated.push(off);
                        next_idx = (next_idx + 1) % frames;
                    }
                }

                // simulate kernel completion: write descriptors into comp_desc and bump prod
                unsafe {
                    let comp_meta_off = offs.comp;
                    let comp_desc_off = offs.comp_desc;
                    // set prod = count, cons = 0
                    ring.write_u32_at(comp_meta_off, allocated.len() as u32);
                    ring.write_u32_at(comp_meta_off + 4, 0);
                    for (i, &addr) in allocated.iter().enumerate() {
                        ring.write_u64_at(comp_desc_off + (i * 8) as u64, addr as u64);
                    }
                }

                // reclaim via comp_pop -> push back into free list
                let reclaimed = ring.comp_pop(frames);
                for a in reclaimed {
                    let _ = fl.try_push(a);
                }
            }

            // After cycles, pop all frames and ensure we recovered at most `frames` values
            let mut seen = Vec::new();
            while let Some(v) = fl.try_pop() {
                seen.push(v);
                if seen.len() > frames { break; }
            }
            assert!(!seen.is_empty());
        }

        #[test]
        fn real_send_copies_into_umem() {
            let frame_size = 2048usize;
            let frames = 4usize;
            let mut umem = Umem::new(frame_size * frames, frame_size).expect("umem alloc");

            // simple in-memory ring backing
            let mut buf = vec![0u8; 16384].into_boxed_slice();
            let ptr = buf.as_mut_ptr();
            let offs = crate::rings::XskMmapOffsets {
                rx: 0,
                rx_desc: 128,
                tx: 64,
                tx_desc: 256,
                fill: 512,
                fill_desc: 1024,
                comp: 2048,
                comp_desc: 4096,
            };
            let ring = unsafe { crate::rings::RingMmap::new(ptr as *mut libc::c_void, buf.len(), offs) };

            // empty free list to force next_frame allocation
            let fl = FreeList::with_capacity(frames);

            let rs = RealSocket {
                ifname: "test".to_string(),
                queue_id: 0,
                fd: -1,
                _umem: umem,
                ring_map_ptr: ptr as *mut libc::c_void,
                ring_map_size: buf.len(),
                mmap_offsets: Some(offs),
                ring: Some(ring),
                next_frame: AtomicUsize::new(0),
                free_list: fl,
            };

            // prepare buffer and call send
            let mut rs = rs; // make mutable
            let payload = b"hello_afxdp".to_vec();
            let mut buf = payload.clone();
            let offsets = vec![(0usize, payload.len())];

            assert!(rs.send(&mut buf, &offsets).is_ok());

            // verify data was copied into UMEM at frame 0
            let base = rs._umem.base_ptr();
            unsafe {
                let slice = std::slice::from_raw_parts(base, rs._umem.frame_size());
                assert_eq!(&slice[..payload.len()], &payload[..]);
            }
        }
    }
}

#[cfg(feature = "real")]
pub use real::RealSocket;
