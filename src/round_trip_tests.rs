use std::io::{Cursor, Read, Write};

use crate::error::Error;
use crate::format::schema::{POSITIONS, VELOCITIES, WIDTHS};
use crate::read::{ArchiveContents, ArchiveReader, ArrayHandle, read_archive};
use crate::write::ArchiveWriter;

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
            let mut points = archive.begin_points(name, 1).unwrap();
            points.attribute(POSITIONS, &[[0.0; 3]]).unwrap();
            points.attribute(name, &[1.0]).unwrap();
            points.finish().unwrap();
            archive.user_data(name, &[1]).unwrap();
        }
    });
    for name in [&two_byte_name, &four_byte_name] {
        assert!(contents.points[name].attributes.contains_key(name));
    }
    let user_data_names: Vec<_> = contents.user_data.keys().collect();
    assert_eq!(user_data_names, [&two_byte_name, &four_byte_name]);
}

#[test]
fn non_ascii_names_and_metadata_survive() {
    let (contents, _) = round_trip(|archive| {
        archive
            .metadata_mut()
            .set("_ai_Description", "Partikel über Wasser, 粒子")
            .unwrap();
        let mut points = archive.begin_points("粒子", 1).unwrap();
        points.attribute(POSITIONS, &[[0.0; 3]]).unwrap();
        points.attribute("température", &[1.0]).unwrap();
        points.finish().unwrap();
    });
    assert_eq!(
        contents.metadata.get("_ai_Description"),
        Some("Partikel über Wasser, 粒子")
    );
    assert!(
        contents.points["粒子"]
            .attributes
            .contains_key("température")
    );
}

#[test]
fn special_float_values_are_kept_bit_for_bit() {
    let unusual = [
        [f32::NAN, f32::INFINITY, f32::NEG_INFINITY],
        [-0.0, f32::MIN_POSITIVE / 2.0, f32::MAX],
        [f32::from_bits(0x7fc0_1234), f32::MIN, 1.0],
    ];
    let (contents, mut reader) = round_trip(|archive| {
        let mut points = archive.begin_points("p", unusual.len()).unwrap();
        points.attribute(POSITIONS, &unusual).unwrap();
        points.attribute(VELOCITIES, &unusual).unwrap();
        points.attribute("a", &unusual).unwrap();
        points.finish().unwrap();
    });
    let points = &contents.points["p"];
    let positions = points.attributes[POSITIONS].typed::<[f32; 3]>().unwrap();
    assert_eq!(bits_of(&values(&mut reader, &positions)), bits_of(&unusual));
    let velocities = points.attributes[VELOCITIES].typed::<[f32; 3]>().unwrap();
    assert_eq!(
        bits_of(&values(&mut reader, &velocities)),
        bits_of(&unusual)
    );
    let attribute = points.attributes["a"].typed::<[f32; 3]>().unwrap();
    assert_eq!(bits_of(&values(&mut reader, &attribute)), bits_of(&unusual));
}

#[test]
fn many_points_round_trip() {
    let positions: Vec<[f32; 3]> = (0..100_000)
        .map(|i| [i as f32, -(i as f32), (i % 7) as f32])
        .collect();
    let widths: Vec<f32> = (0..100_000).map(|i| i as f32 * 0.5).collect();
    let (contents, mut reader) = round_trip(|archive| {
        let mut points = archive.begin_points("p", positions.len()).unwrap();
        points.attribute(POSITIONS, &positions).unwrap();
        points.attribute(WIDTHS, &widths).unwrap();
        points.finish().unwrap();
    });
    let points = &contents.points["p"];
    let read_positions = points.attributes[POSITIONS].typed::<[f32; 3]>().unwrap();
    assert_eq!(values(&mut reader, &read_positions), positions);
    let read_widths = points.attributes[WIDTHS].typed::<f32>().unwrap();
    assert_eq!(values(&mut reader, &read_widths), widths);
    assert_eq!(
        points.bounds,
        Some([0.0, -99_999.0, 0.0, 99_999.0, 0.0, 6.0])
    );
}

#[test]
fn point_arrays_reject_buffers_of_the_wrong_length() {
    let (contents, mut reader) = round_trip(|archive| {
        let mut points = archive.begin_points("p", 3).unwrap();
        points.attribute(POSITIONS, &[[0.0; 3]; 3]).unwrap();
        points.attribute(WIDTHS, &[1.0; 3]).unwrap();
        points.attribute("uv", &[[0.0; 2]; 3]).unwrap();
        points.finish().unwrap();
    });
    let points = &contents.points["p"];
    let positions = points.attributes[POSITIONS].typed::<[f32; 3]>().unwrap();
    let widths = points.attributes[WIDTHS].typed::<f32>().unwrap();
    let too_short = reader.read_into(&positions, &mut [[0.0; 3]; 2]);
    let too_long = reader.read_into(&widths, &mut [0.0; 4]);
    let uv = points.attributes["uv"].typed::<[f32; 2]>().unwrap();
    let empty = reader.read_into(&uv, &mut []);
    for (result, actual) in [(too_short, 2), (too_long, 4), (empty, 0)] {
        assert!(
            matches!(result, Err(Error::BufferLengthMismatch { expected: 3, actual: a }) if a == actual),
            "{result:?}"
        );
    }
    assert_eq!(values(&mut reader, &positions), [[0.0; 3]; 3]);
}

#[test]
fn streamed_user_data_reads_back_as_a_stream() {
    let blob: Vec<u8> = (0..=255).cycle().take(100_000).collect();
    let (contents, mut reader) = round_trip(|archive| {
        archive
            .user_data_with("blob", |w| {
                blob.chunks(777).try_for_each(|chunk| w.write_all(chunk))
            })
            .unwrap()
            .unwrap();
        archive.user_data("small", &[7, 8, 9]).unwrap();
    });
    let blob_data = &contents.user_data["blob"];
    let small = &contents.user_data["small"];

    let mut whole = Vec::new();
    reader
        .user_data_reader(blob_data)
        .unwrap()
        .read_to_end(&mut whole)
        .unwrap();
    assert_eq!(whole, blob);

    let mut in_small_reads = Vec::new();
    {
        let mut stream = reader.user_data_reader(blob_data).unwrap();
        let mut chunk = [0u8; 100];
        loop {
            let read = stream.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            in_small_reads.extend_from_slice(&chunk[..read]);
        }
    }
    assert_eq!(in_small_reads, blob);

    let mut start = [0u8; 10];
    reader
        .user_data_reader(blob_data)
        .unwrap()
        .read_exact(&mut start)
        .unwrap();
    assert_eq!(start, blob[..10]);
    assert_eq!(values(&mut reader, small), [7, 8, 9]);
}
