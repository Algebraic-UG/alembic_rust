use std::env;

use alembic_rust::{ArchiveWriter, POSITIONS, VELOCITIES, WIDTHS};

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "points_edge_cases.abc".to_owned());
    let mut archive = ArchiveWriter::create_file(path)?;

    let mut no_points = archive.begin_points("no_points", 0)?;
    no_points.attribute(POSITIONS, &[] as &[[f32; 3]])?;
    no_points.attribute(VELOCITIES, &[] as &[[f32; 3]])?;
    no_points.attribute(WIDTHS, &[] as &[f32])?;
    no_points.attribute("Cd", &[] as &[[f32; 3]])?;
    no_points.finish()?;

    let mut one_point = archive.begin_points("one_point", 1)?;
    one_point.attribute(POSITIONS, &[[1.0, 2.0, 3.0]])?;
    one_point.attribute(WIDTHS, &[0.5])?;
    one_point.attribute("Cd", &[[0.0, 1.0, 0.0]])?;
    one_point.finish()?;

    let long_name = "a".repeat(300);
    let mut long_named = archive.begin_points(&long_name, 2)?;
    long_named.attribute(POSITIONS, &[[0.0; 3]; 2])?;
    long_named.attribute(&long_name, &[1.0, 2.0])?;
    long_named.finish()?;

    archive.finish()?;
    Ok(())
}
