//! AF_XDP UMEM and socket abstractions.

pub mod rings;
pub mod socket;
pub mod umem;

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
