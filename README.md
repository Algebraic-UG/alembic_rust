# alembic_rust

A small, streaming writer for [Alembic](https://www.alembic.io/) `.abc` files, written in pure Rust.

It writes one specific kind of archive:

- **Ogawa** backend only. This is the current Alembic format; HDF5 is not supported.
- **Points** objects only (`AbcGeom_Points_v1`). All objects sit side by side directly under the archive root.
- **A single time sample**, at time 0.0.
- **Per-point `f32` attributes** with 1, 2 or 3 components.

The output follows the layout of Alembic 1.8.12. It writes the same property names, order and metadata as upstream's `OPointsSchema`, so Alembic-aware tools read it as ordinary points.

The library only writes. It has no reader.

## Usage

```rust
use alembic_rust::{ArchiveWriter, Attribute};

fn main() -> alembic_rust::Result<()> {
    let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    let mut archive = ArchiveWriter::create_file("particles.abc")?;
    archive.metadata_mut().set("_ai_Application", "my app")?;

    let mut particles = archive.begin_points("particles", &positions)?;
    particles.velocities(&[[0.0, 1.0, 0.0]; 3])?;
    particles.widths(&[0.1, 0.2, 0.3])?;
    particles.attribute("Cd", Attribute::Float3(&[[1.0, 0.0, 0.0]; 3]))?;
    particles.attribute("uv", Attribute::Float2(&[[0.0, 0.0]; 3]))?;
    particles.attribute("temperature", Attribute::Float1(&[20.0, 21.0, 22.0]))?;
    particles.finish()?;

    archive.finish()?;
    Ok(())
}
```

`ArchiveWriter::create` accepts any `Write + Seek` stream in place of a file path.

More complete programs are in [`examples/`](examples/):

```sh
cargo run --example points -- out.abc
```

## Testing

```sh
just test
```

This runs the unit tests (`cargo test`), then validates real files with upstream Alembic:

1. It builds upstream's `abcecho` tool from the `alembic_cpp` submodule (`just abcecho`).
2. Each listed example `examples/<name>.rs` writes `target/abc-archives/<name>.abc`.
3. `abcecho` reads each archive. Its output is compared with `tests/expected/<name>.txt`, ignoring the first line, which contains abcecho's build date.

The recipe fails if an example fails or any output differs. You can check a single example with `just example points`.

To add a case:

1. Add `examples/<name>.rs`.
2. Add its expected output as `tests/expected/<name>.txt`.
3. Append `(example "<name>")` to the `test` recipe in the `justfile`.

`abcecho` checks that a file parses and shows its structure: objects, properties, types, metadata and sample counts. It does not print values, so unit tests cover data correctness.

`tests/expected/empty_info.txt` and `tests/expected/points.txt` contain the crate version (`written by alembic_rust 0.1.0`) and need updating when it changes.

### Requirements for `just test`

- [`just`](https://github.com/casey/just), e.g. `cargo install just`.
- CMake **3.29 or newer**, which Alembic 1.8.12 requires, e.g. `uv tool install cmake`.
- A C++ compiler, `make`, and the Imath development files (`libimath-dev` on Debian/Ubuntu).
- The `alembic_cpp` submodule. `.gitmodules` uses an SSH URL. Without SSH access to GitHub, clone it once over HTTPS:

  ```sh
  git -c url."https://github.com/".insteadOf="git@github.com:" submodule update --init alembic_cpp
  ```
