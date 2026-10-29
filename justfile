abcecho_bin := justfile_directory() / "alembic_cpp/build/bin/AbcEcho/abcecho"
expected_dir := justfile_directory() / "examples/expected"

[working-directory: 'alembic_cpp/build']
abcecho: cpp_build_dir
    cmake -DALEMBIC_SHARED_LIBS=OFF ..
    make -j abcecho

cpp_build_dir: get_cpp
    mkdir -p alembic_cpp/build/

get_cpp:
    git submodule update --init --checkout alembic_cpp

test: unit_tests (read_back "empty") (read_back "empty_info") (read_back "unfinished") (read_back "points") (read_back "points_edge_cases") (read_back "points_unfinished") (read_back "user_data")

unit_tests:
    cargo test --quiet

[working-directory: 'target/abc-archives']
example name: abcecho archives_dir
    cargo run --quiet --example {{name}} -- {{name}}.abc
    {{abcecho_bin}} {{name}}.abc | tail -n +2 | diff -u {{expected_dir}}/{{name}}.txt -

[working-directory: 'target/abc-archives']
read_back name: (example name)
    cargo run --quiet --example read_summary -- {{name}}.abc | diff -u {{expected_dir}}/{{name}}.read.txt -

archives_dir:
    mkdir -p target/abc-archives
