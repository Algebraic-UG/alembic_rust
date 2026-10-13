use std::env;

use alembic_rust::ArchiveWriter;

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "user_data.abc".to_owned());
    let mut archive = ArchiveWriter::create_file(path)?;
    let every_byte_value: Vec<u8> = (0..=255).collect();
    archive.user_data("every_byte_value", &every_byte_value)?;
    archive.user_data("empty", &[])?;
    archive
        .begin_points("particles", &[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]])?
        .finish()?;
    archive.finish()?;
    Ok(())
}
