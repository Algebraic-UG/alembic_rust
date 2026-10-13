use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("metadata {what} {text:?} contains a separator character (';' or '=')")]
    InvalidMetaData { what: &'static str, text: String },

    #[error("name {name:?} is empty or contains '/'")]
    InvalidName { name: String },

    #[error("name {name:?} is already used by a sibling")]
    DuplicateName { name: String },

    #[error("{what} is too long to be stored in a 32-bit length field")]
    TooLong { what: &'static str },

    #[error("{what} {name:?} has {actual} values but the points object has {expected} points")]
    PointCountMismatch {
        what: &'static str,
        name: String,
        expected: usize,
        actual: usize,
    },

    #[error("object {name:?} was dropped before it was finished")]
    UnfinishedObject { name: String },

    #[error("archive exceeds the maximum Ogawa file size")]
    FileTooLarge,

    #[error("archive is unusable because an earlier write failed")]
    Poisoned,
}

pub type Result<T> = std::result::Result<T, Error>;
