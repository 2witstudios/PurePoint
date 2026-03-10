/// Serde helper: encode `Vec<u8>` as hex in JSON for binary PTY data.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    use std::fmt::Write;
    let mut hex = String::with_capacity(data.len() * 2);
    for b in data {
        write!(hex, "{b:02x}").unwrap();
    }
    hex.serialize(serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    use serde::de::Error;
    let s = String::deserialize(deserializer)?;
    if s.len() % 2 != 0 {
        return Err(D::Error::custom("odd-length hex string"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| D::Error::custom(format!("invalid hex: {e}")))
        })
        .collect()
}
