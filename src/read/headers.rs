use crate::data_type::{DataType, Pod};
use crate::error::{Error, Result};
use crate::hash::ObjectHierarchyHashes;
use crate::headers::{
    ARRAY_PROPERTY, CHANGED_SAMPLE_RANGE_FLAG, COMPOUND_PROPERTY, EXTENT_SHIFT, FIELD_WIDTH_SHIFT,
    METADATA_INDEX_SHIFT, POD_SHIFT, PROPERTY_TYPE_MASK, SCALAR_LIKE_ARRAY_PROPERTY,
    SCALAR_PROPERTY, TIME_SAMPLING_INDEX_FLAG,
};
use crate::metadata::{INLINE_INDEX, MetaData};

const LARGEST_INDEXED_METADATA: usize = 65536;
const FIELD_WIDTH_MASK: u32 = 0b11;
const POD_MASK: u32 = 0xf;
const EXTENT_MASK: u32 = 0xff;
const METADATA_INDEX_MASK: u32 = 0xff;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectHeader {
    pub(crate) name: String,
    pub(crate) metadata: MetaData,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PropertyHeader {
    pub(crate) name: String,
    pub(crate) metadata: MetaData,
    pub(crate) kind: PropertyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertyKind {
    Compound,
    Scalar(Samples),
    Array(Samples),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Samples {
    pub(crate) data_type: Option<DataType>,
    pub(crate) count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IndexedMetaData {
    entries_by_index: Vec<MetaData>,
}

impl IndexedMetaData {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > LARGEST_INDEXED_METADATA {
            return Err(corrupt("indexed metadata is unexpectedly large"));
        }
        let mut entries_by_index = vec![MetaData::new()];
        let mut fields = Fields::new(bytes);
        while !fields.is_exhausted() {
            let len = fields.u8("indexed metadata length")?;
            let entry = fields.text(u32::from(len), "indexed metadata entry")?;
            entries_by_index.push(MetaData::deserialize(&entry));
        }
        Ok(IndexedMetaData { entries_by_index })
    }

    fn resolve(
        &self,
        index: u8,
        fields: &mut Fields,
        read_inline_len: InlineLen,
    ) -> Result<MetaData> {
        if index == INLINE_INDEX {
            let len = match read_inline_len {
                InlineLen::FourBytes => fields.u32("inline metadata length")?,
                InlineLen::Hinted(width) => fields.hinted(width, "inline metadata length")?,
            };
            return Ok(MetaData::deserialize(&fields.text(len, "inline metadata")?));
        }
        self.entries_by_index
            .get(usize::from(index))
            .cloned()
            .ok_or_else(|| corrupt("metadata index is not in the metadata dictionary"))
    }
}

#[derive(Clone, Copy)]
enum InlineLen {
    FourBytes,
    Hinted(u32),
}

pub(crate) fn decode_object_headers(
    bytes: &[u8],
    dictionary: &IndexedMetaData,
) -> Result<Vec<ObjectHeader>> {
    let Some(headers_len) = bytes.len().checked_sub(ObjectHierarchyHashes::ENCODED_LEN) else {
        return Ok(Vec::new());
    };
    let mut fields = Fields::new(&bytes[..headers_len]);
    let mut headers = Vec::new();
    while !fields.is_exhausted() {
        let name_len = fields.u32("object name length")?;
        let name = fields.name(name_len, "object name")?;
        let metadata_index = fields.u8("object metadata index")?;
        let metadata = dictionary.resolve(metadata_index, &mut fields, InlineLen::FourBytes)?;
        headers.push(ObjectHeader { name, metadata });
    }
    Ok(headers)
}

pub(crate) fn decode_property_headers(
    bytes: &[u8],
    dictionary: &IndexedMetaData,
) -> Result<Vec<PropertyHeader>> {
    let mut fields = Fields::new(bytes);
    let mut headers = Vec::new();
    while !fields.is_exhausted() {
        let info = fields.u32("property info")?;
        let width = (info >> FIELD_WIDTH_SHIFT) & FIELD_WIDTH_MASK;
        let kind = match info & PROPERTY_TYPE_MASK {
            COMPOUND_PROPERTY => PropertyKind::Compound,
            SCALAR_PROPERTY => PropertyKind::Scalar(decode_samples(info, width, &mut fields)?),
            ARRAY_PROPERTY | SCALAR_LIKE_ARRAY_PROPERTY => {
                PropertyKind::Array(decode_samples(info, width, &mut fields)?)
            }
            _ => unreachable!("property type is two bits"),
        };
        let name_len = fields.hinted(width, "property name length")?;
        let name = fields.name(name_len, "property name")?;
        let metadata_index = ((info >> METADATA_INDEX_SHIFT) & METADATA_INDEX_MASK) as u8;
        let metadata = dictionary.resolve(metadata_index, &mut fields, InlineLen::Hinted(width))?;
        headers.push(PropertyHeader {
            name,
            metadata,
            kind,
        });
    }
    Ok(headers)
}

fn decode_samples(info: u32, width: u32, fields: &mut Fields) -> Result<Samples> {
    let pod_code = ((info >> POD_SHIFT) & POD_MASK) as u8;
    let extent = ((info >> EXTENT_SHIFT) & EXTENT_MASK) as u8;
    let data_type = Pod::from_code(pod_code)
        .filter(|_| extent > 0)
        .map(|pod| DataType::new(pod, extent));
    let count = fields.hinted(width, "property sample count")?;
    if info & CHANGED_SAMPLE_RANGE_FLAG != 0 {
        fields.hinted(width, "first changed sample")?;
        fields.hinted(width, "last changed sample")?;
    }
    if info & TIME_SAMPLING_INDEX_FLAG != 0 {
        fields.hinted(width, "time sampling index")?;
    }
    Ok(Samples { data_type, count })
}

struct Fields<'a> {
    remaining: &'a [u8],
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Fields { remaining: bytes }
    }

    fn is_exhausted(&self) -> bool {
        self.remaining.is_empty()
    }

    fn take(&mut self, len: usize, what: &'static str) -> Result<&'a [u8]> {
        if len > self.remaining.len() {
            return Err(Error::Corrupt { what });
        }
        let (taken, rest) = self.remaining.split_at(len);
        self.remaining = rest;
        Ok(taken)
    }

    fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.take(1, what)?[0])
    }

    fn u32(&mut self, what: &'static str) -> Result<u32> {
        self.hinted(2, what)
    }

    fn hinted(&mut self, width: u32, what: &'static str) -> Result<u32> {
        let byte_count = match width {
            0 => 1,
            1 => 2,
            2 => 4,
            _ => return Err(corrupt("property header has an invalid field width")),
        };
        let mut little_endian = [0u8; 4];
        little_endian[..byte_count].copy_from_slice(self.take(byte_count, what)?);
        Ok(u32::from_le_bytes(little_endian))
    }

    fn text(&mut self, len: u32, what: &'static str) -> Result<String> {
        let bytes = self.take(len as usize, what)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn name(&mut self, len: u32, what: &'static str) -> Result<String> {
        if len == 0 {
            return Err(corrupt("name is empty"));
        }
        self.text(len, what)
    }
}

fn corrupt(what: &'static str) -> Error {
    Error::Corrupt { what }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headers::{PropertyLayout, encode_object_header, encode_property_header};
    use crate::metadata::MetaDataDictionary;

    const FLOAT3: DataType = DataType::new(Pod::Float32, 3);

    fn metadata(serialized: &str) -> MetaData {
        MetaData::deserialize(serialized)
    }

    fn written_property_headers(
        headers: &[(&str, &str, PropertyLayout)],
    ) -> (Vec<u8>, IndexedMetaData) {
        let mut dictionary = MetaDataDictionary::default();
        let mut encoded = Vec::new();
        for (name, serialized_metadata, layout) in headers {
            encode_property_header(
                &mut encoded,
                name,
                serialized_metadata,
                &mut dictionary,
                *layout,
            )
            .unwrap();
        }
        (
            encoded,
            IndexedMetaData::decode(&dictionary.encode()).unwrap(),
        )
    }

    fn corruption_of<T: std::fmt::Debug>(result: Result<T>) -> &'static str {
        match result {
            Err(Error::Corrupt { what }) => what,
            other => panic!("expected corruption, got {other:?}"),
        }
    }

    #[test]
    fn reads_back_written_property_headers() {
        let long_name = "n".repeat(300);
        let long_metadata = format!("key={}", "v".repeat(70_000));
        let (encoded, dictionary) = written_property_headers(&[
            (
                "P",
                "geoScope=var;interpretation=point",
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
                PropertyLayout::Scalar(DataType::new(Pod::Float64, 6)),
            ),
            (
                ".geom",
                "schema=AbcGeom_Points_v1",
                PropertyLayout::Compound,
            ),
            (&long_name, &long_metadata, PropertyLayout::Compound),
        ]);
        let headers = decode_property_headers(&encoded, &dictionary).unwrap();
        let one_sample_of = |data_type| Samples {
            data_type: Some(data_type),
            count: 1,
        };
        assert_eq!(
            headers,
            vec![
                PropertyHeader {
                    name: "P".into(),
                    metadata: metadata("geoScope=var;interpretation=point"),
                    kind: PropertyKind::Array(one_sample_of(FLOAT3)),
                },
                PropertyHeader {
                    name: "one".into(),
                    metadata: MetaData::new(),
                    kind: PropertyKind::Array(one_sample_of(FLOAT3)),
                },
                PropertyHeader {
                    name: ".selfBnds".into(),
                    metadata: metadata("interpretation=box"),
                    kind: PropertyKind::Scalar(one_sample_of(DataType::new(Pod::Float64, 6))),
                },
                PropertyHeader {
                    name: ".geom".into(),
                    metadata: metadata("schema=AbcGeom_Points_v1"),
                    kind: PropertyKind::Compound,
                },
                PropertyHeader {
                    name: long_name,
                    metadata: metadata(&long_metadata),
                    kind: PropertyKind::Compound,
                },
            ]
        );
    }

    #[test]
    fn reads_optional_sample_fields_written_by_upstream() {
        let info = ARRAY_PROPERTY
            | 1 << FIELD_WIDTH_SHIFT
            | 10 << POD_SHIFT
            | CHANGED_SAMPLE_RANGE_FLAG
            | TIME_SAMPLING_INDEX_FLAG
            | 3 << EXTENT_SHIFT;
        let mut encoded = info.to_le_bytes().to_vec();
        for two_byte_field in [24u16, 1, 23, 1, 1] {
            encoded.extend_from_slice(&two_byte_field.to_le_bytes());
        }
        encoded.push(b'P');
        let headers =
            decode_property_headers(&encoded, &IndexedMetaData::decode(&[]).unwrap()).unwrap();
        assert_eq!(
            headers[0].kind,
            PropertyKind::Array(Samples {
                data_type: Some(FLOAT3),
                count: 24,
            })
        );
        assert_eq!(headers[0].name, "P");
    }

    #[test]
    fn unknown_pods_and_zero_extents_have_no_data_type() {
        let string_pod = 12;
        for (pod, extent) in [(string_pod, 1), (10, 0)] {
            let info = SCALAR_PROPERTY | pod << POD_SHIFT | extent << EXTENT_SHIFT;
            let mut encoded = info.to_le_bytes().to_vec();
            encoded.extend_from_slice(&[1, 1, b's']);
            let headers =
                decode_property_headers(&encoded, &IndexedMetaData::decode(&[]).unwrap()).unwrap();
            assert_eq!(
                headers[0].kind,
                PropertyKind::Scalar(Samples {
                    data_type: None,
                    count: 1,
                })
            );
        }
    }

    #[test]
    fn rejects_malformed_property_headers() {
        let no_dictionary = IndexedMetaData::decode(&[]).unwrap();
        let compound_with_width =
            |width: u32| (COMPOUND_PROPERTY | width << FIELD_WIDTH_SHIFT).to_le_bytes();
        let mut invalid_width = compound_with_width(3).to_vec();
        invalid_width.extend_from_slice(&[1, b'x']);
        let mut empty_name = compound_with_width(0).to_vec();
        empty_name.push(0);
        let mut truncated_name = compound_with_width(0).to_vec();
        truncated_name.extend_from_slice(&[5, b'x']);
        let mut unknown_metadata_index = (COMPOUND_PROPERTY | 7 << METADATA_INDEX_SHIFT)
            .to_le_bytes()
            .to_vec();
        unknown_metadata_index.extend_from_slice(&[1, b'x']);
        for malformed in [
            &[1, 0][..],
            &invalid_width,
            &empty_name,
            &truncated_name,
            &unknown_metadata_index,
        ] {
            corruption_of(decode_property_headers(malformed, &no_dictionary));
        }
    }

    #[test]
    fn reads_back_written_object_headers() {
        let mut dictionary = MetaDataDictionary::default();
        let long_metadata = format!("schema={}", "s".repeat(300));
        let mut encoded = Vec::new();
        encode_object_header(
            &mut encoded,
            "particles",
            "schema=AbcGeom_Points_v1",
            &mut dictionary,
        )
        .unwrap();
        encode_object_header(&mut encoded, "inline", &long_metadata, &mut dictionary).unwrap();
        encode_object_header(&mut encoded, "plain", "", &mut dictionary).unwrap();
        encoded.extend_from_slice(&ObjectHierarchyHashes::placeholder().encode());
        let dictionary = IndexedMetaData::decode(&dictionary.encode()).unwrap();
        assert_eq!(
            decode_object_headers(&encoded, &dictionary).unwrap(),
            vec![
                ObjectHeader {
                    name: "particles".into(),
                    metadata: metadata("schema=AbcGeom_Points_v1"),
                },
                ObjectHeader {
                    name: "inline".into(),
                    metadata: metadata(&long_metadata),
                },
                ObjectHeader {
                    name: "plain".into(),
                    metadata: MetaData::new(),
                },
            ]
        );
    }

    #[test]
    fn object_headers_without_children_are_just_hashes() {
        let no_dictionary = IndexedMetaData::decode(&[]).unwrap();
        let hashes = ObjectHierarchyHashes::placeholder().encode();
        assert!(
            decode_object_headers(&hashes, &no_dictionary)
                .unwrap()
                .is_empty()
        );
        assert!(
            decode_object_headers(&[], &no_dictionary)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_malformed_object_headers() {
        let no_dictionary = IndexedMetaData::decode(&[]).unwrap();
        let with_hashes = |headers: &[u8]| {
            let mut bytes = headers.to_vec();
            bytes.extend_from_slice(&ObjectHierarchyHashes::placeholder().encode());
            bytes
        };
        for malformed in [
            with_hashes(&[1, 0]),
            with_hashes(&[0, 0, 0, 0, 0]),
            with_hashes(&[1, 0, 0, 0, b'x']),
            with_hashes(&[1, 0, 0, 0, b'x', 3]),
            with_hashes(&[1, 0, 0, 0, b'x', INLINE_INDEX, 9, 0, 0, 0]),
        ] {
            corruption_of(decode_object_headers(&malformed, &no_dictionary));
        }
    }

    #[test]
    fn decodes_indexed_metadata() {
        let dictionary = IndexedMetaData::decode(b"\x03a=1\x00\x03b=2").unwrap();
        assert_eq!(
            dictionary.entries_by_index,
            vec![
                MetaData::new(),
                metadata("a=1"),
                MetaData::new(),
                metadata("b=2"),
            ]
        );
        corruption_of(IndexedMetaData::decode(b"\x05a=1"));
        corruption_of(IndexedMetaData::decode(&vec![
            0;
            LARGEST_INDEXED_METADATA + 1
        ]));
    }
}
