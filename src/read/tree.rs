use std::io::{Read, Seek};

use crate::data_type::DataType;
use crate::error::Result;
use crate::hash::DIGEST_LEN;
use crate::ogawa::{Child, GroupRef};
use crate::read::headers::{PropertyKind, decode_object_names, decode_property_headers};
use crate::read::ogawa::{DataSpan, OgawaReader};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectNode {
    pub(crate) name: String,
    pub(crate) properties: Vec<PropertyNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PropertyNode {
    pub(crate) name: String,
    pub(crate) value: PropertyValue,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PropertyValue {
    Compound(Vec<PropertyNode>),
    Scalar(FirstSample),
    Array(FirstSample),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FirstSample {
    pub(crate) data_type: DataType,
    pub(crate) values: DataSpan,
    pub(crate) len: usize,
}

pub(crate) struct Hierarchy {
    pub(crate) top_properties: Vec<PropertyNode>,
    pub(crate) objects: Vec<ObjectNode>,
}

pub(crate) fn walk_hierarchy<R: Read + Seek>(
    ogawa: &mut OgawaReader<R>,
    top_object: GroupRef,
) -> Result<Hierarchy> {
    let children = ogawa.read_group(top_object)?;
    let top_properties = walk_properties_of(ogawa, &children)?;
    let names = decode_object_names(&read_last_data(ogawa, &children)?);
    let mut objects = Vec::with_capacity(names.len());
    for (index, name) in names.into_iter().enumerate() {
        let Some(&Child::Group(object)) = children.get(index + 1) else {
            continue;
        };
        let object_children = ogawa.read_group(object)?;
        let properties = walk_properties_of(ogawa, &object_children)?;
        objects.push(ObjectNode { name, properties });
    }
    Ok(Hierarchy {
        top_properties,
        objects,
    })
}

fn walk_properties_of<R: Read + Seek>(
    ogawa: &mut OgawaReader<R>,
    object_children: &[Child],
) -> Result<Vec<PropertyNode>> {
    match object_children.first() {
        Some(&Child::Group(properties)) => walk_compound(ogawa, properties),
        _ => Ok(Vec::new()),
    }
}

fn walk_compound<R: Read + Seek>(
    ogawa: &mut OgawaReader<R>,
    group: GroupRef,
) -> Result<Vec<PropertyNode>> {
    let children = ogawa.read_group(group)?;
    let headers = decode_property_headers(&read_last_data(ogawa, &children)?);
    let mut properties = Vec::with_capacity(headers.len());
    for (index, header) in headers.into_iter().enumerate() {
        let Some(&Child::Group(property_group)) = children.get(index) else {
            continue;
        };
        let value = match header.kind {
            PropertyKind::Compound => Some(PropertyValue::Compound(walk_compound(
                ogawa,
                property_group,
            )?)),
            PropertyKind::Scalar(data_type) => {
                first_sample(ogawa, property_group, data_type)?.map(PropertyValue::Scalar)
            }
            PropertyKind::Array(data_type) => {
                first_sample(ogawa, property_group, data_type)?.map(PropertyValue::Array)
            }
            PropertyKind::Unsupported => None,
        };
        if let Some(value) = value {
            properties.push(PropertyNode {
                name: header.name,
                value,
            });
        }
    }
    Ok(properties)
}

fn first_sample<R: Read + Seek>(
    ogawa: &mut OgawaReader<R>,
    group: GroupRef,
    data_type: DataType,
) -> Result<Option<FirstSample>> {
    let Some(&Child::Data(first)) = ogawa.read_group(group)?.first() else {
        return Ok(None);
    };
    let values = ogawa.data_span(first)?.after_prefix(DIGEST_LEN as u64);
    let len = values
        .len()
        .checked_div(data_type.element_byte_width() as u64);
    Ok(len.map(|len| FirstSample {
        data_type,
        values,
        len: len as usize,
    }))
}

fn read_last_data<R: Read + Seek>(
    ogawa: &mut OgawaReader<R>,
    children: &[Child],
) -> Result<Vec<u8>> {
    match children.last() {
        Some(&Child::Data(last)) => {
            let span = ogawa.data_span(last)?;
            ogawa.read_bytes(span)
        }
        _ => Ok(Vec::new()),
    }
}
