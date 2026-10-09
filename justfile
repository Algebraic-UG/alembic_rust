[working-directory: 'alembic_cpp/build']
abcecho: cpp_build_dir
    cmake -DALEMBIC_SHARED_LIBS=OFF ..
    make -j abcecho

cpp_build_dir: get_cpp
    mkdir -p alembic_cpp/build/

get_cpp:
    git submodule update --init alembic_cpp
