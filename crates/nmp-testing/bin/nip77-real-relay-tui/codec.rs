pub fn hex_encode(bytes: &[u8]) -> String {
    static HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex payload".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().chunks(2) {
        let byte = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        out.push(byte);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("invalid hex nibble".to_string()),
    }
}
