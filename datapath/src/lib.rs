use crypto::session::{HybridSession, SessionError, TAG_SIZE};
use rayon::prelude::*;
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::cell::RefCell;
#[cfg(target_arch = "x86_64")]
use std::is_x86_feature_detected;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread_local;

/// Application-level processed packet counter (samples per-second externally)
pub static PACKETS_PROCESSED: AtomicU64 = AtomicU64::new(0);
use routing::Table;
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

    #[cfg(test)]
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

thread_local! {
    static TLS_CIPHERTEXT: RefCell<AlignedBuffer> = RefCell::new(AlignedBuffer::with_capacity(4096));
}

/// Forwarder manages the high-speed packet processing hot-path.
/// Configuration options (like `mcr_enabled` and `mcr_spray_mode`) are cached on
/// initialization inside the struct to completely avoid expensive runtime
/// `std::env::var` environment lookups during packet processing.
pub struct Forwarder {
    pub routes: Table,
    session: Option<HybridSession>,
    arena: AlignedBuffer,
    offsets: Vec<(usize, usize)>,
    /// MCR telemetry: forwarded output packets
    mcr_forwarded: AtomicU64,
    /// MCR telemetry: dropped outputs (route misses / encrypt failures)
    mcr_dropped: AtomicU64,
    mcr_enabled: bool,
    mcr_spray_mode: String,
    profile_enabled: bool,
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
    GLOBAL_PROFILER.get_or_init(Profiler::default)
}

#[derive(Default)]
struct LocalMetrics {
    handle_ns: u64,
    handle_count: u64,
    encrypt_ns: u64,
    encrypt_count: u64,
    lookup_calls: u64,
    lookup_hits: u64,
    predict_calls: u64,
    predict_hits: u64,
}

impl LocalMetrics {
    fn apply(&self) {
        let prof = global_profiler();
        if self.handle_count > 0 {
            prof.handle_count
                .fetch_add(self.handle_count, Ordering::Relaxed);
            prof.handle_ns.fetch_add(self.handle_ns, Ordering::Relaxed);
        }
        if self.encrypt_count > 0 {
            prof.encrypt_count
                .fetch_add(self.encrypt_count, Ordering::Relaxed);
            prof.encrypt_ns
                .fetch_add(self.encrypt_ns, Ordering::Relaxed);
        }
        if self.lookup_calls > 0 {
            prof.lookup_next_hop_calls
                .fetch_add(self.lookup_calls, Ordering::Relaxed);
        }
        if self.lookup_hits > 0 {
            prof.lookup_next_hop_hits
                .fetch_add(self.lookup_hits, Ordering::Relaxed);
        }
        if self.predict_calls > 0 {
            prof.lookup_predict_calls
                .fetch_add(self.predict_calls, Ordering::Relaxed);
        }
        if self.predict_hits > 0 {
            prof.lookup_predict_hits
                .fetch_add(self.predict_hits, Ordering::Relaxed);
        }
    }
}

struct PacketOutput {
    bytes: Vec<u8>,
    encrypted: bool,
    route_miss: bool,
    enc_ns: u64,
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
        let mcr_enabled = mcr_config::get_mcr_enabled();
        let mcr_spray_mode = mcr_config::get_mcr_spray_mode();
        let profile_enabled = mcr_config::get_profile_enabled();
        Self {
            routes,
            session,
            // Pre-reserve aligned scratch/output storage to avoid mid-run allocations.
            arena: AlignedBuffer::with_capacity(262144),
            offsets: Vec::with_capacity(4096),
            mcr_forwarded: AtomicU64::new(0),
            mcr_dropped: AtomicU64::new(0),
            mcr_enabled,
            mcr_spray_mode,
            profile_enabled,
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
        eprintln!(
            "handle: {} calls, {} ns total, avg {} ns",
            handle_count,
            handle_ns,
            if handle_count > 0 {
                handle_ns / handle_count
            } else {
                0
            }
        );
        eprintln!(
            "encrypt: {} calls, {} ns total, avg {} ns",
            encrypt_count,
            encrypt_ns,
            if encrypt_count > 0 {
                encrypt_ns / encrypt_count
            } else {
                0
            }
        );
        eprintln!(
            "append: {} calls, {} ns total, avg {} ns",
            append_count,
            append_ns,
            if append_count > 0 {
                append_ns / append_count
            } else {
                0
            }
        );
        eprintln!(
            "lookup_next_hop: {} calls, {} hits",
            lookup_calls, lookup_hits
        );
        eprintln!(
            "lookup_predict: {} calls, {} hits",
            predict_calls, predict_hits
        );
        eprintln!(
            "global packets_processed={}",
            PACKETS_PROCESSED.load(Ordering::Relaxed)
        );
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

    fn handle_packet(
        &mut self,
        pkt: &[u8],
        _use_avx2: bool,
        stats: &mut ForwarderStats,
        metrics: &mut LocalMetrics,
    ) -> bool {
        let mut forwarded = false;

        if let Ok(h) = HeaderViewRef::new(pkt) {
            let src_id: [u8; 32] = *h.src_id();
            let dst_id: [u8; 32] = *h.dst_id();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            metrics.lookup_calls += 1;
            if self.routes.lookup_next_hop(dst_id, flow_label).is_some() {
                metrics.lookup_hits += 1;

                if let Some(session) = self.session.as_ref() {
                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let start = self.arena.len();
                        let needed = HEADER_SIZE + payload_len + TAG_SIZE;
                        let remaining = self.arena.capacity().saturating_sub(self.arena.len());
                        if remaining < needed {
                            self.arena.reserve(needed - remaining);
                        }

                        // Combine header and payload into a single copy operation to reduce slice copy overhead
                        self.arena
                            .extend_from_slice(&pkt[..HEADER_SIZE + payload_len]);
                        let payload_start = start + HEADER_SIZE;

                        let enc_start = if self.profile_enabled {
                            Some(std::time::Instant::now())
                        } else {
                            None
                        };
                        match session.encrypt_into_slice(
                            &mut self.arena.as_mut_slice()
                                [payload_start..payload_start + payload_len],
                            seq_num,
                        ) {
                            Ok(tag) => {
                                if let Some(start) = enc_start {
                                    let enc_ns = start.elapsed().as_nanos() as u64;
                                    metrics.encrypt_ns += enc_ns;
                                }
                                metrics.encrypt_count += 1;
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
            } else if self
                .routes
                .lookup_or_predict(src_id, dst_id, flow_label)
                .is_some()
            {
                metrics.predict_calls += 1;
                metrics.predict_hits += 1;
                if let Some(session) = self.session.as_ref() {
                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let start = self.arena.len();
                        let needed = HEADER_SIZE + payload_len + TAG_SIZE;
                        let remaining = self.arena.capacity().saturating_sub(self.arena.len());
                        if remaining < needed {
                            self.arena.reserve(needed - remaining);
                        }

                        // Combine header and payload into a single copy operation to reduce slice copy overhead
                        self.arena
                            .extend_from_slice(&pkt[..HEADER_SIZE + payload_len]);
                        let payload_start = start + HEADER_SIZE;

                        let enc_start = if self.profile_enabled {
                            Some(std::time::Instant::now())
                        } else {
                            None
                        };
                        match session.encrypt_into_slice(
                            &mut self.arena.as_mut_slice()
                                [payload_start..payload_start + payload_len],
                            seq_num,
                        ) {
                            Ok(tag) => {
                                if let Some(start) = enc_start {
                                    let enc_ns = start.elapsed().as_nanos() as u64;
                                    metrics.encrypt_ns += enc_ns;
                                }
                                metrics.encrypt_count += 1;
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
        _use_avx2: bool,
        profile_enabled: bool,
    ) -> PacketOutput {
        if let Ok(h) = HeaderViewRef::new(&pkt) {
            let src_id: [u8; 32] = *h.src_id();
            let dst_id: [u8; 32] = *h.dst_id();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            // measure encrypt under caller's profiler by timing around call sites
            if routes
                .lookup_or_predict(src_id, dst_id, flow_label)
                .is_some()
            {
                let enc_start = if profile_enabled {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                let mut out = Self::encrypt_packet_owned(pkt, seq_num, payload_len, session);
                if let Some(start) = enc_start {
                    out.enc_ns = start.elapsed().as_nanos() as u64;
                }
                return out;
            } else {
                return PacketOutput {
                    bytes: pkt,
                    encrypted: false,
                    route_miss: true,
                    enc_ns: 0,
                };
            }
        }

        PacketOutput {
            bytes: pkt,
            encrypted: false,
            route_miss: false,
            enc_ns: 0,
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
        profile_enabled: bool,
    ) -> (Vec<u8>, bool, bool, u64) {
        if let Ok(h) = HeaderViewRef::new(&pkt) {
            let src_id: [u8; 32] = *h.src_id();
            let dst_id: [u8; 32] = *h.dst_id();
            let flow_label = h.flow_label();
            let seq_num = h.seq_num();
            let payload_len = h.length() as usize;

            if routes
                .lookup_or_predict(src_id, dst_id, flow_label)
                .is_some()
            {
                let enc_start = if profile_enabled {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                let mut encrypted = false;
                let mut route_miss = false;
                if let Some(session) = session {
                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let target_len = HEADER_SIZE + payload_len + TAG_SIZE;
                        if pkt.len() < target_len {
                            pkt.resize(target_len, 0);
                        }
                        match session.encrypt_into_slice(
                            &mut pkt[HEADER_SIZE..HEADER_SIZE + payload_len],
                            seq_num,
                        ) {
                            Ok(tag) => {
                                pkt[HEADER_SIZE + payload_len..target_len]
                                    .copy_from_slice(tag.as_slice());
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
                let enc_ns = if let Some(start) = enc_start {
                    start.elapsed().as_nanos() as u64
                } else {
                    0
                };

                return (pkt, encrypted, route_miss, enc_ns);
            } else {
                return (pkt, false, true, 0);
            }
        }

        (pkt, false, false, 0)
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
                match session
                    .encrypt_into_slice(&mut pkt[HEADER_SIZE..HEADER_SIZE + payload_len], seq_num)
                {
                    Ok(tag) => {
                        pkt[HEADER_SIZE + payload_len..target_len].copy_from_slice(tag.as_slice());
                        return PacketOutput {
                            bytes: pkt,
                            encrypted: true,
                            route_miss: false,
                            enc_ns: 0,
                        };
                    }
                    Err(SessionError::AuthenticationFailed)
                    | Err(SessionError::PayloadTooLarge)
                    | Err(SessionError::CiphertextTooShort)
                    | Err(SessionError::AeadError)
                    | Err(SessionError::BufferTooSmall)
                    | Err(SessionError::InsufficientCapacity) => {
                        return PacketOutput {
                            bytes: pkt,
                            encrypted: false,
                            route_miss: true,
                            enc_ns: 0,
                        };
                    }
                }
            } else if payload_len > 0 {
                return PacketOutput {
                    bytes: pkt,
                    encrypted: false,
                    route_miss: true,
                    enc_ns: 0,
                };
            }
        }

        PacketOutput {
            bytes: pkt,
            encrypted: false,
            route_miss: false,
            enc_ns: 0,
        }
    }

    fn append_outputs(&mut self, outputs: Vec<PacketOutput>, received: usize) -> ForwarderStats {
        let append_start = if self.profile_enabled {
            Some(std::time::Instant::now())
        } else {
            None
        };
        self.arena.clear();
        self.offsets.clear();

        let mut stats = ForwarderStats {
            received,
            ..ForwarderStats::default()
        };
        // record that we began processing this batch (MCR path)
        let prof = global_profiler();
        prof.handle_count
            .fetch_add(received as u64, Ordering::Relaxed);

        self.arena.reserve(
            outputs
                .iter()
                .map(|output| output.bytes.len())
                .sum::<usize>(),
        );
        self.offsets.reserve(outputs.len());

        let mut metrics = LocalMetrics::default();

        for output in outputs {
            let start = self.arena.len();
            self.arena.extend_from_slice(&output.bytes);
            let len = self.arena.len() - start;
            self.offsets.push((start, len));
            if output.encrypted {
                stats.encrypted += 1;
                metrics.encrypt_count += 1;
                metrics.encrypt_ns += output.enc_ns;
            } else {
                stats.forwarded += 1;
            }
            if output.route_miss {
                stats.route_misses += 1;
            }
        }

        metrics.apply();

        if let Some(start) = append_start {
            let append_ns = start.elapsed().as_nanos();
            let prof = global_profiler();
            prof.append_count.fetch_add(1, Ordering::Relaxed);
            prof.append_ns
                .fetch_add(append_ns as u64, Ordering::Relaxed);
        }

        stats
    }

    pub fn process_batch(&mut self, sock: &mut dyn XdpSocket) -> ForwarderStats {
        // If MCR is enabled, use the MCR-aware processing path.
        if self.mcr_enabled {
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

            let mut metrics = LocalMetrics::default();
            let start_batch = if self.profile_enabled {
                Some(std::time::Instant::now())
            } else {
                None
            };

            for pkt in frames {
                let forwarded = self.handle_packet(&pkt, use_avx2, &mut stats, &mut metrics);

                if !forwarded {
                    let start = self.arena.len();
                    self.arena.extend_from_slice(&pkt);
                    let len = self.arena.len() - start;
                    self.offsets.push((start, len));
                    stats.forwarded += 1;
                }
            }

            if let Some(start) = start_batch {
                let elapsed = start.elapsed().as_nanos() as u64;
                metrics.handle_ns += elapsed;
            }
            metrics.handle_count += received as u64;

            metrics.apply();

            let _ = sock.send(self.arena.as_slice(), &self.offsets);
            PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
            return stats;
        }

        let routes = &self.routes;
        let session = self.session.as_ref();
        let profile_enabled = self.profile_enabled;
        let outputs = frames
            .into_par_iter()
            .map(|pkt| Self::process_packet_owned(pkt, routes, session, use_avx2, profile_enabled))
            .collect::<Vec<_>>();

        let stats = self.append_outputs(outputs, received);
        let _ = sock.send(self.arena.as_slice(), &self.offsets);
        // update global application pconf counter
        PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
        let prof = global_profiler();
        prof.handle_count
            .fetch_add(stats.received as u64, Ordering::Relaxed);
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
        self.arena
            .reserve(frames.iter().map(|p| p.len()).sum::<usize>() + frames.len() * TAG_SIZE);
        self.offsets.reserve(frames.len());

        // Default primary-spray mode can process in place, avoiding a second
        // copy and an intermediate duplicated packet vector.
        let routes_ref = &self.routes;
        let session_ref = self.session.as_ref();
        // Defensive: ensure `stats` exists before any use in this function.
        let mut stats = ForwarderStats {
            received,
            ..ForwarderStats::default()
        };

        let spray_mode = &self.mcr_spray_mode;

        if spray_mode != "full" {
            let mut metrics = LocalMetrics::default();
            let start_batch = if self.profile_enabled {
                Some(std::time::Instant::now())
            } else {
                None
            };

            for pkt in frames {
                if let Ok(h) = HeaderViewRef::new(&pkt) {
                    let _src_id: [u8; 32] = *h.src_id();
                    let dst_id: [u8; 32] = *h.dst_id();
                    let flow_label = h.flow_label();
                    let seq_num = h.seq_num();
                    let payload_len = h.length() as usize;

                    metrics.lookup_calls += 1;
                    let next_hop = match self.routes.lookup_spray_primary(dst_id, flow_label) {
                        Some(next_hop) => {
                            metrics.lookup_hits += 1;
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

                    let start = self.arena.len();
                    let mut was_encrypted = false;
                    let mut was_route_miss = false;

                    if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                        let needed = HEADER_SIZE + payload_len + TAG_SIZE;
                        let remaining = self.arena.capacity().saturating_sub(self.arena.len());
                        if remaining < needed {
                            self.arena.reserve(needed - remaining);
                        }

                        // Combine header and payload into a single copy operation to reduce slice copy overhead
                        self.arena
                            .extend_from_slice(&pkt[..HEADER_SIZE + payload_len]);
                        // Overwrite next_hop field directly in the arena using register-level vectorized assignment
                        *<&mut [u8; 32]>::try_from(
                            &mut self.arena.as_mut_slice()[start + 32..start + 64],
                        )
                        .unwrap() = next_hop;

                        let payload_start = start + HEADER_SIZE;

                        if let Some(session) = session_ref {
                            let enc_start = if self.profile_enabled {
                                Some(std::time::Instant::now())
                            } else {
                                None
                            };
                            match session.encrypt_into_slice(
                                &mut self.arena.as_mut_slice()
                                    [payload_start..payload_start + payload_len],
                                seq_num,
                            ) {
                                Ok(tag) => {
                                    if let Some(start) = enc_start {
                                        let enc_ns = start.elapsed().as_nanos() as u64;
                                        metrics.encrypt_ns += enc_ns;
                                    }
                                    metrics.encrypt_count += 1;

                                    self.arena.extend_from_slice(tag.as_slice());
                                    if pkt.len() > HEADER_SIZE + payload_len {
                                        self.arena
                                            .extend_from_slice(&pkt[HEADER_SIZE + payload_len..]);
                                    }
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
                        }
                    } else {
                        let needed = HEADER_SIZE + payload_len + TAG_SIZE;
                        let remaining = self.arena.capacity().saturating_sub(self.arena.len());
                        if remaining < needed {
                            self.arena.reserve(needed - remaining);
                        }

                        self.arena.extend_from_slice(&pkt[..HEADER_SIZE]);
                        // Overwrite next_hop field directly in the arena using register-level vectorized assignment
                        *<&mut [u8; 32]>::try_from(
                            &mut self.arena.as_mut_slice()[start + 32..start + 64],
                        )
                        .unwrap() = next_hop;

                        if payload_len > 0 {
                            was_route_miss = true;
                        }
                    }

                    if was_route_miss {
                        self.arena.truncate(start);
                        stats.route_misses += 1;

                        let f_start = self.arena.len();
                        // Copy entire packet in a single operation to reduce slice copy overhead
                        self.arena.extend_from_slice(&pkt);
                        // Overwrite next_hop field directly in the arena using register-level vectorized assignment
                        *<&mut [u8; 32]>::try_from(
                            &mut self.arena.as_mut_slice()[f_start + 32..f_start + 64],
                        )
                        .unwrap() = next_hop;
                        let len = self.arena.len() - f_start;
                        self.offsets.push((f_start, len));
                        stats.forwarded += 1;
                    } else {
                        let len = self.arena.len() - start;
                        self.offsets.push((start, len));
                        if was_encrypted {
                            stats.encrypted += 1;
                        } else {
                            stats.forwarded += 1;
                        }
                    }
                } else {
                    let start = self.arena.len();
                    self.arena.extend_from_slice(&pkt);
                    let len = self.arena.len() - start;
                    self.offsets.push((start, len));
                    stats.forwarded += 1;
                    stats.route_misses += 1;
                }
            }

            if let Some(start) = start_batch {
                let elapsed = start.elapsed().as_nanos() as u64;
                metrics.handle_ns += elapsed;
            }
            metrics.handle_count += received as u64;

            metrics.apply();
        } else if frames.len() < PARALLEL_BATCH_THRESHOLD || rayon::current_num_threads() <= 1 {
            // Serial path for full spray: process directly into self.arena, bypassing intermediate duplicated Vec allocations and clones.
            let mut local_enc_count = 0u64;
            let mut local_enc_ns = 0u64;
            let mut metrics = LocalMetrics::default();
            let start_batch = if self.profile_enabled {
                Some(std::time::Instant::now())
            } else {
                None
            };

            for pkt in frames {
                if let Ok(h) = HeaderViewRef::new(&pkt) {
                    let src_id: [u8; 32] = *h.src_id();
                    let dst_id: [u8; 32] = *h.dst_id();
                    let flow_label = h.flow_label();
                    let seq_num = h.seq_num();
                    let payload_len = h.length() as usize;

                    metrics.lookup_calls += 1;
                    let channels = self.routes.lookup_spray(dst_id, flow_label);
                    if channels.is_empty() {
                        let start = self.arena.len();

                        let route_exists = self
                                    .routes
                                    .lookup_or_predict(src_id, dst_id, flow_label)
                                    .is_some();

                        let mut was_encrypted = false;
                        let mut was_route_miss = false;

                        if route_exists {
                            metrics.lookup_hits += 1;
                            if let Some(session) = session_ref {
                                if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                                    let needed = HEADER_SIZE + payload_len + TAG_SIZE;
                                    let remaining = self.arena.capacity().saturating_sub(start);
                                    if remaining < needed {
                                        self.arena.reserve(needed - remaining);
                                    }
                                    self.arena
                                        .extend_from_slice(&pkt[..HEADER_SIZE + payload_len]);

                                    let payload_start = start + HEADER_SIZE;
                                    let enc_start = if self.profile_enabled {
                                        Some(std::time::Instant::now())
                                    } else {
                                        None
                                    };
                                    match session.encrypt_into_slice(
                                        &mut self.arena.as_mut_slice()
                                            [payload_start..payload_start + payload_len],
                                        seq_num,
                                    ) {
                                        Ok(tag) => {
                                            if let Some(st) = enc_start {
                                                local_enc_ns += st.elapsed().as_nanos() as u64;
                                            }
                                            local_enc_count += 1;
                                            self.arena.extend_from_slice(tag.as_slice());
                                            let target_len = HEADER_SIZE + payload_len + TAG_SIZE;
                                            if pkt.len() > target_len {
                                                self.arena.extend_from_slice(&pkt[target_len..]);
                                            }
                                            was_encrypted = true;
                                        }
                                        Err(_) => {
                                            self.arena.truncate(start);
                                            self.arena.extend_from_slice(&pkt);
                                            was_route_miss = true;
                                        }
                                    }
                                } else {
                                    self.arena.extend_from_slice(&pkt);
                                    if payload_len > 0 {
                                        was_route_miss = true;
                                    }
                                }
                            } else {
                                self.arena.extend_from_slice(&pkt);
                            }
                        } else {
                            self.arena.extend_from_slice(&pkt);
                            was_route_miss = true;
                        }

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
                        continue;
                    }
                    metrics.lookup_hits += 1;

                    for (nh, _is_primary) in channels {
                        let start = self.arena.len();

                        let route_exists = self
                                .routes
                                .lookup_or_predict(src_id, nh, flow_label)
                                .is_some();

                        let mut was_encrypted = false;
                        let mut was_route_miss = false;

                        if route_exists {
                            if let Some(session) = session_ref {
                                if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                                    let needed = HEADER_SIZE + payload_len + TAG_SIZE;
                                    let remaining = self.arena.capacity().saturating_sub(start);
                                    if remaining < needed {
                                        self.arena.reserve(needed - remaining);
                                    }
                                    self.arena
                                        .extend_from_slice(&pkt[..HEADER_SIZE + payload_len]);

                                    if let Ok(mut view) = wire::HeaderView::view(
                                        &mut self.arena.as_mut_slice()[start..],
                                    ) {
                                        view.set_dst_id(nh);
                                    }

                                    let payload_start = start + HEADER_SIZE;
                                    let enc_start = if self.profile_enabled {
                                        Some(std::time::Instant::now())
                                    } else {
                                        None
                                    };
                                    match session.encrypt_into_slice(
                                        &mut self.arena.as_mut_slice()
                                            [payload_start..payload_start + payload_len],
                                        seq_num,
                                    ) {
                                        Ok(tag) => {
                                            if let Some(st) = enc_start {
                                                local_enc_ns += st.elapsed().as_nanos() as u64;
                                            }
                                            local_enc_count += 1;
                                            self.arena.extend_from_slice(tag.as_slice());
                                            let target_len = HEADER_SIZE + payload_len + TAG_SIZE;
                                            if pkt.len() > target_len {
                                                self.arena.extend_from_slice(&pkt[target_len..]);
                                            }
                                            was_encrypted = true;
                                        }
                                        Err(_) => {
                                            self.arena.truncate(start);
                                            self.arena.extend_from_slice(&pkt);
                                            if let Ok(mut view) = wire::HeaderView::view(
                                                &mut self.arena.as_mut_slice()[start..],
                                            ) {
                                                view.set_dst_id(nh);
                                            }
                                            was_route_miss = true;
                                        }
                                    }
                                } else {
                                    self.arena.extend_from_slice(&pkt);
                                    if let Ok(mut view) = wire::HeaderView::view(
                                        &mut self.arena.as_mut_slice()[start..],
                                    ) {
                                        view.set_dst_id(nh);
                                    }
                                    if payload_len > 0 {
                                        was_route_miss = true;
                                    }
                                }
                            } else {
                                self.arena.extend_from_slice(&pkt);
                                if let Ok(mut view) =
                                    wire::HeaderView::view(&mut self.arena.as_mut_slice()[start..])
                                {
                                    view.set_dst_id(nh);
                                }
                            }
                        } else {
                            self.arena.extend_from_slice(&pkt);
                            if let Ok(mut view) =
                                wire::HeaderView::view(&mut self.arena.as_mut_slice()[start..])
                            {
                                view.set_dst_id(nh);
                            }
                            was_route_miss = true;
                        }

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
                    let start = self.arena.len();
                    self.arena.extend_from_slice(&pkt);
                    let len = self.arena.len() - start;
                    self.offsets.push((start, len));
                    stats.forwarded += 1;
                    stats.route_misses += 1;
                }
            }

            if let Some(start) = start_batch {
                let elapsed = start.elapsed().as_nanos() as u64;
                metrics.handle_ns += elapsed;
            }
            metrics.handle_count += received as u64;

            metrics.apply();

            if local_enc_count > 0 {
                let prof = global_profiler();
                prof.encrypt_count
                    .fetch_add(local_enc_count, Ordering::Relaxed);
                prof.encrypt_ns.fetch_add(local_enc_ns, Ordering::Relaxed);
            }

            self.mcr_forwarded
                .fetch_add(stats.forwarded as u64, Ordering::Relaxed);
            self.mcr_dropped
                .fetch_add(stats.route_misses as u64, Ordering::Relaxed);
            let _ = sock.send(self.arena.as_slice(), &self.offsets);
            PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
            let prof = global_profiler();
            prof.handle_count
                .fetch_add(stats.received as u64, Ordering::Relaxed);
            return stats;
        } else {
            // Parallel path keeps the existing duplication behavior because one
            // input packet can expand to multiple outputs.
            let mut duplicated: Vec<(Vec<u8>, [u8; 32])> = Vec::with_capacity(received);
            for pkt in frames {
                if let Ok(h) = HeaderViewRef::new(&pkt) {
                    let dst_id: [u8; 32] = *h.dst_id();
                    let flow_label = h.flow_label();

                    let channels = self.routes.lookup_spray(dst_id, flow_label);
                    if channels.is_empty() {
                        duplicated.push((pkt, dst_id));
                        continue;
                    }

                    let mut channels = channels;
                    if let Some((last_nh, _)) = channels.pop() {
                        for (nh, _is_primary) in channels {
                            let mut modified = pkt.clone();
                            // Overwrite next_hop field using register-level vectorized assignment
                            *<&mut [u8; 32]>::try_from(&mut modified[32..64]).unwrap() = nh;
                            duplicated.push((modified, dst_id));
                        }
                        let mut modified = pkt;
                        // Overwrite next_hop field using register-level vectorized assignment
                        *<&mut [u8; 32]>::try_from(&mut modified[32..64]).unwrap() = last_nh;
                        duplicated.push((modified, dst_id));
                    }
                } else {
                    duplicated.push((pkt, [0u8; 32]));
                }
            }
            // Parallel path: process packets in parallel but return owned
            // Vecs and flags to the main thread which will append into the
            // arena. This avoids extra intermediate allocations inside the
            // parallel map.
            let profile_enabled = self.profile_enabled;
            let outputs: Vec<(Vec<u8>, bool, bool, u64)> = duplicated
                .into_par_iter()
                .map(|(pkt, _)| {
                    Self::process_packet_owned_consuming(
                        pkt,
                        routes_ref,
                        session_ref,
                        use_avx2,
                        profile_enabled,
                    )
                })
                .collect();

            // Append results in the main thread to the arena.
            let mut local_enc_count = 0u64;
            let mut local_enc_ns = 0u64;
            for (bytes, encrypted, route_miss, enc_ns) in outputs {
                let start = self.arena.len();
                self.arena.extend_from_slice(&bytes);
                let len = self.arena.len() - start;
                self.offsets.push((start, len));
                if encrypted {
                    stats.encrypted += 1;
                    local_enc_count += 1;
                    local_enc_ns += enc_ns;
                } else {
                    stats.forwarded += 1;
                }
                if route_miss {
                    stats.route_misses += 1;
                }
            }

            if local_enc_count > 0 {
                let prof = global_profiler();
                prof.encrypt_count
                    .fetch_add(local_enc_count, Ordering::Relaxed);
                prof.encrypt_ns.fetch_add(local_enc_ns, Ordering::Relaxed);
            }

            self.mcr_forwarded
                .fetch_add(stats.forwarded as u64, Ordering::Relaxed);
            self.mcr_dropped
                .fetch_add(stats.route_misses as u64, Ordering::Relaxed);
            let _ = sock.send(self.arena.as_slice(), &self.offsets);
            PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
            return stats;
        }

        self.mcr_forwarded
            .fetch_add(stats.forwarded as u64, Ordering::Relaxed);
        self.mcr_dropped
            .fetch_add(stats.route_misses as u64, Ordering::Relaxed);

        let _ = sock.send(self.arena.as_slice(), &self.offsets);
        PACKETS_PROCESSED.fetch_add(stats.received as u64, Ordering::Relaxed);
        stats
    }

    /// Full-spray mode: duplicate to all MCR channels per-packet.
    /// Uses lookup_spray() to get primary+alternate next-hops, then processes
    /// each channel's copy in parallel (when batch threshold met) or serially.
    pub fn process_batch_spray_full(&mut self, sock: &mut dyn XdpSocket) -> ForwarderStats {
        self.process_batch_mcr(sock)
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

        let mut metrics = LocalMetrics::default();
        let start_batch = if self.profile_enabled {
            Some(std::time::Instant::now())
        } else {
            None
        };

        for &idx in ring.active.iter().take(received) {
            let pkt = ring.slot(idx);
            let forwarded = self.handle_packet(pkt, use_avx2, &mut stats, &mut metrics);

            if !forwarded {
                let start = self.arena.len();
                self.arena.extend_from_slice(pkt);
                let len = self.arena.len() - start;
                self.offsets.push((start, len));
                stats.forwarded += 1;
            }
        }

        if let Some(start) = start_batch {
            let elapsed = start.elapsed().as_nanos() as u64;
            metrics.handle_ns += elapsed;
        }
        metrics.handle_count += received as u64;

        metrics.apply();

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

    #[test]
    fn mcr_primary_spray_balances_flows() {
        use std::env;
        env::set_var("MOHAWK_MCR_SPRAY_MODE", "primary");
        env::set_var("MOHAWK_MCR_ENABLED", "1");

        // We will try multiple flow labels and ensure they don't all map to next_hop_id [3u8; 32]
        let mut unique_next_hops = std::collections::HashSet::new();

        for flow_label in 0..10 {
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
                flow_label,
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
            assert_eq!(sock.sent.len(), 1);

            let sent_pkt = &sock.sent[0];
            let next_hop: [u8; 32] = sent_pkt[32..64].try_into().unwrap();
            unique_next_hops.insert(next_hop);
        }

        // Verify that we saw more than 1 next hop (i.e. load balancing actually occurred!)
        assert!(
            unique_next_hops.len() > 1,
            "Expected load balancing to choose multiple next hops but got {:?}",
            unique_next_hops
        );
    }
}
