use std::io::{Read, Seek};

use crate::error::Result;
use crate::format::data_type::DataType;
use crate::format::schema::{
    ARB_GEOM_PARAMS_COMPOUND, BOX3D, FLOAT1, FLOAT2, FLOAT3, GEOM_COMPOUND, POSITIONS, SELF_BOUNDS,
    VELOCITIES, WIDTHS,
};
use crate::read::archive::ArrayHandle;
use crate::read::ogawa::OgawaReader;
use crate::read::tree::{ObjectNode, PropertyNode, PropertyValue};

#[derive(Debug, Clone)]
pub struct Points {
    pub name: String,
    pub bounds: Option<[f64; 6]>,
    pub positions: ArrayHandle<[f32; 3]>,
    pub velocities: Option<ArrayHandle<[f32; 3]>>,
    pub widths: Option<ArrayHandle<f32>>,
    pub attributes: Vec<PointAttribute>,
}

#[derive(Debug, Clone)]
pub struct PointAttribute {
    pub name: String,
    pub values: AttributeValues,
}

#[derive(Debug, Clone, Copy)]
pub enum AttributeValues {
    Float1(ArrayHandle<f32>),
    Float2(ArrayHandle<[f32; 2]>),
    Float3(ArrayHandle<[f32; 3]>),
}

pub(crate) fn points_among<R: Read + Seek>(
    objects: Vec<ObjectNode>,
    ogawa: &mut OgawaReader<R>,
) -> Result<Vec<Points>> {
    let mut points = Vec::with_capacity(objects.len());
    for object in objects {
        let Some(PropertyValue::Compound(geom)) = find(&object.properties, GEOM_COMPOUND) else {
            continue;
        };
        let Some(positions) = array_of(geom, POSITIONS, FLOAT3) else {
            continue;
        };
        points.push(Points {
            name: object.name,
            bounds: bounds_of(geom, ogawa)?,
            positions,
            velocities: array_of(geom, VELOCITIES, FLOAT3),
            widths: array_of(geom, WIDTHS, FLOAT1),
            attributes: attributes_of(geom),
        });
    }
    Ok(points)
}

fn find<'a>(properties: &'a [PropertyNode], name: &str) -> Option<&'a PropertyValue> {
    properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| &property.value)
}

fn array_of<T>(
    properties: &[PropertyNode],
    name: &str,
    data_type: DataType,
) -> Option<ArrayHandle<T>> {
    match find(properties, name)? {
        PropertyValue::Array(sample) if sample.data_type == data_type => {
            Some(ArrayHandle::of_first_sample(*sample))
        }
        _ => None,
    }
}

fn bounds_of<R: Read + Seek>(
    geom: &[PropertyNode],
    ogawa: &mut OgawaReader<R>,
) -> Result<Option<[f64; 6]>> {
    let Some(PropertyValue::Scalar(sample)) = find(geom, SELF_BOUNDS) else {
        return Ok(None);
    };
    if sample.data_type != BOX3D || sample.len == 0 {
        return Ok(None);
    }
    let mut bounds = [0.0f64; 6];
    ogawa.read_into(sample.values, bytemuck::cast_slice_mut(&mut bounds))?;
    Ok(Some(bounds))
}

fn attributes_of(geom: &[PropertyNode]) -> Vec<PointAttribute> {
    let Some(PropertyValue::Compound(arb_geom_params)) = find(geom, ARB_GEOM_PARAMS_COMPOUND)
    else {
        return Vec::new();
    };
    arb_geom_params
        .iter()
        .filter_map(|property| {
            let PropertyValue::Array(sample) = property.value else {
                return None;
            };
            let values = match sample.data_type {
                FLOAT1 => AttributeValues::Float1(ArrayHandle::of_first_sample(sample)),
                FLOAT2 => AttributeValues::Float2(ArrayHandle::of_first_sample(sample)),
                FLOAT3 => AttributeValues::Float3(ArrayHandle::of_first_sample(sample)),
                _ => return None,
            };
            Some(PointAttribute {
                name: property.name.clone(),
                values,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::archive::ArchiveWriter;
    use crate::points::Attribute;
    use crate::read::{ArchiveReader, ArrayHandle, AttributeValues, Points, read_archive};
    use std::io::Cursor;

    type Reader = ArchiveReader<Cursor<Vec<u8>>>;

    fn read_back(build: impl FnOnce(&mut ArchiveWriter<Cursor<Vec<u8>>>)) -> (Vec<Points>, Reader) {
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

        let [particles] = &points[..] else {
            panic!("expected one points object, got {points:?}");
        };
        assert_eq!(particles.name, "particles");
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

        let names: Vec<_> = particles
            .attributes
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, ["Cd", "uv", "temperature"]);
        let AttributeValues::Float3(cd) = particles.attributes[0].values else {
            panic!("Cd is not a three-float attribute");
        };
        assert_eq!(values(&mut reader, &cd), colors);
        let AttributeValues::Float2(uv) = particles.attributes[1].values else {
            panic!("uv is not a two-float attribute");
        };
        assert_eq!(values(&mut reader, &uv), uvs);
        let AttributeValues::Float1(temperature) = particles.attributes[2].values else {
            panic!("temperature is not a float");
        };
        assert_eq!(values(&mut reader, &temperature), temperatures);
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
        let [positions_only, empty] = &points[..] else {
            panic!("expected two points objects, got {points:?}");
        };
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
