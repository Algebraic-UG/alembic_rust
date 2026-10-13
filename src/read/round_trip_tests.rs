use std::io::Cursor;

use crate::archive::ArchiveWriter;
use crate::compound::CompoundWriter;
use crate::error::Error;
use crate::metadata::MetaData;
use crate::points::Attribute;
use crate::read::{ArchiveContents, ArchiveReader, ArrayHandle, AttributeValues, read_archive};

type Reader = ArchiveReader<Cursor<Vec<u8>>>;

fn round_trip(
    build: impl FnOnce(&mut ArchiveWriter<Cursor<Vec<u8>>>),
) -> (ArchiveContents, Reader) {
    let mut archive = ArchiveWriter::create(Cursor::new(Vec::new())).unwrap();
    build(&mut archive);
    let file = archive.finish().unwrap().into_inner();
    read_archive(Cursor::new(file)).unwrap()
}

fn values<T: bytemuck::Pod>(reader: &mut Reader, handle: &ArrayHandle<T>) -> Vec<T> {
    let mut values = vec![T::zeroed(); handle.len()];
    reader.read_into(handle, &mut values).unwrap();
    values
}

fn bits_of(values: &[[f32; 3]]) -> Vec<[u32; 3]> {
    values.iter().map(|v| v.map(f32::to_bits)).collect()
}

#[test]
fn long_names_need_wider_header_fields() {
    let two_byte_name = "a".repeat(300);
    let four_byte_name = "b".repeat(70_000);
    let (contents, _) = round_trip(|archive| {
        for name in [&two_byte_name, &four_byte_name] {
            let mut points = archive.begin_points(name, &[[0.0; 3]]).unwrap();
            points.attribute(name, Attribute::Float1(&[1.0])).unwrap();
            points.finish().unwrap();
            archive.user_data(name, &[1]).unwrap();
        }
    });
    for (points, name) in contents
        .points
        .iter()
        .zip([&two_byte_name, &four_byte_name])
    {
        assert_eq!(points.path, format!("/{name}"));
        assert_eq!(&points.attributes[0].name, name);
    }
    let user_data_names: Vec<_> = contents.user_data.iter().map(|u| &u.name).collect();
    assert_eq!(user_data_names, [&two_byte_name, &four_byte_name]);
}

#[test]
fn non_ascii_names_and_metadata_survive() {
    let (contents, _) = round_trip(|archive| {
        archive
            .metadata_mut()
            .set("_ai_Description", "Partikel über Wasser, 粒子")
            .unwrap();
        let mut points = archive.begin_points("粒子", &[[0.0; 3]]).unwrap();
        points
            .attribute("température", Attribute::Float1(&[1.0]))
            .unwrap();
        points.finish().unwrap();
    });
    assert_eq!(
        contents.metadata.get("_ai_Description"),
        Some("Partikel über Wasser, 粒子")
    );
    assert_eq!(contents.points[0].path, "/粒子");
    assert_eq!(contents.points[0].attributes[0].name, "température");
}

#[test]
fn special_float_values_are_kept_bit_for_bit() {
    let unusual = [
        [f32::NAN, f32::INFINITY, f32::NEG_INFINITY],
        [-0.0, f32::MIN_POSITIVE / 2.0, f32::MAX],
        [f32::from_bits(0x7fc0_1234), f32::MIN, 1.0],
    ];
    let (contents, mut reader) = round_trip(|archive| {
        let mut points = archive.begin_points("p", &unusual).unwrap();
        points.velocities(&unusual).unwrap();
        points.attribute("a", Attribute::Float3(&unusual)).unwrap();
        points.finish().unwrap();
    });
    let points = &contents.points[0];
    assert_eq!(
        bits_of(&values(&mut reader, &points.positions)),
        bits_of(&unusual)
    );
    let velocities = points.velocities.as_ref().unwrap();
    assert_eq!(bits_of(&values(&mut reader, velocities)), bits_of(&unusual));
    let AttributeValues::Float3(attribute) = points.attributes[0].values else {
        panic!("expected a three-float attribute");
    };
    assert_eq!(bits_of(&values(&mut reader, &attribute)), bits_of(&unusual));
}

#[test]
fn many_points_round_trip() {
    let positions: Vec<[f32; 3]> = (0..100_000)
        .map(|i| [i as f32, -(i as f32), (i % 7) as f32])
        .collect();
    let widths: Vec<f32> = (0..100_000).map(|i| i as f32 * 0.5).collect();
    let (contents, mut reader) = round_trip(|archive| {
        let mut points = archive.begin_points("p", &positions).unwrap();
        points.widths(&widths).unwrap();
        points.finish().unwrap();
    });
    let points = &contents.points[0];
    assert_eq!(values(&mut reader, &points.positions), positions);
    assert_eq!(values(&mut reader, points.widths.as_ref().unwrap()), widths);
    assert_eq!(
        points.bounds,
        Some([0.0, -99_999.0, 0.0, 99_999.0, 0.0, 6.0])
    );
}

#[test]
fn point_arrays_reject_buffers_of_the_wrong_length() {
    let (contents, mut reader) = round_trip(|archive| {
        let mut points = archive.begin_points("p", &[[0.0; 3]; 3]).unwrap();
        points.widths(&[1.0; 3]).unwrap();
        points
            .attribute("uv", Attribute::Float2(&[[0.0; 2]; 3]))
            .unwrap();
        points.finish().unwrap();
    });
    let points = &contents.points[0];
    let too_short = reader.read_into(&points.positions, &mut [[0.0; 3]; 2]);
    let too_long = reader.read_into(points.widths.as_ref().unwrap(), &mut [0.0; 4]);
    let AttributeValues::Float2(uv) = points.attributes[0].values else {
        panic!("expected a two-float attribute");
    };
    let empty = reader.read_into(&uv, &mut []);
    for (result, actual) in [(too_short, 2), (too_long, 4), (empty, 0)] {
        assert!(
            matches!(result, Err(Error::BufferLengthMismatch { expected: 3, actual: a }) if a == actual),
            "{result:?}"
        );
    }
    assert_eq!(values(&mut reader, &points.positions), [[0.0; 3]; 3]);
}

#[test]
fn full_metadata_dictionary_falls_back_to_inline_metadata() {
    let object_count = 300;
    let mut archive = ArchiveWriter::create(Cursor::new(Vec::new())).unwrap();
    for i in 0..object_count {
        let name = format!("object{i}");
        let mut metadata = MetaData::new();
        metadata.set("index", i.to_string()).unwrap();
        archive.claim_child_name(&name).unwrap();
        archive
            .add_claimed_child_object(&name, &metadata, CompoundWriter::new())
            .unwrap();
    }
    let mut long_metadata = MetaData::new();
    long_metadata.set("long", "x".repeat(300)).unwrap();
    archive.claim_child_name("long").unwrap();
    archive
        .add_claimed_child_object("long", &long_metadata, CompoundWriter::new())
        .unwrap();
    let file = archive.finish().unwrap().into_inner();

    let hierarchy = crate::read::tree::tests::hierarchy_of(file);
    assert_eq!(hierarchy.objects_depth_first.len(), object_count + 1);
    for (i, object) in hierarchy.objects_depth_first[..object_count]
        .iter()
        .enumerate()
    {
        assert_eq!(object.path, format!("/object{i}"));
        assert_eq!(object.metadata.get("index"), Some(i.to_string().as_str()));
    }
    assert_eq!(
        hierarchy.objects_depth_first[object_count].metadata,
        long_metadata
    );
}
