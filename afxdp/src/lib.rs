//! AF_XDP UMEM and socket abstractions.

pub mod umem;
pub mod socket;
pub mod rings;

pub use socket::{AfXdpSocket, MockSocket};

pub fn available() -> bool {
    cfg!(feature = "real")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoke() {
        assert!(!available());
    }
}
