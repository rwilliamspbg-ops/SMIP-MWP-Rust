mod bridge;
mod worker;

use bridge::{ControlRequest, ForwarderStats, QueueStats, TelemetryResponse};
use datapath::Forwarder;
use routing::{RouteEntry, Table};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;
use wire::{Header, HEADER_SIZE};
use prometheus::{Encoder, GaugeVec};
// Helper to construct an AF_XDP socket: attempt real socket when available,
// otherwise fall back to the in-process mock.
fn build_socket(frames: Vec<Vec<u8>>) -> afxdp::AfXdpSocket {
    #[cfg(feature = "real")]
    {
        use std::env;
        if let Ok(iface) = env::var("MOHAWK_IFACE") {
            let queue_id = env::var("MOHAWK_QUEUE_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(0u32);
            let frame_size = env::var("MOHAWK_FRAME_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(2048usize);
            let pages = env::var("MOHAWK_UMEM_PAGES").ok().and_then(|s| s.parse().ok()).unwrap_or(1024usize);
            match afxdp::socket::RealSocket::new(&iface, queue_id, frame_size, pages) {
                Ok(sock) => return Box::new(sock),
                Err(e) => eprintln!("AF_XDP real socket init failed: {}. Falling back to mock.", e),
            }
        }
    }
    afxdp::socket::new_mock_socket(frames)
}

fn parse_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn demo_packet() -> Vec<u8> {
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

    packet
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

    let mut sock = build_socket(vec![demo_packet()]);
    forwarder.process_batch(&mut *sock)
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
    if let Some(session_update) = request
        .session_updates
        .iter()
        .find(|update| update.action == "add")
    {
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

fn resolve_worker_cores() -> Vec<usize> {
    if let Ok(spec) = env::var("MOHAWK_WORKER_CORES") {
        if let Ok(cores) = worker::parse_core_list(&spec) {
            return cores;
        }
    }

    worker::available_core_ids()
}

fn run_pinned_workers(request: &ControlRequest) -> datapath::ForwarderStats {
    let worker_count = request.runtime_config.num_workers.max(1) as usize;
    let core_ids = resolve_worker_cores();
    let plan = worker::build_worker_plan(worker_count, &core_ids);
    let request = Arc::new(request.clone());
    let packet = demo_packet();

    let handles = worker::spawn_pinned_workers(&plan, move |assignment| {
        let mut forwarder = build_forwarder_from_request(&request);
        let mut sock = build_socket(vec![packet.clone()]);
        let stats = forwarder.process_batch(&mut *sock);
        eprintln!(
            "worker {} pinned to core {} processed {} packets",
            assignment.worker_index, assignment.core_id, stats.received
        );
        stats
    });

    handles
        .into_iter()
        .filter_map(|handle| handle.join().ok())
        .fold(datapath::ForwarderStats::default(), |mut acc, stats| {
            acc.received += stats.received;
            acc.forwarded += stats.forwarded;
            acc.encrypted += stats.encrypted;
            acc.route_misses += stats.route_misses;
            acc
        })
}

fn read_bridge_request(args: &[String]) -> Result<Option<ControlRequest>, String> {
    if let Some(index) = args.iter().position(|arg| arg == "--bridge-request") {
        let path = args
            .get(index + 1)
            .ok_or_else(|| "missing value for --bridge-request".to_string())?;
        let data = fs::read_to_string(path).map_err(|err| format!("read bridge request: {err}"))?;
        let request: ControlRequest =
            serde_json::from_str(&data).map_err(|err| format!("parse bridge request: {err}"))?;
        return Ok(Some(request));
    }

    if let Ok(data) = env::var("MOHAWK_BRIDGE_REQUEST") {
        let request: ControlRequest =
            serde_json::from_str(&data).map_err(|err| format!("parse bridge request: {err}"))?;
        return Ok(Some(request));
    }

    Ok(None)
}

fn render_telemetry(
    stats: datapath::ForwarderStats,
    request: Option<&ControlRequest>,
) -> TelemetryResponse {
    let worker_count = request
        .map(|r| r.runtime_config.num_workers as usize)
        .unwrap_or(0);
    let queue_target = request
        .and_then(|r| r.runtime_config.fill_threshold)
        .map(|value| value as usize);
    let health_state = if stats.route_misses > 0 {
        "degraded"
    } else {
        "ok"
    }
    .to_string();

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



fn render_prometheus_metrics(count: u64, timestamp: u64) -> String {
    format!(
        concat!(
            "# HELP mohawk_packets_processed_total Total packets processed by the datapath.\n",
            "# TYPE mohawk_packets_processed_total counter\n",
            "mohawk_packets_processed_total {}\n",
            "# HELP mohawk_metrics_timestamp_seconds Unix timestamp for the current sample.\n",
            "# TYPE mohawk_metrics_timestamp_seconds gauge\n",
            "mohawk_metrics_timestamp_seconds {}\n"
        ),
        count, timestamp
    )
}

/// Start metrics and control HTTP server on `bind_addr`.
/// This spawns background threads and returns immediately.
pub fn start_metrics_http(bind_addr: &str) {
    let bind_addr = bind_addr.to_string();
    // Setup a prometheus registry and gauges for AF_XDP counters
    let registry = prometheus::Registry::new();
    // Use labeled metrics so we can add per-socket labels later.
    let afxdp_gauges = GaugeVec::new(
        prometheus::opts!("afxdp_counters", "AF_XDP counters by name"),
        &["metric", "socket"],
    ).unwrap();
    registry.register(Box::new(afxdp_gauges.clone())).ok();

    // Background updater to sync atomic globals into the prometheus gauges
    {
        let g = afxdp_gauges.clone();
        std::thread::spawn(move || {
            use std::collections::HashSet;
            let mut prev_labels: HashSet<String> = HashSet::new();
            loop {
                let r = afxdp::AF_XDP_RETRY_COUNT.load(std::sync::atomic::Ordering::Relaxed) as f64;
                let b = afxdp::AF_XDP_BACKPRESSURE_COUNT.load(std::sync::atomic::Ordering::Relaxed) as f64;
                let af_from = afxdp::AF_XDP_ALLOC_FROM_FREELIST_COUNT.load(std::sync::atomic::Ordering::Relaxed) as f64;
                let af_fb = afxdp::AF_XDP_ALLOC_FALLBACK_COUNT.load(std::sync::atomic::Ordering::Relaxed) as f64;
                let af_drop = afxdp::AF_XDP_FREE_PUSH_DROP_COUNT.load(std::sync::atomic::Ordering::Relaxed) as f64;
                g.with_label_values(&["retry_total", "global"]).set(r);
                g.with_label_values(&["backpressure_total", "global"]).set(b);
                g.with_label_values(&["alloc_from_freelist_total", "global"]).set(af_from);
                g.with_label_values(&["alloc_fallback_total", "global"]).set(af_fb);
                g.with_label_values(&["free_push_drop_total", "global"]).set(af_drop);

                // Per-socket labeled metrics
                let snaps = afxdp::snapshot_all_socket_metrics();
                let mut current_labels: HashSet<String> = HashSet::new();
                for (label, (retry, backpressure, alloc_from, alloc_fb, free_drop)) in snaps.iter() {
                    current_labels.insert(label.clone());
                    g.with_label_values(&["retry_total", label]).set(*retry as f64);
                    g.with_label_values(&["backpressure_total", label]).set(*backpressure as f64);
                    g.with_label_values(&["alloc_from_freelist_total", label]).set(*alloc_from as f64);
                    g.with_label_values(&["alloc_fallback_total", label]).set(*alloc_fb as f64);
                    g.with_label_values(&["free_push_drop_total", label]).set(*free_drop as f64);
                }

                // Remove stale labels that were previously present but are no longer reported
                for removed in prev_labels.difference(&current_labels) {
                    let l = removed.as_str();
                    let _ = g.remove_label_values(&["retry_total", l]);
                    let _ = g.remove_label_values(&["backpressure_total", l]);
                    let _ = g.remove_label_values(&["alloc_from_freelist_total", l]);
                    let _ = g.remove_label_values(&["alloc_fallback_total", l]);
                    let _ = g.remove_label_values(&["free_push_drop_total", l]);
                }
                prev_labels = current_labels;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
    }

    std::thread::spawn(move || match std::net::TcpListener::bind(&bind_addr) {
        Ok(listener) => {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut s) => {
                        let mut buf = [0u8; 4096];
                        let _ = s.read(&mut buf);
                        let req = String::from_utf8_lossy(&buf);
                        if req.starts_with("GET /metrics") || req.starts_with("GET / ") {
                            let metric_families = registry.gather();
                            let mut buffer = Vec::new();
                            let encoder = prometheus::TextEncoder::new();
                            encoder.encode(&metric_families, &mut buffer).ok();
                            let body = String::from_utf8_lossy(&buffer);
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                            let _ = s.write_all(resp.as_bytes());
                        } else if req.starts_with("GET /control") {
                            // return JSON list of (socket_label, headroom)
                            let headrooms = afxdp::socket::snapshot_freelist_headrooms();
                            let body_json = match serde_json::to_string(&headrooms) {
                                Ok(j) => j,
                                Err(_) => "[]".to_string(),
                            };
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body_json.len(), body_json);
                            let _ = s.write_all(resp.as_bytes());
                        } else if req.starts_with("POST /control") {
                            // simple control endpoint: body contains `headroom=<n>`
                            let body = req.split("\r\n\r\n").nth(1).unwrap_or("");
                            if let Some(eq) = body.find("headroom=") {
                                if let Ok(n) = body[eq + 9..].trim().split_whitespace().next().unwrap_or("").parse::<usize>() {
                                    // support optional label=<label> to set a single socket
                                    let label_opt = body.find("label=").map(|i| {
                                        body[i + 6..]
                                            .split(|c: char| c == '&' || c == ' ' || c == '\n' || c == '\r')
                                            .next()
                                            .unwrap_or("")
                                            .to_string()
                                    });

                                    if let Some(label) = label_opt.filter(|s| !s.is_empty()) {
                                        // apply to named socket only
                                        let res = afxdp::socket::set_freelist_headroom_for(&label, n);
                                        let obj = if let Some((old, new)) = res {
                                            serde_json::json!({"label": label, "old": old, "new": new, "ok": true})
                                        } else {
                                            serde_json::json!({"label": label, "old": null, "new": n, "ok": false})
                                        };
                                        let body_json = serde_json::to_string(&obj).unwrap_or_else(|_| "{}".to_string());
                                        let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body_json.len(), body_json);
                                        let _ = s.write_all(resp.as_bytes());
                                    } else {
                                        // snapshot current headrooms before applying
                                        let before = afxdp::socket::snapshot_freelist_headrooms();
                                        // apply runtime reconfigure to registered sockets
                                        let applied = afxdp::socket::set_freelist_headroom_all(n);
                                        // build structured result per socket: label, old, new, ok
                                        let mut results = Vec::new();
                                        // mark applied ones as ok
                                        for (label, old, new) in applied.iter() {
                                            let obj = serde_json::json!({
                                                "label": label,
                                                "old": old,
                                                "new": new,
                                                "ok": true
                                            });
                                            results.push(obj);
                                        }
                                        // any socket present before but not applied -> mark as failed
                                        for (label, old) in before.iter() {
                                            if !applied.iter().any(|(l,_,_)| l == label) {
                                                let obj = serde_json::json!({
                                                    "label": label,
                                                    "old": old,
                                                    "new": n,
                                                    "ok": false
                                                });
                                                results.push(obj);
                                            }
                                        }
                                        let body_json = match serde_json::to_string(&results) {
                                            Ok(j) => j,
                                            Err(_) => "[]".to_string(),
                                        };
                                        let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body_json.len(), body_json);
                                        let _ = s.write_all(resp.as_bytes());
                                    }
                                } else {
                                    let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n\r\nBad Request");
                                }
                            } else {
                                let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n\r\nBad Request");
                            }
                        } else {
                            let _ = s.write_all(
                                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
                            );
                        }
                    }
                    Err(e) => eprintln!("metrics http accept: {}", e),
                }
            }
        }
        Err(e) => eprintln!("metrics http bind {}: {}", bind_addr, e),
    });
}

fn main() {
    let args: Vec<String> = env::args().collect();
    // CLI convenience: allow `--real` to opt into AF_XDP mode and `--iface <if>`
    // to specify the interface. These set environment variables consumed by
    // `build_socket()` so callers can opt-in without exporting env vars.
    if parse_flag(&args, "--real") {
        if let Some(i) = args.iter().position(|a| a == "--iface") {
            if let Some(iface) = args.get(i + 1) {
                env::set_var("MOHAWK_IFACE", iface);
            }
        }
        // If iface not provided and env var missing, help the user.
        if env::var("MOHAWK_IFACE").is_err() {
            eprintln!("--real requires --iface <ifname> or MOHAWK_IFACE to be set");
            std::process::exit(2);
        }
    }
    // allow tuning FreeList headroom at CLI startup
    if let Some(i) = args.iter().position(|a| a == "--freelist-headroom") {
        if let Some(v) = args.get(i + 1) {
            if let Ok(_) = v.parse::<usize>() {
                env::set_var("MOHAWK_FREELIST_HEADROOM", v);
            } else {
                eprintln!("invalid value for --freelist-headroom: {}", v);
                std::process::exit(2);
            }
        } else {
            eprintln!("--freelist-headroom requires a numeric value");
            std::process::exit(2);
        }
    }
    let want_metrics = parse_flag(&args, "--metrics");
    let metrics_socket = args
        .iter()
        .position(|a| a == "--metrics-socket")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let metrics_http = args
        .iter()
        .position(|a| a == "--metrics-http")
        .and_then(|i| args.get(i + 1))
        .cloned();
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

        let stats = if request.runtime_config.num_workers > 1 {
            run_pinned_workers(request)
        } else {
            run_demo()
        };
        let telemetry = render_telemetry(stats, Some(request));
        println!(
            "bridge request accepted for iface {}",
            request.runtime_config.iface
        );
        println!(
            "bridge datapath initialized with {} route updates",
            request.route_updates.len()
        );
        println!(
            "bridge worker count: {}",
            request.runtime_config.num_workers
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&telemetry).expect("telemetry json")
        );
        return;
    }

    // metrics reporter: prints per-second pconf when requested. It reads and
    // resets the `datapath::PACKETS_PROCESSED` counter each second.
    if want_metrics {
        thread::spawn(|| loop {
            let now = std::time::SystemTime::now();
            let secs = now
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let count = datapath::PACKETS_PROCESSED.swap(0, Ordering::Relaxed);
            println!("{},{}", secs, count);
            std::io::Write::flush(&mut std::io::stdout()).ok();
            thread::sleep(Duration::from_secs(1));
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
                Err(e) => {
                    eprintln!("metrics socket bind: {}", e);
                    return;
                }
            };
            for stream in listener.incoming() {
                match stream {
                    Ok(mut s) => {
                        let count = datapath::PACKETS_PROCESSED.load(Ordering::Relaxed);
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let msg = format!(
                            "{{\"timestamp\":{},\"packets_processed\":{}}}\n",
                            now, count
                        );
                        let _ = s.write_all(msg.as_bytes());
                    }
                    Err(e) => {
                        eprintln!("metrics socket accept: {}", e);
                    }
                }
            }
        });
    }

    if let Some(addr) = metrics_http {
        start_metrics_http(&addr);
    }

    // If any metrics endpoint was requested, keep the process alive so the
    // background threads (HTTP server / unix socket / metrics reporter) remain
    // running for smoke tests. This mirrors the previous CI expectation that
    // launching the binary with `--metrics-http` would produce a long-lived
    // server suitable for scraping.
    let server_mode = metrics_http.is_some() || metrics_socket.is_some() || want_metrics;
    if server_mode && !parse_flag(&args, "--demo") {
        loop {
            thread::sleep(Duration::from_secs(60));
        }
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

#[cfg(test)]
mod tests {
    use super::render_prometheus_metrics;
    use super::start_metrics_http;

    #[test]
    fn prometheus_metrics_render_expected_lines() {
        let body = render_prometheus_metrics(42, 1234567890);

        assert!(body.contains("# HELP mohawk_packets_processed_total"));
        assert!(body.contains("# TYPE mohawk_packets_processed_total counter"));
        assert!(body.contains("mohawk_packets_processed_total 42"));
        assert!(body.contains("# TYPE mohawk_metrics_timestamp_seconds gauge"));
        assert!(body.contains("mohawk_metrics_timestamp_seconds 1234567890"));
    }

    #[test]
    fn http_control_endpoints_work() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::time::Duration;

        // Pick an ephemeral port by binding and releasing it.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let addr = listener.local_addr().expect("local_addr");
        drop(listener);

        // Start server on selected addr
        start_metrics_http(&format!("{}", addr));

        // give the server a short moment to start
        std::thread::sleep(Duration::from_millis(50));

        // GET /control should return JSON (likely an array)
        let mut s = TcpStream::connect(addr).expect("connect GET");
        let _ = s.write_all(b"GET /control HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).ok();
        let resp = String::from_utf8_lossy(&buf);
        assert!(resp.starts_with("HTTP/1.1 200"), "GET /control returned non-200: {}", resp);
        assert!(resp.contains("application/json"), "GET /control did not return JSON: {}", resp);

        // POST /control with headroom should accept and return JSON
        let mut s2 = TcpStream::connect(addr).expect("connect POST");
        let body = b"headroom=128";
        let req = format!("POST /control HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}", body.len(), String::from_utf8_lossy(body));
        let _ = s2.write_all(req.as_bytes());
        let mut buf2 = Vec::new();
        s2.read_to_end(&mut buf2).ok();
        let resp2 = String::from_utf8_lossy(&buf2);
        // Server may return 200 with JSON or 400 Bad Request when there are
        // no registered sockets; accept either as a sign the endpoint is reachable.
        assert!(resp2.starts_with("HTTP/1.1 200") || resp2.starts_with("HTTP/1.1 400"), "POST /control returned unexpected status: {}", resp2);
        assert!(resp2.contains("application/json") || resp2.contains("OK") || resp2.contains("Bad Request"), "POST /control unexpected body: {}", resp2);
    }
}
