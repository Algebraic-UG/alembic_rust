use std::io::{Seek, Write};

use crate::error::{Error, Result};
use crate::format::data_type::DataType;
use crate::format::metadata::MetaData;
use crate::format::schema::{
    ARB_GEOM_PARAMS_COMPOUND, BOX3D, FLOAT1, FLOAT2, FLOAT3, GEOM_BASE_SCHEMA, GEOM_COMPOUND,
    PER_POINT_SCOPE, POINT_ID, POINT_IDS, POINTS_SCHEMA, POSITIONS, SELF_BOUNDS, VELOCITIES,
    WIDTHS,
};
use crate::write::archive::ArchiveWriter;
use crate::write::compound::CompoundWriter;

/// The values of a per-point attribute, one element per point.
///
/// Attributes are written without `interpretation` metadata, so readers see them as plain float
/// arrays. Blender, for example, imports a 3-float `Cd` as a vector rather than a colour, and
/// converts it from Y-up to Z-up.
#[derive(Debug, Clone, Copy)]
pub enum Attribute<'a> {
    /// One float per point.
    Float1(&'a [f32]),
    /// Two floats per point.
    Float2(&'a [[f32; 2]]),
    /// Three floats per point.
    Float3(&'a [[f32; 3]]),
}

impl Attribute<'_> {
    fn num_points(&self) -> usize {
        match self {
            Attribute::Float1(values) => values.len(),
            Attribute::Float2(values) => values.len(),
            Attribute::Float3(values) => values.len(),
        }
    }

    fn data_type(&self) -> DataType {
        match self {
            Attribute::Float1(_) => FLOAT1,
            Attribute::Float2(..) => FLOAT2,
            Attribute::Float3(..) => FLOAT3,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Attribute::Float1(values) => bytemuck::cast_slice(values),
            Attribute::Float2(values) => bytemuck::cast_slice(values),
            Attribute::Float3(values) => bytemuck::cast_slice(values),
        }
    }
}

/// Adds the optional properties of one points object, created by
/// [`ArchiveWriter::begin_points`].
///
/// The object becomes part of the archive only when [`finish`](PointsWriter::finish) is called.
/// Dropping it unfinished makes [`ArchiveWriter::finish`] fail with
/// [`Error::Unfinished`].
#[must_use = "a points object is only added to the archive by calling finish()"]
pub struct PointsWriter<'a, W: Write + Seek> {
    archive: &'a mut ArchiveWriter<W>,
    name: String,
    num_points: usize,
    geom: CompoundWriter,
    arb_geom_params: CompoundWriter,
    finished: bool,
}

impl<W: Write + Seek> ArchiveWriter<W> {
    /// Starts a points object called `name` with the given positions.
    ///
    /// This writes `P`, the bounding box `.selfBnds` computed from the positions, and an empty
    /// `.pointIds`, as upstream does when no IDs are given. The number of positions is the point
    /// count that every other per-point property must match.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidName`] or [`Error::DuplicateName`] if `name` is empty, contains `/` or
    /// is already used by another object.
    pub fn begin_points(
        &mut self,
        name: &str,
        positions: &[[f32; 3]],
    ) -> Result<PointsWriter<'_, W>> {
        self.claim_child_name(name)?;
        let mut geom = CompoundWriter::new();
        let output = self.output();
        geom.add_array(
            output,
            POSITIONS,
            &metadata(&[("geoScope", PER_POINT_SCOPE), ("interpretation", "point")]),
            FLOAT3,
            bytemuck::cast_slice(positions),
        )?;
        geom.add_scalar(
            output,
            SELF_BOUNDS,
            &metadata(&[("interpretation", "box")]),
            BOX3D,
            bytemuck::cast_slice(&bounds_of(positions)),
        )?;
        geom.add_array(
            output,
            POINT_IDS,
            &metadata(&[("geoScope", PER_POINT_SCOPE)]),
            POINT_ID,
            &[],
        )?;
        Ok(PointsWriter {
            archive: self,
            name: name.to_owned(),
            num_points: positions.len(),
            geom,
            arb_geom_params: CompoundWriter::new(),
            finished: false,
        })
    }
}

impl<W: Write + Seek> PointsWriter<'_, W> {
    /// Writes the velocities, as the standard `.velocities` property.
    ///
    /// # Errors
    ///
    /// [`Error::PointCountMismatch`] if there is not one velocity per point, and
    /// [`Error::DuplicateName`] if velocities were already written.
    pub fn velocities(&mut self, velocities: &[[f32; 3]]) -> Result<()> {
        self.check_point_count("velocities", VELOCITIES, velocities.len())?;
        self.geom.add_array(
            self.archive.output(),
            VELOCITIES,
            &metadata(&[("interpretation", "vector")]),
            FLOAT3,
            bytemuck::cast_slice(velocities),
        )
    }

    /// Writes the widths, as the standard `.widths` property.
    ///
    /// # Errors
    ///
    /// [`Error::PointCountMismatch`] if there is not one width per point, and
    /// [`Error::DuplicateName`] if widths were already written.
    pub fn widths(&mut self, widths: &[f32]) -> Result<()> {
        self.check_point_count("widths", WIDTHS, widths.len())?;
        self.geom.add_array(
            self.archive.output(),
            WIDTHS,
            &per_point_geom_param_metadata(FLOAT1),
            FLOAT1,
            bytemuck::cast_slice(widths),
        )
    }

    /// Writes a per-point attribute called `name` into `.arbGeomParams`.
    ///
    /// # Errors
    ///
    /// [`Error::PointCountMismatch`] if there is not one value per point; nothing is written and
    /// the points writer stays usable. [`Error::InvalidName`] or [`Error::DuplicateName`] if
    /// `name` is empty, contains `/` or is already used by another attribute.
    pub fn attribute(&mut self, name: &str, attribute: Attribute<'_>) -> Result<()> {
        self.check_point_count("attribute", name, attribute.num_points())?;
        self.arb_geom_params.add_array(
            self.archive.output(),
            name,
            &per_point_geom_param_metadata(attribute.data_type()),
            attribute.data_type(),
            attribute.bytes(),
        )
    }

    /// Adds the points object to the archive.
    pub fn finish(mut self) -> Result<()> {
        self.finished = true;
        let mut geom = std::mem::take(&mut self.geom);
        let arb_geom_params = std::mem::take(&mut self.arb_geom_params);
        let output = self.archive.output();
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

    fn check_point_count(&self, what: &'static str, name: &str, actual: usize) -> Result<()> {
        if actual != self.num_points {
            return Err(Error::PointCountMismatch {
                what,
                name: name.to_owned(),
                expected: self.num_points,
                actual,
            });
        }
        Ok(())
    }
}

impl<W: Write + Seek> Drop for PointsWriter<'_, W> {
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
        let mut points = archive.begin_points("p", &[[0.0; 3]; 2]).unwrap();
        let mismatch = points.attribute("t", Attribute::Float1(&[1.0]));
        assert!(matches!(
            mismatch,
            Err(Error::PointCountMismatch {
                expected: 2,
                actual: 1,
                ..
            })
        ));
        assert!(points.velocities(&[[0.0; 3]; 3]).is_err());
        assert!(points.widths(&[]).is_err());
        points
            .attribute("t", Attribute::Float1(&[1.0, 2.0]))
            .unwrap();
        points.finish().unwrap();
        archive.finish().unwrap();
    }

    #[test]
    fn rejects_duplicate_names() {
        let mut archive = new_archive();
        archive.begin_points("p", &[]).unwrap().finish().unwrap();
        assert!(matches!(
            archive.begin_points("p", &[]),
            Err(Error::DuplicateName { .. })
        ));
        let mut points = archive.begin_points("q", &[]).unwrap();
        points.widths(&[]).unwrap();
        assert!(matches!(
            points.widths(&[]),
            Err(Error::DuplicateName { .. })
        ));
        points.attribute("a", Attribute::Float1(&[])).unwrap();
        assert!(points.attribute("a", Attribute::Float1(&[])).is_err());
        points.finish().unwrap();
    }

    #[test]
    fn dropping_unfinished_points_fails_the_archive() {
        let mut archive = new_archive();
        drop(archive.begin_points("lost", &[[0.0; 3]]).unwrap());
        assert!(matches!(
            archive.finish(),
            Err(Error::Unfinished { name }) if name == "lost"
        ));
    }

    #[test]
    fn positions_are_written_verbatim() {
        let positions = [[1.5f32, -2.25, 3.0], [4.0, 5.0, 6.0]];
        let mut archive = new_archive();
        archive
            .begin_points("p", &positions)
            .unwrap()
            .finish()
            .unwrap();
        let file = archive.finish().unwrap().into_inner();
        let position_bytes: &[u8] = bytemuck::cast_slice(&positions);
        assert!(
            file.windows(position_bytes.len())
                .any(|w| w == position_bytes)
        );
    }
}
