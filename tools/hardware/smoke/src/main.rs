use std::env;
use std::process;

fn main() {
    let iface = env::var("MOHAWK_IFACE").unwrap_or_else(|_| {
        eprintln!("MOHAWK_IFACE not set; specify interface for smoke test");
        String::new()
    });
    if iface.is_empty() {
        eprintln!("No interface provided; aborting");
        process::exit(2);
    }
    let queue_id: u32 = env::var("MOHAWK_QUEUE_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let frame_size: usize = env::var("MOHAWK_FRAME_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(2048);
    let pages: usize = env::var("MOHAWK_UMEM_PAGES").ok().and_then(|s| s.parse().ok()).unwrap_or(1024);

    println!("Starting AF_XDP smoke test against iface={} queue={}", iface, queue_id);

    match afxdp::socket::RealSocket::new(&iface, queue_id, frame_size, pages) {
        Ok(sock) => {
            println!("RealSocket created successfully");
            // Basic introspection
            println!("retry_count={} tx_backpressure_count={}", sock.retry_count(), sock.tx_backpressure_count());
            // Drop socket and exit success
            drop(sock);
            process::exit(0);
        }
        Err(e) => {
            eprintln!("RealSocket init failed: {}", e);
            process::exit(3);
        }
    }
}
