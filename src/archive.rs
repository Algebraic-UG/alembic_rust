use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;

use byteorder::{LittleEndian, WriteBytesExt};

use crate::error::{Error, Result};
use crate::metadata::{MetaData, MetaDataDictionary};
use crate::ogawa::{Child, DataRef, GroupRef, OgawaWriter};

const ABC_OGAWA_FILE_VERSION: i32 = 0;
const ABC_LIBRARY_VERSION: i32 = 10812;
const ALEMBIC_VERSION_KEY: &str = "_ai_AlembicVersion";

pub struct ArchiveWriter<W: Write + Seek> {
    ogawa: OgawaWriter<W>,
    abc_version: DataRef,
    library_version: DataRef,
    metadata: MetaData,
    metadata_map: MetaDataDictionary,
    poisoned: bool,
}

impl ArchiveWriter<File> {
    pub fn create_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::create(File::create(path)?)
    }
}

impl<W: Write + Seek> ArchiveWriter<W> {
    pub fn create(out: W) -> Result<Self> {
        let mut ogawa = OgawaWriter::new(out)?;
        let abc_version = ogawa.write_data(&[&ABC_OGAWA_FILE_VERSION.to_le_bytes()])?;
        let library_version = ogawa.write_data(&[&ABC_LIBRARY_VERSION.to_le_bytes()])?;
        Ok(ArchiveWriter {
            ogawa,
            abc_version,
            library_version,
            metadata: MetaData::new(),
            metadata_map: MetaDataDictionary::default(),
            poisoned: false,
        })
    }

    pub fn metadata_mut(&mut self) -> &mut MetaData {
        &mut self.metadata
    }

    pub fn finish(mut self) -> Result<W> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        self.metadata.set(
            ALEMBIC_VERSION_KEY,
            concat!(
                "Alembic 1.8.12 (written by alembic_rust ",
                env!("CARGO_PKG_VERSION"),
                ")"
            ),
        )?;

        let top = self.write_top_object()?;
        let metadata = self
            .ogawa
            .write_data(&[self.metadata.serialize().as_bytes()])?;
        let time_samplings = self.ogawa.write_data(&[&encode_default_time_sampling(0)])?;
        let indexed_metadata = self.ogawa.write_data(&[&self.metadata_map.encode()])?;
        let root = self.ogawa.write_group(&[
            self.abc_version.into(),
            self.library_version.into(),
            top.into(),
            metadata.into(),
            time_samplings.into(),
            indexed_metadata.into(),
        ])?;
        self.ogawa.finish(root)
    }

    fn write_top_object(&mut self) -> Result<GroupRef> {
        let properties = self.ogawa.write_group(&[])?;
        let hashes = [0u8; 32];
        let headers = self.ogawa.write_data(&[&hashes])?;
        self.ogawa
            .write_group(&[Child::Group(properties), Child::Data(headers)])
    }
}

fn encode_default_time_sampling(max_samples: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(24);
    out.write_u32::<LittleEndian>(max_samples).unwrap();
    out.write_f64::<LittleEndian>(1.0).unwrap();
    out.write_u32::<LittleEndian>(1).unwrap();
    out.write_f64::<LittleEndian>(0.0).unwrap();
    out
}
