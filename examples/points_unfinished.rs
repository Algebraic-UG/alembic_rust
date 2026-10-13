use std::env;

use alembic_rust::{ArchiveWriter, Error};

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "points_unfinished.abc".to_owned());
    let mut archive = ArchiveWriter::create_file(path)?;
    let points_dropped_before_finish = archive.begin_points("lost", &[[0.0; 3]])?;
    drop(points_dropped_before_finish);
    match archive.finish() {
        Err(Error::UnfinishedObject { .. }) => Ok(()),
        other => panic!("expected the archive to refuse finishing, got {other:?}"),
    }
}
