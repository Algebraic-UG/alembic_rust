use std::collections::BTreeMap;
use std::io::{Read, Seek};

use crate::error::{Error, Result};
use crate::format::attribute::{AttributeElement, AttributeType};
use crate::format::data_type::DataType;
use crate::format::schema::{
    ARB_GEOM_PARAMS_COMPOUND, BOX3D, FLOAT1, FLOAT3, GEOM_COMPOUND, POSITIONS, SELF_BOUNDS,
    VELOCITIES, WIDTHS,
};
use crate::read::archive::ArrayHandle;
use crate::read::ogawa::OgawaReader;
use crate::read::tree::{FirstSample, Properties, PropertyValue};

/// A points object read from an archive.
#[derive(Debug, Clone)]
pub struct Points {
    /// The bounding box `.selfBnds` as `[min_x, min_y, min_z, max_x, max_y, max_z]`.
    ///
    /// For an object without points, the minimum is [`f64::MAX`] and the maximum [`f64::MIN`].
    pub bounds: Option<[f64; 6]>,
    /// The positions `P`.
    pub positions: ArrayHandle<[f32; 3]>,
    /// The `.velocities`, if written.
    pub velocities: Option<ArrayHandle<[f32; 3]>>,
    /// The `.widths`, if written.
    pub widths: Option<ArrayHandle<f32>>,
    /// The attributes in `.arbGeomParams`, by name.
    pub attributes: BTreeMap<String, AttributeHandle>,
}

/// Where the values of a per-point attribute are stored.
#[derive(Debug, Clone, Copy)]
pub struct AttributeHandle {
    attribute_type: AttributeType,
    sample: FirstSample,
}

impl AttributeHandle {
    /// The type of the attribute's values.
    pub fn attribute_type(self) -> AttributeType {
        self.attribute_type
    }

    /// The values as an array of `T`, to read with
    /// [`ArchiveReader::read_into`](crate::ArchiveReader::read_into).
    ///
    /// # Errors
    ///
    /// [`Error::AttributeTypeMismatch`] if the attribute holds another type.
    pub fn typed<T: AttributeElement>(self) -> Result<ArrayHandle<T>> {
        if T::TYPE != self.attribute_type {
            return Err(Error::AttributeTypeMismatch {
                expected: T::TYPE,
                actual: self.attribute_type,
            });
        }
        Ok(ArrayHandle::of_first_sample(self.sample))
    }
}

pub(crate) fn points_among<R: Read + Seek>(
    objects: BTreeMap<String, Properties>,
    ogawa: &mut OgawaReader<R>,
) -> Result<BTreeMap<String, Points>> {
    let mut points = BTreeMap::new();
    for (name, properties) in objects {
        let Some(PropertyValue::Compound(geom)) = properties.get(GEOM_COMPOUND) else {
            continue;
        };
        let Some(positions) = array_of(geom, POSITIONS, FLOAT3) else {
            continue;
        };
        let object_points = Points {
            bounds: bounds_of(geom, ogawa)?,
            positions,
            velocities: array_of(geom, VELOCITIES, FLOAT3),
            widths: array_of(geom, WIDTHS, FLOAT1),
            attributes: attributes_of(geom),
        };
        points.insert(name, object_points);
    }
    Ok(points)
}

fn array_of<T>(properties: &Properties, name: &str, data_type: DataType) -> Option<ArrayHandle<T>> {
    match properties.get(name)? {
        PropertyValue::Array(sample) if sample.data_type == data_type => {
            Some(ArrayHandle::of_first_sample(*sample))
        }
        _ => None,
    }
}

fn bounds_of<R: Read + Seek>(
    geom: &Properties,
    ogawa: &mut OgawaReader<R>,
) -> Result<Option<[f64; 6]>> {
    let Some(PropertyValue::Scalar(sample)) = geom.get(SELF_BOUNDS) else {
        return Ok(None);
    };
    if sample.data_type != BOX3D || sample.len == 0 {
        return Ok(None);
    }
    let mut bounds = [0.0f64; 6];
    ogawa.read_into(sample.values, bytemuck::cast_slice_mut(&mut bounds))?;
    Ok(Some(bounds))
}

fn attributes_of(geom: &Properties) -> BTreeMap<String, AttributeHandle> {
    let Some(PropertyValue::Compound(arb_geom_params)) = geom.get(ARB_GEOM_PARAMS_COMPOUND) else {
        return BTreeMap::new();
    };
    arb_geom_params
        .iter()
        .filter_map(|(name, value)| {
            let PropertyValue::Array(sample) = *value else {
                return None;
            };
            let attribute_type = AttributeType::of_data_type(sample.data_type)?;
            Some((
                name.clone(),
                AttributeHandle {
                    attribute_type,
                    sample,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::error::Error;
    use crate::format::attribute::AttributeType;
    use crate::read::{ArchiveReader, ArrayHandle, Points, read_archive};
    use crate::write::ArchiveWriter;
    use crate::write::Attribute;
    use std::collections::BTreeMap;
    use std::io::Cursor;

    type Reader = ArchiveReader<Cursor<Vec<u8>>>;

    fn read_back(
        build: impl FnOnce(&mut ArchiveWriter<Cursor<Vec<u8>>>),
    ) -> (BTreeMap<String, Points>, Reader) {
        let mut archive = ArchiveWriter::create(Cursor::new(Vec::new())).unwrap();
        build(&mut archive);
        let file = archive.finish().unwrap().into_inner();
        let (contents, reader) = read_archive(Cursor::new(file)).unwrap();
        (contents.points, reader)
    }

    fn values<T: bytemuck::Pod>(reader: &mut Reader, handle: &ArrayHandle<T>) -> Vec<T> {
        let mut values = vec![T::zeroed(); handle.len()];
        reader.read_into(handle, &mut values).unwrap();
        values
    }

    #[test]
    fn reads_back_points_with_everything() {
        let positions = [[1.0, -2.0, 3.0], [-1.0, 5.0, 0.5]];
        let velocities = [[0.0, 1.0, 0.0], [0.5, 0.0, -1.0]];
        let widths = [0.1, 0.2];
        let colors = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let uvs = [[0.25, 0.75], [1.0, 0.0]];
        let temperatures = [20.0, 21.5];
        let (points, mut reader) = read_back(|archive| {
            let mut p = archive.begin_points("particles", &positions).unwrap();
            p.velocities(&velocities).unwrap();
            p.widths(&widths).unwrap();
            p.attribute("Cd", Attribute::Float3(&colors)).unwrap();
            p.attribute("uv", Attribute::Float2(&uvs)).unwrap();
            p.attribute("temperature", Attribute::Float1(&temperatures))
                .unwrap();
            p.finish().unwrap();
        });

        let names: Vec<_> = points.keys().map(String::as_str).collect();
        assert_eq!(names, ["particles"]);
        let particles = &points["particles"];
        assert_eq!(particles.bounds, Some([-1.0, -2.0, 0.5, 1.0, 5.0, 3.0]));
        assert_eq!(values(&mut reader, &particles.positions), positions);
        assert_eq!(
            values(&mut reader, particles.velocities.as_ref().unwrap()),
            velocities
        );
        assert_eq!(
            values(&mut reader, particles.widths.as_ref().unwrap()),
            widths
        );

        let names: Vec<_> = particles.attributes.keys().map(String::as_str).collect();
        assert_eq!(names, ["Cd", "temperature", "uv"]);
        let cd = particles.attributes["Cd"].typed::<[f32; 3]>().unwrap();
        assert_eq!(values(&mut reader, &cd), colors);
        let uv = particles.attributes["uv"].typed::<[f32; 2]>().unwrap();
        assert_eq!(values(&mut reader, &uv), uvs);
        let temperature = particles.attributes["temperature"].typed::<f32>().unwrap();
        assert_eq!(values(&mut reader, &temperature), temperatures);
    }

    #[test]
    fn attributes_are_typed_only_as_the_type_they_hold() {
        let (points, _) = read_back(|archive| {
            let mut p = archive.begin_points("p", &[[0.0; 3]]).unwrap();
            p.attribute("one", Attribute::Float1(&[1.0])).unwrap();
            p.attribute("two", Attribute::Float2(&[[1.0, 2.0]]))
                .unwrap();
            p.attribute("three", Attribute::Float3(&[[1.0, 2.0, 3.0]]))
                .unwrap();
            p.finish().unwrap();
        });
        let attributes = &points["p"].attributes;
        use AttributeType::{Float1, Float2, Float3};
        assert_eq!(attributes["one"].attribute_type(), Float1);
        assert_eq!(attributes["two"].attribute_type(), Float2);
        assert_eq!(attributes["three"].attribute_type(), Float3);

        let mismatches = [
            (
                attributes["one"].typed::<[f32; 2]>().unwrap_err(),
                Float2,
                Float1,
            ),
            (
                attributes["two"].typed::<f32>().unwrap_err(),
                Float1,
                Float2,
            ),
            (
                attributes["three"].typed::<[f32; 2]>().unwrap_err(),
                Float2,
                Float3,
            ),
        ];
        for (error, asked, held) in mismatches {
            assert!(
                matches!(error, Error::AttributeTypeMismatch { expected, actual } if expected == asked && actual == held),
                "{error:?}"
            );
        }
    }

    #[test]
    fn optional_parts_are_absent_when_not_written() {
        let (points, mut reader) = read_back(|archive| {
            archive
                .begin_points("positions_only", &[[1.0, 2.0, 3.0]])
                .unwrap()
                .finish()
                .unwrap();
            archive
                .begin_points("empty", &[])
                .unwrap()
                .finish()
                .unwrap();
        });
        let names: Vec<_> = points.keys().map(String::as_str).collect();
        assert_eq!(names, ["empty", "positions_only"]);
        let positions_only = &points["positions_only"];
        let empty = &points["empty"];
        assert!(positions_only.velocities.is_none());
        assert!(positions_only.widths.is_none());
        assert!(positions_only.attributes.is_empty());
        assert_eq!(
            values(&mut reader, &positions_only.positions),
            [[1.0, 2.0, 3.0]]
        );

        assert!(empty.positions.is_empty());
        reader.read_into(&empty.positions, &mut []).unwrap();
        assert_eq!(
            empty.bounds,
            Some([f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN])
        );
    }
}
