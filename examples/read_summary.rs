use std::env;
use std::fmt::Debug;
use std::fs::File;
use std::io::{Read, Seek};

use alembic_rust::{ArchiveReader, ArrayHandle, AttributeValues, read_archive};

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
    for user_data in &contents.user_data {
        print_values(
            &mut reader,
            &format!("user data {}", user_data.name),
            &user_data.bytes,
        )?;
    }
    for points in &contents.points {
        println!("points {}: {} points", points.name, points.positions.len());
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
        for attribute in &points.attributes {
            let label = format!("  attribute {}", attribute.name);
            match &attribute.values {
                AttributeValues::Float1(values) => print_values(&mut reader, &label, values)?,
                AttributeValues::Float2(values) => print_values(&mut reader, &label, values)?,
                AttributeValues::Float3(values) => print_values(&mut reader, &label, values)?,
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
