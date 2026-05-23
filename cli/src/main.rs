use datapath::Forwarder;
use routing::{RouteEntry, Table};
use std::env;
use std::time::SystemTime;
use wire::{Header, HEADER_SIZE};

fn parse_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn run_demo() -> datapath::ForwarderStats {
    let routes = Table::new();
    routes.update_route(RouteEntry {
        dest_id: [2u8; 32],
        next_hop_id: [3u8; 32],
        metric: 1,
        last_seen: SystemTime::now(),
    });

    let forwarder = Forwarder::with_session(routes, vec![0x42; 32], b"cli-demo".to_vec());
    let mut packet = Header::new_header_buffer(4);
    let header = Header {
        src_id: [1u8; 32],
        dst_id: [2u8; 32],
        flow_label: 7,
        seq_num: 1,
        session_id: [0u8; 16],
        flags: 0,
        length: 4,
    };
    header.marshal_into(&mut packet).expect("marshal header");
    packet[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&[1, 2, 3, 4]);

    let mut sock = afxdp::MockSocket::new(vec![packet]);
    forwarder.process_batch(&mut sock)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if parse_flag(&args, "--help") || parse_flag(&args, "-h") {
        println!("mohawk-node (Rust rewrite)");
        println!("  --demo   run the in-process forwarding demo");
        println!("  --help   show this message");
        return;
    }

    if !parse_flag(&args, "--demo") {
        println!("mohawk-node (Rust rewrite)");
        println!("  --demo   run the in-process forwarding demo");
        println!("  --help   show this message");
        return;
    }

    let stats = run_demo();
    println!(
        "forwarder demo: received={} forwarded={} encrypted={} route_misses={}",
        stats.received, stats.forwarded, stats.encrypted, stats.route_misses
    );
}
