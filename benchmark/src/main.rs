use afxdp::MockSocket;
use datapath::Forwarder;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use routing::{RouteEntry, Table};
use std::env;
use std::time::{Instant, SystemTime};
use wire::{Header, HEADER_SIZE};

#[derive(Debug, Clone)]
struct Config {
    packets: usize,
    batch_size: usize,
    payload_len: usize,
    loss_percent: f64,
    corrupt_percent: f64,
    duplicate_percent: f64,
    seed: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ChaosCounters {
    dropped: usize,
    corrupted: usize,
    duplicated: usize,
}

fn parse_or_default<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|idx| args.get(idx + 1))
        .and_then(|value| value.parse::<T>().ok())
        .unwrap_or(default)
}

fn parse_config() -> Config {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Chaos benchmark harness");
        println!("  --packets <N>           Number of packets to generate (default 20000)");
        println!("  --batch-size <N>        Packets per forwarder batch (default 64)");
        println!("  --payload-len <N>       Payload bytes per packet (default 512)");
        println!(
            "  --loss-percent <f64>    Byzantine packet drop rate, 0-100 (default 5.0)"
        );
        println!(
            "  --corrupt-percent <f64> Byzantine packet corruption rate, 0-100 (default 2.0)"
        );
        println!(
            "  --duplicate-percent <f64> Byzantine duplicate injection rate, 0-100 (default 1.0)"
        );
        println!("  --seed <u64>            RNG seed for reproducibility (default 1337)");
        std::process::exit(0);
    }

    Config {
        packets: parse_or_default(&args, "--packets", 20_000usize),
        batch_size: parse_or_default(&args, "--batch-size", 64usize),
        payload_len: parse_or_default(&args, "--payload-len", 512usize),
        loss_percent: parse_or_default(&args, "--loss-percent", 5.0f64),
        corrupt_percent: parse_or_default(&args, "--corrupt-percent", 2.0f64),
        duplicate_percent: parse_or_default(&args, "--duplicate-percent", 1.0f64),
        seed: parse_or_default(&args, "--seed", 1337u64),
    }
}

fn build_forwarder() -> Forwarder {
    let routes = Table::new();
    routes.update_route(RouteEntry {
        dest_id: [2u8; 32],
        next_hop_id: [3u8; 32],
        metric: 1,
        last_seen: SystemTime::now(),
        channel_count: 1,
        alternate_channels: Vec::new(),
        mcr_epoch: 1,
    });

    Forwarder::new(routes)
}

fn build_packet(payload_len: usize, seq: u64) -> Vec<u8> {
    let mut packet = Header::new_header_buffer(payload_len);
    let header = Header {
        src_id: [1u8; 32],
        dst_id: [2u8; 32],
        flow_label: 7,
        seq_num: seq,
        session_id: [0u8; 16],
        flags: 0,
        length: payload_len as u16,
    };
    header.marshal_into(&mut packet).expect("marshal header");

    for (i, b) in packet[HEADER_SIZE..].iter_mut().enumerate() {
        *b = (i & 0xFF) as u8;
    }

    packet
}

fn inject_byzantine_noise(
    packet: &mut Vec<u8>,
    config: &Config,
    rng: &mut StdRng,
    counters: &mut ChaosCounters,
) -> bool {
    if rng.gen_bool((config.loss_percent / 100.0).clamp(0.0, 1.0)) {
        counters.dropped += 1;
        return false;
    }

    if rng.gen_bool((config.corrupt_percent / 100.0).clamp(0.0, 1.0)) {
        counters.corrupted += 1;
        // Corrupt destination to intentionally trigger miss-path behavior.
        packet[32..64].fill(9);
        // Truncate payload occasionally to simulate malformed packet framing.
        if packet.len() > HEADER_SIZE + 4 {
            packet.truncate(packet.len() - 4);
        }
    }

    true
}

fn percentile_ns(samples: &[u128], pct: f64) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let idx = (((pct / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize)
        .min(sorted.len().saturating_sub(1));
    sorted[idx]
}

fn main() {
    let config = parse_config();
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut forwarder = build_forwarder();
    let mut latencies_ns = Vec::with_capacity(config.packets);
    let mut total_socket_packets = 0usize;
    let mut counters = ChaosCounters::default();
    let mut pending_frames: Vec<Vec<u8>> = Vec::with_capacity(config.batch_size.max(1));

    let harness_start = Instant::now();

    for seq in 0..config.packets {
        let mut packet = build_packet(config.payload_len, seq as u64);
        if !inject_byzantine_noise(&mut packet, &config, &mut rng, &mut counters) {
            continue;
        }

        pending_frames.push(packet);
        if rng.gen_bool((config.duplicate_percent / 100.0).clamp(0.0, 1.0)) {
            counters.duplicated += 1;
            if let Some(duplicate) = pending_frames.last().cloned() {
                pending_frames.push(duplicate);
            }
        }

        if pending_frames.len() < config.batch_size.max(1) {
            continue;
        }

        total_socket_packets += pending_frames.len();
        let mut sock = MockSocket::new(std::mem::take(&mut pending_frames));

        let start = Instant::now();
        let _stats = forwarder.process_batch(&mut sock);
        latencies_ns.push(start.elapsed().as_nanos());
    }

    if !pending_frames.is_empty() {
        total_socket_packets += pending_frames.len();
        let mut sock = MockSocket::new(std::mem::take(&mut pending_frames));
        let start = Instant::now();
        let _stats = forwarder.process_batch(&mut sock);
        latencies_ns.push(start.elapsed().as_nanos());
    }

    let elapsed = harness_start.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();
    let pkt_per_sec = if elapsed_secs > 0.0 {
        total_socket_packets as f64 / elapsed_secs
    } else {
        0.0
    };

    let p50 = percentile_ns(&latencies_ns, 50.0);
    let p99 = percentile_ns(&latencies_ns, 99.0);
    let p999 = percentile_ns(&latencies_ns, 99.9);

    println!("chaos_benchmark packets_requested={} packets_sent={} samples={} payload_len={} batch_size={} seed={}",
        config.packets,
        total_socket_packets,
        latencies_ns.len(),
        config.payload_len,
        config.batch_size,
        config.seed
    );
    println!(
        "fault_injection drop={} corrupt={} duplicate={}",
        counters.dropped, counters.corrupted, counters.duplicated
    );
    println!("throughput_pkt_s={:.2}", pkt_per_sec);
    println!(
        "latency_ns p50={} p99={} p99_9={}",
        p50, p99, p999
    );
}
