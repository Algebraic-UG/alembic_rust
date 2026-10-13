use crate::format::data_type::{DataType, Pod};
use crate::format::layout::{
    COMPOUND_PROPERTY, EXTENT_SHIFT, FIELD_WIDTH_SHIFT, HIERARCHY_HASHES_LEN,
    INLINE_METADATA_INDEX, METADATA_INDEX_SHIFT, POD_SHIFT, PROPERTY_TYPE_MASK, SCALAR_PROPERTY,
};

const FIELD_WIDTH_MASK: u32 = 0b11;
const FOUR_BYTE_FIELD_WIDTH: u32 = 2;
const POD_MASK: u32 = 0xf;
const EXTENT_MASK: u32 = 0xff;
const METADATA_INDEX_MASK: u32 = 0xff;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PropertyHeader {
    pub(crate) name: String,
    pub(crate) kind: PropertyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertyKind {
    Compound,
    Scalar(DataType),
    Array(DataType),
    Unsupported,
}

pub(crate) fn decode_object_names(bytes: &[u8]) -> Vec<String> {
    let headers_len = bytes.len().saturating_sub(HIERARCHY_HASHES_LEN);
    let mut fields = Fields::new(&bytes[..headers_len]);
    std::iter::from_fn(|| decode_object_name(&mut fields)).collect()
}

pub(crate) fn decode_property_headers(bytes: &[u8]) -> Vec<PropertyHeader> {
    let mut fields = Fields::new(bytes);
    std::iter::from_fn(|| decode_property_header(&mut fields)).collect()
}

fn decode_object_name(fields: &mut Fields) -> Option<String> {
    let name_len = fields.sized(FOUR_BYTE_FIELD_WIDTH)?;
    let name = fields.text(name_len)?;
    let metadata_index = fields.u8()?;
    fields.skip_metadata(metadata_index, FOUR_BYTE_FIELD_WIDTH)?;
    Some(name)
}

fn decode_property_header(fields: &mut Fields) -> Option<PropertyHeader> {
    let info = fields.sized(FOUR_BYTE_FIELD_WIDTH)?;
    let width = (info >> FIELD_WIDTH_SHIFT) & FIELD_WIDTH_MASK;
    let kind = match info & PROPERTY_TYPE_MASK {
        COMPOUND_PROPERTY => PropertyKind::Compound,
        SCALAR_PROPERTY => decode_data_type(info, width, fields)?
            .map_or(PropertyKind::Unsupported, PropertyKind::Scalar),
        _ => decode_data_type(info, width, fields)?
            .map_or(PropertyKind::Unsupported, PropertyKind::Array),
    };
    let name_len = fields.sized(width)?;
    let name = fields.text(name_len)?;
    let metadata_index = ((info >> METADATA_INDEX_SHIFT) & METADATA_INDEX_MASK) as u8;
    fields.skip_metadata(metadata_index, width)?;
    Some(PropertyHeader { name, kind })
}

fn decode_data_type(info: u32, width: u32, fields: &mut Fields) -> Option<Option<DataType>> {
    let _sample_count = fields.sized(width)?;
    let pod = Pod::from_code(((info >> POD_SHIFT) & POD_MASK) as u8);
    let extent = ((info >> EXTENT_SHIFT) & EXTENT_MASK) as u8;
    Some(pod.map(|pod| DataType::new(pod, extent)))
}

struct Fields<'a> {
    remaining: &'a [u8],
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Fields { remaining: bytes }
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        if len > self.remaining.len() {
            return None;
        }
        let (taken, rest) = self.remaining.split_at(len);
        self.remaining = rest;
        Some(taken)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn sized(&mut self, width: u32) -> Option<u32> {
        let byte_count = 1usize << width.min(FOUR_BYTE_FIELD_WIDTH);
        let mut little_endian = [0u8; 4];
        little_endian[..byte_count].copy_from_slice(self.take(byte_count)?);
        Some(u32::from_le_bytes(little_endian))
    }

    fn text(&mut self, len: u32) -> Option<String> {
        Some(String::from_utf8_lossy(self.take(len as usize)?).into_owned())
    }

    fn skip_metadata(&mut self, metadata_index: u8, length_width: u32) -> Option<()> {
        if metadata_index == INLINE_METADATA_INDEX {
            let len = self.sized(length_width)?;
            self.take(len as usize)?;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::ObjectHierarchyHashes;
    use crate::headers::{PropertyLayout, encode_object_header, encode_property_header};
    use crate::metadata_dictionary::MetaDataDictionary;

    const FLOAT3: DataType = DataType::new(Pod::Float32, 3);

    #[test]
    fn reads_back_written_property_headers() {
        let long_name = "n".repeat(300);
        let long_metadata = format!("key={}", "v".repeat(70_000));
        let box3d = DataType::new(Pod::Float64, 6);
        let written = [
            (
                "P",
                "geoScope=var",
                PropertyLayout::Array {
                    data_type: FLOAT3,
                    num_points: 5,
                },
            ),
            (
                "one",
                "",
                PropertyLayout::Array {
                    data_type: FLOAT3,
                    num_points: 1,
                },
            ),
            (
                ".selfBnds",
                "interpretation=box",
                PropertyLayout::Scalar(box3d),
            ),
            (&long_name, &long_metadata, PropertyLayout::Compound),
        ];
        let mut encoded = Vec::new();
        let mut dictionary = MetaDataDictionary::default();
        for (name, metadata, layout) in written {
            encode_property_header(&mut encoded, name, metadata, &mut dictionary, layout).unwrap();
        }
        let expected = [
            ("P", PropertyKind::Array(FLOAT3)),
            ("one", PropertyKind::Array(FLOAT3)),
            (".selfBnds", PropertyKind::Scalar(box3d)),
            (long_name.as_str(), PropertyKind::Compound),
        ]
        .map(|(name, kind)| PropertyHeader {
            name: name.to_owned(),
            kind,
        });
        assert_eq!(decode_property_headers(&encoded), expected);
    }

    #[test]
    fn reads_object_names_past_indexed_and_inline_metadata() {
        let mut encoded = Vec::new();
        let mut dictionary = MetaDataDictionary::default();
        let names: Vec<String> = (0..300).map(|i| format!("object{i}")).collect();
        for (i, name) in names.iter().enumerate() {
            let distinct_metadata = format!("index={i}");
            encode_object_header(&mut encoded, name, &distinct_metadata, &mut dictionary).unwrap();
        }
        encode_object_header(&mut encoded, "long", &"x".repeat(300), &mut dictionary).unwrap();
        encoded.extend_from_slice(&ObjectHierarchyHashes::placeholder().encode());

        let mut expected = names;
        expected.push("long".to_owned());
        assert_eq!(decode_object_names(&encoded), expected);
    }

    #[test]
    fn object_headers_without_children_are_just_hashes() {
        let hashes = ObjectHierarchyHashes::placeholder().encode();
        assert!(decode_object_names(&hashes).is_empty());
    }
}
