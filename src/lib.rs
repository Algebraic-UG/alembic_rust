//! A streaming writer and reader for Alembic Ogawa archives of points objects.
//!
//! [`ArchiveWriter`] writes points objects with per-point `f32` attributes, all at a single time
//! sample at 0.0 and directly under the archive root, plus archive metadata and binary user data.
//! The output follows the layout of Alembic 1.8.12, so Alembic-aware tools read it as ordinary
//! points. [`read_archive`] reads such archives back.
//!
//! # Writing
//!
//! ```
//! use std::io::Cursor;
//!
//! use alembic_rust::{ArchiveWriter, Attribute};
//!
//! # fn main() -> alembic_rust::Result<()> {
//! let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
//!
//! let mut archive = ArchiveWriter::create(Cursor::new(Vec::new()))?;
//! archive.metadata_mut().set("_ai_Application", "my app")?;
//! archive.user_data("settings", b"{}")?;
//!
//! let mut particles = archive.begin_points("particles", &positions)?;
//! particles.velocities(&[[0.0, 1.0, 0.0]; 3])?;
//! particles.widths(&[0.1, 0.2, 0.3])?;
//! particles.attribute("Cd", Attribute::Float3(&[[1.0, 0.0, 0.0]; 3]))?;
//! particles.attribute("uv", Attribute::Float2(&[[0.0, 0.0]; 3]))?;
//! particles.attribute("temperature", Attribute::Float1(&[20.0, 21.0, 22.0]))?;
//! particles.finish()?;
//!
//! let file: Vec<u8> = archive.finish()?.into_inner();
//! # Ok(())
//! # }
//! ```
//!
//! [`ArchiveWriter::create_file`] writes to a file instead.
//!
//! # Reading
//!
//! [`read_archive`] reads only the archive's structure. Array values are read on demand with
//! [`ArchiveReader::read_into`], straight into buffers the caller provides.
//!
//! ```
//! use std::io::Cursor;
//!
//! use alembic_rust::{ArchiveWriter, AttributeValues, read_archive};
//!
//! # fn main() -> alembic_rust::Result<()> {
//! # let mut archive = ArchiveWriter::create(Cursor::new(Vec::new()))?;
//! # archive.metadata_mut().set("_ai_Application", "my app")?;
//! # archive.begin_points("particles", &[[1.0, 2.0, 3.0]])?.finish()?;
//! # let file = archive.finish()?.into_inner();
//! let (contents, mut reader) = read_archive(Cursor::new(file))?;
//! assert_eq!(contents.metadata.get("_ai_Application"), Some("my app"));
//!
//! for points in &contents.points {
//!     let mut positions = vec![[0.0f32; 3]; points.positions.len()];
//!     reader.read_into(&points.positions, &mut positions)?;
//!
//!     for attribute in &points.attributes {
//!         if let AttributeValues::Float1(values) = &attribute.values {
//!             let mut buffer = vec![0.0f32; values.len()];
//!             reader.read_into(values, &mut buffer)?;
//!         }
//!     }
//! }
//! for blob in &contents.user_data {
//!     let mut bytes = vec![0u8; blob.bytes.len()];
//!     reader.read_into(&blob.bytes, &mut bytes)?;
//! }
//! # Ok(())
//! # }
//! ```

#[cfg(target_endian = "big")]
compile_error!("Ogawa archives can only be written on little-endian targets");

mod error;
mod format;
mod read;
mod write;

#[cfg(test)]
mod round_trip_tests;

pub use error::{Error, Result};
pub use format::metadata::MetaData;
pub use read::{
    ArchiveContents, ArchiveReader, ArrayHandle, AttributeValues, PointAttribute, Points, UserData,
    read_archive,
};
pub use write::{ArchiveWriter, Attribute, PointsWriter};
