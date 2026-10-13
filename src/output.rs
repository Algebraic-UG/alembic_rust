use std::io::{Seek, Write};

use crate::error::Result;
use crate::format::layout::SAMPLES_PER_PROPERTY;
use crate::format::ogawa::DataRef;
use crate::hash::placeholder_sample_digest;
use crate::metadata_dictionary::MetaDataDictionary;
use crate::ogawa::OgawaWriter;

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
}
