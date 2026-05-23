use crypto::session::{HybridSession, SessionError};
use routing::Table;
use wire::{Header, HEADER_SIZE};

pub use socket::XdpSocket;

pub struct Forwarder {
    pub routes: Table,
    session: Option<HybridSession>,
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
        Self { routes, session }
    }

    pub fn process_batch(&self, sock: &mut dyn XdpSocket) -> ForwarderStats {
        let frames = sock.poll(64);
        let mut out: Vec<Vec<u8>> = Vec::with_capacity(frames.len());
        let mut stats = ForwarderStats {
            received: frames.len(),
            ..ForwarderStats::default()
        };

        if frames.is_empty() {
            return stats;
        }

        for pkt in frames {
            let mut forwarded = false;

            if let Ok(h) = Header::parse(&pkt) {
                if self.routes.lookup_or_predict(h.src_id, h.dst_id, h.flow_label).is_some() {
                    if let Some(session) = &self.session {
                        let payload_len = h.length as usize;
                        if pkt.len() >= HEADER_SIZE + payload_len && payload_len > 0 {
                            let payload = &pkt[HEADER_SIZE..HEADER_SIZE + payload_len];
                            match session.encrypt(payload, h.seq_num) {
                                Ok(ct) => {
                                    // Assemble packet using hybrid copy: allocate exact size and
                                    // copy header+ciphertext. For small packets this behaves
                                    // like a tiled/memcpy-friendly copy, while for larger
                                    // packets we use chunked scalar copies. This reduces
                                    // intermediate allocations and gives more control over
                                    // copy strategy.
                                    let total = HEADER_SIZE + ct.len();
                                    let mut newpkt = Vec::with_capacity(total);
                                    unsafe { newpkt.set_len(total); }
                                    // copy header
                                    newpkt[..HEADER_SIZE].copy_from_slice(&pkt[..HEADER_SIZE]);
                                    // copy ciphertext
                                    let dst = &mut newpkt[HEADER_SIZE..];
                                    // Hybrid copy: small -> single bulk copy, large -> chunked
                                    // Threshold: 4 KiB (4096 bytes).
                                    // Rationale: microbenchmarks (see tools/bench_harness results)
                                    // show that tiled/AVX2 or single memcpy-style copies win for
                                    // small payloads (<=4KiB) while chunked scalar copies are
                                    // more robust for larger payloads where cache behavior
                                    // and branch overhead make tiled partial-copying less
                                    // efficient. Keep this threshold conservative; it's
                                    // hardware-dependent and can be tuned after running
                                    // `perf stat` on target hardware.
                                    if dst.len() <= 4096 {
                                        // single bulk copy
                                        dst.copy_from_slice(&ct);
                                    } else {
                                        // chunked 256-byte scalar copy
                                        let mut off = 0usize;
                                        while off + 256 <= dst.len() {
                                            dst[off..off+256].copy_from_slice(&ct[off..off+256]);
                                            off += 256;
                                        }
                                        if off < dst.len() {
                                            let rem = dst.len() - off;
                                            dst[off..].copy_from_slice(&ct[off..off+rem]);
                                        }
                                    }
                                    out.push(newpkt);
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
                out.push(pkt);
                stats.forwarded += 1;
            }
        }

        let _ = sock.send(out);
        stats
    }
}

pub mod socket {
    // Minimal XDP-like socket trait used by forwarder tests and mocks
    pub trait XdpSocket {
        fn poll(&mut self, max: usize) -> Vec<Vec<u8>>;
        fn send(&mut self, pkts: Vec<Vec<u8>>) -> Result<(), ()>;
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
        sent: Vec<Vec<u8>>,
    }
    impl MockSocket {
        fn new(frames: Vec<Vec<u8>>) -> Self { Self { frames, sent: Vec::new() } }
    }
    impl XdpSocket for MockSocket {
        fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> { std::mem::take(&mut self.frames) }
        fn send(&mut self, pkts: Vec<Vec<u8>>) -> Result<(), ()> { self.sent = pkts; Ok(()) }
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
        let fwd = Forwarder::new(rt);

        // build header + payload
        let mut buf = wire::Header::new_header_buffer(4);
        let h = Header { src_id: [1u8;32], dst_id: [2u8;32], flow_label: 0x1, seq_num: 1, session_id: [0u8;16], flags: 0, length: 4 };
        h.marshal_into(&mut buf).unwrap();
        // append payload
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
        let fwd = Forwarder::new(rt);

        let mut buf = wire::Header::new_header_buffer(4);
        let h = Header {
            src_id: [1u8;32],
            dst_id: [2u8;32],
            flow_label: 0x1,
            seq_num: 1,
            session_id: [0u8;16],
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
}
