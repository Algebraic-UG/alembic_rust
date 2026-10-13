#[cfg(target_endian = "big")]
compile_error!("Ogawa archives can only be written on little-endian targets");

mod archive;
mod compound;
mod data_type;
mod error;
mod hash;
mod headers;
mod metadata;
mod names;
mod ogawa;
mod output;
mod points;
mod read;

#[cfg(test)]
mod structure_tests;

pub use archive::ArchiveWriter;
pub use error::{Error, Result};
pub use metadata::MetaData;
pub use points::{Attribute, PointsWriter};
pub use read::{
    ArchiveContents, ArchiveReader, ArrayHandle, AttributeValues, PointAttribute, Points, UserData,
    read_archive,
};
