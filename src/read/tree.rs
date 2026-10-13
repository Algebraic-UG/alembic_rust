use std::io::{Read, Seek};

use crate::data_type::DataType;
use crate::error::{Error, Result};
use crate::hash::DIGEST_LEN;
use crate::metadata::MetaData;
use crate::ogawa::{Child, GroupRef};
use crate::read::headers::{
    IndexedMetaData, PropertyKind, Samples, decode_object_headers, decode_property_headers,
};
use crate::read::ogawa::{DataSpan, OgawaReader};

const DEEPEST_NESTING: usize = 512;
const SMALLEST_GROUP_RECORD: u64 = 16;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectNode {
    pub(crate) path: String,
    pub(crate) metadata: MetaData,
    pub(crate) properties: Vec<PropertyNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PropertyNode {
    pub(crate) name: String,
    pub(crate) metadata: MetaData,
    pub(crate) value: PropertyValue,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PropertyValue {
    Compound(Vec<PropertyNode>),
    Scalar(Option<FirstSample>),
    Array(Option<FirstSample>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FirstSample {
    pub(crate) data_type: DataType,
    pub(crate) values: DataSpan,
    pub(crate) len: usize,
}

pub(crate) struct Hierarchy {
    pub(crate) top_properties: Vec<PropertyNode>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) objects_depth_first: Vec<ObjectNode>,
}

pub(crate) fn walk_hierarchy<R: Read + Seek>(
    ogawa: &mut OgawaReader<R>,
    dictionary: &IndexedMetaData,
    top_object: GroupRef,
) -> Result<Hierarchy> {
    let mut walker = Walker {
        groups_left_to_visit: ogawa.file_len() / SMALLEST_GROUP_RECORD,
        ogawa,
        dictionary,
        objects_depth_first: Vec::new(),
    };
    let top_properties = walker.walk_object(top_object, "", 0)?;
    Ok(Hierarchy {
        top_properties,
        objects_depth_first: walker.objects_depth_first,
    })
}

struct Walker<'a, R: Read + Seek> {
    ogawa: &'a mut OgawaReader<R>,
    dictionary: &'a IndexedMetaData,
    groups_left_to_visit: u64,
    objects_depth_first: Vec<ObjectNode>,
}

impl<R: Read + Seek> Walker<'_, R> {
    fn walk_object(
        &mut self,
        group: GroupRef,
        path: &str,
        depth: usize,
    ) -> Result<Vec<PropertyNode>> {
        let children = self.visit_group(group, depth)?;
        let properties = match children.first() {
            Some(&Child::Group(properties)) => self.walk_compound(properties, depth + 1)?,
            _ => Vec::new(),
        };
        let headers = match children.last() {
            Some(&Child::Data(headers)) => {
                let span = self.ogawa.data_span(headers)?;
                decode_object_headers(&self.ogawa.read_bytes(span)?, self.dictionary)?
            }
            _ => Vec::new(),
        };
        for (index, header) in headers.into_iter().enumerate() {
            let Some(&Child::Group(child_group)) = children.get(index + 1) else {
                continue;
            };
            let child_path = format!("{path}/{}", header.name);
            let child_properties = self.walk_object(child_group, &child_path, depth + 1)?;
            self.objects_depth_first.push(ObjectNode {
                path: child_path,
                metadata: header.metadata,
                properties: child_properties,
            });
        }
        Ok(properties)
    }

    fn walk_compound(&mut self, group: GroupRef, depth: usize) -> Result<Vec<PropertyNode>> {
        let children = self.visit_group(group, depth)?;
        let headers = match children.last() {
            Some(&Child::Data(headers)) => {
                let span = self.ogawa.data_span(headers)?;
                decode_property_headers(&self.ogawa.read_bytes(span)?, self.dictionary)?
            }
            _ => Vec::new(),
        };
        let mut properties = Vec::with_capacity(headers.len());
        for (index, header) in headers.into_iter().enumerate() {
            let Some(&Child::Group(property_group)) = children.get(index) else {
                continue;
            };
            let value = match header.kind {
                PropertyKind::Compound => {
                    PropertyValue::Compound(self.walk_compound(property_group, depth + 1)?)
                }
                PropertyKind::Scalar(samples) => {
                    PropertyValue::Scalar(self.first_sample(property_group, samples, depth + 1)?)
                }
                PropertyKind::Array(samples) => {
                    PropertyValue::Array(self.first_sample(property_group, samples, depth + 1)?)
                }
            };
            properties.push(PropertyNode {
                name: header.name,
                metadata: header.metadata,
                value,
            });
        }
        Ok(properties)
    }

    fn first_sample(
        &mut self,
        group: GroupRef,
        samples: Samples,
        depth: usize,
    ) -> Result<Option<FirstSample>> {
        let Some(data_type) = samples.data_type else {
            return Ok(None);
        };
        if samples.count == 0 {
            return Ok(None);
        }
        let Some(&Child::Data(first)) = self.visit_group(group, depth)?.first() else {
            return Ok(None);
        };
        let values = self.ogawa.data_span(first)?.after_prefix(DIGEST_LEN as u64);
        let len = (values.len() / data_type.element_byte_width() as u64) as usize;
        Ok(Some(FirstSample {
            data_type,
            values,
            len,
        }))
    }

    fn visit_group(&mut self, group: GroupRef, depth: usize) -> Result<Vec<Child>> {
        if depth > DEEPEST_NESTING {
            return Err(Error::Corrupt {
                what: "hierarchy is nested too deeply",
            });
        }
        if group == GroupRef::EMPTY {
            return Ok(Vec::new());
        }
        self.groups_left_to_visit =
            self.groups_left_to_visit
                .checked_sub(1)
                .ok_or(Error::Corrupt {
                    what: "hierarchy visits more groups than fit in the file",
                })?;
        self.ogawa.read_group(group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::ArchiveWriter;
    use crate::data_type::Pod;
    use crate::ogawa::Child;
    use crate::points::Attribute;
    use std::io::Cursor;

    fn hierarchy_of(file: Vec<u8>) -> Hierarchy {
        let (mut ogawa, root) = OgawaReader::open(Cursor::new(file)).unwrap();
        let root_children = ogawa.read_group(root).unwrap();
        let (Child::Group(top_object), Child::Data(dictionary)) =
            (root_children[2], root_children[5])
        else {
            panic!("unexpected root {root_children:?}");
        };
        let span = ogawa.data_span(dictionary).unwrap();
        let dictionary = IndexedMetaData::decode(&ogawa.read_bytes(span).unwrap()).unwrap();
        walk_hierarchy(&mut ogawa, &dictionary, top_object).unwrap()
    }

    fn property<'a>(properties: &'a [PropertyNode], name: &str) -> &'a PropertyNode {
        properties
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("no property {name}"))
    }

    #[test]
    fn walks_objects_and_their_properties() {
        let mut archive = ArchiveWriter::create(Cursor::new(Vec::new())).unwrap();
        let mut a = archive.begin_points("a", &[[0.0; 3]; 4]).unwrap();
        a.attribute("t", Attribute::Float1(&[1.0; 4])).unwrap();
        a.finish().unwrap();
        archive.begin_points("b", &[]).unwrap().finish().unwrap();
        let hierarchy = hierarchy_of(archive.finish().unwrap().into_inner());

        assert!(hierarchy.top_properties.is_empty());
        let paths: Vec<_> = hierarchy
            .objects_depth_first
            .iter()
            .map(|o| o.path.as_str())
            .collect();
        assert_eq!(paths, ["/a", "/b"]);
        let a = &hierarchy.objects_depth_first[0];
        assert_eq!(a.metadata.get("schema"), Some("AbcGeom_Points_v1"));

        let PropertyValue::Compound(geom) = &property(&a.properties, ".geom").value else {
            panic!(".geom is not a compound");
        };
        let PropertyValue::Array(Some(positions)) = property(geom, "P").value else {
            panic!("P is not a readable array");
        };
        assert_eq!(positions.data_type, DataType::new(Pod::Float32, 3));
        assert_eq!(positions.len, 4);
        let PropertyValue::Scalar(Some(bounds)) = property(geom, ".selfBnds").value else {
            panic!(".selfBnds is not a readable scalar");
        };
        assert_eq!(bounds.len, 1);
        let PropertyValue::Array(Some(ids)) = property(geom, ".pointIds").value else {
            panic!(".pointIds is not a readable array");
        };
        assert_eq!((ids.len, ids.values), (0, DataSpan::EMPTY));
        let PropertyValue::Compound(arb) = &property(geom, ".arbGeomParams").value else {
            panic!(".arbGeomParams is not a compound");
        };
        assert_eq!(arb.len(), 1);

        let b = &hierarchy.objects_depth_first[1];
        let PropertyValue::Compound(geom) = &property(&b.properties, ".geom").value else {
            panic!(".geom is not a compound");
        };
        let PropertyValue::Array(Some(positions)) = property(geom, "P").value else {
            panic!("P is not a readable array");
        };
        assert_eq!(positions.len, 0);
    }
}
