use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::ogawa::{
    CHILD_REFERENCE_SIZE, Child, DataRef, FORMAT_VERSION, FORMAT_VERSION_OFFSET, FROZEN,
    FROZEN_BYTE_OFFSET, GroupRef, HEADER_LEN, MAGIC, RECORD_LENGTH_FIELD, ROOT_POSITION_OFFSET,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DataSpan {
    payload_start: u64,
    len: u64,
}

impl DataSpan {
    pub(crate) const EMPTY: DataSpan = DataSpan {
        payload_start: 0,
        len: 0,
    };

    pub(crate) fn len(self) -> u64 {
        self.len
    }

    pub(crate) fn after_prefix(self, prefix_len: u64) -> DataSpan {
        match self.len.checked_sub(prefix_len) {
            Some(len) if len > 0 => DataSpan {
                payload_start: self.payload_start + prefix_len,
                len,
            },
            _ => DataSpan::EMPTY,
        }
    }
}

pub(crate) struct OgawaReader<R: Read + Seek> {
    source: R,
    file_len: u64,
}

impl<R: Read + Seek> OgawaReader<R> {
    pub(crate) fn open(mut source: R) -> Result<(Self, GroupRef)> {
        let file_len = source.seek(SeekFrom::End(0))?;
        if file_len < HEADER_LEN {
            return Err(Error::NotOgawa);
        }
        let mut header = [0u8; HEADER_LEN as usize];
        source.seek(SeekFrom::Start(0))?;
        source.read_exact(&mut header)?;
        if &header[..MAGIC.len()] != MAGIC {
            return Err(Error::NotOgawa);
        }
        if header[FROZEN_BYTE_OFFSET as usize] != FROZEN {
            return Err(Error::Incomplete);
        }
        let version_at = FORMAT_VERSION_OFFSET as usize;
        let version = [header[version_at], header[version_at + 1]];
        if version != FORMAT_VERSION {
            return Err(Error::UnsupportedVersion {
                what: "Ogawa format",
                version: i64::from(u16::from_be_bytes(version)),
            });
        }
        let root_at = ROOT_POSITION_OFFSET as usize;
        let root_position = u64::from_le_bytes(header[root_at..root_at + 8].try_into().unwrap());
        Ok((OgawaReader { source, file_len }, GroupRef(root_position)))
    }

    pub(crate) fn file_len(&self) -> u64 {
        self.file_len
    }

    pub(crate) fn read_group(&mut self, group: GroupRef) -> Result<Vec<Child>> {
        if group == GroupRef::EMPTY {
            return Ok(Vec::new());
        }
        let child_count = self.read_record_length(group.0, "group position is out of bounds")?;
        let references_start = group.0 + RECORD_LENGTH_FIELD;
        if child_count > (self.file_len - references_start) / CHILD_REFERENCE_SIZE {
            return Err(Error::Corrupt {
                what: "group has more children than fit in the file",
            });
        }
        let mut references = vec![0u8; (child_count * CHILD_REFERENCE_SIZE) as usize];
        self.read_at(references_start, &mut references)?;
        let (encoded_references, _) = references.as_chunks::<{ CHILD_REFERENCE_SIZE as usize }>();
        Ok(encoded_references
            .iter()
            .map(|reference| Child::from_encoded_reference(u64::from_le_bytes(*reference)))
            .collect())
    }

    pub(crate) fn data_span(&mut self, data: DataRef) -> Result<DataSpan> {
        if data == DataRef::EMPTY {
            return Ok(DataSpan::EMPTY);
        }
        let len = self.read_record_length(data.0, "data position is out of bounds")?;
        let payload_start = data.0 + RECORD_LENGTH_FIELD;
        if len > self.file_len - payload_start {
            return Err(Error::Corrupt {
                what: "data record extends past the end of the file",
            });
        }
        Ok(DataSpan { payload_start, len })
    }

    pub(crate) fn read_bytes(&mut self, span: DataSpan) -> Result<Vec<u8>> {
        let mut bytes = vec![0u8; span.len as usize];
        self.read_into(span, 0, &mut bytes)?;
        Ok(bytes)
    }

    pub(crate) fn read_into(
        &mut self,
        span: DataSpan,
        offset_in_payload: u64,
        destination: &mut [u8],
    ) -> Result<()> {
        let fits = offset_in_payload
            .checked_add(destination.len() as u64)
            .is_some_and(|end| end <= span.len);
        if !fits {
            return Err(Error::Corrupt {
                what: "read extends past the end of a data record",
            });
        }
        if destination.is_empty() {
            return Ok(());
        }
        self.read_at(span.payload_start + offset_in_payload, destination)
    }

    fn read_record_length(
        &mut self,
        record_start: u64,
        out_of_bounds: &'static str,
    ) -> Result<u64> {
        let length_field_fits = record_start >= HEADER_LEN
            && record_start
                .checked_add(RECORD_LENGTH_FIELD)
                .is_some_and(|end| end <= self.file_len);
        if !length_field_fits {
            return Err(Error::Corrupt {
                what: out_of_bounds,
            });
        }
        let mut length = [0u8; RECORD_LENGTH_FIELD as usize];
        self.read_at(record_start, &mut length)?;
        Ok(u64::from_le_bytes(length))
    }

    fn read_at(&mut self, position: u64, destination: &mut [u8]) -> Result<()> {
        self.source.seek(SeekFrom::Start(position))?;
        self.source.read_exact(destination)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ogawa::OgawaWriter;
    use std::io::Cursor;

    fn written_file(build: impl FnOnce(&mut OgawaWriter<Cursor<Vec<u8>>>) -> GroupRef) -> Vec<u8> {
        let mut writer = OgawaWriter::new(Cursor::new(Vec::new())).unwrap();
        let root = build(&mut writer);
        writer.finish(root).unwrap().into_inner()
    }

    fn open(file: Vec<u8>) -> Result<(OgawaReader<Cursor<Vec<u8>>>, GroupRef)> {
        OgawaReader::open(Cursor::new(file))
    }

    fn corruption_of(result: Result<impl std::fmt::Debug>) -> &'static str {
        match result {
            Err(Error::Corrupt { what }) => what,
            other => panic!("expected corruption, got {other:?}"),
        }
    }

    fn file_with_root_children(references: &[u64]) -> Vec<u8> {
        let mut file = written_file(|_| GroupRef::EMPTY);
        let root_position = file.len() as u64;
        file.extend_from_slice(&(references.len() as u64).to_le_bytes());
        for reference in references {
            file.extend_from_slice(&reference.to_le_bytes());
        }
        file[8..16].copy_from_slice(&root_position.to_le_bytes());
        file
    }

    #[test]
    fn reads_back_written_groups_and_data() {
        let file = written_file(|w| {
            let data = w.write_data(&[b"ab", b"c"]).unwrap();
            let inner = w.write_group(&[data.into()]).unwrap();
            w.write_group(&[
                data.into(),
                DataRef::EMPTY.into(),
                inner.into(),
                GroupRef::EMPTY.into(),
            ])
            .unwrap()
        });
        let (mut reader, root) = open(file).unwrap();
        let children = reader.read_group(root).unwrap();
        let [
            Child::Data(data),
            Child::Data(empty_data),
            Child::Group(inner),
            Child::Group(empty_group),
        ] = children[..]
        else {
            panic!("unexpected children {children:?}");
        };

        let span = reader.data_span(data).unwrap();
        assert_eq!(reader.read_bytes(span).unwrap(), b"abc");
        assert_eq!(reader.data_span(empty_data).unwrap(), DataSpan::EMPTY);
        assert_eq!(reader.read_group(inner).unwrap(), vec![Child::Data(data)]);
        assert!(reader.read_group(empty_group).unwrap().is_empty());
    }

    #[test]
    fn reads_part_of_a_payload() {
        let file = written_file(|w| {
            let data = w.write_data(&[b"0123456789"]).unwrap();
            w.write_group(&[data.into()]).unwrap()
        });
        let (mut reader, root) = open(file).unwrap();
        let [Child::Data(data)] = reader.read_group(root).unwrap()[..] else {
            panic!("expected one data child");
        };
        let span = reader.data_span(data).unwrap();
        let mut middle = [0u8; 4];
        reader.read_into(span, 3, &mut middle).unwrap();
        assert_eq!(&middle, b"3456");
        reader.read_into(span, 10, &mut []).unwrap();
        corruption_of(reader.read_into(span, 7, &mut middle));
        corruption_of(reader.read_into(span, u64::MAX, &mut middle));
    }

    #[test]
    fn spans_after_a_prefix() {
        let span = DataSpan {
            payload_start: 100,
            len: 20,
        };
        assert_eq!(
            span.after_prefix(16),
            DataSpan {
                payload_start: 116,
                len: 4
            }
        );
        assert_eq!(span.after_prefix(20), DataSpan::EMPTY);
        assert_eq!(span.after_prefix(21), DataSpan::EMPTY);
        assert_eq!(DataSpan::EMPTY.after_prefix(16), DataSpan::EMPTY);
    }

    #[test]
    fn empty_archive_has_empty_root() {
        let (mut reader, root) = open(written_file(|_| GroupRef::EMPTY)).unwrap();
        assert_eq!(root, GroupRef::EMPTY);
        assert!(reader.read_group(root).unwrap().is_empty());
    }

    #[test]
    fn group_with_zero_children_is_empty() {
        let (mut reader, root) = open(file_with_root_children(&[])).unwrap();
        assert!(reader.read_group(root).unwrap().is_empty());
    }

    #[test]
    fn rejects_files_that_are_not_ogawa() {
        assert!(matches!(open(Vec::new()), Err(Error::NotOgawa)));
        assert!(matches!(open(b"Ogawa".to_vec()), Err(Error::NotOgawa)));
        let mut file = written_file(|_| GroupRef::EMPTY);
        file[0] = b'o';
        assert!(matches!(open(file), Err(Error::NotOgawa)));
    }

    #[test]
    fn rejects_unfinished_files() {
        let mut buffer = Vec::new();
        drop(OgawaWriter::new(Cursor::new(&mut buffer)).unwrap());
        assert!(matches!(open(buffer), Err(Error::Incomplete)));
    }

    #[test]
    fn rejects_unknown_format_versions() {
        let mut file = written_file(|_| GroupRef::EMPTY);
        file[6..8].copy_from_slice(&[0, 2]);
        assert!(matches!(
            open(file),
            Err(Error::UnsupportedVersion { version: 2, .. })
        ));
    }

    #[test]
    fn rejects_root_outside_the_file() {
        let mut file = written_file(|_| GroupRef::EMPTY);
        let past_the_end = file.len() as u64 - 4;
        file[8..16].copy_from_slice(&past_the_end.to_le_bytes());
        let (mut reader, root) = open(file).unwrap();
        corruption_of(reader.read_group(root));
    }

    #[test]
    fn rejects_group_inside_the_header() {
        let (mut reader, root) = open(file_with_root_children(&[4])).unwrap();
        let [Child::Group(inside_header)] = reader.read_group(root).unwrap()[..] else {
            panic!("expected one group child");
        };
        corruption_of(reader.read_group(inside_header));
    }

    #[test]
    fn rejects_child_counts_larger_than_the_file() {
        let mut file = file_with_root_children(&[0]);
        let root_position = u64::from_le_bytes(file[8..16].try_into().unwrap()) as usize;
        file[root_position..root_position + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let (mut reader, root) = open(file).unwrap();
        corruption_of(reader.read_group(root));
    }

    #[test]
    fn rejects_data_longer_than_the_file() {
        let mut file = written_file(|w| {
            let data = w.write_data(&[b"abc"]).unwrap();
            w.write_group(&[data.into()]).unwrap()
        });
        file[16..24].copy_from_slice(&1000u64.to_le_bytes());
        let (mut reader, root) = open(file).unwrap();
        let [Child::Data(data)] = reader.read_group(root).unwrap()[..] else {
            panic!("expected one data child");
        };
        corruption_of(reader.data_span(data));
    }

    #[test]
    fn rejects_data_positions_outside_the_file() {
        let data_bit = 1 << 63;
        let (mut reader, root) = open(file_with_root_children(&[
            data_bit | 3,
            data_bit | u64::MAX >> 1,
        ]))
        .unwrap();
        for child in reader.read_group(root).unwrap() {
            let Child::Data(data) = child else {
                panic!("expected data child");
            };
            corruption_of(reader.data_span(data));
        }
    }
}
