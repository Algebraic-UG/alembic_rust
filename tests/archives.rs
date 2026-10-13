mod support;

use alembic_rust::ArchiveWriter;
use support::archive_path;

#[test]
fn writes_empty_archive() {
    ArchiveWriter::create_file(archive_path("empty"))
        .unwrap()
        .finish()
        .unwrap();
}

#[test]
fn writes_empty_archive_with_info() {
    let mut w = ArchiveWriter::create_file(archive_path("empty_info")).unwrap();
    let md = w.metadata_mut();
    md.set("_ai_Application", "alembic_rust tests").unwrap();
    md.set("_ai_DateWritten", "Fri Oct  9 12:00:00 2026")
        .unwrap();
    md.set("_ai_Description", "an empty archive").unwrap();
    w.finish().unwrap();
}

#[test]
fn leaves_dropped_archive_unfinished() {
    drop(ArchiveWriter::create_file(archive_path("unfinished")).unwrap());
}
