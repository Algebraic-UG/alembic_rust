use std::env;
use std::io::Write;

use alembic_rust::{ArchiveWriter, POSITIONS};

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "user_data.abc".to_owned());
    let mut archive = ArchiveWriter::create_file(path)?;
    archive.user_data_with("every_byte_value", |w| {
        (0..=255u8).try_for_each(|byte| w.write_all(&[byte]))
    })??;
    archive.user_data("empty", &[])?;
    let mut particles = archive.begin_points("particles", 2)?;
    particles.attribute(POSITIONS, &[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]])?;
    particles.finish()?;
    archive.finish()?;
    Ok(())
}
