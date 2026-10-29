# alembic_rust

A small, streaming writer and reader for [Alembic](https://www.alembic.io/) `.abc` files, written in pure Rust.

It writes one specific kind of archive:

- **Ogawa** backend only. This is the current Alembic format; HDF5 is not supported.
- **Points** objects only (`AbcGeom_Points_v1`). All objects sit side by side directly under the archive root.
- **A single time sample**, at time 0.0.
- **Per-point `f32` attributes** with 1, 2 or 3 components, without `interpretation` metadata. Tools see them as plain float arrays; Blender, for example, imports a 3-float `Cd` as a vector and converts it from Y-up to Z-up.
- **Placeholder hashes.** Sample digests and hierarchy hashes are zeros. Readers don't verify them, but tools that use them to detect identical data would treat same-sized samples as equal.
- **No sample deduplication.** Only empty samples share a record.
- **Little-endian targets only**, like upstream's Ogawa writer.

The output follows the layout of Alembic 1.8.12. It writes the same property names, order and metadata as upstream's `OPointsSchema`, so Alembic-aware tools read it as ordinary points.

The reader is meant for the archives this library writes. It skips anything it doesn't recognise, reads only the first sample of each property, and doesn't check versions, hashes or time sampling.

## Usage

```rust
use alembic_rust::{ArchiveWriter, POSITIONS, VELOCITIES, WIDTHS};

fn main() -> alembic_rust::Result<()> {
    let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    let mut archive = ArchiveWriter::create_file("particles.abc")?;
    archive.metadata_mut().set("_ai_Application", "my app")?;

    let mut particles = archive.begin_points("particles", positions.len())?;
    particles.attribute(POSITIONS, &positions)?;
    particles.attribute(VELOCITIES, &[[0.0, 1.0, 0.0]; 3])?;
    particles.attribute(WIDTHS, &[0.1, 0.2, 0.3])?;
    particles.attribute("Cd", &[[1.0, 0.0, 0.0]; 3])?;
    particles.attribute("uv", &[[0.0, 0.0]; 3])?;
    particles.attribute("temperature", &[20.0, 21.0, 22.0])?;
    particles.finish()?;

    archive.finish()?;
    Ok(())
}
```

`ArchiveWriter::create` accepts any `Write + Seek` stream in place of a file path.

More complete programs are in [`examples/`](examples/):

```sh
cargo run --example points -- out.abc
cargo run --example read_summary -- out.abc
```

### Reading

```rust
use std::fs::File;

use alembic_rust::{POSITIONS, read_archive};

fn main() -> alembic_rust::Result<()> {
    let (contents, mut reader) = read_archive(File::open("particles.abc")?)?;
    println!("written by {:?}", contents.metadata.get("_ai_Application"));

    for points in contents.points.values() {
        let positions = points.attributes[POSITIONS].typed::<[f32; 3]>()?;
        let mut buffer = vec![[0.0f32; 3]; positions.len()];
        reader.read_into(&positions, &mut buffer)?;

        for attribute in points.attributes.values() {
            if let Ok(values) = attribute.typed::<f32>() {
                println!("{:?}", reader.read_to_vec(&values)?);
            }
        }
    }
    for blob in contents.user_data.values() {
        let mut bytes = vec![0u8; blob.len()];
        reader.read_into(blob, &mut bytes)?;
    }
    Ok(())
}
```

`read_archive` accepts any `Read + Seek` stream. It reads only the archive's structure: names, metadata, bounds, and where each array is stored. Array values are read only when you call `read_into`, straight into a buffer you provide, `read_to_vec`, which returns them in a new vector, or `user_data_reader`, which streams a blob's bytes. Apart from `read_to_vec`, the reader never allocates memory for bulk data.

The API documentation, including every error, is in the rustdoc: `cargo doc --open`.

## Testing

```sh
just test
```

This runs the unit tests (`cargo test`), then validates real files with upstream Alembic:

1. It builds upstream's `abcecho` tool from the `alembic_cpp` submodule (`just abcecho`).
2. Each listed example `examples/<name>.rs` writes `target/abc-archives/<name>.abc`.
3. `abcecho` reads each archive. Its output is compared with `examples/expected/<name>.txt`, ignoring the first line, which contains abcecho's build date.
4. `examples/read_summary.rs` reads each archive back with this library's reader and prints everything, values included. Its output is compared with `examples/expected/<name>.read.txt`.

The recipe fails if an example fails or any output differs. You can check a single example with `just read_back points`, or only the abcecho part with `just example points`.

To add a case:

1. Add `examples/<name>.rs`.
2. Add its expected outputs as `examples/expected/<name>.txt` (abcecho) and `examples/expected/<name>.read.txt` (read-back).
3. Append `(read_back "<name>")` to the `test` recipe in the `justfile`.

`abcecho` checks that upstream Alembic can parse a file and shows its structure: objects, properties, types, metadata and sample counts. It does not print values. The read-back check covers values.

The expected outputs contain the crate version (`written by alembic_rust 0.1.0`): `empty_info.txt`, `points.txt` and every `.read.txt` of a finished archive. They need updating when the version changes.

### Requirements for `just test`

- [`just`](https://github.com/casey/just), e.g. `cargo install just`.
- CMake **3.29 or newer**, which Alembic 1.8.12 requires, e.g. `uv tool install cmake`.
- A C++ compiler, `make`, and the Imath development files (`libimath-dev` on Debian/Ubuntu).
- Network access the first time, when `just` fetches the `alembic_cpp` submodule.
