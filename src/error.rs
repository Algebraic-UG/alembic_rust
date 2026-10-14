use std::io;

/// Errors of the writer and the reader.
///
/// Besides the errors listed on each function, any call that writes can fail with
/// [`Io`](Error::Io), [`Poisoned`](Error::Poisoned), [`FileTooLarge`](Error::FileTooLarge) or
/// [`TooLong`](Error::TooLong).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reading or writing the stream failed.
    ///
    /// After a failed write the writer is unusable, and later writes return
    /// [`Error::Poisoned`].
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// A metadata key or value contains `;` or `=`.
    #[error("metadata {what} {text:?} contains a separator character (';' or '=')")]
    InvalidMetaData {
        /// Whether the key or the value is invalid.
        what: &'static str,
        /// The invalid key or value.
        text: String,
    },

    /// A name is empty or contains `/`.
    #[error("name {name:?} is empty or contains '/'")]
    InvalidName {
        /// The invalid name.
        name: String,
    },

    /// A name is already used by a sibling: another object, another blob, another attribute,
    /// or velocities or widths written twice.
    #[error("name {name:?} is already used by a sibling")]
    DuplicateName {
        /// The name that is already used.
        name: String,
    },

    /// A name or metadata string is longer than Ogawa can store.
    #[error("{what} is too long to be stored in a 32-bit length field")]
    TooLong {
        /// What is too long.
        what: &'static str,
    },

    /// Velocities, widths or an attribute do not have one value per point. Nothing was
    /// written, and the points writer is still usable.
    #[error("{what} {name:?} has {actual} values but the points object has {expected} points")]
    PointCountMismatch {
        /// Whether velocities, widths or an attribute were rejected.
        what: &'static str,
        /// The name of the rejected property.
        name: String,
        /// The number of points.
        expected: usize,
        /// The number of values given.
        actual: usize,
    },

    /// A [`PointsWriter`](crate::PointsWriter) was dropped without being finished, so the
    /// archive cannot be completed.
    #[error("{name:?} was dropped before it was finished")]
    Unfinished {
        /// The name of the dropped points object.
        name: String,
    },

    /// The archive has grown past the largest position Ogawa can address.
    #[error("archive exceeds the maximum Ogawa file size")]
    FileTooLarge,

    /// An earlier write failed, or a
    /// [`user_data_with`](crate::ArchiveWriter::user_data_with) closure failed or panicked, so
    /// the archive can no longer be written or completed.
    #[error("archive is unusable after an earlier failure")]
    Poisoned,

    /// The file is not an Ogawa archive.
    #[error("not an Ogawa archive")]
    NotOgawa,

    /// The archive was never completed: its writer crashed, failed or was dropped before
    /// [`ArchiveWriter::finish`](crate::ArchiveWriter::finish).
    #[error("archive was never finished: its frozen flag is not set")]
    Incomplete,

    /// The archive is damaged: a record runs past the end of the file, or the root is not an
    /// Alembic root.
    #[error("corrupt archive: {what}")]
    Corrupt {
        /// What is damaged.
        what: &'static str,
    },

    /// The buffer passed to [`ArchiveReader::read_into`](crate::ArchiveReader::read_into)
    /// does not hold exactly [`ArrayHandle::len`](crate::ArrayHandle::len) elements.
    #[error("buffer holds {actual} elements but the array has {expected}")]
    BufferLengthMismatch {
        /// The number of elements in the array.
        expected: usize,
        /// The number of elements in the buffer.
        actual: usize,
    },
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;
