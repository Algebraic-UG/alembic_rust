use std::env;

use alembic_rust::ArchiveWriter;

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "unfinished.abc".to_owned());
    let archive_dropped_before_finish = ArchiveWriter::create_file(path)?;
    drop(archive_dropped_before_finish);
    Ok(())
}
