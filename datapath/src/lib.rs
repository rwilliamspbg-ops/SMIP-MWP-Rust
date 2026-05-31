use crypto::session::{HybridSession, SessionError, TAG_SIZE};
use rayon::prelude::*;
#[cfg(target_arch = "x86_64")]
use std::is_x86_feature_detected;
use std::sync::atomic::{AtomicU64, Ordering};
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr::NonNull;
use std::sync::OnceLock;

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

struct Profiler {
    handle_ns: AtomicU64,
    handle_count: AtomicU64,

    encrypt_ns: AtomicU64,
    encrypt_count: AtomicU64,

    append_ns: AtomicU64,
    append_count: AtomicU64,

    lookup_next_hop_calls: AtomicU64,
    lookup_next_hop_hits: AtomicU64,

    lookup_predict_calls: AtomicU64,
    lookup_predict_hits: AtomicU64,
}

impl Default for Profiler {
    fn default() -> Self {
        Self {
            handle_ns: AtomicU64::new(0),
            handle_count: AtomicU64::new(0),
            encrypt_ns: AtomicU64::new(0),
            encrypt_count: AtomicU64::new(0),
            append_ns: AtomicU64::new(0),
            append_count: AtomicU64::new(0),
            lookup_next_hop_calls: AtomicU64::new(0),
            lookup_next_hop_hits: AtomicU64::new(0),
            lookup_predict_calls: AtomicU64::new(0),
            lookup_predict_hits: AtomicU64::new(0),
        }
    }
}

static GLOBAL_PROFILER: OnceLock<Profiler> = OnceLock::new();

fn global_profiler() -> &'static Profiler {
    GLOBAL_PROFILER.get_or_init(|| Profiler::default())
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

    /// Print profiling counters collected during runs. Safe to call while
    /// the forwarder is idle; locks the profiler and prints a summary.
    pub fn print_profile(&self) {
        let p = global_profiler();
        let handle_count = p.handle_count.load(Ordering::Relaxed);
        let handle_ns = p.handle_ns.load(Ordering::Relaxed);
        let encrypt_count = p.encrypt_count.load(Ordering::Relaxed);
        let encrypt_ns = p.encrypt_ns.load(Ordering::Relaxed);
        let append_count = p.append_count.load(Ordering::Relaxed);
        let append_ns = p.append_ns.load(Ordering::Relaxed);
        let lookup_calls = p.lookup_next_hop_calls.load(Ordering::Relaxed);
        let lookup_hits = p.lookup_next_hop_hits.load(Ordering::Relaxed);
        let predict_calls = p.lookup_predict_calls.load(Ordering::Relaxed);
        let predict_hits = p.lookup_predict_hits.load(Ordering::Relaxed);

        eprintln!("--- Forwarder profile ---");
        eprintln!("handle: {} calls, {} ns total, avg {} ns", handle_count, handle_ns, if handle_count>0 { handle_ns / handle_count } else { 0 });
        eprintln!("encrypt: {} calls, {} ns total, avg {} ns", encrypt_count, encrypt_ns, if encrypt_count>0 { encrypt_ns / encrypt_count } else { 0 });
        eprintln!("append: {} calls, {} ns total, avg {} ns", append_count, append_ns, if append_count>0 { append_ns / append_count } else { 0 });
        eprintln!("lookup_next_hop: {} calls, {} hits", lookup_calls, lookup_hits);
        eprintln!("lookup_predict: {} calls, {} hits", predict_calls, predict_hits);
        eprintln!("global packets_processed={}", PACKETS_PROCESSED.load(Ordering::Relaxed));
    }

    /// Ensure the internal arena has capacity for approximately `cap` bytes.
    /// This is a low-risk tuning knob for benchmark harnesses to avoid
    /// mid-run reallocations when the expected batch size and packet sizes
    /// are known.
    pub fn ensure_arena_capacity(&mut self, cap: usize) {
        // AlignedBuffer::reserve expects an "additional" amount relative
        // to the current length; when called on an empty arena this
        // effectively sets the desired capacity.
        self.arena.reserve(cap);
    }

    fn handle_packet(&mut self, pkt: &[u8], _use_avx2: bool, stats: &mut ForwarderStats) -> bool {
        let start_handle = std::time::Instant::now();
        let mut forwarded = false;

        if let Ok(h) = HeaderViewRef::new(pkt) {
            let src_id: [u8; 32] = h.src_id().try_into().unwrap();
            let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            // instrument lookup_next_hop
            {
                let prof = global_profiler();
                prof.lookup_next_hop_calls.fetch_add(1, Ordering::Relaxed);
            }
            if self
                .routes
                .lookup_next_hop(dst_id, flow_label)
                .is_some()
            {
                let prof = global_profiler();
                prof.lookup_next_hop_hits.fetch_add(1, Ordering::Relaxed);
            
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

                                let enc_start = std::time::Instant::now();
                                match session.encrypt_into_slice(
                                    &mut self.arena.as_mut_slice()[payload_start..payload_start + payload_len],
                                    seq_num,
                                ) {
                                    Ok(tag) => {
                                        let enc_ns = enc_start.elapsed().as_nanos();
                                        let prof = global_profiler();
                                        prof.encrypt_count.fetch_add(1, Ordering::Relaxed);
                                        prof.encrypt_ns.fetch_add(enc_ns as u64, Ordering::Relaxed);
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
                        if self
                    .routes
                    .lookup_or_predict(src_id, dst_id, flow_label)
                    .is_some()
                {
                        let prof = global_profiler();
                        prof.lookup_predict_calls.fetch_add(1, Ordering::Relaxed);
                        prof.lookup_predict_hits.fetch_add(1, Ordering::Relaxed);
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

                            let enc_start = std::time::Instant::now();
                            match session.encrypt_into_slice(
                                &mut self.arena.as_mut_slice()[payload_start..payload_start + payload_len],
                                seq_num,
                            ) {
                                    Ok(tag) => {
                                                                let enc_ns = enc_start.elapsed().as_nanos();
                                                                let prof = global_profiler();
                                                                prof.encrypt_count.fetch_add(1, Ordering::Relaxed);
                                                                prof.encrypt_ns.fetch_add(enc_ns as u64, Ordering::Relaxed);
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
        }

        let handle_ns = start_handle.elapsed().as_nanos();
        let prof = global_profiler();
        prof.handle_count.fetch_add(1, Ordering::Relaxed);
        prof.handle_ns.fetch_add(handle_ns as u64, Ordering::Relaxed);

        forwarded
    }

    fn process_packet_owned(
        pkt: Vec<u8>,
        routes: &Table,
        session: Option<&HybridSession>,
        _use_avx2: bool,
    ) -> PacketOutput {
        if let Ok(h) = HeaderViewRef::new(&pkt) {
            let src_id: [u8; 32] = h.src_id().try_into().unwrap();
            let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            // measure encrypt under caller's profiler by timing around call sites
            if routes.lookup_next_hop(dst_id, flow_label).is_some()
                || routes.lookup_or_predict(src_id, dst_id, flow_label).is_some()
            {
                let enc_start = std::time::Instant::now();
                let out = Self::encrypt_packet_owned(pkt, seq_num, payload_len, session);
                let enc_ns = enc_start.elapsed().as_nanos();
                let prof = global_profiler();
                prof.encrypt_count.fetch_add(1, Ordering::Relaxed);
                prof.encrypt_ns.fetch_add(enc_ns as u64, Ordering::Relaxed);
                return out;
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

    // Consuming variant for parallel paths: takes ownership of the Vec and
    // performs in-place encryption where possible, returning the same Vec
    // with flags to avoid extra copies or clones during parallel processing.
    fn process_packet_owned_consuming(
        mut pkt: Vec<u8>,
        routes: &Table,
        session: Option<&HybridSession>,
        _use_avx2: bool,
    ) -> (Vec<u8>, bool, bool) {
        if let Ok(h) = HeaderViewRef::new(&pkt) {
            let src_id: [u8; 32] = h.src_id().try_into().unwrap();
            let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            if routes.lookup_next_hop(dst_id, flow_label).is_some()
                || routes.lookup_or_predict(src_id, dst_id, flow_label).is_some()
            {
                let enc_start = std::time::Instant::now();
                let mut encrypted = false;
                let mut route_miss = false;
                if let Some(session) = session {
                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let target_len = HEADER_SIZE + payload_len + TAG_SIZE;
                        if pkt.len() < target_len {
                            pkt.resize(target_len, 0);
                        }
                        match session.encrypt_into_slice(&mut pkt[HEADER_SIZE..HEADER_SIZE + payload_len], seq_num) {
                            Ok(tag) => {
                                pkt[HEADER_SIZE + payload_len..target_len].copy_from_slice(tag.as_slice());
                                encrypted = true;
                            }
                            Err(SessionError::AuthenticationFailed)
                            | Err(SessionError::PayloadTooLarge)
                            | Err(SessionError::CiphertextTooShort)
                            | Err(SessionError::AeadError)
                            | Err(SessionError::BufferTooSmall)
                            | Err(SessionError::InsufficientCapacity) => {
                                route_miss = true;
                            }
                        }
                    } else if payload_len > 0 {
                        route_miss = true;
                    }
                }
                let enc_ns = enc_start.elapsed().as_nanos();
                let prof = global_profiler();
                prof.encrypt_count.fetch_add(1, Ordering::Relaxed);
                prof.encrypt_ns.fetch_add(enc_ns as u64, Ordering::Relaxed);

                return (pkt, encrypted, route_miss);
            } else {
                return (pkt, false, true);
            }
        }

        (pkt, false, false)
    }

    // Inline variant used by serial MCR paths to avoid allocating PacketOutput/Vecs.
    // Returns a tuple: (bytes, encrypted, route_miss)
    fn process_packet_owned_inline(
        pkt: &mut Vec<u8>,
        routes: &Table,
        session: Option<&HybridSession>,
        _use_avx2: bool,
    ) -> (Vec<u8>, bool, bool) {
        if let Ok(h) = HeaderViewRef::new(&pkt) {
            let src_id: [u8; 32] = h.src_id().try_into().unwrap();
            let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            if routes.lookup_next_hop(dst_id, flow_label).is_some()
                || routes.lookup_or_predict(src_id, dst_id, flow_label).is_some()
            {
                let enc_start = std::time::Instant::now();
                let mut encrypted = false;
                let mut route_miss = false;
                if let Some(session) = session {
                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let target_len = HEADER_SIZE + payload_len + TAG_SIZE;
                        if pkt.len() < target_len {
                            pkt.resize(target_len, 0);
                        }
                        match session.encrypt_into_slice(&mut pkt[HEADER_SIZE..HEADER_SIZE + payload_len], seq_num) {
                            Ok(tag) => {
                                pkt[HEADER_SIZE + payload_len..target_len].copy_from_slice(tag.as_slice());
                                encrypted = true;
                            }
                            Err(SessionError::AuthenticationFailed)
                            | Err(SessionError::PayloadTooLarge)
                            | Err(SessionError::CiphertextTooShort)
                            | Err(SessionError::AeadError)
                            | Err(SessionError::BufferTooSmall)
                            | Err(SessionError::InsufficientCapacity) => {
                                route_miss = true;
                            }
                        }
                    } else if payload_len > 0 {
                        route_miss = true;
                    }
                }
                let enc_ns = enc_start.elapsed().as_nanos();
                let prof = global_profiler();
                prof.encrypt_count.fetch_add(1, Ordering::Relaxed);
                prof.encrypt_ns.fetch_add(enc_ns as u64, Ordering::Relaxed);

                return (pkt.clone(), encrypted, route_miss);
            } else {
                return (pkt.clone(), false, true);
            }
        }

        (pkt.clone(), false, false)
    }

    fn encrypt_packet_owned(
        pkt: Vec<u8>,
        seq_num: u64,
        payload_len: usize,
        session: Option<&HybridSession>,
    ) -> PacketOutput {
        if let Some(session) = session {
            if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                let mut pkt = pkt;
                let target_len = HEADER_SIZE + payload_len + TAG_SIZE;
                if pkt.len() < target_len {
                    pkt.resize(target_len, 0);
                }
                match session.encrypt_into_slice(&mut pkt[HEADER_SIZE..HEADER_SIZE + payload_len], seq_num) {
                    Ok(tag) => {
                        pkt[HEADER_SIZE + payload_len..target_len].copy_from_slice(tag.as_slice());
                        return PacketOutput {
                            bytes: pkt,
                            encrypted: true,
                            route_miss: false,
                        };
                    }
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

        PacketOutput {
            bytes: pkt,
            encrypted: false,
            route_miss: false,
        }
    }

    fn append_outputs(&mut self, outputs: Vec<PacketOutput>, received: usize) -> ForwarderStats {
        let append_start = std::time::Instant::now();
        self.arena.clear();
        self.offsets.clear();

        let mut stats = ForwarderStats {
            received,
            ..ForwarderStats::default()
        };
        // record that we began processing this batch (MCR path)
        let prof = global_profiler();
        prof.handle_count.fetch_add(received as u64, Ordering::Relaxed);

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

        let append_ns = append_start.elapsed().as_nanos();
        let prof = global_profiler();
        prof.append_count.fetch_add(1, Ordering::Relaxed);
        prof.append_ns.fetch_add(append_ns as u64, Ordering::Relaxed);

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
            let prof = global_profiler();
            prof.handle_count.fetch_add(stats.received as u64, Ordering::Relaxed);
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
        let prof = global_profiler();
        prof.handle_count.fetch_add(stats.received as u64, Ordering::Relaxed);
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
        self.offsets.reserve(frames.len());

        // Default primary-spray mode can process in place, avoiding a second
        // copy and an intermediate duplicated packet vector.
        let routes_ref = &self.routes;
        let session_ref = self.session.as_ref();
        let mut stats = ForwarderStats {
            received,
            ..ForwarderStats::default()
        };

        let spray_mode = mcr_config::get_mcr_spray_mode();

        if spray_mode != "full" {
            for mut pkt in frames {
                let (seq_num, payload_len) = if let Ok(h) = HeaderViewRef::new(&pkt) {
                    let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
                    let flow_label = h.flow_label();
                    let seq_num = h.seq_num();
                    let payload_len = h.length() as usize;

                    let prof = global_profiler();
                    prof.lookup_next_hop_calls.fetch_add(1, Ordering::Relaxed);
                    let next_hop = match self.routes.lookup_next_hop(dst_id, flow_label) {
                        Some(next_hop) => {
                            prof.lookup_next_hop_hits.fetch_add(1, Ordering::Relaxed);
                            next_hop
                        }
                        None => {
                            let start = self.arena.len();
                            self.arena.extend_from_slice(&pkt);
                            let len = self.arena.len() - start;
                            self.offsets.push((start, len));
                            stats.forwarded += 1;
                            stats.route_misses += 1;
                            continue;
                        }
                    };

                    pkt[32..64].copy_from_slice(&next_hop);
                    (seq_num, payload_len)
                } else {
                    let start = self.arena.len();
                    self.arena.extend_from_slice(&pkt);
                    let len = self.arena.len() - start;
                    self.offsets.push((start, len));
                    stats.forwarded += 1;
                    stats.route_misses += 1;
                    continue;
                };

                // Inline encryption into the arena to avoid allocating per-packet Vecs.
                let enc_start = std::time::Instant::now();
                let mut was_encrypted = false;
                let mut was_route_miss = false;
                if let Some(session) = session_ref {
                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let target_len = HEADER_SIZE + payload_len + TAG_SIZE;
                        if pkt.len() < target_len {
                            pkt.resize(target_len, 0);
                        }
                        match session.encrypt_into_slice(&mut pkt[HEADER_SIZE..HEADER_SIZE + payload_len], seq_num) {
                            Ok(tag) => {
                                pkt[HEADER_SIZE + payload_len..target_len].copy_from_slice(tag.as_slice());
                                was_encrypted = true;
                            }
                            Err(SessionError::AuthenticationFailed)
                            | Err(SessionError::PayloadTooLarge)
                            | Err(SessionError::CiphertextTooShort)
                            | Err(SessionError::AeadError)
                            | Err(SessionError::BufferTooSmall)
                            | Err(SessionError::InsufficientCapacity) => {
                                was_route_miss = true;
                            }
                        }
                    } else if payload_len > 0 {
                        was_route_miss = true;
                    }
                }
                let enc_ns = enc_start.elapsed().as_nanos();
                let prof = global_profiler();
                prof.encrypt_count.fetch_add(1, Ordering::Relaxed);
                prof.encrypt_ns.fetch_add(enc_ns as u64, Ordering::Relaxed);

                let start = self.arena.len();
                self.arena.extend_from_slice(&pkt);
                let len = self.arena.len() - start;
                self.offsets.push((start, len));
                if was_encrypted {
                    stats.encrypted += 1;
                } else {
                    stats.forwarded += 1;
                }
                if was_route_miss {
                    stats.route_misses += 1;
                }
            }
        } else {
            // Full spray keeps the existing duplication behavior because one
            // input packet can expand to multiple outputs.
            let mut duplicated: Vec<(Vec<u8>, [u8; 32])> = Vec::with_capacity(received);
            for pkt in frames {
                if let Ok(h) = HeaderViewRef::new(&pkt) {
                    let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
                    let flow_label = h.flow_label();

                    let channels = self.routes.lookup_spray(dst_id, flow_label);
                    if channels.is_empty() {
                        duplicated.push((pkt, dst_id));
                        continue;
                    }

                    for (nh, _is_primary) in channels.iter() {
                        let mut modified = pkt.clone();
                        modified[32..64].copy_from_slice(nh);
                        duplicated.push((modified, dst_id));
                    }
                } else {
                    duplicated.push((pkt, [0u8; 32]));
                }
            }

            if duplicated.len() < PARALLEL_BATCH_THRESHOLD || rayon::current_num_threads() <= 1 {
                // Serial path: process in-place and append directly to arena to avoid
                // allocating intermediate PacketOutput/Vecs.
                for (mut pkt, _dst) in duplicated.into_iter() {
                    let out = Self::process_packet_owned_inline(&mut pkt, routes_ref, session_ref, use_avx2);
                    let start = self.arena.len();
                    self.arena.extend_from_slice(&out.0);
                    let len = self.arena.len() - start;
                    self.offsets.push((start, len));
                    if out.1 { stats.encrypted += 1 } else { stats.forwarded += 1 }
                    if out.2 { stats.route_misses += 1 }
                }

                self.mcr_forwarded.fetch_add(stats.forwarded as u64, Ordering::Relaxed);
                self.mcr_dropped.fetch_add(stats.route_misses as u64, Ordering::Relaxed);
                let _ = sock.send(self.arena.as_slice(), &self.offsets);
                PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
                let prof = global_profiler();
                prof.handle_count.fetch_add(stats.received as u64, Ordering::Relaxed);
                return stats;
            } else {
                // Parallel path: process packets in parallel but return owned
                // Vecs and flags to the main thread which will append into the
                // arena. This avoids extra intermediate allocations inside the
                // parallel map.
                let outputs: Vec<(Vec<u8>, bool, bool)> = duplicated
                    .into_par_iter()
                    .map(|(pkt, _)| Self::process_packet_owned_consuming(pkt, routes_ref, session_ref, use_avx2))
                    .collect();

                // Append results in the main thread to the arena.
                for (bytes, encrypted, route_miss) in outputs {
                    let start = self.arena.len();
                    self.arena.extend_from_slice(&bytes);
                    let len = self.arena.len() - start;
                    self.offsets.push((start, len));
                    if encrypted { stats.encrypted += 1 } else { stats.forwarded += 1 }
                    if route_miss { stats.route_misses += 1 }
                }

                self.mcr_forwarded.fetch_add(stats.forwarded as u64, Ordering::Relaxed);
                self.mcr_dropped.fetch_add(stats.route_misses as u64, Ordering::Relaxed);
                let _ = sock.send(self.arena.as_slice(), &self.offsets);
                PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
                return stats;
            }
        }

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
