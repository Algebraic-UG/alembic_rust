use std::path::{Path, PathBuf};

const ARCHIVES_DIR: &str = "target/abc-archives";

pub fn archive_path(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(ARCHIVES_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}.abc"))
}
