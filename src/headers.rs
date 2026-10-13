use byteorder::{LittleEndian, WriteBytesExt};

use crate::error::{Error, Result};
use crate::format::data_type::DataType;
use crate::format::layout::{
    ARRAY_PROPERTY, COMPOUND_PROPERTY, CONSTANT_FLAG, EXTENT_SHIFT, FIELD_WIDTH_SHIFT,
    HOMOGENEOUS_FLAG, METADATA_INDEX_SHIFT, POD_SHIFT, SAMPLES_PER_PROPERTY,
    SCALAR_LIKE_ARRAY_PROPERTY, SCALAR_PROPERTY,
};
use crate::metadata_dictionary::{MetaDataDictionary, MetaDataIndex};

#[derive(Debug, Clone, Copy)]
pub(crate) enum PropertyLayout {
    Compound,
    Scalar(DataType),
    Array {
        data_type: DataType,
        num_points: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldWidth {
    OneByte = 0,
    TwoBytes = 1,
    FourBytes = 2,
}

impl FieldWidth {
    fn fitting(largest_value: u32) -> Self {
        match largest_value {
            0..=0xff => FieldWidth::OneByte,
            0x100..=0xffff => FieldWidth::TwoBytes,
            _ => FieldWidth::FourBytes,
        }
    }

    fn push(self, out: &mut Vec<u8>, value: u32) {
        let little_endian = value.to_le_bytes();
        let used_bytes = match self {
            FieldWidth::OneByte => 1,
            FieldWidth::TwoBytes => 2,
            FieldWidth::FourBytes => 4,
        };
        out.extend_from_slice(&little_endian[..used_bytes]);
    }
}

pub(crate) fn encode_property_header(
    out: &mut Vec<u8>,
    name: &str,
    serialized_metadata: &str,
    dictionary: &mut MetaDataDictionary,
    layout: PropertyLayout,
) -> Result<()> {
    let name_len = length_as_u32("property name", name.len())?;
    let metadata_len = length_as_u32("property metadata", serialized_metadata.len())?;
    let metadata_index = dictionary.index_of(serialized_metadata);

    let sample_count_in_header = match layout {
        PropertyLayout::Compound => 0,
        PropertyLayout::Scalar(_) | PropertyLayout::Array { .. } => SAMPLES_PER_PROPERTY,
    };
    let width = FieldWidth::fitting(name_len.max(metadata_len).max(sample_count_in_header));

    let mut info = (width as u32) << FIELD_WIDTH_SHIFT
        | u32::from(metadata_index.encoded()) << METADATA_INDEX_SHIFT;
    let simple_data_type = match layout {
        PropertyLayout::Compound => {
            info |= COMPOUND_PROPERTY;
            None
        }
        PropertyLayout::Scalar(data_type) => {
            info |= SCALAR_PROPERTY;
            Some(data_type)
        }
        PropertyLayout::Array {
            data_type,
            num_points,
        } => {
            info |= if num_points == 1 {
                SCALAR_LIKE_ARRAY_PROPERTY
            } else {
                ARRAY_PROPERTY
            };
            Some(data_type)
        }
    };
    if let Some(data_type) = simple_data_type {
        info |= u32::from(data_type.pod as u8) << POD_SHIFT
            | u32::from(data_type.extent) << EXTENT_SHIFT
            | CONSTANT_FLAG
            | HOMOGENEOUS_FLAG;
    }

    out.write_u32::<LittleEndian>(info)?;
    if simple_data_type.is_some() {
        width.push(out, SAMPLES_PER_PROPERTY);
    }
    width.push(out, name_len);
    out.extend_from_slice(name.as_bytes());
    if metadata_index == MetaDataIndex::Inline {
        width.push(out, metadata_len);
        out.extend_from_slice(serialized_metadata.as_bytes());
    }
    Ok(())
}

pub(crate) fn encode_object_header(
    out: &mut Vec<u8>,
    name: &str,
    serialized_metadata: &str,
    dictionary: &mut MetaDataDictionary,
) -> Result<()> {
    let name_len = length_as_u32("object name", name.len())?;
    let metadata_len = length_as_u32("object metadata", serialized_metadata.len())?;
    let metadata_index = dictionary.index_of(serialized_metadata);

    out.write_u32::<LittleEndian>(name_len)?;
    out.extend_from_slice(name.as_bytes());
    out.push(metadata_index.encoded());
    if metadata_index == MetaDataIndex::Inline {
        out.write_u32::<LittleEndian>(metadata_len)?;
        out.extend_from_slice(serialized_metadata.as_bytes());
    }
    Ok(())
}

fn length_as_u32(what: &'static str, len: usize) -> Result<u32> {
    u32::try_from(len).map_err(|_| Error::TooLong { what })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::data_type::Pod;

    const V3F: DataType = DataType::new(Pod::Float32, 3);

    fn encode(name: &str, metadata: &str, layout: PropertyLayout) -> Vec<u8> {
        let mut out = Vec::new();
        let mut dictionary = MetaDataDictionary::default();
        encode_property_header(&mut out, name, metadata, &mut dictionary, layout).unwrap();
        out
    }

    fn info_of(encoded: &[u8]) -> u32 {
        u32::from_le_bytes(encoded[..4].try_into().unwrap())
    }

    #[test]
    fn array_property_header() {
        let encoded = encode(
            "P",
            "geoScope=var",
            PropertyLayout::Array {
                data_type: V3F,
                num_points: 4,
            },
        );
        let expected_info = ARRAY_PROPERTY
            | (10 << POD_SHIFT)
            | HOMOGENEOUS_FLAG
            | CONSTANT_FLAG
            | (3 << EXTENT_SHIFT)
            | (1 << METADATA_INDEX_SHIFT);
        assert_eq!(info_of(&encoded), expected_info);
        assert_eq!(&encoded[4..], b"\x01\x01P");
    }

    #[test]
    fn single_point_array_is_scalar_like() {
        let encoded = encode(
            "P",
            "",
            PropertyLayout::Array {
                data_type: V3F,
                num_points: 1,
            },
        );
        assert_eq!(info_of(&encoded) & 0b11, SCALAR_LIKE_ARRAY_PROPERTY);
    }

    #[test]
    fn compound_property_header_has_no_sample_fields() {
        let encoded = encode(".geom", "", PropertyLayout::Compound);
        assert_eq!(info_of(&encoded), COMPOUND_PROPERTY);
        assert_eq!(&encoded[4..], b"\x05.geom");
    }

    #[test]
    fn long_inline_metadata_widens_fields() {
        let metadata = format!("k={}", "v".repeat(300));
        let encoded = encode("s", &metadata, PropertyLayout::Scalar(V3F));
        let info = info_of(&encoded);
        assert_eq!(info & 0b11, SCALAR_PROPERTY);
        assert_eq!(
            (info >> FIELD_WIDTH_SHIFT) & 0b11,
            FieldWidth::TwoBytes as u32
        );
        assert_eq!(info >> METADATA_INDEX_SHIFT & 0xff, 255);
        let mut expected_tail = vec![1, 0, 1, 0, b's'];
        expected_tail.extend_from_slice(&(metadata.len() as u16).to_le_bytes());
        expected_tail.extend_from_slice(metadata.as_bytes());
        assert_eq!(&encoded[4..], &expected_tail[..]);
    }

    #[test]
    fn field_width_boundaries() {
        assert_eq!(FieldWidth::fitting(255), FieldWidth::OneByte);
        assert_eq!(FieldWidth::fitting(256), FieldWidth::TwoBytes);
        assert_eq!(FieldWidth::fitting(65535), FieldWidth::TwoBytes);
        assert_eq!(FieldWidth::fitting(65536), FieldWidth::FourBytes);
    }

    #[test]
    fn object_header_with_dictionary_and_inline_metadata() {
        let mut dictionary = MetaDataDictionary::default();
        let mut out = Vec::new();
        encode_object_header(&mut out, "pts", "schema=X", &mut dictionary).unwrap();
        assert_eq!(out, b"\x03\x00\x00\x00pts\x01");

        let long = format!("k={}", "v".repeat(300));
        out.clear();
        encode_object_header(&mut out, "a", &long, &mut dictionary).unwrap();
        let mut expected = b"\x01\x00\x00\x00a\xff".to_vec();
        expected.extend_from_slice(&(long.len() as u32).to_le_bytes());
        expected.extend_from_slice(long.as_bytes());
        assert_eq!(out, expected);
    }
}
