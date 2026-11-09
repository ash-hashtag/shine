//! The session: the open project, its tabs and whether it was saved, kept in
//! the app's storage (a settings file on desktop, browser storage on the
//! web) every few seconds and restored at the next start. Closing the app or
//! the tab, or a crash, loses at most a few seconds of work.

use serde::{Deserialize, Serialize};

/// Storage key of the session.
pub const KEY: &str = "shine.session";
/// Prefix marking the encoding, so a future one can be told apart.
const PREFIX: &str = "shine-session-1:";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// The project in Shine's file format.
    pub project: Vec<u8>,
    /// The file it was last saved to or opened from.
    pub path: Option<String>,
    /// The project's fingerprint when it was last saved, so unsaved changes
    /// stay marked after a restore.
    pub saved_fingerprint: u64,
    /// Open tabs and the active one (document ids).
    pub tabs: Vec<u64>,
    pub active: Option<u64>,
    /// Each open tab's view: document id, pan (x, y) and zoom.
    pub views: Vec<(u64, f32, f32, f32)>,
    /// Whether the app was closed normally (desktop), rather than crashing.
    pub clean_exit: bool,
}

impl Session {
    /// As text for the storage: the prefix, then base64 of postcard.
    pub fn encode(&self) -> String {
        format!("{PREFIX}{}", base64_encode(&postcard::to_allocvec(self).expect("serialisable")))
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        let body = text.strip_prefix(PREFIX).ok_or("not a Shine session")?;
        let bytes = base64_decode(body)?;
        postcard::from_bytes(&bytes).map_err(|e| format!("damaged session: {e}"))
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let v = ALPHABET.iter().position(|&a| a == c).ok_or("invalid base64")? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_and_matches_the_standard() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        for len in 0..300 {
            let bytes: Vec<u8> = (0..len).map(|k| (k * 37 + 11) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes, "length {len}");
        }
        assert!(base64_decode("Zm9v!").is_err());
    }

    #[test]
    fn sessions_round_trip_and_reject_garbage() {
        let s = Session {
            project: vec![1, 2, 3, 255],
            path: Some("/tmp/x.shine".into()),
            saved_fingerprint: 42,
            tabs: vec![1, 3],
            active: Some(3),
            views: vec![(3, 10.0, -20.5, 1.5)],
            clean_exit: true,
        };
        assert_eq!(Session::decode(&s.encode()).unwrap(), s);
        assert!(Session::decode("hello").is_err());
        assert!(Session::decode("shine-session-1:AAAA").is_err());
    }
}
