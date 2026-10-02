use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub fn verified_manifest(source: &[u8], directory: &Path) -> serde_json::Value {
    let provenance: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("provenance.json")).unwrap()).unwrap();
    assert_eq!(provenance["schema"], 2);
    assert_eq!(
        provenance["sha256"].as_str().unwrap(),
        format!("{:x}", Sha256::digest(source))
    );
    assert_eq!(
        provenance["versions"],
        serde_json::json!(["2.2.6", "2025.5.10", "2025.3.30"])
    );
    assert!(!provenance["decoder"].as_str().unwrap().is_empty());
    let manifest = std::fs::read(directory.join("tiles.txt")).unwrap();
    assert_eq!(
        provenance["manifest_sha256"].as_str().unwrap(),
        format!("{:x}", Sha256::digest(&manifest))
    );
    let tiles = provenance["tiles"].as_array().unwrap();
    assert!(!tiles.is_empty());
    let lines = std::str::from_utf8(&manifest)
        .unwrap()
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), tiles.len());
    for (line, tile) in lines.iter().zip(tiles) {
        let expected = format!(
            "{} {} {} {} {}",
            tile["offset"],
            tile["size"],
            tile["width"],
            tile["height"],
            tile["file"].as_str().unwrap()
        );
        assert_eq!(*line, expected);
    }
    let mut names = std::collections::BTreeSet::new();
    for item in tiles.iter().chain(provenance["stages"].as_array().unwrap()) {
        let name = item["file"].as_str().unwrap();
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
                && !name.starts_with('.')
        );
        assert!(names.insert(name));
        let mut file = std::fs::File::open(directory.join(name)).unwrap();
        let mut buffer = [0u8; 65536];
        let mut hash = Sha256::new();
        loop {
            let count = file.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        assert_eq!(
            format!("{:x}", hash.finalize()),
            item["sha256"].as_str().unwrap()
        );
    }
    assert!(names.contains("encoded.u16le") && names.contains("normalized.f32le"));
    provenance
}
