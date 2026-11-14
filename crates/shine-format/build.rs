//! Generates the Rust types of `proto/shine.proto` into OUT_DIR.

use std::path::PathBuf;

use pb_rs::ConfigBuilder;
use pb_rs::types::FileDescriptor;

fn main() {
    let proto = PathBuf::from("proto/shine.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("proto");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let config = ConfigBuilder::new(&[proto], None, Some(&out), &[PathBuf::from("proto")])
        .unwrap()
        .dont_use_cow(true)
        .build();
    FileDescriptor::run(&config).unwrap();
}
