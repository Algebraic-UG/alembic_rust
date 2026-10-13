use std::env;

use alembic_rust::ArchiveWriter;

fn main() -> alembic_rust::Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| "empty.abc".to_owned());
    ArchiveWriter::create_file(path)?.finish()?;
    Ok(())
}
