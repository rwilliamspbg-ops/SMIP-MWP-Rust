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
        Self { frames: Arc::new(Mutex::new(frames)), sent: Arc::new(Mutex::new(Vec::new())) }
    }

    pub fn take_sent(&self) -> Vec<Vec<u8>> { std::mem::take(&mut self.sent.lock().unwrap()) }
}

impl DatapathXdpSocket for MockSocket {
    fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> { std::mem::take(&mut self.frames.lock().unwrap()) }
    fn send(&mut self, pkts: Vec<Vec<u8>>) -> Result<(), ()> { *self.sent.lock().unwrap() = pkts; Ok(()) }
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
    use crate::umem::Umem;
    use crate::rings::RingMmap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::os::unix::io::RawFd;

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
    }

    impl RealSocket {
        pub fn new(ifname: &str, queue_id: u32, umem_frame_size: usize, umem_pages: usize) -> Result<Self, AfXdpError> {
            // Allocate UMEM backing region
            let umem = Umem::new(umem_frame_size * umem_pages, umem_frame_size)
                .map_err(|e| AfXdpError::Init(format!("umem alloc: {}", e)))?;

            // Create AF_XDP socket
            const AF_XDP: libc::c_int = 44; // PF_XDP / AF_XDP
            let fd = unsafe { libc::socket(AF_XDP, libc::SOCK_RAW, 0) };
            if fd < 0 {
                return Err(AfXdpError::Init(std::io::Error::last_os_error().to_string()));
            }

            // Resolve interface index
            let ifc = std::ffi::CString::new(ifname).map_err(|e| AfXdpError::Init(e.to_string()))?;
            let ifindex = unsafe { libc::if_nametoindex(ifc.as_ptr()) };
            if ifindex == 0 {
                unsafe { libc::close(fd); }
                return Err(AfXdpError::Init(format!("if_nametoindex failed for {}", ifname)));
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
                unsafe { libc::close(fd); }
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
                unsafe { libc::close(fd); }
                return Err(AfXdpError::Init(format!("setsockopt(UmemReg) failed: {}", err)));
            }

            // Query mmap offsets for rings using XDP_MMAP_OFFSETS
            const XDP_MMAP_OFFSETS: libc::c_int = 7;
            let mut offs = crate::rings::XskMmapOffsets { rx:0, rx_desc:0, tx:0, tx_desc:0, fill:0, fill_desc:0, comp:0, comp_desc:0 };
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
                unsafe { libc::close(fd); }
                return Err(AfXdpError::Init(format!("getsockopt(MMAP_OFFSETS) failed: {}", err)));
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
                unsafe { libc::close(fd); }
                return Err(AfXdpError::Init(format!("mmap rings failed: {}", err)));
            }

            // For demo purposes we don't fully implement ring cursors here. A full
            // implementation would wrap the mapped memory with safe ring types and
            // provide enqueue/dequeue helpers for RX/TX/FILL/COMP.

            let ring = unsafe { RingMmap::new(map, mmap_size, offs) };
            Ok(RealSocket { ifname: ifname.to_string(), queue_id, fd, _umem: umem, ring_map_ptr: map, ring_map_size: mmap_size, mmap_offsets: Some(offs), ring: Some(ring), next_frame: AtomicUsize::new(0) })
        }
    }

    impl Drop for RealSocket {
        fn drop(&mut self) {
            if !self.ring_map_ptr.is_null() {
                unsafe { libc::munmap(self.ring_map_ptr, self.ring_map_size); }
            }
            unsafe { libc::close(self.fd); }
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
        fn send(&mut self, pkts: Vec<Vec<u8>>) -> Result<(), ()> {
            // Use ring-based TX if available
            let ring = match &self.ring {
                Some(r) => r,
                None => return Err(()),
            };

            let mut addrs: Vec<u64> = Vec::with_capacity(pkts.len());
            let frames = self._umem.len() / self._umem.frame_size();
            for pkt in pkts.iter() {
                let idx = self.next_frame.fetch_add(1, Ordering::Relaxed) % frames;
                let off = idx * self._umem.frame_size();
                unsafe {
                    let dst = self._umem.base_ptr().add(off);
                    std::ptr::copy_nonoverlapping(pkt.as_ptr(), dst, std::cmp::min(pkt.len(), self._umem.frame_size()));
                }
                addrs.push(off as u64);
            }

            let pushed = ring.tx_push(&addrs);
            if pushed == 0 { return Err(()); }
            Ok(())
        }
    }
}

#[cfg(feature = "real")]
pub use real::RealSocket;
