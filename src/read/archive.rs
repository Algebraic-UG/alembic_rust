use std::io::{Read, Seek};
use std::marker::PhantomData;

use crate::archive::{ALEMBIC_OGAWA_FILE_VERSION, USER_DATA_BYTE};
use crate::error::{Error, Result};
use crate::metadata::MetaData;
use crate::ogawa::{Child, DataRef};
use crate::read::headers::IndexedMetaData;
use crate::read::ogawa::{DataSpan, OgawaReader};
use crate::read::tree::{FirstSample, PropertyValue, walk_hierarchy};

const OLDEST_READABLE_LIBRARY_VERSION: i32 = 9999;

#[derive(Debug, Clone)]
pub struct ArchiveContents {
    pub metadata: MetaData,
    pub user_data: Vec<UserData>,
}

#[derive(Debug, Clone)]
pub struct UserData {
    pub name: String,
    pub bytes: ArrayHandle<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct ArrayHandle<T> {
    values: DataSpan,
    len: usize,
    element: PhantomData<fn() -> T>,
}

impl<T> ArrayHandle<T> {
    pub fn len(&self) -> usize {
        self.len
    }

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

pub struct ArchiveReader<R: Read + Seek> {
    ogawa: OgawaReader<R>,
}

impl<R: Read + Seek> ArchiveReader<R> {
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
            .read_into(handle.values, 0, bytemuck::cast_slice_mut(destination))
    }
}

pub fn read_archive<R: Read + Seek>(source: R) -> Result<(ArchiveContents, ArchiveReader<R>)> {
    let (mut ogawa, root) = OgawaReader::open(source)?;
    let [
        Child::Data(file_version),
        Child::Data(library_version),
        Child::Group(top_object),
        Child::Data(archive_metadata),
        Child::Data(_time_samplings),
        Child::Data(indexed_metadata),
        ..,
    ] = ogawa.read_group(root)?[..]
    else {
        return Err(Error::Corrupt {
            what: "root group does not have the Alembic layout",
        });
    };

    let file_version = read_version(&mut ogawa, file_version)?.unwrap_or(-1);
    if !(0..=ALEMBIC_OGAWA_FILE_VERSION).contains(&file_version) {
        return Err(Error::UnsupportedVersion {
            what: "Alembic Ogawa file",
            version: i64::from(file_version),
        });
    }
    let library_version = read_version(&mut ogawa, library_version)?.unwrap_or(0);
    if library_version < OLDEST_READABLE_LIBRARY_VERSION {
        return Err(Error::UnsupportedVersion {
            what: "Alembic library",
            version: i64::from(library_version),
        });
    }

    let span = ogawa.data_span(indexed_metadata)?;
    let dictionary = IndexedMetaData::decode(&ogawa.read_bytes(span)?)?;
    let span = ogawa.data_span(archive_metadata)?;
    let metadata = MetaData::deserialize(&String::from_utf8_lossy(&ogawa.read_bytes(span)?));
    let hierarchy = walk_hierarchy(&mut ogawa, &dictionary, top_object)?;

    let user_data = hierarchy
        .top_properties
        .into_iter()
        .filter_map(|property| match property.value {
            PropertyValue::Array(Some(sample)) if sample.data_type == USER_DATA_BYTE => {
                Some(UserData {
                    name: property.name,
                    bytes: ArrayHandle::of_first_sample(sample),
                })
            }
            _ => None,
        })
        .collect();

    Ok((
        ArchiveContents {
            metadata,
            user_data,
        },
        ArchiveReader { ogawa },
    ))
}

fn read_version<R: Read + Seek>(ogawa: &mut OgawaReader<R>, data: DataRef) -> Result<Option<i32>> {
    let span = ogawa.data_span(data)?;
    if span.len() != 4 {
        return Ok(None);
    }
    let mut little_endian = [0u8; 4];
    ogawa.read_into(span, 0, &mut little_endian)?;
    Ok(Some(i32::from_le_bytes(little_endian)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::ArchiveWriter;
    use crate::ogawa::{GroupRef, OgawaWriter};
    use std::io::Cursor;

    fn written_archive(build: impl FnOnce(&mut ArchiveWriter<Cursor<Vec<u8>>>)) -> Vec<u8> {
        let mut archive = ArchiveWriter::create(Cursor::new(Vec::new())).unwrap();
        build(&mut archive);
        archive.finish().unwrap().into_inner()
    }

    fn read(file: Vec<u8>) -> Result<(ArchiveContents, ArchiveReader<Cursor<Vec<u8>>>)> {
        read_archive(Cursor::new(file))
    }

    fn corruption_of<T>(result: Result<T>) -> &'static str {
        match result {
            Err(Error::Corrupt { what }) => what,
            Err(other) => panic!("expected corruption, got {other:?}"),
            Ok(_) => panic!("expected corruption, got success"),
        }
    }

    fn ogawa_file(build: impl FnOnce(&mut OgawaWriter<Cursor<Vec<u8>>>) -> Vec<Child>) -> Vec<u8> {
        let mut ogawa = OgawaWriter::new(Cursor::new(Vec::new())).unwrap();
        let root_children = build(&mut ogawa);
        let root = ogawa.write_group(&root_children).unwrap();
        ogawa.finish(root).unwrap().into_inner()
    }

    fn root_with_versions(
        ogawa: &mut OgawaWriter<Cursor<Vec<u8>>>,
        file_version: &[u8],
        library_version: &[u8],
    ) -> Vec<Child> {
        vec![
            ogawa.write_data(&[file_version]).unwrap().into(),
            ogawa.write_data(&[library_version]).unwrap().into(),
            GroupRef::EMPTY.into(),
            DataRef::EMPTY.into(),
            DataRef::EMPTY.into(),
            DataRef::EMPTY.into(),
        ]
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
    fn rejects_buffers_of_the_wrong_length() {
        let file = written_archive(|archive| archive.user_data("blob", &[1, 2, 3]).unwrap());
        let (contents, mut reader) = read(file).unwrap();
        let mismatch = reader.read_into(&contents.user_data[0].bytes, &mut [0u8; 2]);
        assert!(matches!(
            mismatch,
            Err(Error::BufferLengthMismatch {
                expected: 3,
                actual: 2
            })
        ));
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
        corruption_of(read(ogawa_file(|_| Vec::new())));
        corruption_of(read(ogawa_file(|ogawa| {
            let mut children =
                root_with_versions(ogawa, &0i32.to_le_bytes(), &10812i32.to_le_bytes());
            children[2] = DataRef::EMPTY.into();
            children
        })));
    }

    #[test]
    fn accepts_extra_root_children() {
        let file = ogawa_file(|ogawa| {
            let mut children =
                root_with_versions(ogawa, &0i32.to_le_bytes(), &10812i32.to_le_bytes());
            children.push(ogawa.write_data(&[b"extra"]).unwrap().into());
            children
        });
        let (contents, _) = read(file).unwrap();
        assert_eq!(contents.metadata, MetaData::new());
    }

    #[test]
    fn rejects_unsupported_versions() {
        let current_library = 10812i32.to_le_bytes();
        for (file_version, library_version, expected_what, expected_version) in [
            (
                &1i32.to_le_bytes()[..],
                &current_library[..],
                "Alembic Ogawa file",
                1,
            ),
            (
                &[0u8; 2][..],
                &current_library[..],
                "Alembic Ogawa file",
                -1,
            ),
            (
                &0i32.to_le_bytes()[..],
                &9998i32.to_le_bytes()[..],
                "Alembic library",
                9998,
            ),
            (&0i32.to_le_bytes()[..], &[1u8][..], "Alembic library", 0),
        ] {
            let file = ogawa_file(|ogawa| root_with_versions(ogawa, file_version, library_version));
            match read(file) {
                Err(Error::UnsupportedVersion { what, version }) => {
                    assert_eq!((what, version), (expected_what, expected_version));
                }
                Err(other) => panic!("expected unsupported version, got {other:?}"),
                Ok(_) => panic!("expected unsupported version, got success"),
            }
        }
    }

    #[test]
    fn rejects_hierarchies_that_loop() {
        let mut file = ogawa_file(|ogawa| {
            let mut dictionary = crate::metadata::MetaDataDictionary::default();
            let mut headers = Vec::new();
            crate::headers::encode_object_header(&mut headers, "loop", "", &mut dictionary)
                .unwrap();
            headers.extend_from_slice(&crate::hash::ObjectHierarchyHashes::placeholder().encode());
            let headers = ogawa.write_data(&[&headers]).unwrap();
            let mut children =
                root_with_versions(ogawa, &0i32.to_le_bytes(), &10812i32.to_le_bytes());
            children.push(headers.into());
            children
        });
        let root_position = u64::from_le_bytes(file[8..16].try_into().unwrap()) as usize;
        let headers_reference = &file[root_position + 8 + 6 * 8..root_position + 8 + 7 * 8];
        let headers_reference = u64::from_le_bytes(headers_reference.try_into().unwrap());
        let looping_top_object = file.len() as u64;
        for reference in [3, 0, looping_top_object, headers_reference] {
            file.extend_from_slice(&reference.to_le_bytes());
        }
        let top_object_slot = root_position + 8 + 2 * 8;
        file[top_object_slot..top_object_slot + 8]
            .copy_from_slice(&looping_top_object.to_le_bytes());
        let what = corruption_of(read(file));
        assert!(
            what.contains("nested too deeply") || what.contains("more groups"),
            "{what}"
        );
    }
}
