use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::ogawa::{
    CHILD_REFERENCE_SIZE, Child, DataRef, FROZEN, FROZEN_BYTE_OFFSET, GroupRef, HEADER_LEN, MAGIC,
    RECORD_LENGTH_FIELD, ROOT_POSITION_OFFSET,
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
        let root_at = ROOT_POSITION_OFFSET as usize;
        let root_position = u64::from_le_bytes(header[root_at..root_at + 8].try_into().unwrap());
        Ok((OgawaReader { source, file_len }, GroupRef(root_position)))
    }

    pub(crate) fn read_group(&mut self, group: GroupRef) -> Result<Vec<Child>> {
        if group == GroupRef::EMPTY {
            return Ok(Vec::new());
        }
        let (references_start, references_len) = self.record_body(group.0, CHILD_REFERENCE_SIZE)?;
        let mut references = vec![0u8; references_len as usize];
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
        let (payload_start, len) = self.record_body(data.0, 1)?;
        Ok(DataSpan { payload_start, len })
    }

    pub(crate) fn read_bytes(&mut self, span: DataSpan) -> Result<Vec<u8>> {
        let mut bytes = vec![0u8; span.len as usize];
        self.read_into(span, &mut bytes)?;
        Ok(bytes)
    }

    pub(crate) fn read_into(&mut self, span: DataSpan, destination: &mut [u8]) -> Result<()> {
        if destination.is_empty() {
            return Ok(());
        }
        self.read_at(span.payload_start, destination)
    }

    fn record_body(
        &mut self,
        record_start: u64,
        bytes_per_counted_unit: u64,
    ) -> Result<(u64, u64)> {
        let mut length_field = [0u8; RECORD_LENGTH_FIELD as usize];
        self.read_at(record_start, &mut length_field)?;
        let body_start = record_start + RECORD_LENGTH_FIELD;
        let body_len = u64::from_le_bytes(length_field).checked_mul(bytes_per_counted_unit);
        let fits = body_len
            .and_then(|len| body_start.checked_add(len))
            .is_some_and(|body_end| body_end <= self.file_len);
        match body_len {
            Some(len) if fits => Ok((body_start, len)),
            _ => Err(Error::Corrupt {
                what: "record extends past the end of the file",
            }),
        }
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

    fn file_with_one_data_child() -> Vec<u8> {
        written_file(|w| {
            let data = w.write_data(&[b"abc"]).unwrap();
            w.write_group(&[data.into()]).unwrap()
        })
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
    fn rejects_files_that_are_not_ogawa() {
        assert!(matches!(open(Vec::new()), Err(Error::NotOgawa)));
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
    fn rejects_records_that_do_not_fit_in_the_file() {
        let mut truncated = file_with_one_data_child();
        truncated.pop();
        let (mut reader, root) = open(truncated).unwrap();
        assert!(matches!(
            reader.read_group(root),
            Err(Error::Corrupt { .. })
        ));

        let mut oversized = file_with_one_data_child();
        oversized[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        let (mut reader, root) = open(oversized).unwrap();
        let [Child::Data(data)] = reader.read_group(root).unwrap()[..] else {
            panic!("expected one data child");
        };
        assert!(matches!(reader.data_span(data), Err(Error::Corrupt { .. })));
    }
}
