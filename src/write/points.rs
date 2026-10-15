use std::io::{Seek, Write};

use crate::error::{Error, Result};
use crate::format::attribute::AttributeElement;
use crate::format::data_type::DataType;
use crate::format::metadata::MetaData;
use crate::format::ogawa::DataRef;
use crate::format::schema::{
    ARB_GEOM_PARAMS_COMPOUND, BOX3D, FLOAT1, FLOAT3, GEOM_BASE_SCHEMA, GEOM_COMPOUND,
    PER_POINT_SCOPE, POINT_ID, POINT_IDS, POINTS_SCHEMA, POSITIONS, RESERVED_PREFIX, SELF_BOUNDS,
    VELOCITIES, WIDTHS,
};
use crate::write::archive::ArchiveWriter;
use crate::write::compound::CompoundWriter;

/// Adds the optional properties of one points object, created by
/// [`ArchiveWriter::begin_points`].
///
/// The object becomes part of the archive only when [`finish`](PointsWriter::finish) is called.
/// Dropping it unfinished makes [`ArchiveWriter::finish`] fail with
/// [`Error::Unfinished`].
#[must_use = "a points object is only added to the archive by calling finish()"]
pub struct PointsWriter<'a, W: Write> {
    archive: &'a mut ArchiveWriter<W>,
    name: String,
    num_points: usize,
    geom: CompoundWriter,
    positions: Option<DataRef>,
    bounds: [f64; 6],
    velocities: Option<DataRef>,
    widths: Option<DataRef>,
    arb_geom_params: CompoundWriter,
    finished: bool,
}

impl<W: Write + Seek> ArchiveWriter<W> {
    /// Starts a points object called `name` with `num_points` points.
    ///
    /// Every attribute must then have one value per point, and [`POSITIONS`] must be set before
    /// [`finish`](PointsWriter::finish).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidName`] or [`Error::DuplicateName`] if `name` is empty, contains `/` or
    /// is already used by another object.
    pub fn begin_points(&mut self, name: &str, num_points: usize) -> Result<PointsWriter<'_, W>> {
        self.claim_child_name(name)?;
        Ok(PointsWriter {
            archive: self,
            name: name.to_owned(),
            num_points,
            geom: CompoundWriter::new(),
            positions: None,
            bounds: bounds_of(&[]),
            velocities: None,
            widths: None,
            arb_geom_params: CompoundWriter::new(),
            finished: false,
        })
    }
}

impl<W: Write + Seek> PointsWriter<'_, W> {
    /// Writes a per-point attribute called `name`, one element of `values` per point. The
    /// element type decides the attribute's type, see [`AttributeElement`].
    ///
    /// Three names are the standard properties of a points object, with a fixed type:
    /// [`POSITIONS`] (`[f32; 3]`, required), [`VELOCITIES`] (`[f32; 3]`) and [`WIDTHS`]
    /// (`f32`). The bounding box `.selfBnds` is computed from the positions. Every other name
    /// is stored in `.arbGeomParams`.
    ///
    /// Attributes are written without `interpretation` metadata, so readers see them as plain
    /// float arrays. Blender, for example, imports a 3-float `Cd` as a vector rather than a
    /// colour, and converts it from Y-up to Z-up.
    ///
    /// # Errors
    ///
    /// [`Error::PointCountMismatch`] if there is not one value per point, and
    /// [`Error::AttributeTypeMismatch`] if a standard property gets the wrong type; nothing is
    /// written and the points writer stays usable. [`Error::InvalidName`] if `name` is empty,
    /// contains `/` or starts with `.` without being a standard property, and
    /// [`Error::DuplicateName`] if it is already used by another attribute.
    pub fn attribute<T: AttributeElement>(&mut self, name: &str, values: &[T]) -> Result<()> {
        self.check_point_count(name, values.len())?;
        match name {
            POSITIONS => {
                let positions = standard_values::<T, [f32; 3]>(values)?;
                self.positions = Some(self.write_standard(POSITIONS, values)?);
                self.bounds = bounds_of(positions);
            }
            VELOCITIES => {
                standard_values::<T, [f32; 3]>(values)?;
                self.velocities = Some(self.write_standard(VELOCITIES, values)?);
            }
            WIDTHS => {
                standard_values::<T, f32>(values)?;
                self.widths = Some(self.write_standard(WIDTHS, values)?);
            }
            _ if name.starts_with(RESERVED_PREFIX) => {
                return Err(Error::InvalidName {
                    name: name.to_owned(),
                });
            }
            _ => {
                let data_type = T::TYPE.data_type();
                self.arb_geom_params.add_array(
                    self.archive.output(),
                    name,
                    &per_point_geom_param_metadata(data_type),
                    data_type,
                    bytemuck::cast_slice(values),
                )?;
            }
        }
        Ok(())
    }

    /// Adds the points object to the archive.
    ///
    /// # Errors
    ///
    /// [`Error::MissingPositions`] if [`POSITIONS`] was not set. The object is then left
    /// unfinished, so [`ArchiveWriter::finish`] fails with [`Error::Unfinished`].
    pub fn finish(mut self) -> Result<()> {
        let Some(positions) = self.positions else {
            return Err(Error::MissingPositions {
                name: self.name.clone(),
            });
        };
        self.finished = true;
        let mut geom = std::mem::take(&mut self.geom);
        let arb_geom_params = std::mem::take(&mut self.arb_geom_params);
        let num_points = self.num_points;
        let output = self.archive.output();
        geom.add_claimed_array(
            output,
            POSITIONS,
            &metadata(&[("geoScope", PER_POINT_SCOPE), ("interpretation", "point")]),
            FLOAT3,
            positions,
            num_points,
        )?;
        geom.add_scalar(
            output,
            SELF_BOUNDS,
            &metadata(&[("interpretation", "box")]),
            BOX3D,
            bytemuck::cast_slice(&self.bounds),
        )?;
        geom.add_array(
            output,
            POINT_IDS,
            &metadata(&[("geoScope", PER_POINT_SCOPE)]),
            POINT_ID,
            &[],
        )?;
        if let Some(velocities) = self.velocities {
            geom.add_claimed_array(
                output,
                VELOCITIES,
                &metadata(&[("interpretation", "vector")]),
                FLOAT3,
                velocities,
                num_points,
            )?;
        }
        if let Some(widths) = self.widths {
            geom.add_claimed_array(
                output,
                WIDTHS,
                &per_point_geom_param_metadata(FLOAT1),
                FLOAT1,
                widths,
                num_points,
            )?;
        }
        if !arb_geom_params.is_empty() {
            geom.add_compound(
                output,
                ARB_GEOM_PARAMS_COMPOUND,
                &MetaData::new(),
                arb_geom_params,
            )?;
        }
        let mut properties = CompoundWriter::new();
        properties.add_compound(output, GEOM_COMPOUND, &geom_compound_metadata(), geom)?;
        self.archive
            .add_claimed_child_object(&self.name, &points_object_metadata(), properties)
    }

    fn write_standard<T: AttributeElement>(&mut self, name: &str, values: &[T]) -> Result<DataRef> {
        self.geom.claim_name(name)?;
        self.archive
            .output()
            .write_sample(bytemuck::cast_slice(values))
    }

    fn check_point_count(&self, name: &str, actual: usize) -> Result<()> {
        if actual != self.num_points {
            return Err(Error::PointCountMismatch {
                name: name.to_owned(),
                expected: self.num_points,
                actual,
            });
        }
        Ok(())
    }
}

fn standard_values<T: AttributeElement, U: AttributeElement>(values: &[T]) -> Result<&[U]> {
    if T::TYPE != U::TYPE {
        return Err(Error::AttributeTypeMismatch {
            expected: U::TYPE,
            actual: T::TYPE,
        });
    }
    Ok(bytemuck::cast_slice(values))
}

impl<W: Write> Drop for PointsWriter<'_, W> {
    fn drop(&mut self) {
        if !self.finished {
            self.archive.abandon_child_object(&self.name);
        }
    }
}

fn metadata(pairs: &[(&str, &str)]) -> MetaData {
    let mut md = MetaData::new();
    for (key, value) in pairs {
        md.set(*key, *value)
            .expect("schema metadata contains no separators");
    }
    md
}

fn points_object_metadata() -> MetaData {
    let schema_object_title = format!("{POINTS_SCHEMA}:{GEOM_COMPOUND}");
    metadata(&[
        ("schema", POINTS_SCHEMA),
        ("schemaBaseType", GEOM_BASE_SCHEMA),
        ("schemaObjTitle", &schema_object_title),
    ])
}

fn geom_compound_metadata() -> MetaData {
    metadata(&[
        ("schema", POINTS_SCHEMA),
        ("schemaBaseType", GEOM_BASE_SCHEMA),
    ])
}

fn per_point_geom_param_metadata(data_type: DataType) -> MetaData {
    let not_an_array_of_arrays = "1";
    metadata(&[
        ("arrayExtent", not_an_array_of_arrays),
        ("geoScope", PER_POINT_SCOPE),
        ("isGeomParam", "true"),
        ("podExtent", &data_type.extent.to_string()),
        ("podName", data_type.pod.name()),
    ])
}

fn bounds_of(positions: &[[f32; 3]]) -> [f64; 6] {
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    for position in positions {
        for axis in 0..3 {
            let value = f64::from(position[axis]);
            if value < min[axis] {
                min[axis] = value;
            }
            if value > max[axis] {
                max[axis] = value;
            }
        }
    }
    [min[0], min[1], min[2], max[0], max[1], max[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn new_archive() -> ArchiveWriter<Cursor<Vec<u8>>> {
        ArchiveWriter::create(Cursor::new(Vec::new())).unwrap()
    }

    fn position_of(file: &[u8], name: &str) -> usize {
        file.windows(name.len())
            .position(|w| w == name.as_bytes())
            .unwrap()
    }

    #[test]
    fn standard_properties_keep_their_order_whatever_the_call_order() {
        let mut archive = new_archive();
        let mut points = archive.begin_points("p", 1).unwrap();
        points.attribute("Cd", &[[1.0, 0.0, 0.0]]).unwrap();
        points.attribute(WIDTHS, &[0.5]).unwrap();
        points.attribute(VELOCITIES, &[[1.0, 0.0, 0.0]]).unwrap();
        points.attribute(POSITIONS, &[[0.0, 1.0, 2.0]]).unwrap();
        points.finish().unwrap();
        let file = archive.finish().unwrap().into_inner();
        let header_order: Vec<_> = [
            ".selfBnds",
            ".pointIds",
            ".velocities",
            ".widths",
            ".arbGeomParams",
        ]
        .map(|name| position_of(&file, name))
        .into();
        assert!(header_order.is_sorted(), "{header_order:?}");
    }

    #[test]
    fn bounds_cover_all_positions() {
        let positions = [[1.0, -2.0, 3.0], [-1.0, 5.0, 0.5]];
        assert_eq!(bounds_of(&positions), [-1.0, -2.0, 0.5, 1.0, 5.0, 3.0]);
    }

    #[test]
    fn bounds_of_no_positions_are_an_empty_box() {
        assert_eq!(
            bounds_of(&[]),
            [f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN]
        );
    }

    #[test]
    fn bounds_ignore_nan() {
        let positions = [[f32::NAN, 0.0, 0.0], [2.0, 1.0, 1.0]];
        assert_eq!(bounds_of(&positions), [2.0, 0.0, 0.0, 2.0, 1.0, 1.0]);
    }

    #[test]
    fn rejects_attributes_with_wrong_point_count() {
        let mut archive = new_archive();
        let mut points = archive.begin_points("p", 2).unwrap();
        let mismatch = points.attribute("t", &[1.0]);
        assert!(matches!(
            mismatch,
            Err(Error::PointCountMismatch {
                expected: 2,
                actual: 1,
                ..
            })
        ));
        assert!(points.attribute(POSITIONS, &[[0.0; 3]; 3]).is_err());
        assert!(points.attribute(WIDTHS, &[] as &[f32]).is_err());
        points.attribute(POSITIONS, &[[0.0; 3]; 2]).unwrap();
        points.attribute("t", &[1.0, 2.0]).unwrap();
        points.finish().unwrap();
        archive.finish().unwrap();
    }

    #[test]
    fn standard_properties_have_fixed_types() {
        let mut archive = new_archive();
        let mut points = archive.begin_points("p", 1).unwrap();
        assert!(matches!(
            points.attribute(POSITIONS, &[1.0]),
            Err(Error::AttributeTypeMismatch { .. })
        ));
        assert!(matches!(
            points.attribute(VELOCITIES, &[[1.0, 2.0]]),
            Err(Error::AttributeTypeMismatch { .. })
        ));
        assert!(matches!(
            points.attribute(WIDTHS, &[[1.0, 2.0, 3.0]]),
            Err(Error::AttributeTypeMismatch { .. })
        ));
        points.attribute(POSITIONS, &[[0.0; 3]]).unwrap();
        points.finish().unwrap();
        archive.finish().unwrap();
    }

    #[test]
    fn rejects_other_names_starting_with_a_dot() {
        let mut archive = new_archive();
        let mut points = archive.begin_points("p", 1).unwrap();
        assert!(matches!(
            points.attribute(".selfBnds", &[[0.0; 3]]),
            Err(Error::InvalidName { .. })
        ));
        assert!(matches!(
            points.attribute(".custom", &[1.0]),
            Err(Error::InvalidName { .. })
        ));
    }

    #[test]
    fn finishing_without_positions_fails_the_archive() {
        let mut archive = new_archive();
        let mut points = archive.begin_points("p", 1).unwrap();
        points.attribute("t", &[1.0]).unwrap();
        assert!(matches!(
            points.finish(),
            Err(Error::MissingPositions { name }) if name == "p"
        ));
        assert!(matches!(archive.finish(), Err(Error::Unfinished { .. })));
    }

    #[test]
    fn rejects_duplicate_names() {
        let mut archive = new_archive();
        let mut p = archive.begin_points("p", 0).unwrap();
        p.attribute(POSITIONS, &[] as &[[f32; 3]]).unwrap();
        p.finish().unwrap();
        assert!(matches!(
            archive.begin_points("p", 0),
            Err(Error::DuplicateName { .. })
        ));
        let mut points = archive.begin_points("q", 0).unwrap();
        points.attribute(POSITIONS, &[] as &[[f32; 3]]).unwrap();
        points.attribute(WIDTHS, &[] as &[f32]).unwrap();
        assert!(matches!(
            points.attribute(WIDTHS, &[] as &[f32]),
            Err(Error::DuplicateName { .. })
        ));
        points.attribute("a", &[] as &[f32]).unwrap();
        assert!(points.attribute("a", &[] as &[f32]).is_err());
        points.finish().unwrap();
    }

    #[test]
    fn dropping_unfinished_points_fails_the_archive() {
        let mut archive = new_archive();
        drop(archive.begin_points("lost", 1).unwrap());
        assert!(matches!(
            archive.finish(),
            Err(Error::Unfinished { name }) if name == "lost"
        ));
    }

    #[test]
    fn positions_are_written_verbatim() {
        let positions = [[1.5f32, -2.25, 3.0], [4.0, 5.0, 6.0]];
        let mut archive = new_archive();
        let mut points = archive.begin_points("p", positions.len()).unwrap();
        points.attribute(POSITIONS, &positions).unwrap();
        points.finish().unwrap();
        let file = archive.finish().unwrap().into_inner();
        let position_bytes: &[u8] = bytemuck::cast_slice(&positions);
        assert!(
            file.windows(position_bytes.len())
                .any(|w| w == position_bytes)
        );
    }
}
