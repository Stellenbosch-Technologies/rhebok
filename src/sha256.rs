use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub fn hex_of_reader(reader: &mut impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return Ok(hex::encode(hasher.finalize())),
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

pub fn hex_of_file(path: &Path) -> std::io::Result<String> {
    hex_of_reader(&mut std::fs::File::open(path)?)
}
