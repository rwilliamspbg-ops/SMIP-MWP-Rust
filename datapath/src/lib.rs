use crypto::session::{HybridSession, SessionError, TAG_SIZE};
use rayon::prelude::*;
use std::cell::RefCell;
thread_local! {
    static TLS_CIPHERTEXT: RefCell<AlignedBuffer> = RefCell::new(AlignedBuffer::with_capacity(65536));
}
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
#[cfg(target_arch = "x86_64")]
use std::is_x86_feature_detected;
use std::sync::atomic::{AtomicU64, Ordering};
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr::NonNull;

/// Application-level processed packet counter (samples per-second externally)
pub static PACKETS_PROCESSED: AtomicU64 = AtomicU64::new(0);
use routing::Table;
use std::convert::TryInto;
use wire::{HeaderViewRef, HEADER_SIZE};
mod mcr_config;

const PARALLEL_BATCH_THRESHOLD: usize = 1024;
const ALIGNMENT: usize = 256;

pub use socket::XdpSocket;

struct AlignedBuffer {
    ptr: NonNull<u8>,
    len: usize,
    cap: usize,
}

impl AlignedBuffer {
    fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(1);
        let layout = Layout::from_size_align(cap, ALIGNMENT).unwrap();
        let ptr = unsafe { alloc(layout) };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
        Self { ptr, len: 0, cap }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn capacity(&self) -> usize {
        self.cap
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    fn reserve(&mut self, additional: usize) {
        let required = self.len.saturating_add(additional);
        if required <= self.cap {
            return;
        }

        let new_cap = required.next_power_of_two().max(self.cap.saturating_mul(2));
        let old_layout = Layout::from_size_align(self.cap, ALIGNMENT).unwrap();
        let new_layout = Layout::from_size_align(new_cap, ALIGNMENT).unwrap();
        let raw = unsafe { realloc(self.ptr.as_ptr(), old_layout, new_layout.size()) };
        let ptr = NonNull::new(raw).unwrap_or_else(|| std::alloc::handle_alloc_error(new_layout));
        self.ptr = ptr;
        self.cap = new_cap;
    }

    fn extend_from_slice(&mut self, src: &[u8]) {
        self.reserve(src.len());
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), self.ptr.as_ptr().add(self.len), src.len());
        }
        self.len += src.len();
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.cap, ALIGNMENT).unwrap();
        unsafe { dealloc(self.ptr.as_ptr(), layout) };
    }
}

pub struct Forwarder {
    pub routes: Table,
    session: Option<HybridSession>,
    arena: AlignedBuffer,
    offsets: Vec<(usize, usize)>,
    /// MCR spray buffer for multi-channel outputs
    spray_buffer: Vec<Vec<u8>>,
    /// Track which channels were used in this batch
    channel_usage: AtomicU64,
    /// MCR telemetry: forwarded output packets
    mcr_forwarded: AtomicU64,
    /// MCR telemetry: dropped outputs (route misses / encrypt failures)
    mcr_dropped: AtomicU64,
}

struct PacketOutput {
    bytes: Vec<u8>,
    encrypted: bool,
    route_miss: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ForwarderStats {
    pub received: usize,
    pub forwarded: usize,
    pub encrypted: usize,
    pub route_misses: usize,
}

impl Forwarder {
    pub fn new(routes: Table) -> Self {
        Self::with_session(routes, vec![0x42; 32], b"datapath-default".to_vec())
    }

    pub fn with_session(routes: Table, session_secret: Vec<u8>, session_info: Vec<u8>) -> Self {
        let session = HybridSession::new(&session_secret, &session_info).ok();
        Self {
            routes,
            session,
            // Pre-reserve aligned scratch/output storage to avoid mid-run allocations.
            arena: AlignedBuffer::with_capacity(262144),
            offsets: Vec::with_capacity(4096),
            spray_buffer: Vec::new(),
            channel_usage: AtomicU64::new(mcr_config::get_mcr_channels() as u64),
            mcr_forwarded: AtomicU64::new(0),
            mcr_dropped: AtomicU64::new(0),
        }
    }

    fn handle_packet(&mut self, pkt: &[u8], _use_avx2: bool, stats: &mut ForwarderStats) -> bool {
        let mut forwarded = false;

        if let Ok(h) = HeaderViewRef::new(pkt) {
            let src_id: [u8; 32] = h.src_id().try_into().unwrap();
            let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            if self
                .routes
                .lookup_or_predict(src_id, dst_id, flow_label)
                .is_some()
            {
                if let Some(session) = self.session.as_ref() {
                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let payload = &pkt[HEADER_SIZE..HEADER_SIZE + payload_len];

                        let start = self.arena.len();
                        let needed = HEADER_SIZE + payload_len + TAG_SIZE;
                        let remaining = self.arena.capacity().saturating_sub(self.arena.len());
                        if remaining < needed {
                            self.arena.reserve(needed - remaining);
                        }

                        self.arena.extend_from_slice(&pkt[..HEADER_SIZE]);
                        let payload_start = self.arena.len();
                        self.arena.extend_from_slice(payload);

                        match session.encrypt_into_slice(
                            &mut self.arena.as_mut_slice()[payload_start..payload_start + payload_len],
                            seq_num,
                        ) {
                            Ok(tag) => {
                                self.arena.extend_from_slice(tag.as_slice());
                                let len = self.arena.len() - start;
                                self.offsets.push((start, len));
                                stats.encrypted += 1;
                                forwarded = true;
                            }
                            Err(SessionError::AuthenticationFailed)
                            | Err(SessionError::PayloadTooLarge)
                            | Err(SessionError::CiphertextTooShort)
                            | Err(SessionError::AeadError)
                            | Err(SessionError::BufferTooSmall)
                            | Err(SessionError::InsufficientCapacity) => {
                                self.arena.truncate(start);
                                stats.route_misses += 1;
                            }
                        }
                    } else if payload_len > 0 {
                        stats.route_misses += 1;
                    }
                }
            } else {
                stats.route_misses += 1;
            }
        }

        forwarded
    }

    fn process_packet_owned(
        pkt: Vec<u8>,
        routes: &Table,
        session: Option<&HybridSession>,
        use_avx2: bool,
    ) -> PacketOutput {
        if let Ok(h) = HeaderViewRef::new(&pkt) {
            let src_id: [u8; 32] = h.src_id().try_into().unwrap();
            let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            if routes
                .lookup_or_predict(src_id, dst_id, flow_label)
                .is_some()
            {
                if let Some(session) = session {
                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let payload = &pkt[HEADER_SIZE..HEADER_SIZE + payload_len];
                        // Reuse a thread-local aligned ciphertext buffer to avoid per-packet allocations.
                        let encrypt_result = TLS_CIPHERTEXT.with(|buf_cell| {
                            let mut buf = buf_cell.borrow_mut();
                            buf.clear();
                            let cap = buf.capacity();
                            if cap < payload_len + TAG_SIZE {
                                buf.reserve(payload_len + TAG_SIZE - cap);
                            }
                            buf.extend_from_slice(payload);
                            match session.encrypt_into_slice(buf.as_mut_slice(), seq_num) {
                                Ok(tag) => {
                                    buf.extend_from_slice(tag.as_slice());
                                    let ct_len = buf.len();
                                    let mut bytes = Vec::with_capacity(HEADER_SIZE + ct_len);
                                    bytes.extend_from_slice(&pkt[..HEADER_SIZE]);

                                    if ct_len >= 16384 {
                                        #[cfg(target_arch = "x86_64")]
                                        {
                                            if use_avx2 {
                                                let start_index = bytes.len();
                                                bytes.resize(start_index + ct_len, 0);
                                                unsafe {
                                                    let dst_ptr = bytes[start_index..].as_mut_ptr();
                                                    let src_ptr = buf.as_ptr();
                                                    copy_avx2(dst_ptr, src_ptr, ct_len);
                                                }
                                            } else {
                                                bytes.extend_from_slice(buf.as_slice());
                                            }
                                        }
                                        #[cfg(not(target_arch = "x86_64"))]
                                        {
                                            bytes.extend_from_slice(buf.as_slice());
                                        }
                                    } else {
                                        bytes.extend_from_slice(buf.as_slice());
                                    }

                                    Ok(PacketOutput { bytes, encrypted: true, route_miss: false })
                                }
                                Err(e) => Err(e),
                            }
                        });

                        match encrypt_result {
                            Ok(pkt_out) => return pkt_out,
                            Err(SessionError::AuthenticationFailed)
                            | Err(SessionError::PayloadTooLarge)
                            | Err(SessionError::CiphertextTooShort)
                            | Err(SessionError::AeadError)
                            | Err(SessionError::BufferTooSmall)
                            | Err(SessionError::InsufficientCapacity) => {
                                return PacketOutput { bytes: pkt, encrypted: false, route_miss: true };
                            }
                        }
                    } else if payload_len > 0 {
                        return PacketOutput {
                            bytes: pkt,
                            encrypted: false,
                            route_miss: true,
                        };
                    }
                }
            } else {
                return PacketOutput {
                    bytes: pkt,
                    encrypted: false,
                    route_miss: true,
                };
            }
        }

        PacketOutput {
            bytes: pkt,
            encrypted: false,
            route_miss: false,
        }
    }

    fn append_outputs(&mut self, outputs: Vec<PacketOutput>, received: usize) -> ForwarderStats {
        self.arena.clear();
        self.offsets.clear();

        let mut stats = ForwarderStats {
            received,
            ..ForwarderStats::default()
        };

        self.arena.reserve(
            outputs
                .iter()
                .map(|output| output.bytes.len())
                .sum::<usize>(),
        );
        self.offsets.reserve(outputs.len());

        for output in outputs {
            let start = self.arena.len();
            self.arena.extend_from_slice(&output.bytes);
            let len = self.arena.len() - start;
            self.offsets.push((start, len));
            if output.encrypted {
                stats.encrypted += 1;
            } else {
                stats.forwarded += 1;
            }
            if output.route_miss {
                stats.route_misses += 1;
            }
        }

        stats
    }

    pub fn process_batch(&mut self, sock: &mut dyn XdpSocket) -> ForwarderStats {
        // If MCR is enabled, use the MCR-aware processing path.
        if mcr_config::get_mcr_enabled() {
            return self.process_batch_mcr(sock);
        }
        let frames = sock.poll(64);
        let received = frames.len();

        if frames.is_empty() {
            return ForwarderStats::default();
        }

        // Hoist feature detection out of the hot loop.
        #[cfg(target_arch = "x86_64")]
        let use_avx2 = is_x86_feature_detected!("avx2");
        #[cfg(not(target_arch = "x86_64"))]
        let use_avx2 = false;

        if received < PARALLEL_BATCH_THRESHOLD || rayon::current_num_threads() <= 1 {
            self.arena.clear();
            self.offsets.clear();
            self.arena
                .reserve(frames.iter().map(|p| p.len()).sum::<usize>() + frames.len() * TAG_SIZE);
            let mut stats = ForwarderStats {
                received,
                ..ForwarderStats::default()
            };

            for pkt in frames {
                let forwarded = self.handle_packet(&pkt, use_avx2, &mut stats);

                if !forwarded {
                    let start = self.arena.len();
                    self.arena.extend_from_slice(&pkt);
                    let len = self.arena.len() - start;
                    self.offsets.push((start, len));
                    stats.forwarded += 1;
                }
            }

            let _ = sock.send(self.arena.as_slice(), &self.offsets);
            PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
            return stats;
        }

        let routes = &self.routes;
        let session = self.session.as_ref();
        let outputs = frames
            .into_par_iter()
            .map(|pkt| Self::process_packet_owned(pkt, routes, session, use_avx2))
            .collect::<Vec<_>>();

        let stats = self.append_outputs(outputs, received);
        let _ = sock.send(self.arena.as_slice(), &self.offsets);
        // update global application pconf counter
        PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
        stats
    }

    /// MCR-aware processing: for now delegates to `process_batch` while
    /// preserving a stable API for future MCR spray behavior.
    pub fn process_batch_mcr(&mut self, sock: &mut dyn XdpSocket) -> ForwarderStats {
        use rayon::prelude::*;

        let frames = sock.poll(64);
        let received = frames.len();

        if frames.is_empty() {
            return ForwarderStats::default();
        }

        #[cfg(target_arch = "x86_64")]
        let use_avx2 = is_x86_feature_detected!("avx2");
        #[cfg(not(target_arch = "x86_64"))]
        let use_avx2 = false;

        self.arena.clear();
        self.offsets.clear();
        self.arena.reserve(frames.iter().map(|p| p.len()).sum::<usize>() + frames.len() * TAG_SIZE);

        let spray_mode = mcr_config::get_mcr_spray_mode();

        // Phase 1: single-threaded channel selection and packet duplication
        let mut duplicated: Vec<Vec<u8>> = Vec::with_capacity(received);
        let mut route_miss_count: usize = 0;
        for pkt in frames {
            if let Ok(h) = HeaderViewRef::new(&pkt) {
                let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
                let flow_label = h.flow_label();

                let channels = self.routes.lookup_spray(dst_id, flow_label);
                if channels.is_empty() {
                    // route miss: forward unchanged
                    duplicated.push(pkt);
                    route_miss_count += 1;
                    continue;
                }

                if spray_mode == "full" {
                    for (nh, _is_primary) in channels.iter() {
                        let mut modified = pkt.clone();
                        modified[32..64].copy_from_slice(nh);
                        duplicated.push(modified);
                    }
                } else {
                    // primary only
                    let nh = channels[0].0;
                    let mut modified = pkt.clone();
                    modified[32..64].copy_from_slice(&nh);
                    duplicated.push(modified);
                }
            } else {
                duplicated.push(pkt);
                route_miss_count += 1;
            }
        }

        // Phase 2: parallel encryption/processing of duplicated packets
        let routes_ref = &self.routes;
        let session_ref = self.session.as_ref();
        let outputs: Vec<PacketOutput> = duplicated
            .into_par_iter()
            .map(|pkt| Self::process_packet_owned(pkt, routes_ref, session_ref, use_avx2))
            .collect();

        // append outputs and update telemetry
        let stats = self.append_outputs(outputs, received);
        self.mcr_forwarded.fetch_add(stats.forwarded as u64, Ordering::Relaxed);
        self.mcr_dropped.fetch_add(stats.route_misses as u64, Ordering::Relaxed);

        let _ = sock.send(self.arena.as_slice(), &self.offsets);
        PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
        stats
    }

    /// Full-spray mode: duplicate to all channels. Currently a stub that
    /// behaves like the normal path until spray is implemented.
    pub fn process_batch_spray_full(&mut self, sock: &mut dyn XdpSocket) -> ForwarderStats {
        // TODO: build outputs for all channels per-packet
        self.process_batch(sock)
    }

    pub fn process_batch_slices(
        &mut self,
        sock: &mut dyn XdpSocket,
        ring: &mut socket::SliceRing,
    ) -> ForwarderStats {
        let received = sock.poll_slices(64, ring);
        self.arena.clear();
        self.offsets.clear();
        let mut stats = ForwarderStats {
            received,
            ..ForwarderStats::default()
        };

        if received == 0 {
            return stats;
        }

        self.arena.reserve(
            ring.active
                .iter()
                .take(received)
                .map(|&idx| ring.slot(idx).len())
                .sum::<usize>()
                + received * TAG_SIZE,
        );

        #[cfg(target_arch = "x86_64")]
        let use_avx2 = is_x86_feature_detected!("avx2");
        #[cfg(not(target_arch = "x86_64"))]
        let use_avx2 = false;

        for &idx in ring.active.iter().take(received) {
            let pkt = ring.slot(idx);
            let forwarded = self.handle_packet(pkt, use_avx2, &mut stats);

            if !forwarded {
                let start = self.arena.len();
                self.arena.extend_from_slice(pkt);
                let len = self.arena.len() - start;
                self.offsets.push((start, len));
                stats.forwarded += 1;
            }
        }

        let _ = sock.send(self.arena.as_slice(), &self.offsets);
        // update global application pconf counter
        PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
        stats
    }
}

// AVX2 accelerated copy helper for x86_64.
// Safety: caller must ensure dst and src are valid for len bytes and non-overlapping.
#[cfg(target_arch = "x86_64")]
unsafe fn copy_avx2(dst: *mut u8, src: *const u8, len: usize) {
    use std::ptr;

    #[target_feature(enable = "avx2")]
    unsafe fn inner(dst: *mut u8, src: *const u8, len: usize) {
        let mut off = 0usize;
        let dst_aligned = (dst as usize) & 31 == 0;

        // Streaming stores for large, aligned transfers — avoids cache pollution.
        // Threshold matches the outer >= 4096 guard, so this branch actually fires.
        if len >= 4096 && dst_aligned {
            while off + 128 <= len {
                let v0 = _mm256_loadu_si256(src.add(off) as *const __m256i);
                let v1 = _mm256_loadu_si256(src.add(off + 32) as *const __m256i);
                let v2 = _mm256_loadu_si256(src.add(off + 64) as *const __m256i);
                let v3 = _mm256_loadu_si256(src.add(off + 96) as *const __m256i);
                _mm256_stream_si256(dst.add(off) as *mut __m256i, v0);
                _mm256_stream_si256(dst.add(off + 32) as *mut __m256i, v1);
                _mm256_stream_si256(dst.add(off + 64) as *mut __m256i, v2);
                _mm256_stream_si256(dst.add(off + 96) as *mut __m256i, v3);
                off += 128;
            }
            _mm_sfence();
        }

        // Unrolled 128-byte vector copy for remainder (or all of a non-aligned buffer)
        while off + 128 <= len {
            let v0 = _mm256_loadu_si256(src.add(off) as *const __m256i);
            let v1 = _mm256_loadu_si256(src.add(off + 32) as *const __m256i);
            let v2 = _mm256_loadu_si256(src.add(off + 64) as *const __m256i);
            let v3 = _mm256_loadu_si256(src.add(off + 96) as *const __m256i);
            _mm256_storeu_si256(dst.add(off) as *mut __m256i, v0);
            _mm256_storeu_si256(dst.add(off + 32) as *mut __m256i, v1);
            _mm256_storeu_si256(dst.add(off + 64) as *mut __m256i, v2);
            _mm256_storeu_si256(dst.add(off + 96) as *mut __m256i, v3);
            off += 128;
        }

        // 32-byte tail
        while off + 32 <= len {
            let v = _mm256_loadu_si256(src.add(off) as *const __m256i);
            _mm256_storeu_si256(dst.add(off) as *mut __m256i, v);
            off += 32;
        }

        // Byte tail
        if off < len {
            ptr::copy_nonoverlapping(src.add(off), dst.add(off), len - off);
        }
    }

    if is_x86_feature_detected!("avx2") {
        inner(dst, src, len);
    } else {
        std::ptr::copy_nonoverlapping(src, dst, len);
    }
}

pub mod socket {
    pub struct SliceRing {
        slots: Vec<Vec<u8>>,
        lens: Vec<usize>,
        pub active: Vec<usize>,
    }

    impl SliceRing {
        pub fn new(slot_count: usize, slot_size: usize) -> Self {
            let mut slots = Vec::with_capacity(slot_count);
            for _ in 0..slot_count {
                slots.push(vec![0u8; slot_size]);
            }
            Self {
                slots,
                lens: vec![0; slot_count],
                active: Vec::with_capacity(slot_count),
            }
        }

        pub fn clear(&mut self) {
            self.active.clear();
        }

        pub fn claim(&self) -> usize {
            let idx = self.active.len();
            assert!(idx < self.slots.len(), "SliceRing exhausted");
            idx
        }

        pub fn slot_mut(&mut self, idx: usize) -> &mut [u8] {
            self.slots[idx].as_mut_slice()
        }

        pub fn set_len(&mut self, idx: usize, len: usize) {
            self.lens[idx] = len.min(self.slots[idx].len());
        }

        pub fn slot(&self, idx: usize) -> &[u8] {
            &self.slots[idx][..self.lens[idx]]
        }
    }

    #[allow(clippy::result_unit_err)]
    pub trait XdpSocket {
        fn poll(&mut self, max: usize) -> Vec<Vec<u8>>;
        fn poll_slices(&mut self, max: usize, ring: &mut SliceRing) -> usize {
            let frames = self.poll(max);
            ring.clear();
            for frame in frames {
                let idx = ring.claim();
                let slot = ring.slot_mut(idx);
                let len = frame.len().min(slot.len());
                slot[..len].copy_from_slice(&frame[..len]);
                ring.set_len(idx, len);
                ring.active.push(idx);
            }
            ring.active.len()
        }
        // Send a single arena buffer with offsets describing individual packets.
        // The socket borrows the arena so the caller retains ownership and can
        // reuse it across batches.
        fn send(&mut self, buf: &[u8], offsets: &[(usize, usize)]) -> Result<(), ()>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket::XdpSocket;
    use routing::{RouteEntry, Table};
    use std::time::SystemTime;
    use wire::Header;

    struct MockSocket {
        frames: Vec<Vec<u8>>,
        sent: Vec<Box<[u8]>>,
    }
    impl MockSocket {
        fn new(frames: Vec<Vec<u8>>) -> Self {
            Self {
                frames,
                sent: Vec::new(),
            }
        }
    }
    impl XdpSocket for MockSocket {
        fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> {
            std::mem::take(&mut self.frames)
        }
        fn send(&mut self, buf: &[u8], offsets: &[(usize, usize)]) -> Result<(), ()> {
            self.sent.clear();
            for (off, len) in offsets.iter().cloned() {
                let slice = &buf[off..off + len];
                self.sent.push(slice.to_vec().into_boxed_slice());
            }
            Ok(())
        }
    }

    #[test]
    fn forwarder_encrypts_and_sends() {
        let rt = Table::new();
        rt.update_route(RouteEntry {
            dest_id: [2u8; 32],
            next_hop_id: [3u8; 32],
            metric: 1,
            last_seen: SystemTime::now(),
            channel_count: 1,
            alternate_channels: Vec::new(),
            mcr_epoch: 1,
        });
        let mut fwd = Forwarder::new(rt);
        let mut buf = wire::Header::new_header_buffer(4);
        let h = Header {
            src_id: [1u8; 32],
            dst_id: [2u8; 32],
            flow_label: 0x1,
            seq_num: 1,
            session_id: [0u8; 16],
            flags: 0,
            length: 4,
        };
        h.marshal_into(&mut buf).unwrap();
        buf[wire::HEADER_SIZE..wire::HEADER_SIZE + 4].copy_from_slice(&[0x1, 0x2, 0x3, 0x4]);
        let mut sock = MockSocket::new(vec![buf]);
        let stats = fwd.process_batch(&mut sock);
        assert_eq!(stats.received, 1);
        assert_eq!(stats.encrypted, 1);
        assert!(!sock.sent.is_empty());
    }

    #[test]
    fn forwarder_rejects_truncated_payloads() {
        let rt = Table::new();
        rt.update_route(RouteEntry {
            dest_id: [2u8; 32],
            next_hop_id: [3u8; 32],
            metric: 1,
            last_seen: SystemTime::now(),
            channel_count: 1,
            alternate_channels: Vec::new(),
            mcr_epoch: 1,
        });
        let mut fwd = Forwarder::new(rt);
        let mut buf = wire::Header::new_header_buffer(4);
        let h = Header {
            src_id: [1u8; 32],
            dst_id: [2u8; 32],
            flow_label: 0x1,
            seq_num: 1,
            session_id: [0u8; 16],
            flags: 0,
            length: 8,
        };
        h.marshal_into(&mut buf).unwrap();
        buf[wire::HEADER_SIZE..wire::HEADER_SIZE + 4].copy_from_slice(&[0x1, 0x2, 0x3, 0x4]);
        let mut sock = MockSocket::new(vec![buf]);
        let stats = fwd.process_batch(&mut sock);
        assert_eq!(stats.received, 1);
        assert_eq!(stats.encrypted, 0);
        assert_eq!(stats.route_misses, 1);
        assert_eq!(stats.forwarded, 1);
        assert_eq!(sock.sent.len(), 1);
    }

    #[test]
    fn slice_ring_clamps_overlong_frames() {
        let mut ring = socket::SliceRing::new(1, 4);
        let mut sock = MockSocket::new(vec![vec![1, 2, 3, 4, 5, 6]]);

        let received = sock.poll_slices(64, &mut ring);

        assert_eq!(received, 1);
        assert_eq!(ring.active, vec![0]);
        assert_eq!(ring.slot(0), &[1, 2, 3, 4]);
    }

    #[test]
    fn forwarder_process_batch_slices_encrypts_and_sends() {
        let rt = Table::new();
        rt.update_route(RouteEntry {
            dest_id: [2u8; 32],
            next_hop_id: [3u8; 32],
            metric: 1,
            last_seen: SystemTime::now(),
            channel_count: 1,
            alternate_channels: Vec::new(),
            mcr_epoch: 1,
        });
        let mut fwd = Forwarder::new(rt);

        let mut buf = wire::Header::new_header_buffer(4);
        let h = Header {
            src_id: [1u8; 32],
            dst_id: [2u8; 32],
            flow_label: 0x1,
            seq_num: 1,
            session_id: [0u8; 16],
            flags: 0,
            length: 4,
        };
        h.marshal_into(&mut buf).unwrap();
        buf[wire::HEADER_SIZE..wire::HEADER_SIZE + 4].copy_from_slice(&[0x1, 0x2, 0x3, 0x4]);

        let mut sock = MockSocket::new(vec![buf]);
        let mut ring = socket::SliceRing::new(1, wire::HEADER_SIZE + 4 + TAG_SIZE);
        let stats = fwd.process_batch_slices(&mut sock, &mut ring);

        assert_eq!(stats.received, 1);
        assert_eq!(stats.encrypted, 1);
        assert_eq!(stats.route_misses, 0);
        assert_eq!(stats.forwarded, 0);
        assert_eq!(sock.sent.len(), 1);
        assert_eq!(sock.sent[0].len(), wire::HEADER_SIZE + 4 + TAG_SIZE);
    }

    #[test]
    fn aligned_buffers_are_256b_aligned() {
        let forwarder = Forwarder::new(Table::new());
        assert_eq!((forwarder.arena.as_ptr() as usize) % ALIGNMENT, 0);

        TLS_CIPHERTEXT.with(|buf_cell| {
            let buf = buf_cell.borrow();
            assert_eq!((buf.as_ptr() as usize) % ALIGNMENT, 0);
        });
    }

    #[test]
    fn mcr_full_spray_duplicates_outputs() {
        use std::env;
        env::set_var("MOHAWK_MCR_SPRAY_MODE", "full");
        env::set_var("MOHAWK_MCR_ENABLED", "1");

        let rt = Table::new();
        rt.update_route(RouteEntry {
            dest_id: [2u8; 32],
            next_hop_id: [3u8; 32],
            metric: 1,
            last_seen: SystemTime::now(),
            channel_count: 3,
            alternate_channels: vec![[4u8; 32], [5u8; 32]],
            mcr_epoch: 1,
        });

        let mut fwd = Forwarder::new(rt);
        let mut buf = wire::Header::new_header_buffer(4);
        let h = Header {
            src_id: [1u8; 32],
            dst_id: [2u8; 32],
            flow_label: 0x1,
            seq_num: 1,
            session_id: [0u8; 16],
            flags: 0,
            length: 4,
        };
        h.marshal_into(&mut buf).unwrap();
        buf[wire::HEADER_SIZE..wire::HEADER_SIZE + 4].copy_from_slice(&[0x1, 0x2, 0x3, 0x4]);
        let mut sock = MockSocket::new(vec![buf]);
        let stats = fwd.process_batch(&mut sock);
        assert_eq!(stats.received, 1);
        // with full spray and 3 channels we expect 3 encrypted outputs
        assert_eq!(stats.encrypted, 3);
        assert_eq!(sock.sent.len(), 3);
    }
}
