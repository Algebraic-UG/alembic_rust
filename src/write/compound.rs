use std::io::{Seek, Write};

use crate::error::Result;
use crate::format::data_type::DataType;
use crate::format::metadata::MetaData;
use crate::format::ogawa::{Child, DataRef, GroupRef};
use crate::write::headers::{PropertyLayout, encode_property_header};
use crate::write::names::SiblingNames;
use crate::write::output::ArchiveOutput;

#[derive(Debug, Default)]
pub(crate) struct CompoundWriter {
    property_groups: Vec<GroupRef>,
    encoded_property_headers: Vec<u8>,
    names: SiblingNames,
}

impl CompoundWriter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.property_groups.is_empty()
    }

    pub(crate) fn add_array<W: Write + Seek>(
        &mut self,
        output: &mut ArchiveOutput<W>,
        name: &str,
        metadata: &MetaData,
        data_type: DataType,
        sample_bytes: &[u8],
    ) -> Result<()> {
        self.names.claim(name)?;
        let num_points = sample_bytes.len() / data_type.element_byte_width();
        let sample = output.write_sample(sample_bytes)?;
        let rank_one_dimensions_are_implied = DataRef::EMPTY;
        let group = output
            .ogawa
            .write_group(&[sample.into(), rank_one_dimensions_are_implied.into()])?;
        self.record(
            output,
            group,
            name,
            metadata,
            PropertyLayout::Array {
                data_type,
                num_points,
            },
        )
    }

    pub(crate) fn add_scalar<W: Write + Seek>(
        &mut self,
        output: &mut ArchiveOutput<W>,
        name: &str,
        metadata: &MetaData,
        data_type: DataType,
        value_bytes: &[u8],
    ) -> Result<()> {
        self.names.claim(name)?;
        let sample = output.write_sample(value_bytes)?;
        let group = output.ogawa.write_group(&[sample.into()])?;
        self.record(
            output,
            group,
            name,
            metadata,
            PropertyLayout::Scalar(data_type),
        )
    }

    pub(crate) fn add_compound<W: Write + Seek>(
        &mut self,
        output: &mut ArchiveOutput<W>,
        name: &str,
        metadata: &MetaData,
        compound: CompoundWriter,
    ) -> Result<()> {
        self.names.claim(name)?;
        let group = compound.finish(output)?;
        self.record(output, group, name, metadata, PropertyLayout::Compound)
    }

    pub(crate) fn finish<W: Write + Seek>(self, output: &mut ArchiveOutput<W>) -> Result<GroupRef> {
        let mut children: Vec<Child> = self.property_groups.into_iter().map(Child::from).collect();
        if !self.encoded_property_headers.is_empty() {
            let headers = output.ogawa.write_data(&[&self.encoded_property_headers])?;
            children.push(headers.into());
        }
        output.ogawa.write_group(&children)
    }

    fn record<W: Write + Seek>(
        &mut self,
        output: &mut ArchiveOutput<W>,
        group: GroupRef,
        name: &str,
        metadata: &MetaData,
        layout: PropertyLayout,
    ) -> Result<()> {
        encode_property_header(
            &mut self.encoded_property_headers,
            name,
            &metadata.serialize(),
            &mut output.metadata_dictionary,
            layout,
        )?;
        self.property_groups.push(group);
        Ok(())
    }
}
