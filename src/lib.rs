#[cfg(target_endian = "big")]
compile_error!("Ogawa archives can only be written on little-endian targets");

mod archive;
mod data_type;
mod error;
mod hash;
mod metadata;
mod names;
mod ogawa;

pub use archive::ArchiveWriter;
pub use error::{Error, Result};
pub use metadata::MetaData;
