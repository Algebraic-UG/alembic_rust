#[cfg(target_endian = "big")]
compile_error!("Ogawa archives can only be written on little-endian targets");

mod error;
mod format;
mod read;
mod write;

pub use error::{Error, Result};
pub use format::metadata::MetaData;
pub use read::{
    ArchiveContents, ArchiveReader, ArrayHandle, AttributeValues, PointAttribute, Points, UserData,
    read_archive,
};
pub use write::{ArchiveWriter, Attribute, PointsWriter};
