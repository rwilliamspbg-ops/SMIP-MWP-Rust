//! Ring mmap abstractions for AF_XDP.

use std::ptr::NonNull;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XskMmapOffsets {
    pub rx: u64,
    pub rx_desc: u64,
    pub tx: u64,
    pub tx_desc: u64,
    pub fill: u64,
    pub fill_desc: u64,
    pub comp: u64,
    pub comp_desc: u64,
}

pub struct RingMmap {
    base: NonNull<u8>,
    size: usize,
    offsets: XskMmapOffsets,
}

impl RingMmap {
    /// Construct a RingMmap wrapper around an mmap'ed pointer and reported offsets.
    ///
    /// # Safety
    ///
    /// The provided `map_ptr` must be a valid pointer to an mmap'ed region at
    /// least `map_size` bytes long. The caller must ensure the memory remains
    /// valid for the lifetime of the returned `RingMmap`.
    pub unsafe fn new(map_ptr: *mut libc::c_void, map_size: usize, offs: XskMmapOffsets) -> Self {
        RingMmap {
            base: NonNull::new_unchecked(map_ptr as *mut u8),
            size: map_size,
            offsets: offs,
        }
    }

    /// Access the raw base pointer
    pub fn base_ptr(&self) -> *mut u8 {
        self.base.as_ptr()
    }

    /// Return the size of the mapped ring region.
    pub fn len(&self) -> usize {
        self.size
    }

    /// Returns true if the mapped region has zero length.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
    /// Report the mmap offsets
    pub fn offsets(&self) -> XskMmapOffsets {
        self.offsets
    }

    pub fn rx_available(&self) -> usize {
        unsafe {
            let prod = self.read_u32_at(self.offsets.rx) as usize;
            let cons = self.read_u32_at(self.offsets.rx + 4) as usize;
            prod.wrapping_sub(cons)
        }
    }

    fn rx_capacity(&self) -> usize {
        let desc_region_bytes = self.offsets.tx_desc.saturating_sub(self.offsets.rx_desc) as usize;
        (desc_region_bytes / std::mem::size_of::<u64>()).max(1)
    }

    fn tx_capacity(&self) -> usize {
        let desc_region_bytes =
            self.offsets.fill_desc.saturating_sub(self.offsets.tx_desc) as usize;
        (desc_region_bytes / std::mem::size_of::<u64>()).max(1)
    }

    /// Pop up to `max` RX frame descriptors and return their offsets.
    pub fn rx_pop(&self, max: usize) -> Vec<u64> {
        unsafe {
            let offs = self.offsets;
            let rx_meta_off = offs.rx;
            let rx_desc_off = offs.rx_desc;

            let prod = self.read_u32_at(rx_meta_off);
            let cons = self.read_u32_at(rx_meta_off + 4);
            let avail = prod.wrapping_sub(cons) as usize;
            if avail == 0 {
                return Vec::new();
            }

            let capacity = self.rx_capacity();
            let mask = capacity - 1;

            let to_take = std::cmp::min(avail, max);
            let mut out = Vec::with_capacity(to_take);
            for i in 0..to_take {
                let idx = (cons as usize + i) & mask;
                let d_off = rx_desc_off + (idx * std::mem::size_of::<u64>()) as u64;
                let desc = self.read_u64_at(d_off);
                out.push(desc);
            }

            let new_cons = cons.wrapping_add(to_take as u32);
            self.write_u32_at(rx_meta_off + 4, new_cons);

            out
        }
    }

    /// Push `addrs` into the TX ring for transmission.
    pub fn tx_push(&self, addrs: &[u64]) -> usize {
        unsafe {
            let offs = self.offsets;
            let tx_meta_off = offs.tx;
            let tx_desc_off = offs.tx_desc;

            let prod = self.read_u32_at(tx_meta_off);
            let cons = self.read_u32_at(tx_meta_off + 4);

            let capacity = self.tx_capacity();
            let mask = capacity - 1;

            let used = prod.wrapping_sub(cons) as usize;
            let free = capacity.saturating_sub(used);
            if free == 0 {
                return 0;
            }

            let to_push = std::cmp::min(free, addrs.len());
            for (i, &addr) in addrs.iter().enumerate().take(to_push) {
                let idx = (prod as usize + i) & mask;
                let d_off = tx_desc_off + (idx * std::mem::size_of::<u64>()) as u64;
                self.write_u64_at(d_off, addr);
            }

            let new_prod = prod.wrapping_add(to_push as u32);
            self.write_u32_at(tx_meta_off, new_prod);
            to_push
        }
    }

    /// Low-level read of a u32 value at an mmap offset (little-endian).
    /// # Safety
    ///
    /// The caller must ensure `off + 4 <= self.len()` and that the memory
    /// at that offset is valid to read a little-endian u32.
    pub unsafe fn read_u32_at(&self, off: u64) -> u32 {
        assert!((off as usize) + std::mem::size_of::<u32>() <= self.size);
        let p = self.base.as_ptr().add(off as usize) as *const u32;
        u32::from_le(std::ptr::read_unaligned(p))
    }

    /// Low-level read of a u64 value at an mmap offset (little-endian).
    /// # Safety
    ///
    /// The caller must ensure `off + 8 <= self.len()` and that the memory
    /// at that offset is valid to read a little-endian u64.
    pub unsafe fn read_u64_at(&self, off: u64) -> u64 {
        assert!((off as usize) + std::mem::size_of::<u64>() <= self.size);
        let p = self.base.as_ptr().add(off as usize) as *const u64;
        u64::from_le(std::ptr::read_unaligned(p))
    }

    /// Low-level write of a u64 value at an mmap offset (little-endian).
    /// # Safety
    ///
    /// The caller must ensure `off + 8 <= self.len()` and that the memory
    /// at that offset is valid to write a little-endian u64.
    pub unsafe fn write_u64_at(&self, off: u64, v: u64) {
        assert!((off as usize) + std::mem::size_of::<u64>() <= self.size);
        let p = self.base.as_ptr().add(off as usize) as *mut u64;
        std::ptr::write_unaligned(p, v.to_le());
    }

    /// Low-level write of a u32 value at an mmap offset (little-endian).
    /// # Safety
    ///
    /// The caller must ensure `off + 4 <= self.len()` and that the memory
    /// at that offset is valid to write a little-endian u32.
    pub unsafe fn write_u32_at(&self, off: u64, v: u32) {
        assert!((off as usize) + std::mem::size_of::<u32>() <= self.size);
        let p = self.base.as_ptr().add(off as usize) as *mut u32;
        std::ptr::write_unaligned(p, v.to_le());
    }
    /// Return a borrow of the mapped slice at offset/len.
    /// # Safety
    ///
    /// The caller must ensure `off + len <= self.len()` and that the underlying
    /// memory remains valid for the returned borrow. The returned slice has a
    /// `'static` lifetime for convenience but the caller must not rely on that
    /// beyond the actual lifetime of the mapping.
    pub unsafe fn slice_at(&self, off: u64, len: usize) -> &'static [u8] {
        assert!((off as usize) + len <= self.size);
        std::slice::from_raw_parts(self.base.as_ptr().add(off as usize), len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rx_pop_advances_consumer() {
        let mut buf = vec![0u8; 4096].into_boxed_slice();
        let ptr = buf.as_mut_ptr();
        let offs = XskMmapOffsets {
            rx: 0,
            rx_desc: 128,
            tx: 0,
            tx_desc: 128 + 64,
            fill: 0,
            fill_desc: 128 + 128,
            comp: 0,
            comp_desc: 0,
        };
        let ring = unsafe { RingMmap::new(ptr as *mut libc::c_void, buf.len(), offs) };

        unsafe {
            // set producer=4, consumer=0
            ring.write_u32_at(offs.rx, 4);
            ring.write_u32_at(offs.rx + 4, 0);
            // write 4 descriptors
            for i in 0..4u64 {
                ring.write_u64_at(offs.rx_desc + (i * 8), 100 + i * 100);
            }
        }

        let popped = ring.rx_pop(2);
        assert_eq!(popped.len(), 2);
        assert_eq!(popped[0], 100);
        assert_eq!(popped[1], 200);

        let cons = unsafe { ring.read_u32_at(offs.rx + 4) };
        assert_eq!(cons, 2);
        drop(buf);
    }

    #[test]
    fn test_tx_push_writes_descriptors_and_advances_prod() {
        let mut buf = vec![0u8; 4096].into_boxed_slice();
        let ptr = buf.as_mut_ptr();
        let offs = XskMmapOffsets {
            rx: 0,
            rx_desc: 128,
            tx: 64,
            tx_desc: 256,
            fill: 0,
            fill_desc: 256 + 8 * 8,
            comp: 0,
            comp_desc: 0,
        };
        let ring = unsafe { RingMmap::new(ptr as *mut libc::c_void, buf.len(), offs) };

        unsafe {
            // set prod=0, cons=0
            ring.write_u32_at(offs.tx, 0);
            ring.write_u32_at(offs.tx + 4, 0);
        }

        let addrs = vec![1000u64, 2000u64, 3000u64];
        let pushed = ring.tx_push(&addrs);
        assert_eq!(pushed, 3);

        unsafe {
            // verify descriptors written
            for (i, &addr) in addrs.iter().enumerate().take(3) {
                let v = ring.read_u64_at(offs.tx_desc + (i * 8) as u64);
                assert_eq!(v, addr);
            }
            let prod = ring.read_u32_at(offs.tx);
            assert_eq!(prod, 3u32);
        }
        drop(buf);
    }
}
