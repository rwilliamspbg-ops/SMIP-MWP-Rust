use crypto::session::{HybridSession, SessionError, TAG_SIZE};
#[cfg(target_arch = "x86_64")]
use std::is_x86_feature_detected;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
use routing::Table;
use wire::{HeaderViewRef, HEADER_SIZE};
use std::convert::TryInto;

pub use socket::XdpSocket;

pub struct Forwarder {
    pub routes: Table,
    session: Option<HybridSession>,
    arena: Vec<u8>,
    offsets: Vec<(usize, usize)>,
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
        Self { routes, session, arena: Vec::new(), offsets: Vec::new() }
    }

    pub fn process_batch(&mut self, sock: &mut dyn XdpSocket) -> ForwarderStats {
        let frames = sock.poll(64);
        // reuse the persistent arena and offsets
        self.arena.clear();
        self.offsets.clear();
        self.arena.reserve(frames.iter().map(|p| p.len()).sum::<usize>() + frames.len() * TAG_SIZE);
        let mut stats = ForwarderStats {
            received: frames.len(),
            ..ForwarderStats::default()
        };

        if frames.is_empty() {
            return stats;
        }

        // Single reusable ciphertext buffer — avoids one heap allocation per packet.
        // Sized to the largest expected payload + AEAD tag.
        let mut ct_buf: Vec<u8> = Vec::with_capacity(65536 + TAG_SIZE);

        // Hoist feature detection out of the hot loop.
        #[cfg(target_arch = "x86_64")]
        let use_avx2 = is_x86_feature_detected!("avx2");
        #[cfg(not(target_arch = "x86_64"))]
        let use_avx2 = false;

        // Borrow session once to avoid repeated borrow resolution in the loop.
        let session_opt = self.session.as_ref();

        for pkt in frames {
            let mut forwarded = false;

            if let Ok(h) = HeaderViewRef::new(&pkt) {
                let src_id: [u8; 32] = h.src_id().try_into().unwrap();
                let dst_id: [u8; 32] = h.dst_id().try_into().unwrap();
                let flow_label = h.flow_label();
                let seq_num    = h.seq_num();
                let payload_len = h.length() as usize;

                if self.routes.lookup_or_predict(src_id, dst_id, flow_label).is_some() {
                    if let Some(session) = session_opt {
                        if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                            let payload = &pkt[HEADER_SIZE..HEADER_SIZE + payload_len];

                            let needed = payload_len + TAG_SIZE;
                            if ct_buf.capacity() < needed {
                                ct_buf.reserve(needed - ct_buf.capacity());
                            }

                            // encrypt_to: copies plaintext into ct_buf, encrypts
                            // in-place via aead::encrypt_in_place, appends tag.
                            // Zero extra Vec allocations.
                            match session.encrypt_to(&mut ct_buf, payload, seq_num) {
                                Ok(()) => {
                                    // Assemble output packet: append header
                                    // and ciphertext into the persistent arena.
                                    let start = self.arena.len();
                                    // append header
                                    self.arena.extend_from_slice(&pkt[..HEADER_SIZE]);

                                    // append ciphertext, using AVX2 optimized copy into
                                    // the arena when available.
                                    let ct_len = ct_buf.len();
                                    if ct_len >= 4096 {
                                        #[cfg(target_arch = "x86_64")]
                                        {
                                            if use_avx2 {
                                                let start_index = self.arena.len();
                                                self.arena.resize(start_index + ct_len, 0);
                                                unsafe {
                                                    let dst_ptr = self.arena[start_index..].as_mut_ptr();
                                                    let src_ptr = ct_buf.as_ptr();
                                                    copy_avx2(dst_ptr, src_ptr, ct_len);
                                                }
                                            } else {
                                                self.arena.extend_from_slice(&ct_buf);
                                            }
                                        }
                                        #[cfg(not(target_arch = "x86_64"))]
                                        {
                                            self.arena.extend_from_slice(&ct_buf);
                                        }
                                    } else {
                                        self.arena.extend_from_slice(&ct_buf);
                                    }

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


            if !forwarded {
                let start = self.arena.len();
                self.arena.extend_from_slice(&pkt);
                let len = self.arena.len() - start;
                self.offsets.push((start, len));
                stats.forwarded += 1;
            }
        }
        let _ = sock.send(&mut self.arena, &self.offsets);
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
                let v0 = _mm256_loadu_si256(src.add(off)       as *const __m256i);
                let v1 = _mm256_loadu_si256(src.add(off + 32)  as *const __m256i);
                let v2 = _mm256_loadu_si256(src.add(off + 64)  as *const __m256i);
                let v3 = _mm256_loadu_si256(src.add(off + 96)  as *const __m256i);
                _mm256_stream_si256(dst.add(off)       as *mut __m256i, v0);
                _mm256_stream_si256(dst.add(off + 32)  as *mut __m256i, v1);
                _mm256_stream_si256(dst.add(off + 64)  as *mut __m256i, v2);
                _mm256_stream_si256(dst.add(off + 96)  as *mut __m256i, v3);
                        off += 128;
                    }
                    _mm_sfence();
                }

        // Unrolled 128-byte vector copy for remainder (or all of a non-aligned buffer)
        while off + 128 <= len {
            let v0 = _mm256_loadu_si256(src.add(off)       as *const __m256i);
            let v1 = _mm256_loadu_si256(src.add(off + 32)  as *const __m256i);
            let v2 = _mm256_loadu_si256(src.add(off + 64)  as *const __m256i);
            let v3 = _mm256_loadu_si256(src.add(off + 96)  as *const __m256i);
            _mm256_storeu_si256(dst.add(off)       as *mut __m256i, v0);
            _mm256_storeu_si256(dst.add(off + 32)  as *mut __m256i, v1);
            _mm256_storeu_si256(dst.add(off + 64)  as *mut __m256i, v2);
            _mm256_storeu_si256(dst.add(off + 96)  as *mut __m256i, v3);
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
    pub trait XdpSocket {
        fn poll(&mut self, max: usize) -> Vec<Vec<u8>>;
        // Send a single arena buffer with offsets describing individual packets.
        // The socket borrows the arena so the caller retains ownership and can
        // reuse it across batches.
        fn send(&mut self, buf: &mut Vec<u8>, offsets: &[(usize, usize)]) -> Result<(), ()>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket::XdpSocket;
    use routing::{RouteEntry, Table};
    use wire::Header;
    use std::time::SystemTime;

    struct MockSocket {
        frames: Vec<Vec<u8>>,
        sent: Vec<Box<[u8]>>,
    }
    impl MockSocket {
        fn new(frames: Vec<Vec<u8>>) -> Self { Self { frames, sent: Vec::new() } }
    }
    impl XdpSocket for MockSocket {
        fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> { std::mem::take(&mut self.frames) }
        fn send(&mut self, buf: &mut Vec<u8>, offsets: &[(usize, usize)]) -> Result<(), ()> {
            self.sent.clear();
            for (off, len) in offsets.iter().cloned() {
                let slice = &buf[off..off+len];
                self.sent.push(slice.to_vec().into_boxed_slice());
            }
            Ok(())
        }
    }

    #[test]
    fn forwarder_encrypts_and_sends() {
        let rt = Table::new();
        rt.update_route(RouteEntry {
            dest_id: [2u8;32],
            next_hop_id: [3u8;32],
            metric: 1,
            last_seen: SystemTime::now(),
        });
        let mut fwd = Forwarder::new(rt);
        let mut buf = wire::Header::new_header_buffer(4);
        let h = Header { src_id: [1u8;32], dst_id: [2u8;32], flow_label: 0x1, seq_num: 1, session_id: [0u8;16], flags: 0, length: 4 };
        h.marshal_into(&mut buf).unwrap();
        buf[wire::HEADER_SIZE..wire::HEADER_SIZE+4].copy_from_slice(&[0x1,0x2,0x3,0x4]);
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
            dest_id: [2u8;32],
            next_hop_id: [3u8;32],
            metric: 1,
            last_seen: SystemTime::now(),
        });
        let mut fwd = Forwarder::new(rt);
        let mut buf = wire::Header::new_header_buffer(4);
        let h = Header { src_id: [1u8;32], dst_id: [2u8;32], flow_label: 0x1, seq_num: 1, session_id: [0u8;16], flags: 0, length: 8 };
        h.marshal_into(&mut buf).unwrap();
        buf[wire::HEADER_SIZE..wire::HEADER_SIZE+4].copy_from_slice(&[0x1,0x2,0x3,0x4]);
        let mut sock = MockSocket::new(vec![buf]);
        let stats = fwd.process_batch(&mut sock);
        assert_eq!(stats.received, 1);
        assert_eq!(stats.encrypted, 0);
        assert_eq!(stats.route_misses, 1);
        assert_eq!(stats.forwarded, 1);
        assert_eq!(sock.sent.len(), 1);
    }
}
