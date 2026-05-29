use std::env;

pub fn get_mcr_enabled() -> bool {
    match env::var("MOHAWK_MCR_ENABLED") {
        Ok(v) => matches!(v.as_str(), "1" | "true" | "yes"),
        Err(_) => true,
    }
}

pub fn get_mcr_spray_mode() -> String {
    env::var("MOHAWK_MCR_SPRAY_MODE").unwrap_or_else(|_| "primary".to_string())
}

pub fn get_mcr_channels() -> usize {
    env::var("MOHAWK_MCR_CHANNELS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&v| v >= 1)
        .unwrap_or(3)
}

pub fn get_mcr_hash_seed() -> u64 {
    env::var("MOHAWK_MCR_HASH_SEED")
        .ok()
        .and_then(|s| {
            if s.starts_with("0x") {
                u64::from_str_radix(&s[2..], 16).ok()
            } else {
                s.parse::<u64>().ok()
            }
        })
        .unwrap_or(0xDEADBEEFu64)
}
