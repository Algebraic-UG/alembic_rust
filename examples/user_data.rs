use std::env;
use std::io::Write;

use alembic_rust::ArchiveWriter;

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "user_data.abc".to_owned());
    let mut archive = ArchiveWriter::create_file(path)?;
    archive.user_data_with("every_byte_value", |w| {
        for byte in 0..=255u8 {
            w.write_all(&[byte])?;
        }
        Ok(())
    })?;
    archive.user_data("empty", &[])?;
    archive
        .begin_points("particles", &[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]])?
        .finish()?;
    archive.finish()?;
    Ok(())
}
