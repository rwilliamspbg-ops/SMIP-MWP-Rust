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

pub fn get_profile_enabled() -> bool {
    match env::var("MOHAWK_PROFILE") {
        Ok(v) => matches!(v.as_str(), "1" | "true" | "yes"),
        Err(_) => false,
    }
}
