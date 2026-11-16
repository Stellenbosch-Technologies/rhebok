use rhebok::bench::golden_sha256;
use rhebok::config::edit::etag_of;
use rhebok::memory::{memory_id, MemoryKind};
use rhebok::model::Document;
use rhebok::sha256::hex_of_file;

const SHA256_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn a_content_hash_is_the_standard_sha256_in_lowercase_hex() {
    assert_eq!(Document::content_hash("abc"), SHA256_OF_ABC);
    assert_eq!(golden_sha256("abc"), SHA256_OF_ABC);
    assert_eq!(etag_of(Some("abc")), SHA256_OF_ABC);
}

#[test]
fn a_document_id_is_the_first_sixteen_bytes_of_its_uri_digest() {
    assert_eq!(
        Document::new_id("file:///notes/adr-0004.md"),
        "1aef1d0f4c2c638d791f04b426ecb3d2"
    );
}

#[test]
fn a_memory_id_is_the_first_six_bytes_of_its_kind_and_text_digest() {
    assert_eq!(
        memory_id(MemoryKind::Lesson, "  Never comment code unless asked.\n"),
        "961b0c07a647"
    );
    assert_eq!(
        memory_id(MemoryKind::Fact, "The vault lives in ~/vault."),
        "71f66489b3a4"
    );
}

#[test]
fn a_file_digest_covers_every_byte_across_read_buffers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.tar.gz");
    let bytes: Vec<u8> = (0..(3 * 65536 + 17)).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        hex_of_file(&path).unwrap(),
        "4bb7439bc39bc2d0e3d6a915d7c81e38250a9c5bb320a94a19a95bba0d5fe40a"
    );
}
