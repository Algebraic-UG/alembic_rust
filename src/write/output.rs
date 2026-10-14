use std::io::{self, Seek, Write};

use crate::error::Result;
use crate::format::layout::SAMPLES_PER_PROPERTY;
use crate::format::ogawa::DataRef;
use crate::write::hash::{placeholder_sample_digest, placeholder_streamed_sample_digest};
use crate::write::metadata_dictionary::MetaDataDictionary;
use crate::write::ogawa::{OgawaWriter, StreamedRecord};

#[derive(Debug, Default)]
pub(crate) struct StreamedSample {
    record: Option<StreamedRecord>,
    len: u64,
}

impl StreamedSample {
    pub(crate) fn len(&self) -> u64 {
        self.len
    }
}

pub(crate) struct ArchiveOutput<W: Write + Seek> {
    pub(crate) ogawa: OgawaWriter<W>,
    pub(crate) metadata_dictionary: MetaDataDictionary,
    pub(crate) default_time_sampling_max_samples: u32,
}

impl<W: Write + Seek> ArchiveOutput<W> {
    pub(crate) fn new(ogawa: OgawaWriter<W>) -> Self {
        ArchiveOutput {
            ogawa,
            metadata_dictionary: MetaDataDictionary::default(),
            default_time_sampling_max_samples: 0,
        }
    }

    pub(crate) fn write_sample(&mut self, sample_bytes: &[u8]) -> Result<DataRef> {
        self.default_time_sampling_max_samples = SAMPLES_PER_PROPERTY;
        if sample_bytes.is_empty() {
            return Ok(DataRef::EMPTY);
        }
        let digest = placeholder_sample_digest(sample_bytes);
        self.ogawa.write_data(&[digest.as_bytes(), sample_bytes])
    }

    pub(crate) fn append_to_sample(
        &mut self,
        sample: &mut StreamedSample,
        bytes: &[u8],
    ) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let record = match &mut sample.record {
            Some(record) => record,
            None => {
                let digest = placeholder_streamed_sample_digest();
                sample
                    .record
                    .insert(self.ogawa.begin_streamed_data(digest.as_bytes())?)
            }
        };
        self.ogawa.append_to_streamed_data(record, bytes)?;
        sample.len = sample.len.saturating_add(bytes.len() as u64);
        Ok(())
    }

    pub(crate) fn finish_sample(&mut self, sample: StreamedSample) -> Result<DataRef> {
        self.default_time_sampling_max_samples = SAMPLES_PER_PROPERTY;
        match sample.record {
            Some(record) => self.ogawa.finish_streamed_data(record),
            None => Ok(DataRef::EMPTY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn new_output() -> ArchiveOutput<Cursor<Vec<u8>>> {
        ArchiveOutput::new(OgawaWriter::new(Cursor::new(Vec::new())).unwrap())
    }

    fn finished(output: ArchiveOutput<Cursor<Vec<u8>>>, sample: DataRef) -> Vec<u8> {
        let mut ogawa = output.ogawa;
        let root = ogawa.write_group(&[sample.into()]).unwrap();
        ogawa.finish(root).unwrap().into_inner()
    }

    #[test]
    fn streamed_sample_matches_sample_written_at_once() {
        let mut at_once = new_output();
        let sample = at_once.write_sample(b"abcdef").unwrap();
        let expected = finished(at_once, sample);

        let mut streamed = new_output();
        let mut sample = StreamedSample::default();
        for part in [&b""[..], b"abc", b"", b"def"] {
            streamed.append_to_sample(&mut sample, part).unwrap();
        }
        assert_eq!(sample.len(), 6);
        let sample = streamed.finish_sample(sample).unwrap();
        assert_eq!(finished(streamed, sample), expected);
    }

    #[test]
    fn streamed_sample_without_bytes_is_empty() {
        let mut output = new_output();
        let mut sample = StreamedSample::default();
        output.append_to_sample(&mut sample, b"").unwrap();
        assert_eq!(output.finish_sample(sample).unwrap(), DataRef::EMPTY);
        assert_eq!(
            finished(output, DataRef::EMPTY),
            finished(new_output(), DataRef::EMPTY)
        );
    }
}
