use std::env;

use alembic_rust::ArchiveWriter;

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "empty_info.abc".to_owned());
    let mut archive = ArchiveWriter::create_file(path)?;
    let metadata = archive.metadata_mut();
    metadata.set("_ai_Application", "alembic_rust tests")?;
    metadata.set("_ai_DateWritten", "Fri Oct  9 12:00:00 2026")?;
    metadata.set("_ai_Description", "an empty archive")?;
    archive.finish()?;
    Ok(())
}
