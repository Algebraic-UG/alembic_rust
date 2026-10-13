use crate::archive::ArchiveWriter;
use crate::compound::CompoundWriter;
use crate::data_type::{DataType, Pod};
use crate::error::Error;
use crate::metadata::MetaData;

fn metadata(pairs: &[(&str, &str)]) -> MetaData {
    let mut md = MetaData::new();
    for (key, value) in pairs {
        md.set(*key, *value).unwrap();
    }
    md
}

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn f64_bytes(values: &[f64]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn writes_objects_with_nested_properties() {
    let mut archive = ArchiveWriter::create(std::io::Cursor::new(Vec::new())).unwrap();
    let out = archive.output();

    let positions = f32_bytes(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    let bounds = f64_bytes(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let long_name = "n".repeat(300);
    let long_metadata = metadata(&[("note", &"x".repeat(400))]);

    let mut extra = CompoundWriter::new();
    extra
        .add_array(
            out,
            "single",
            &MetaData::new(),
            DataType::new(Pod::Float32, 1),
            &f32_bytes(&[5.0]),
        )
        .unwrap();
    extra
        .add_array(
            out,
            &long_name,
            &long_metadata,
            DataType::new(Pod::Float32, 2),
            &f32_bytes(&[1.0, 2.0]),
        )
        .unwrap();

    let mut geom = CompoundWriter::new();
    geom.add_array(
        out,
        "P",
        &metadata(&[("interpretation", "point"), ("geoScope", "var")]),
        DataType::new(Pod::Float32, 3),
        &positions,
    )
    .unwrap();
    geom.add_array(
        out,
        ".pointIds",
        &metadata(&[("geoScope", "var")]),
        DataType::new(Pod::Uint64, 1),
        &[],
    )
    .unwrap();
    geom.add_scalar(
        out,
        ".selfBnds",
        &metadata(&[("interpretation", "box")]),
        DataType::new(Pod::Float64, 6),
        &bounds,
    )
    .unwrap();
    geom.add_compound(out, "extra", &metadata(&[("schema", "Extra_v1")]), extra)
        .unwrap();

    let mut properties = CompoundWriter::new();
    properties
        .add_compound(out, ".geom", &metadata(&[("schema", "Test_Geom_v1")]), geom)
        .unwrap();
    archive
        .add_child_object("pts", &metadata(&[("schema", "Test_v1")]), properties)
        .unwrap();
    archive
        .add_child_object("empty", &MetaData::new(), CompoundWriter::new())
        .unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    assert_eq!(&bytes[..6], b"Ogawa\xff");
}

#[test]
fn sibling_names_must_be_unique_and_valid() {
    let mut archive = ArchiveWriter::create(std::io::Cursor::new(Vec::new())).unwrap();
    archive
        .add_child_object("a", &MetaData::new(), CompoundWriter::new())
        .unwrap();
    let duplicate = archive.add_child_object("a", &MetaData::new(), CompoundWriter::new());
    assert!(matches!(duplicate, Err(Error::DuplicateName { .. })));

    let out = archive.output();
    let mut properties = CompoundWriter::new();
    let slashed = properties.add_array(
        out,
        "a/b",
        &MetaData::new(),
        DataType::new(Pod::Float32, 1),
        &[],
    );
    assert!(matches!(slashed, Err(Error::InvalidName { .. })));
}
