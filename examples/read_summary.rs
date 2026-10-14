use std::env;
use std::fmt::Debug;
use std::fs::File;
use std::io::{Read, Seek};

use alembic_rust::{ArchiveReader, ArrayHandle, AttributeType, read_archive};

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .expect("usage: read_summary <archive.abc>");
    let (contents, mut reader) = match read_archive(File::open(path)?) {
        Ok(opened) => opened,
        Err(error) => {
            println!("error: {error}");
            return Ok(());
        }
    };

    println!("metadata:");
    for (key, value) in contents.metadata.entries() {
        println!("  {key}={value}");
    }
    for (name, handle) in &contents.user_data {
        let mut bytes = Vec::new();
        reader.user_data_reader(handle)?.read_to_end(&mut bytes)?;
        println!("user data {name}: {bytes:?}");
    }
    for (name, points) in &contents.points {
        println!("points {name}: {} points", points.positions.len());
        if let Some(bounds) = points.bounds {
            println!("  bounds: {bounds:?}");
        }
        print_values(&mut reader, "  positions", &points.positions)?;
        if let Some(velocities) = &points.velocities {
            print_values(&mut reader, "  velocities", velocities)?;
        }
        if let Some(widths) = &points.widths {
            print_values(&mut reader, "  widths", widths)?;
        }
        for (name, attribute) in &points.attributes {
            let label = format!("  attribute {name}");
            match attribute.attribute_type() {
                AttributeType::Float1 => {
                    print_values(&mut reader, &label, &attribute.typed::<f32>()?)?
                }
                AttributeType::Float2 => {
                    print_values(&mut reader, &label, &attribute.typed::<[f32; 2]>()?)?
                }
                AttributeType::Float3 => {
                    print_values(&mut reader, &label, &attribute.typed::<[f32; 3]>()?)?
                }
            }
        }
    }
    Ok(())
}

fn print_values<R: Read + Seek, T: bytemuck::Pod + Debug>(
    reader: &mut ArchiveReader<R>,
    label: &str,
    handle: &ArrayHandle<T>,
) -> alembic_rust::Result<()> {
    let mut values = vec![T::zeroed(); handle.len()];
    reader.read_into(handle, &mut values)?;
    println!("{label}: {values:?}");
    Ok(())
}
