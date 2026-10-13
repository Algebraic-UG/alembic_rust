use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("metadata {what} {text:?} contains a separator character (';' or '=')")]
    InvalidMetaData { what: &'static str, text: String },

    #[error("archive exceeds the maximum Ogawa file size")]
    FileTooLarge,

    #[error("archive is unusable because an earlier write failed")]
    Poisoned,
}

pub type Result<T> = std::result::Result<T, Error>;
