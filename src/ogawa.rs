use std::io::{BufWriter, Seek, SeekFrom, Write};

use byteorder::{LittleEndian, WriteBytesExt};

use crate::error::{Error, Result};

const MAGIC: &[u8; 5] = b"Ogawa";
const VERSION: [u8; 2] = [0, 1];
const FROZEN: u8 = 0xff;
const HEADER_LEN: u64 = 16;
const FROZEN_OFFSET: u64 = 5;
const ROOT_OFFSET: u64 = 8;

const DATA_BIT: u64 = 0x8000_0000_0000_0000;
const MAX_POS: u64 = 0x7fff_ffff_ffff_fffe;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GroupRef(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DataRef(u64);

impl GroupRef {
    pub(crate) const EMPTY: GroupRef = GroupRef(0);
}

impl DataRef {
    pub(crate) const EMPTY: DataRef = DataRef(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Child {
    Group(GroupRef),
    Data(DataRef),
}

impl Child {
    fn encode(self) -> u64 {
        match self {
            Child::Group(GroupRef(pos)) => pos,
            Child::Data(DataRef(pos)) => pos | DATA_BIT,
        }
    }
}

impl From<GroupRef> for Child {
    fn from(g: GroupRef) -> Self {
        Child::Group(g)
    }
}

impl From<DataRef> for Child {
    fn from(d: DataRef) -> Self {
        Child::Data(d)
    }
}

pub(crate) struct OgawaWriter<W: Write + Seek> {
    out: BufWriter<W>,
    pos: u64,
}

impl<W: Write + Seek> OgawaWriter<W> {
    pub(crate) fn new(out: W) -> Result<Self> {
        let mut out = BufWriter::new(out);
        out.write_all(MAGIC)?;
        out.write_u8(0)?;
        out.write_all(&VERSION)?;
        out.write_u64::<LittleEndian>(0)?;
        Ok(OgawaWriter {
            out,
            pos: HEADER_LEN,
        })
    }

    pub(crate) fn write_data(&mut self, parts: &[&[u8]]) -> Result<DataRef> {
        let size = parts
            .iter()
            .try_fold(0u64, |acc, p| acc.checked_add(p.len() as u64))
            .ok_or(Error::FileTooLarge)?;
        if size == 0 {
            return Ok(DataRef::EMPTY);
        }
        let at = self.reserve(8, size)?;
        self.out.write_u64::<LittleEndian>(size)?;
        for part in parts {
            self.out.write_all(part)?;
        }
        Ok(DataRef(at))
    }

    pub(crate) fn write_group(&mut self, children: &[Child]) -> Result<GroupRef> {
        if children.is_empty() {
            return Ok(GroupRef::EMPTY);
        }
        let len = children.len() as u64;
        let at = self.reserve(8, len.checked_mul(8).ok_or(Error::FileTooLarge)?)?;
        self.out.write_u64::<LittleEndian>(len)?;
        for child in children {
            self.out.write_u64::<LittleEndian>(child.encode())?;
        }
        Ok(GroupRef(at))
    }

    pub(crate) fn finish(mut self, root: GroupRef) -> Result<W> {
        self.out.seek(SeekFrom::Start(ROOT_OFFSET))?;
        self.out.write_u64::<LittleEndian>(root.0)?;
        self.out.flush()?;
        self.out.seek(SeekFrom::Start(FROZEN_OFFSET))?;
        self.out.write_u8(FROZEN)?;
        self.out.flush()?;
        self.out.into_inner().map_err(|e| Error::Io(e.into_error()))
    }

    fn reserve(&mut self, head: u64, body: u64) -> Result<u64> {
        let at = self.pos;
        self.pos = at
            .checked_add(head)
            .and_then(|p| p.checked_add(body))
            .filter(|&end| end <= MAX_POS)
            .ok_or(Error::FileTooLarge)?;
        Ok(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn finish(w: OgawaWriter<Cursor<Vec<u8>>>, root: GroupRef) -> Vec<u8> {
        w.finish(root).unwrap().into_inner()
    }

    #[test]
    fn header_and_records() {
        let mut w = OgawaWriter::new(Cursor::new(Vec::new())).unwrap();
        let d = w.write_data(&[b"ab", b"c"]).unwrap();
        let e = w.write_data(&[b"", b""]).unwrap();
        let root = w
            .write_group(&[d.into(), e.into(), GroupRef::EMPTY.into()])
            .unwrap();
        assert_eq!(d, DataRef(16));
        assert_eq!(e, DataRef::EMPTY);
        assert_eq!(root, GroupRef(16 + 8 + 3));

        let bytes = finish(w, root);
        let mut expected = Vec::new();
        expected.extend_from_slice(b"Ogawa\xff\x00\x01");
        expected.extend_from_slice(&27u64.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(b"abc");
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(&(16 | DATA_BIT).to_le_bytes());
        expected.extend_from_slice(&DATA_BIT.to_le_bytes());
        expected.extend_from_slice(&0u64.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn empty_group_writes_nothing() {
        let mut w = OgawaWriter::new(Cursor::new(Vec::new())).unwrap();
        assert_eq!(w.write_group(&[]).unwrap(), GroupRef::EMPTY);
        assert_eq!(w.pos, HEADER_LEN);
    }

    #[test]
    fn unfinished_stream_is_not_frozen() {
        let mut buf = Vec::new();
        {
            let mut w = OgawaWriter::new(Cursor::new(&mut buf)).unwrap();
            w.write_data(&[b"x"]).unwrap();
        }
        assert_eq!(&buf[..8], b"Ogawa\x00\x00\x01");
        assert_eq!(&buf[8..16], &[0; 8]);
    }
}
