/// BLAKE3 content hashing. Wire output shows `b3:<16 hex chars>`; the full
/// 64-char hex digest is kept in the ledger/manifest.
pub fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

pub fn short(full_hex: &str) -> String {
    let s: String = full_hex.chars().take(16).collect();
    format!("b3:{s}")
}

pub fn hash_str(s: &str) -> String {
    hash_bytes(s.as_bytes())
}
