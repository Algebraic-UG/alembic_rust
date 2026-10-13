use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;

use byteorder::{LittleEndian, WriteBytesExt};

use crate::error::{Error, Result};
use crate::format::metadata::MetaData;
use crate::format::ogawa::{Child, DataRef, GroupRef};
use crate::format::schema::USER_DATA_BYTE;
use crate::write::compound::CompoundWriter;
use crate::write::hash::ObjectHierarchyHashes;
use crate::write::headers::encode_object_header;
use crate::write::names::SiblingNames;
use crate::write::ogawa::OgawaWriter;
use crate::write::output::ArchiveOutput;

const ALEMBIC_OGAWA_FILE_VERSION: i32 = 0;
const ALEMBIC_LIBRARY_VERSION_1_8_12: i32 = 10812;
const ALEMBIC_VERSION_KEY: &str = "_ai_AlembicVersion";
const ALEMBIC_VERSION_VALUE: &str = concat!(
    "Alembic 1.8.12 (written by alembic_rust ",
    env!("CARGO_PKG_VERSION"),
    ")"
);

/// Writes an Alembic Ogawa archive of points objects to a stream.
///
/// Data is written to the stream as soon as it is passed in; the writer keeps no copies of
/// positions, attributes or user data. Only names, metadata, record positions and headers stay
/// in memory until [`finish`](ArchiveWriter::finish).
///
/// Once writing to the stream fails, the writer is poisoned: every later write returns
/// [`Error::Poisoned`] and the archive is never marked complete.
pub struct ArchiveWriter<W: Write + Seek> {
    output: ArchiveOutput<W>,
    ogawa_file_version: DataRef,
    library_version: DataRef,
    metadata: MetaData,
    top_object_properties: CompoundWriter,
    top_object_children: ChildObjects,
    abandoned_object: Option<String>,
}

#[derive(Default)]
struct ChildObjects {
    groups: Vec<GroupRef>,
    encoded_headers: Vec<u8>,
    names: SiblingNames,
}

impl ArchiveWriter<File> {
    /// Creates (or truncates) the file at `path` and starts an archive in it.
    pub fn create_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::create(File::create(path)?)
    }
}

impl<W: Write + Seek> ArchiveWriter<W> {
    /// Starts an archive in `out`, which can be any seekable stream, such as a
    /// [`File`] or a [`Cursor`](std::io::Cursor).
    pub fn create(out: W) -> Result<Self> {
        let mut ogawa = OgawaWriter::new(out)?;
        let ogawa_file_version = ogawa.write_data(&[&ALEMBIC_OGAWA_FILE_VERSION.to_le_bytes()])?;
        let library_version = ogawa.write_data(&[&ALEMBIC_LIBRARY_VERSION_1_8_12.to_le_bytes()])?;
        Ok(ArchiveWriter {
            output: ArchiveOutput::new(ogawa),
            ogawa_file_version,
            library_version,
            metadata: MetaData::new(),
            top_object_properties: CompoundWriter::new(),
            top_object_children: ChildObjects::default(),
            abandoned_object: None,
        })
    }

    /// The archive metadata, for keys like `_ai_Application`, `_ai_DateWritten` or
    /// `_ai_Description`.
    ///
    /// It is written by [`finish`](ArchiveWriter::finish), which also sets
    /// `_ai_AlembicVersion`.
    pub fn metadata_mut(&mut self) -> &mut MetaData {
        &mut self.metadata
    }

    /// Stores `bytes` as a binary blob called `name`.
    ///
    /// The blob is an ordinary `uint8_t` array property on the archive's top object, so any
    /// Alembic reader can get it back; in C++ that is
    /// `IArrayProperty(archive.getTop().getProperties(), name)`. Blob names must be unique among
    /// blobs but may match object names.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidName`] or [`Error::DuplicateName`] if `name` is empty, contains `/` or
    /// is already used by another blob.
    pub fn user_data(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.top_object_properties.add_array(
            &mut self.output,
            name,
            &MetaData::new(),
            USER_DATA_BYTE,
            bytes,
        )
    }

    /// Completes the archive and returns the stream.
    ///
    /// The archive is marked complete at the very end: the root position is patched in and the
    /// header's frozen byte is written last. If writing stops before that, because of a crash,
    /// an error or dropping the writer, Alembic readers refuse to open the file.
    ///
    /// # Errors
    ///
    /// [`Error::UnfinishedObject`] if a [`PointsWriter`](crate::PointsWriter) was dropped without being finished.
    /// The archive is then left incomplete.
    pub fn finish(mut self) -> Result<W> {
        if let Some(name) = self.abandoned_object.take() {
            return Err(Error::UnfinishedObject { name });
        }
        self.metadata
            .set(ALEMBIC_VERSION_KEY, ALEMBIC_VERSION_VALUE)?;
        let top_object = self.write_top_object()?;
        let ogawa = &mut self.output.ogawa;
        let metadata = ogawa.write_data(&[self.metadata.serialize().as_bytes()])?;
        let time_samplings = ogawa.write_data(&[&encode_time_samplings(
            self.output.default_time_sampling_max_samples,
        )])?;
        let metadata_dictionary = ogawa.write_data(&[&self.output.metadata_dictionary.encode()])?;
        let root = ogawa.write_group(&[
            self.ogawa_file_version.into(),
            self.library_version.into(),
            top_object.into(),
            metadata.into(),
            time_samplings.into(),
            metadata_dictionary.into(),
        ])?;
        self.output.ogawa.finish(root)
    }

    pub(crate) fn output(&mut self) -> &mut ArchiveOutput<W> {
        &mut self.output
    }

    pub(crate) fn claim_child_name(&mut self, name: &str) -> Result<()> {
        self.top_object_children.names.claim(name)
    }

    pub(crate) fn abandon_child_object(&mut self, name: &str) {
        self.abandoned_object.get_or_insert_with(|| name.to_owned());
    }

    pub(crate) fn add_claimed_child_object(
        &mut self,
        name: &str,
        metadata: &MetaData,
        properties: CompoundWriter,
    ) -> Result<()> {
        let children = &mut self.top_object_children;
        let properties = properties.finish(&mut self.output)?;
        let headers_of_no_children = ObjectHierarchyHashes::placeholder().encode();
        let child_headers = self.output.ogawa.write_data(&[&headers_of_no_children])?;
        let group = self
            .output
            .ogawa
            .write_group(&[properties.into(), child_headers.into()])?;
        encode_object_header(
            &mut children.encoded_headers,
            name,
            &metadata.serialize(),
            &mut self.output.metadata_dictionary,
        )?;
        children.groups.push(group);
        Ok(())
    }

    fn write_top_object(&mut self) -> Result<GroupRef> {
        let properties =
            std::mem::take(&mut self.top_object_properties).finish(&mut self.output)?;
        let ogawa = &mut self.output.ogawa;
        let child_headers = ogawa.write_data(&[
            &self.top_object_children.encoded_headers,
            &ObjectHierarchyHashes::placeholder().encode(),
        ])?;
        let mut children = vec![Child::from(properties)];
        children.extend(
            self.top_object_children
                .groups
                .iter()
                .copied()
                .map(Child::from),
        );
        children.push(child_headers.into());
        ogawa.write_group(&children)
    }
}

fn encode_time_samplings(default_time_sampling_max_samples: u32) -> Vec<u8> {
    let time_per_cycle = 1.0;
    let stored_times = [0.0];
    let mut encoded = Vec::new();
    encoded
        .write_u32::<LittleEndian>(default_time_sampling_max_samples)
        .unwrap();
    encoded.write_f64::<LittleEndian>(time_per_cycle).unwrap();
    encoded
        .write_u32::<LittleEndian>(stored_times.len() as u32)
        .unwrap();
    for time in stored_times {
        encoded.write_f64::<LittleEndian>(time).unwrap();
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn new_archive() -> ArchiveWriter<Cursor<Vec<u8>>> {
        ArchiveWriter::create(Cursor::new(Vec::new())).unwrap()
    }

    #[test]
    fn user_data_is_written_verbatim() {
        let blob: Vec<u8> = (0..=255).collect();
        let mut archive = new_archive();
        archive.user_data("blob", &blob).unwrap();
        let file = archive.finish().unwrap().into_inner();
        assert!(file.windows(blob.len()).any(|w| w == blob));
    }

    #[test]
    fn user_data_names_must_be_unique() {
        let mut archive = new_archive();
        archive.user_data("blob", &[1]).unwrap();
        assert!(matches!(
            archive.user_data("blob", &[2]),
            Err(Error::DuplicateName { .. })
        ));
        archive.begin_points("blob", &[]).unwrap().finish().unwrap();
        archive.finish().unwrap();
    }
}
