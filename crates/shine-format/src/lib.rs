//! Shine's file format: projects, designs and components as Protocol Buffers
//! (`proto/shine.proto`), behind a small header.
//!
//! A file is the 4 bytes `SHPB`, a 2-byte format version (little-endian), a
//! byte saying how the rest is stored ([`Storage`]), then one [`ShineFile`]
//! message: as it is, or compressed with zlib (files over
//! [`COMPRESS_FROM`] bytes, whose boxes repeat a lot).

#[allow(clippy::all, unused_imports, non_camel_case_types, non_snake_case, dead_code, missing_docs)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/proto/mod.rs"));
}

pub use generated::shine::v1::*;

use std::io::{Read, Write};

use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};

/// The first bytes of every file.
pub const MAGIC: &[u8; 4] = b"SHPB";
/// The format version files are written with. It changes only for a change
/// protobuf can't make compatibly; the schema grows without it.
pub const VERSION: u16 = 1;

/// The header's length: magic, version and storage.
pub const HEADER: usize = 7;
/// Messages at least this long are compressed.
pub const COMPRESS_FROM: usize = 64 * 1024;

/// How the message after the header is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Storage {
    /// As it is: readable by any protobuf tool after the header.
    Plain = 0,
    /// Compressed with zlib (RFC 1950).
    Zlib = 1,
}

/// Whether `bytes` are a file in this format (of any version).
pub fn is_file(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER && bytes[..4] == MAGIC[..]
}

/// `file` with the header, ready to write: compressed if it is big.
pub fn encode(file: &ShineFile) -> Vec<u8> {
    let mut message = Vec::with_capacity(file.get_size());
    file.write_message(&mut Writer::new(&mut message)).expect("writing to memory doesn't fail");
    let storage = if message.len() >= COMPRESS_FROM { Storage::Zlib } else { Storage::Plain };
    let mut bytes = Vec::with_capacity(HEADER + if storage == Storage::Zlib { message.len() / 4 } else { message.len() });
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.push(storage as u8);
    match storage {
        Storage::Plain => bytes.extend_from_slice(&message),
        Storage::Zlib => {
            // Level 6, zlib's usual: most of the size of the best, quickly.
            let mut z = flate2::write::ZlibEncoder::new(bytes, flate2::Compression::new(6));
            z.write_all(&message).expect("writing to memory doesn't fail");
            bytes = z.finish().expect("writing to memory doesn't fail");
        }
    }
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
    let inflated;
    let message = match bytes[6] {
        0 => &bytes[HEADER..],
        1 => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(&bytes[HEADER..]).read_to_end(&mut out).map_err(|e| format!("damaged file: {e}"))?;
            inflated = out;
            &inflated[..]
        }
        s => return Err(format!("stored in a way this version doesn't know ({s}); update Shine")),
    };
    let mut reader = BytesReader::from_bytes(message);
    ShineFile::from_reader(&mut reader, message).map_err(|e| format!("damaged file: {e}"))
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
        assert_eq!(bytes[6], Storage::Plain as u8, "small files aren't compressed");
        assert_eq!(decode(&bytes).unwrap(), file);
    }

    #[test]
    fn big_files_are_compressed() {
        let pin = Instance { id: 1, block: mod_Instance::OneOfblock::builtin(Builtin { kind: mod_Builtin::OneOfkind::input_pin(Pin { name: "in".into() }) }), ..Default::default() };
        let layout = Layout { boxes: vec![pin; 20_000], ..Default::default() };
        let file = ShineFile { content: mod_ShineFile::OneOfcontent::document(Document { layout: Some(layout), ..Default::default() }) };
        let bytes = encode(&file);
        assert_eq!(bytes[6], Storage::Zlib as u8);
        assert!(bytes.len() < file.get_size() / 10, "{} bytes of {}", bytes.len(), file.get_size());
        assert_eq!(decode(&bytes).unwrap(), file);
    }

    #[test]
    fn refuses_other_damaged_and_newer_files() {
        assert!(decode(b"SHNE\x08\x00\x00").unwrap_err().contains("not a Shine file"));
        assert!(decode(b"SHPB\x02\x00\x00").unwrap_err().contains("newer"));
        assert!(decode(b"SHPB\x01\x00\x00\x0a\xff").unwrap_err().contains("damaged"));
        assert!(decode(b"SHPB\x01\x00\x01\x0a\xff").unwrap_err().contains("damaged"));
        assert!(decode(b"SHPB\x01\x00\x07").unwrap_err().contains("update Shine"));
    }
}
