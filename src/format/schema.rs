use crate::format::data_type::{DataType, Pod};

pub(crate) const POINTS_SCHEMA: &str = "AbcGeom_Points_v1";
pub(crate) const GEOM_BASE_SCHEMA: &str = "AbcGeom_GeomBase_v1";
pub(crate) const GEOM_COMPOUND: &str = ".geom";
pub(crate) const ARB_GEOM_PARAMS_COMPOUND: &str = ".arbGeomParams";

pub(crate) const POSITIONS: &str = "P";
pub(crate) const SELF_BOUNDS: &str = ".selfBnds";
pub(crate) const POINT_IDS: &str = ".pointIds";
pub(crate) const VELOCITIES: &str = ".velocities";
pub(crate) const WIDTHS: &str = ".widths";

pub(crate) const PER_POINT_SCOPE: &str = "var";

pub(crate) const FLOAT1: DataType = DataType::new(Pod::Float32, 1);
pub(crate) const FLOAT2: DataType = DataType::new(Pod::Float32, 2);
pub(crate) const FLOAT3: DataType = DataType::new(Pod::Float32, 3);
pub(crate) const POINT_ID: DataType = DataType::new(Pod::Uint64, 1);
pub(crate) const BOX3D: DataType = DataType::new(Pod::Float64, 6);

pub(crate) const USER_DATA_BYTE: DataType = DataType::new(Pod::Uint8, 1);
