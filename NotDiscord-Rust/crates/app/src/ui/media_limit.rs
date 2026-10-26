//! The local encoded download limit is independent of Discord's upload limit
//! and of the existing decoded image/frame limits.
pub(super) const DEFAULT_MIB: u32 = 25;
pub(super) const MAX_MIB: u32 = 256;
pub(super) const MAX_BYTES: u64 = MAX_MIB as u64 * 1024 * 1024;

pub(super) fn bytes(mib: u32) -> u64 {
    u64::from(mib.clamp(1, MAX_MIB)) * 1024 * 1024
}
pub(super) fn error(limit: u64) -> String {
    if limit.is_multiple_of(1024 * 1024) {
        format!("La imagen supera el límite local de {} MiB.", limit / (1024 * 1024))
    } else {
        format!("La imagen supera el límite local de {limit} bytes.")
    }
}
pub(super) fn is_limit_error(error: &str) -> bool {
    error.starts_with("La imagen supera el límite local de ")
}
