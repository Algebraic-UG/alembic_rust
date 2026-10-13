pub(crate) const PROPERTY_TYPE_MASK: u32 = 0b11;
pub(crate) const COMPOUND_PROPERTY: u32 = 0;
pub(crate) const SCALAR_PROPERTY: u32 = 1;
pub(crate) const ARRAY_PROPERTY: u32 = 2;
pub(crate) const SCALAR_LIKE_ARRAY_PROPERTY: u32 = 3;

pub(crate) const FIELD_WIDTH_SHIFT: u32 = 2;
pub(crate) const POD_SHIFT: u32 = 4;
pub(crate) const HOMOGENEOUS_FLAG: u32 = 1 << 10;
pub(crate) const CONSTANT_FLAG: u32 = 1 << 11;
pub(crate) const EXTENT_SHIFT: u32 = 12;
pub(crate) const METADATA_INDEX_SHIFT: u32 = 20;

pub(crate) const SAMPLES_PER_PROPERTY: u32 = 1;

pub(crate) const EMPTY_METADATA_INDEX: u8 = 0;
pub(crate) const INLINE_METADATA_INDEX: u8 = 255;

pub(crate) const DIGEST_LEN: usize = 16;
pub(crate) const HIERARCHY_HASHES_LEN: usize = 2 * DIGEST_LEN;
