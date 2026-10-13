use std::io::{Read, Seek};
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::format::metadata::MetaData;
use crate::format::ogawa::Child;
use crate::format::schema::USER_DATA_BYTE;
use crate::read::ogawa::{DataSpan, OgawaReader};
use crate::read::points::{Points, points_among};
use crate::read::tree::{FirstSample, PropertyValue, walk_hierarchy};

/// The structure of an archive, as returned by [`read_archive`].
#[derive(Debug, Clone)]
pub struct ArchiveContents {
    /// The archive metadata.
    pub metadata: MetaData,
    /// The points objects, in file order.
    pub points: Vec<Points>,
    /// The user data blobs, in file order.
    pub user_data: Vec<UserData>,
}

/// A binary blob written with [`ArchiveWriter::user_data`](crate::ArchiveWriter::user_data).
#[derive(Debug, Clone)]
pub struct UserData {
    /// The blob's name.
    pub name: String,
    /// Where the blob's bytes are stored.
    pub bytes: ArrayHandle<u8>,
}

/// Where an array of `T` is stored in the archive.
///
/// Pass it to [`ArchiveReader::read_into`] to read the values.
#[derive(Debug, Clone, Copy)]
pub struct ArrayHandle<T> {
    values: DataSpan,
    len: usize,
    element: PhantomData<fn() -> T>,
}

impl<T> ArrayHandle<T> {
    /// The number of elements, which is the buffer length `read_into` expects.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the array has no elements.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn of_first_sample(sample: FirstSample) -> Self {
        ArrayHandle {
            values: sample.values,
            len: sample.len,
            element: PhantomData,
        }
    }
}

/// Reads array values from an archive opened with [`read_archive`].
pub struct ArchiveReader<R: Read + Seek> {
    ogawa: OgawaReader<R>,
}

impl<R: Read + Seek> ArchiveReader<R> {
    /// Reads the array behind `handle` into `destination`, without allocating.
    ///
    /// # Errors
    ///
    /// [`Error::BufferLengthMismatch`] if `destination` does not hold exactly
    /// [`handle.len()`](ArrayHandle::len) elements, and [`Error::Io`] if reading fails.
    pub fn read_into<T: bytemuck::Pod>(
        &mut self,
        handle: &ArrayHandle<T>,
        destination: &mut [T],
    ) -> Result<()> {
        if destination.len() != handle.len {
            return Err(Error::BufferLengthMismatch {
                expected: handle.len,
                actual: destination.len(),
            });
        }
        self.ogawa
            .read_into(handle.values, bytemuck::cast_slice_mut(destination))
    }
}

/// Reads the structure of an archive written by this library.
///
/// This reads names, metadata, bounds and where each array is stored, but no array values;
/// those are read on demand with the returned [`ArchiveReader`]. Objects without `P`,
/// properties of other types and anything else unrecognised are skipped. Only the first sample
/// of each property is read, and stored versions, hashes and time sampling are not checked.
///
/// # Errors
///
/// [`Error::NotOgawa`] if `source` is not an Ogawa file, [`Error::Incomplete`] if the archive
/// was never finished, [`Error::Corrupt`] if a record runs past the end of the file or the root
/// is not an Alembic root, and [`Error::Io`] if reading fails.
pub fn read_archive<R: Read + Seek>(source: R) -> Result<(ArchiveContents, ArchiveReader<R>)> {
    let (mut ogawa, root) = OgawaReader::open(source)?;
    let [
        Child::Data(_ogawa_file_version),
        Child::Data(_library_version),
        Child::Group(top_object),
        Child::Data(archive_metadata),
        Child::Data(_time_samplings),
        Child::Data(_metadata_dictionary),
        ..,
    ] = ogawa.read_group(root)?[..]
    else {
        return Err(Error::Corrupt {
            what: "root group is not an Alembic archive root",
        });
    };

    let span = ogawa.data_span(archive_metadata)?;
    let metadata = MetaData::deserialize(&String::from_utf8_lossy(&ogawa.read_bytes(span)?));
    let hierarchy = walk_hierarchy(&mut ogawa, top_object)?;
    let user_data = hierarchy
        .top_properties
        .into_iter()
        .filter_map(|property| match property.value {
            PropertyValue::Array(sample) if sample.data_type == USER_DATA_BYTE => Some(UserData {
                name: property.name,
                bytes: ArrayHandle::of_first_sample(sample),
            }),
            _ => None,
        })
        .collect();
    let points = points_among(hierarchy.objects, &mut ogawa)?;

    Ok((
        ArchiveContents {
            metadata,
            points,
            user_data,
        },
        ArchiveReader { ogawa },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::ArchiveWriter;
    use crate::write::ogawa::OgawaWriter;
    use std::io::Cursor;

    fn written_archive(build: impl FnOnce(&mut ArchiveWriter<Cursor<Vec<u8>>>)) -> Vec<u8> {
        let mut archive = ArchiveWriter::create(Cursor::new(Vec::new())).unwrap();
        build(&mut archive);
        archive.finish().unwrap().into_inner()
    }

    fn read(file: Vec<u8>) -> Result<(ArchiveContents, ArchiveReader<Cursor<Vec<u8>>>)> {
        read_archive(Cursor::new(file))
    }

    #[test]
    fn reads_archive_metadata() {
        let file = written_archive(|archive| {
            archive
                .metadata_mut()
                .set("_ai_Application", "tests")
                .unwrap();
        });
        let (contents, _) = read(file).unwrap();
        assert_eq!(contents.metadata.get("_ai_Application"), Some("tests"));
        assert!(
            contents
                .metadata
                .get("_ai_AlembicVersion")
                .unwrap()
                .starts_with("Alembic 1.8.12")
        );
        assert!(contents.points.is_empty());
        assert!(contents.user_data.is_empty());
    }

    #[test]
    fn reads_user_data_back() {
        let every_byte_value: Vec<u8> = (0..=255).collect();
        let file = written_archive(|archive| {
            archive.user_data("bytes", &every_byte_value).unwrap();
            archive.user_data("empty", &[]).unwrap();
            archive.user_data("one", &[42]).unwrap();
            archive
                .begin_points("p", &[[1.0; 3]])
                .unwrap()
                .finish()
                .unwrap();
        });
        let (contents, mut reader) = read(file).unwrap();
        let names: Vec<_> = contents.user_data.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["bytes", "empty", "one"]);
        assert_eq!(contents.points.len(), 1);

        let mut bytes = vec![0u8; contents.user_data[0].bytes.len()];
        reader
            .read_into(&contents.user_data[0].bytes, &mut bytes)
            .unwrap();
        assert_eq!(bytes, every_byte_value);
        assert!(contents.user_data[1].bytes.is_empty());
        reader
            .read_into(&contents.user_data[1].bytes, &mut [])
            .unwrap();
        let mut one = [0u8];
        reader
            .read_into(&contents.user_data[2].bytes, &mut one)
            .unwrap();
        assert_eq!(one, [42]);
    }

    #[test]
    fn rejects_unfinished_archives() {
        let mut buffer = Vec::new();
        {
            let mut archive = ArchiveWriter::create(Cursor::new(&mut buffer)).unwrap();
            archive.user_data("blob", &[1]).unwrap();
        }
        assert!(matches!(read(buffer), Err(Error::Incomplete)));
    }

    #[test]
    fn rejects_ogawa_files_without_the_alembic_layout() {
        let mut ogawa = OgawaWriter::new(Cursor::new(Vec::new())).unwrap();
        let root = ogawa.write_group(&[]).unwrap();
        let file = ogawa.finish(root).unwrap().into_inner();
        assert!(matches!(read(file), Err(Error::Corrupt { .. })));
    }
}
