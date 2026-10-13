#[cfg(target_endian = "big")]
compile_error!("Ogawa archives can only be written on little-endian targets");

mod error;
mod metadata;
mod ogawa;

pub use error::{Error, Result};
pub use metadata::MetaData;
