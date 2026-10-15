use std::env;

use alembic_rust::{ArchiveWriter, POSITIONS, VELOCITIES, WIDTHS};

fn main() -> alembic_rust::Result<()> {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "points.abc".to_owned());
    let mut archive = ArchiveWriter::create_file(path)?;
    archive
        .metadata_mut()
        .set("_ai_Application", "alembic_rust points example")?;

    let corners = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let mut particles = archive.begin_points("particles", corners.len())?;
    particles.attribute(POSITIONS, &corners)?;
    particles.attribute(VELOCITIES, &[[0.0, 1.0, 0.0]; 4])?;
    particles.attribute(WIDTHS, &[0.1, 0.2, 0.3, 0.4])?;
    particles.attribute("Cd", &[[1.0, 0.0, 0.0]; 4])?;
    particles.attribute("N", &[[0.0, 0.0, 1.0]; 4])?;
    particles.attribute("rest", &corners)?;
    particles.attribute("uv", &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]])?;
    particles.attribute("temperature", &[20.0, 21.0, 22.0, 23.0])?;
    particles.finish()?;

    let mut debris = archive.begin_points("debris", 2)?;
    debris.attribute(POSITIONS, &[[5.0, 5.0, 5.0], [6.0, 6.0, 6.0]])?;
    debris.attribute("mass", &[1.0, 2.0])?;
    debris.finish()?;

    let mut positions_only = archive.begin_points("positions_only", corners.len())?;
    positions_only.attribute(POSITIONS, &corners)?;
    positions_only.finish()?;

    archive.finish()?;
    Ok(())
}
