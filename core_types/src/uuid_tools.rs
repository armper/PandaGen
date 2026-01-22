use uuid::Uuid;

#[cfg(target_os = "none")]
pub fn new_uuid() -> Uuid {
    use core::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let hi = COUNTER.fetch_add(1, Ordering::Relaxed);
    let lo = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&hi.to_le_bytes());
    bytes[8..].copy_from_slice(&lo.to_le_bytes());

    // Mark as UUIDv4 and RFC 4122 variant.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    Uuid::from_bytes(bytes)
}

#[cfg(not(target_os = "none"))]
pub fn new_uuid() -> Uuid {
    Uuid::new_v4()
}
