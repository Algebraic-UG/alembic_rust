abcecho_bin := justfile_directory() / "alembic_cpp/build/bin/AbcEcho/abcecho"
expected_dir := justfile_directory() / "tests/expected"

[working-directory: 'alembic_cpp/build']
abcecho: cpp_build_dir
    cmake -DALEMBIC_SHARED_LIBS=OFF ..
    make -j abcecho

cpp_build_dir: get_cpp
    mkdir -p alembic_cpp/build/

get_cpp:
    git submodule update --init alembic_cpp

test: unit_tests (check "empty") (check "empty_info") (check "unfinished")

unit_tests:
    cargo test --quiet

[working-directory: 'target/abc-archives']
check name: abcecho
    {{abcecho_bin}} {{name}}.abc | tail -n +2 | diff -u {{expected_dir}}/{{name}}.txt -
