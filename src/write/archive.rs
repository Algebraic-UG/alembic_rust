use std::fs::File;
use std::io::{self, Seek, Write};
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
use crate::write::output::{ArchiveOutput, StreamedSample};

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
/// Once writing to the stream fails, or a [`user_data_with`](ArchiveWriter::user_data_with)
/// closure fails or panics, the writer is poisoned: every later write returns
/// [`Error::Poisoned`] and the archive is never marked complete.
pub struct ArchiveWriter<W: Write> {
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

impl<W: Write> ArchiveWriter<W> {
    pub(crate) fn abandon_child_object(&mut self, name: &str) {
        self.abandoned_object.get_or_insert_with(|| name.to_owned());
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
    /// To write a blob piece by piece instead of from one slice, use
    /// [`user_data_with`](ArchiveWriter::user_data_with).
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

    /// Stores a binary blob called `name` whose bytes `write` writes into a [`UserDataWriter`].
    ///
    /// [`UserDataWriter`] implements [`Write`], so serializers and [`io::copy`] can write into
    /// the archive without the whole blob in memory.
    ///
    /// The blob is added only if `write` returns `Ok`. Its error is returned unchanged in the
    /// inner result, this crate's errors in the outer one. If `write` fails or panics, the
    /// archive is poisoned.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidName`] or [`Error::DuplicateName`] if `name` is empty, contains `/` or
    /// is already used by another blob; `write` is then not called.
    pub fn user_data_with<E>(
        &mut self,
        name: &str,
        write: impl FnOnce(&mut UserDataWriter<'_, W>) -> std::result::Result<(), E>,
    ) -> Result<std::result::Result<(), E>> {
        self.output.ogawa.ensure_usable()?;
        self.top_object_properties.claim_name(name)?;
        let mut writer = UserDataWriter {
            output: &mut self.output,
            sample: StreamedSample::default(),
            completed: false,
        };
        if let Err(error) = write(&mut writer) {
            return Ok(Err(error));
        }
        let streamed = writer.complete();
        let byte_count = streamed.len();
        let sample = self.output.finish_sample(streamed)?;
        let Ok(len) = usize::try_from(byte_count) else {
            self.output.ogawa.poison();
            return Err(Error::FileTooLarge);
        };
        self.top_object_properties.add_claimed_array(
            &mut self.output,
            name,
            &MetaData::new(),
            USER_DATA_BYTE,
            sample,
            len,
        )?;
        Ok(Ok(()))
    }

    /// Completes the archive and returns the stream.
    ///
    /// The archive is marked complete at the very end: the root position is patched in and the
    /// header's frozen byte is written last. If writing stops before that, because of a crash,
    /// an error or dropping the writer, Alembic readers refuse to open the file.
    ///
    /// # Errors
    ///
    /// [`Error::Unfinished`] if a [`PointsWriter`](crate::PointsWriter) was dropped without being finished.
    /// The archive is then left incomplete.
    pub fn finish(mut self) -> Result<W> {
        if let Some(name) = self.abandoned_object.take() {
            return Err(Error::Unfinished { name });
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

/// Writes the bytes of a blob, inside [`ArchiveWriter::user_data_with`].
///
/// Every write goes straight to the archive's stream and fails once writing to the stream has
/// failed. [`flush`](Write::flush) does nothing: the archive is complete only after
/// [`ArchiveWriter::finish`].
pub struct UserDataWriter<'a, W: Write> {
    output: &'a mut ArchiveOutput<W>,
    sample: StreamedSample,
    completed: bool,
}

impl<W: Write + Seek> UserDataWriter<'_, W> {
    fn complete(mut self) -> StreamedSample {
        self.completed = true;
        std::mem::take(&mut self.sample)
    }
}

impl<W: Write> Drop for UserDataWriter<'_, W> {
    fn drop(&mut self) {
        if !self.completed {
            self.output.ogawa.poison();
        }
    }
}

impl<W: Write + Seek> Write for UserDataWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.output.append_to_sample(&mut self.sample, bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
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
    use crate::format::schema::POSITIONS;
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
    fn streamed_user_data_matches_user_data_written_at_once() {
        let blob: Vec<u8> = (0..=255).cycle().take(20_000).collect();
        let mut at_once = new_archive();
        at_once.user_data("blob", &blob).unwrap();
        at_once.user_data("empty", &[]).unwrap();
        let expected = at_once.finish().unwrap().into_inner();

        let mut streamed = new_archive();
        streamed
            .user_data_with("blob", |w| {
                blob.chunks(3000).try_for_each(|chunk| w.write_all(chunk))
            })
            .unwrap()
            .unwrap();
        streamed
            .user_data_with("empty", |_| Ok::<_, Error>(()))
            .unwrap()
            .unwrap();
        assert_eq!(streamed.finish().unwrap().into_inner(), expected);
    }

    #[test]
    fn closure_errors_come_back_unchanged_and_poison_the_archive() {
        let mut archive = new_archive();
        let failure = archive.user_data_with("blob", |w| {
            w.write_all(b"partial").unwrap();
            Err("unsupported value")
        });
        assert!(matches!(failure, Ok(Err("unsupported value"))));
        assert!(matches!(
            archive.user_data("next", &[1]),
            Err(Error::Poisoned)
        ));
        assert!(matches!(archive.finish(), Err(Error::Poisoned)));
    }

    #[test]
    fn panicking_closures_poison_the_archive() {
        let mut archive = new_archive();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            archive.user_data_with("blob", |w| -> io::Result<()> {
                w.write_all(b"partial")?;
                panic!("serializer panicked")
            })
        }));
        assert!(panicked.is_err());
        assert!(matches!(archive.finish(), Err(Error::Poisoned)));
    }

    #[test]
    fn user_data_with_checks_the_name_first() {
        let mut archive = new_archive();
        archive.user_data("blob", &[1]).unwrap();
        let mut called = false;
        let duplicate = archive.user_data_with("blob", |_| {
            called = true;
            Ok::<_, Error>(())
        });
        assert!(matches!(duplicate, Err(Error::DuplicateName { .. })));
        assert!(!called);
        archive.finish().unwrap();
    }

    struct FailingAfter {
        written: Cursor<Vec<u8>>,
        capacity: usize,
    }

    impl Write for FailingAfter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.written.get_ref().len() + bytes.len() > self.capacity {
                return Err(io::Error::other("disk full"));
            }
            self.written.write(bytes)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Seek for FailingAfter {
        fn seek(&mut self, position: std::io::SeekFrom) -> io::Result<u64> {
            self.written.seek(position)
        }
    }

    fn archive_failing_after_one_kibibyte() -> ArchiveWriter<FailingAfter> {
        let sink = FailingAfter {
            written: Cursor::new(Vec::new()),
            capacity: 1024,
        };
        ArchiveWriter::create(sink).unwrap()
    }

    #[test]
    fn failed_stream_writes_reach_the_closure_and_poison_the_archive() {
        let mut archive = archive_failing_after_one_kibibyte();
        let failure = archive
            .user_data_with("blob", |w| w.write_all(&[0; 64 * 1024]))
            .unwrap();
        assert!(matches!(failure, Err(e) if e.to_string() == "disk full"));
        assert!(matches!(archive.finish(), Err(Error::Poisoned)));
    }

    #[test]
    fn swallowed_stream_errors_still_fail_the_blob() {
        let mut archive = archive_failing_after_one_kibibyte();
        let swallowed = archive.user_data_with("blob", |w| {
            let _ignored = w.write_all(&[0; 64 * 1024]);
            Ok::<_, io::Error>(())
        });
        assert!(matches!(swallowed, Err(Error::Poisoned)));
    }

    #[test]
    fn user_data_names_must_be_unique() {
        let mut archive = new_archive();
        archive.user_data("blob", &[1]).unwrap();
        assert!(matches!(
            archive.user_data("blob", &[2]),
            Err(Error::DuplicateName { .. })
        ));
        let mut points = archive.begin_points("blob", 0).unwrap();
        points.attribute(POSITIONS, &[] as &[[f32; 3]]).unwrap();
        points.finish().unwrap();
        archive.finish().unwrap();
    }
}
