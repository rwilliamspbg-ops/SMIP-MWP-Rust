mod bridge;

use bridge::{ControlRequest, ForwarderStats, QueueStats, TelemetryResponse};
use datapath::Forwarder;
use routing::{RouteEntry, Table};
use std::env;
use std::fs;
use std::time::SystemTime;
use wire::{Header, HEADER_SIZE};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::io::{Read, Write};
use std::net::TcpListener;

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

    let mut forwarder = Forwarder::with_session(routes, vec![0x42; 32], b"cli-demo".to_vec());
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

fn build_forwarder_from_request(request: &ControlRequest) -> Forwarder {
    let routes = Table::new();
    for update in &request.route_updates {
        routes.update_route(RouteEntry {
            dest_id: update.dest_id,
            next_hop_id: update.next_hop_id,
            metric: update.metric.unwrap_or(1),
            last_seen: SystemTime::now(),
        });
    }

    let mut secret = vec![0x42; 32];
    let mut info = b"cli-demo".to_vec();
    if let Some(session_update) = request.session_updates.iter().find(|update| update.action == "add") {
        if let Some(provided_secret) = session_update.secret_bytes() {
            if !provided_secret.is_empty() {
                secret = provided_secret;
            }
        }
        if let Some(provided_info) = session_update.info_bytes() {
            if !provided_info.is_empty() {
                info = provided_info;
            }
        }
    }

    Forwarder::with_session(routes, secret, info)
}

fn read_bridge_request(args: &[String]) -> Result<Option<ControlRequest>, String> {
    if let Some(index) = args.iter().position(|arg| arg == "--bridge-request") {
        let path = args.get(index + 1).ok_or_else(|| "missing value for --bridge-request".to_string())?;
        let data = fs::read_to_string(path).map_err(|err| format!("read bridge request: {err}"))?;
        let request: ControlRequest = serde_json::from_str(&data).map_err(|err| format!("parse bridge request: {err}"))?;
        return Ok(Some(request));
    }

    if let Ok(data) = env::var("MOHAWK_BRIDGE_REQUEST") {
        let request: ControlRequest = serde_json::from_str(&data).map_err(|err| format!("parse bridge request: {err}"))?;
        return Ok(Some(request));
    }

    Ok(None)
}

fn render_telemetry(stats: datapath::ForwarderStats, request: Option<&ControlRequest>) -> TelemetryResponse {
    let worker_count = request.map(|r| r.runtime_config.num_workers as usize).unwrap_or(0);
    let queue_target = request.and_then(|r| r.runtime_config.fill_threshold).map(|value| value as usize);
    let health_state = if stats.route_misses > 0 { "degraded" } else { "ok" }.to_string();

    TelemetryResponse {
        health_state,
        forwarder_stats: ForwarderStats {
            received: stats.received,
            forwarded: stats.forwarded,
            encrypted: stats.encrypted,
            route_misses: stats.route_misses,
        },
        queue_stats: Some(QueueStats {
            queue_depth: queue_target,
            fill_target: queue_target,
            fill_actual: queue_target.map(|target| target.saturating_sub(16)),
            worker_count: Some(worker_count),
        }),
        last_error: None,
        timestamp: "2026-05-24T00:00:00Z".to_string(),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let want_metrics = parse_flag(&args, "--metrics");
    let metrics_socket = args.iter().position(|a| a == "--metrics-socket").and_then(|i| args.get(i+1)).map(|s| s.clone());
    let metrics_http = args.iter().position(|a| a == "--metrics-http").and_then(|i| args.get(i+1)).map(|s| s.clone());
    if parse_flag(&args, "--help") || parse_flag(&args, "-h") {
        println!("mohawk-node (Rust rewrite)");
        println!("  --demo   run the in-process forwarding demo");
        println!("  --bridge-request <path>  run the bridge demo from a JSON control payload");
        println!("  --help   show this message");
        return;
    }

    let bridge_request = match read_bridge_request(&args) {
        Ok(request) => request,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    if let Some(request) = bridge_request.as_ref() {
        if let Err(err) = request.validate() {
            eprintln!("{err}");
            std::process::exit(1);
        }

        let _forwarder = build_forwarder_from_request(request);
        let stats = run_demo();
        let telemetry = render_telemetry(stats, Some(request));
        println!("bridge request accepted for iface {}", request.runtime_config.iface);
        println!("bridge datapath initialized with {} route updates", request.route_updates.len());
        println!("{}", serde_json::to_string_pretty(&telemetry).expect("telemetry json"));
        return;
    }

    // metrics reporter: prints per-second pconf when requested. It reads and
    // resets the `datapath::PACKETS_PROCESSED` counter each second.
    if want_metrics {
        thread::spawn(|| {
            loop {
                let now = std::time::SystemTime::now();
                let secs = now.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let count = datapath::PACKETS_PROCESSED.swap(0, Ordering::Relaxed);
                println!("{},{}", secs, count);
                std::io::Write::flush(&mut std::io::stdout()).ok();
                thread::sleep(Duration::from_secs(1));
            }
        });
    }

    if let Some(sock) = metrics_socket {
        // Spawn a unix-domain socket listener that returns current cumulative counter
        let sock_path = sock.clone();
        thread::spawn(move || {
            use std::os::unix::net::UnixListener;
            if std::path::Path::new(&sock_path).exists() {
                let _ = std::fs::remove_file(&sock_path);
            }
            let listener = match UnixListener::bind(&sock_path) {
                Ok(l) => l,
                Err(e) => { eprintln!("metrics socket bind: {}", e); return; }
            };
            for stream in listener.incoming() {
                match stream {
                    Ok(mut s) => {
                        let count = datapath::PACKETS_PROCESSED.load(Ordering::Relaxed);
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                        let msg = format!("{{\"timestamp\":{},\"packets_processed\":{}}}\n", now, count);
                        let _ = s.write_all(msg.as_bytes());
                    }
                    Err(e) => { eprintln!("metrics socket accept: {}", e); }
                }
            }
        });
    }

    if let Some(addr) = metrics_http {
        let bind_addr = addr.clone();
        thread::spawn(move || {
            match TcpListener::bind(&bind_addr) {
                Ok(listener) => {
                    for stream in listener.incoming() {
                        match stream {
                            Ok(mut s) => {
                                let mut buf = [0u8; 1024];
                                let _ = s.read(&mut buf);
                                let req = String::from_utf8_lossy(&buf);
                                if req.starts_with("GET /metrics") || req.starts_with("GET / ") {
                                    let count = datapath::PACKETS_PROCESSED.load(Ordering::Relaxed);
                                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                                    let body = format!("{{\"timestamp\":{},\"packets_processed\":{}}}\n", now, count);
                                    let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                                    let _ = s.write_all(resp.as_bytes());
                                } else {
                                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                                }
                            }
                            Err(e) => eprintln!("metrics http accept: {}", e),
                        }
                    }
                }
                Err(e) => eprintln!("metrics http bind {}: {}", bind_addr, e),
            }
        });
    }

    if !parse_flag(&args, "--demo") {
        println!("mohawk-node (Rust rewrite)");
        println!("  --demo   run the in-process forwarding demo");
        println!("  --bridge-request <path>  run the bridge demo from a JSON control payload");
        println!("  --help   show this message");
        return;
    }

    let stats = run_demo();
    println!(
        "forwarder demo: received={} forwarded={} encrypted={} route_misses={}",
        stats.received, stats.forwarded, stats.encrypted, stats.route_misses
    );
}
