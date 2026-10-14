use std::io::{self, BufWriter, Seek, SeekFrom, Write};

use byteorder::{LittleEndian, WriteBytesExt};

use crate::error::{Error, Result};
use crate::format::ogawa::{
    CHILD_REFERENCE_SIZE, Child, DataRef, FORMAT_VERSION, FROZEN, FROZEN_BYTE_OFFSET, GroupRef,
    HEADER_LEN, INVALID_GROUP_POSITION, MAGIC, NOT_FROZEN, RECORD_LENGTH_FIELD,
    ROOT_POSITION_OFFSET,
};

const UNPATCHED_RECORD_LENGTH: u64 = 0;
const EARLIER_FAILURE: &str = "an earlier write to the archive failed";

#[derive(Debug)]
pub(crate) struct StreamedRecord {
    record_start: u64,
    payload_len: u64,
}

pub(crate) struct OgawaWriter<W: Write> {
    out: BufWriter<W>,
    position: u64,
    poisoned: bool,
}

impl<W: Write> OgawaWriter<W> {
    pub(crate) fn poison(&mut self) {
        self.poisoned = true;
    }
}

impl<W: Write + Seek> OgawaWriter<W> {
    pub(crate) fn new(out: W) -> Result<Self> {
        let mut writer = OgawaWriter {
            out: BufWriter::new(out),
            position: 0,
            poisoned: false,
        };
        writer.poison_on_error(|w| {
            w.out.write_all(MAGIC)?;
            w.out.write_u8(NOT_FROZEN)?;
            w.out.write_all(&FORMAT_VERSION)?;
            w.out.write_u64::<LittleEndian>(GroupRef::EMPTY.0)?;
            w.position = HEADER_LEN;
            Ok(())
        })?;
        Ok(writer)
    }

    pub(crate) fn write_data(&mut self, payload_parts: &[&[u8]]) -> Result<DataRef> {
        let payload_len = payload_parts
            .iter()
            .try_fold(0u64, |total, part| total.checked_add(part.len() as u64))
            .ok_or(Error::FileTooLarge)?;
        if payload_len == 0 {
            return Ok(DataRef::EMPTY);
        }
        self.poison_on_error(|w| {
            let record_start = w.claim_record_space(payload_len)?;
            w.out.write_u64::<LittleEndian>(payload_len)?;
            for part in payload_parts {
                w.out.write_all(part)?;
            }
            Ok(DataRef(record_start))
        })
    }

    pub(crate) fn ensure_usable(&self) -> Result<()> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        Ok(())
    }

    pub(crate) fn begin_streamed_data(&mut self, first_part: &[u8]) -> io::Result<StreamedRecord> {
        let mut record = StreamedRecord {
            record_start: self.position,
            payload_len: 0,
        };
        self.stream(&UNPATCHED_RECORD_LENGTH.to_le_bytes())?;
        self.append_to_streamed_data(&mut record, first_part)?;
        Ok(record)
    }

    pub(crate) fn append_to_streamed_data(
        &mut self,
        record: &mut StreamedRecord,
        part: &[u8],
    ) -> io::Result<()> {
        self.stream(part)?;
        record.payload_len = record.payload_len.saturating_add(part.len() as u64);
        Ok(())
    }

    pub(crate) fn finish_streamed_data(&mut self, record: StreamedRecord) -> Result<DataRef> {
        self.poison_on_error(|w| {
            if w.position >= INVALID_GROUP_POSITION {
                return Err(Error::FileTooLarge);
            }
            w.out.seek(SeekFrom::Start(record.record_start))?;
            w.out.write_u64::<LittleEndian>(record.payload_len)?;
            w.out.seek(SeekFrom::Start(w.position))?;
            Ok(DataRef(record.record_start))
        })
    }

    pub(crate) fn write_group(&mut self, children: &[Child]) -> Result<GroupRef> {
        if children.is_empty() {
            return Ok(GroupRef::EMPTY);
        }
        let child_count = children.len() as u64;
        let references_len = child_count
            .checked_mul(CHILD_REFERENCE_SIZE)
            .ok_or(Error::FileTooLarge)?;
        self.poison_on_error(|w| {
            let record_start = w.claim_record_space(references_len)?;
            w.out.write_u64::<LittleEndian>(child_count)?;
            for child in children {
                w.out.write_u64::<LittleEndian>(child.encoded_reference())?;
            }
            Ok(GroupRef(record_start))
        })
    }

    pub(crate) fn finish(mut self, root: GroupRef) -> Result<W> {
        self.poison_on_error(|w| {
            w.out.seek(SeekFrom::Start(ROOT_POSITION_OFFSET))?;
            w.out.write_u64::<LittleEndian>(root.0)?;
            w.out.flush()?;
            w.out.seek(SeekFrom::Start(FROZEN_BYTE_OFFSET))?;
            w.out.write_u8(FROZEN)?;
            w.out.flush()?;
            Ok(())
        })?;
        self.out.into_inner().map_err(|e| Error::Io(e.into_error()))
    }

    fn stream(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other(EARLIER_FAILURE));
        }
        if let Err(error) = self.out.write_all(bytes) {
            self.poisoned = true;
            return Err(error);
        }
        self.position = self.position.saturating_add(bytes.len() as u64);
        Ok(())
    }

    fn claim_record_space(&mut self, body_len: u64) -> Result<u64> {
        let record_start = self.position;
        self.position = record_start
            .checked_add(RECORD_LENGTH_FIELD)
            .and_then(|p| p.checked_add(body_len))
            .filter(|&record_end| record_end < INVALID_GROUP_POSITION)
            .ok_or(Error::FileTooLarge)?;
        Ok(record_start)
    }

    fn poison_on_error<T>(&mut self, write: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        let result = write(self);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::ogawa::DATA_REFERENCE_BIT;
    use std::io::Cursor;

    fn new_writer() -> OgawaWriter<Cursor<Vec<u8>>> {
        OgawaWriter::new(Cursor::new(Vec::new())).unwrap()
    }

    #[test]
    fn writes_header_records_and_root_position() {
        let mut w = new_writer();
        let data = w.write_data(&[b"ab", b"c"]).unwrap();
        let empty = w.write_data(&[b"", b""]).unwrap();
        let root = w
            .write_group(&[data.into(), empty.into(), GroupRef::EMPTY.into()])
            .unwrap();
        assert_eq!(data, DataRef(16));
        assert_eq!(empty, DataRef::EMPTY);
        assert_eq!(root, GroupRef(16 + 8 + 3));

        let bytes = w.finish(root).unwrap().into_inner();
        let mut expected = Vec::new();
        expected.extend_from_slice(b"Ogawa\xff\x00\x01");
        expected.extend_from_slice(&27u64.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(b"abc");
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(&(16 | DATA_REFERENCE_BIT).to_le_bytes());
        expected.extend_from_slice(&DATA_REFERENCE_BIT.to_le_bytes());
        expected.extend_from_slice(&0u64.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn streamed_data_matches_data_written_at_once() {
        let mut at_once = new_writer();
        let data = at_once.write_data(&[b"head", b"abcdef"]).unwrap();
        let group = at_once.write_group(&[data.into()]).unwrap();
        let expected = at_once.finish(group).unwrap().into_inner();

        let mut streamed = new_writer();
        let mut record = streamed.begin_streamed_data(b"head").unwrap();
        for part in [&b"ab"[..], b"", b"cde", b"f"] {
            streamed.append_to_streamed_data(&mut record, part).unwrap();
        }
        let data = streamed.finish_streamed_data(record).unwrap();
        let group = streamed.write_group(&[data.into()]).unwrap();
        assert_eq!(streamed.finish(group).unwrap().into_inner(), expected);
    }

    #[test]
    fn streamed_data_past_the_largest_position_is_too_large() {
        let mut w = new_writer();
        w.position = INVALID_GROUP_POSITION - 4;
        let mut record = w.begin_streamed_data(b"").unwrap();
        w.append_to_streamed_data(&mut record, b"x").unwrap();
        assert!(matches!(
            w.finish_streamed_data(record),
            Err(Error::FileTooLarge)
        ));
        assert!(matches!(w.ensure_usable(), Err(Error::Poisoned)));
    }

    #[test]
    fn empty_group_writes_nothing() {
        let mut w = new_writer();
        assert_eq!(w.write_group(&[]).unwrap(), GroupRef::EMPTY);
        assert_eq!(w.position, HEADER_LEN);
    }

    #[test]
    fn dropped_writer_leaves_stream_unfrozen() {
        let mut buf = Vec::new();
        {
            let mut w = OgawaWriter::new(Cursor::new(&mut buf)).unwrap();
            w.write_data(&[b"x"]).unwrap();
        }
        assert_eq!(&buf[..8], b"Ogawa\x00\x00\x01");
        assert_eq!(&buf[8..16], &[0; 8]);
    }

    struct FailingSink;

    impl Write for FailingSink {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("disk full"))
        }
    }

    impl Seek for FailingSink {
        fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
            Ok(0)
        }
    }

    #[test]
    fn failed_streamed_write_poisons_the_writer() {
        let mut w = OgawaWriter::new(FailingSink).unwrap();
        let big_part = vec![0u8; 64 * 1024];
        let failure = w.begin_streamed_data(&big_part).unwrap_err();
        assert_eq!(failure.to_string(), "disk full");
        let refused = w.begin_streamed_data(b"x").unwrap_err();
        assert_eq!(refused.to_string(), EARLIER_FAILURE);
        assert!(matches!(w.ensure_usable(), Err(Error::Poisoned)));
    }

    #[test]
    fn io_failure_poisons_the_writer() {
        let mut w = OgawaWriter::new(FailingSink).unwrap();
        let big_payload = vec![0u8; 64 * 1024];
        assert!(matches!(w.write_data(&[&big_payload]), Err(Error::Io(_))));
        assert!(matches!(w.write_data(&[b"x"]), Err(Error::Poisoned)));
        assert!(matches!(w.finish(GroupRef::EMPTY), Err(Error::Poisoned)));
    }
}
