//! Shine's binary file format.
//!
//! ```text
//! offset  size  content
//! 0       4     magic "SHNE"
//! 4       2     format version, little endian
//! 6       ...   body: postcard-encoded value (varint integers, no field names)
//! ```
//!
//! postcard is positional, so the body layout is defined by the Rust types.
//! Any change to a saved type must bump [`VERSION`] and keep a decoder for
//! the previous version.

use serde::Serialize;
use serde::de::DeserializeOwned;

pub const MAGIC: [u8; 4] = *b"SHNE";
pub const VERSION: u16 = 2;
pub const EXTENSION: &str = "shine";
const HEADER: usize = MAGIC.len() + 2;

pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(256);
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    postcard::to_extend(value, bytes).expect("in-memory serialisation cannot fail")
}

/// Whether `bytes` start with the Shine header.
pub fn is_shine(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER && bytes[..4] == MAGIC
}

/// Format version of a Shine file, or an error if it isn't one.
pub fn version(bytes: &[u8]) -> Result<u16, String> {
    if !is_shine(bytes) {
        return Err("not a Shine file".into());
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version > VERSION {
        return Err(format!("file format {version} is newer than supported ({VERSION}); update Shine"));
    }
    if version == 0 {
        return Err(format!("file format {version} is no longer supported"));
    }
    Ok(version)
}

/// Decode a file in the current format.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    match version(bytes)? {
        VERSION => decode_body(bytes),
        v => Err(format!("file format {v} needs converting before it can be read")),
    }
}

/// Decode the body of a Shine file of any version into `T` (the caller
/// picks the type matching [`version`]).
pub fn decode_body<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    version(bytes)?;
    postcard::from_bytes(&bytes[HEADER..]).map_err(|e| format!("damaged Shine file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        values: Vec<f64>,
        flag: Option<bool>,
    }

    fn sample() -> Sample {
        Sample {
            name: "mzi".into(),
            values: vec![1.5, -2.0],
            flag: Some(true),
        }
    }

    #[test]
    fn round_trip_with_header() {
        let bytes = encode(&sample());
        assert_eq!(&bytes[..4], b"SHNE");
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), VERSION);
        assert!(is_shine(&bytes));
        assert_eq!(decode::<Sample>(&bytes).unwrap(), sample());
    }

    #[test]
    fn rejects_foreign_damaged_and_future_files() {
        assert_eq!(
            decode::<Sample>(b"{\"json\": true}").unwrap_err(),
            "not a Shine file"
        );
        assert!(!is_shine(b"SHN"));
        let mut bytes = encode(&sample());
        bytes.truncate(bytes.len() - 3);
        assert!(decode::<Sample>(&bytes).unwrap_err().starts_with("damaged"));
        let mut future = encode(&sample());
        future[4..6].copy_from_slice(&9u16.to_le_bytes());
        assert!(decode::<Sample>(&future).unwrap_err().contains("newer"));
        let mut old = encode(&sample());
        old[4..6].copy_from_slice(&0u16.to_le_bytes());
        assert!(
            decode::<Sample>(&old)
                .unwrap_err()
                .contains("no longer supported")
        );
    }

    #[test]
    fn compact_compared_to_json() {
        let bytes = encode(&sample());
        let json = serde_json::to_vec(&sample()).unwrap();
        // Even a tiny value with the 6-byte header is smaller; see the project
        // tests for realistic files.
        assert!(
            bytes.len() < json.len(),
            "{} vs {} bytes",
            bytes.len(),
            json.len()
        );
    }
}
