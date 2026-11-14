//! Shine's file format: projects, designs and components as Protocol Buffers
//! (`proto/shine.proto`), behind a small header.
//!
//! A file is the 4 bytes `SHPB`, a 2-byte format version (little-endian),
//! then one [`ShineFile`] message.

#[allow(clippy::all, unused_imports, non_camel_case_types, non_snake_case, dead_code, missing_docs)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/proto/mod.rs"));
}

pub use generated::shine::v1::*;

use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};

/// The first bytes of every file.
pub const MAGIC: &[u8; 4] = b"SHPB";
/// The format version files are written with. It changes only for a change
/// protobuf can't make compatibly; the schema grows without it.
pub const VERSION: u16 = 1;

/// Whether `bytes` are a file in this format (of any version).
pub fn is_file(bytes: &[u8]) -> bool {
    bytes.len() >= 6 && bytes[..4] == MAGIC[..]
}

/// `file` with the header, ready to write.
pub fn encode(file: &ShineFile) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(6 + file.get_size());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    let mut writer = Writer::new(&mut bytes);
    file.write_message(&mut writer).expect("writing to memory doesn't fail");
    bytes
}

/// Read a file written by [`encode`].
pub fn decode(bytes: &[u8]) -> Result<ShineFile, String> {
    if !is_file(bytes) {
        return Err("not a Shine file".into());
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version > VERSION {
        return Err(format!("file format {version} is newer than supported ({VERSION}); update Shine"));
    }
    let body = &bytes[6..];
    let mut reader = BytesReader::from_bytes(body);
    ShineFile::from_reader(&mut reader, body).map_err(|e| format!("damaged file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let file = ShineFile {
            content: mod_ShineFile::OneOfcontent::document(Document {
                name: "Ring filter".into(),
                cell_um: 10.0,
                layout: Some(Layout { next_box: 3, ..Default::default() }),
                ..Default::default()
            }),
        };
        let bytes = encode(&file);
        assert!(is_file(&bytes));
        assert_eq!(decode(&bytes).unwrap(), file);
    }

    #[test]
    fn refuses_other_damaged_and_newer_files() {
        assert!(decode(b"SHNE\x08\x00").unwrap_err().contains("not a Shine file"));
        assert!(decode(b"SHPB\x02\x00").unwrap_err().contains("newer"));
        assert!(decode(b"SHPB\x01\x00\x0a\xff").unwrap_err().contains("damaged"));
    }
}
