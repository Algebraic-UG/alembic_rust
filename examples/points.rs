use std::env;

use alembic_rust::{ArchiveWriter, Attribute};

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
    let mut particles = archive.begin_points("particles", &corners)?;
    particles.velocities(&[[0.0, 1.0, 0.0]; 4])?;
    particles.widths(&[0.1, 0.2, 0.3, 0.4])?;
    particles.attribute("Cd", Attribute::Float3(&[[1.0, 0.0, 0.0]; 4]))?;
    particles.attribute("N", Attribute::Float3(&[[0.0, 0.0, 1.0]; 4]))?;
    particles.attribute("rest", Attribute::Float3(&corners))?;
    particles.attribute(
        "uv",
        Attribute::Float2(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]),
    )?;
    particles.attribute("temperature", Attribute::Float1(&[20.0, 21.0, 22.0, 23.0]))?;
    particles.finish()?;

    let mut debris = archive.begin_points("debris", &[[5.0, 5.0, 5.0], [6.0, 6.0, 6.0]])?;
    debris.attribute("mass", Attribute::Float1(&[1.0, 2.0]))?;
    debris.finish()?;

    archive.begin_points("positions_only", &corners)?.finish()?;

    archive.finish()?;
    Ok(())
}
