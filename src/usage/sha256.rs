//! SHA-256 helpers for the usage cache (the digest lives in `misc::sha256`).

/// `createHash('sha256').update(s).digest('hex').slice(0, n)`
pub fn hex_prefix(s: &str, n: usize) -> String {
    let mut hex = crate::misc::sha256::hex(s.as_bytes());
    hex.truncate(n);
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(hex_prefix("", 64), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex_prefix("abc", 64), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = "a".repeat(1000);
        assert_eq!(hex_prefix(&long, 16), "41edece42d63e8d9");
    }
}
