use crate::format::data_type::DataType;
use crate::format::schema::{FLOAT1, FLOAT2, FLOAT3};

/// The type of a per-point attribute's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeType {
    /// One float per point.
    Float1,
    /// Two floats per point.
    Float2,
    /// Three floats per point.
    Float3,
}

impl AttributeType {
    pub(crate) fn data_type(self) -> DataType {
        match self {
            AttributeType::Float1 => FLOAT1,
            AttributeType::Float2 => FLOAT2,
            AttributeType::Float3 => FLOAT3,
        }
    }

    pub(crate) fn of_data_type(data_type: DataType) -> Option<Self> {
        match data_type {
            FLOAT1 => Some(AttributeType::Float1),
            FLOAT2 => Some(AttributeType::Float2),
            FLOAT3 => Some(AttributeType::Float3),
            _ => None,
        }
    }
}

pub(crate) mod sealed {
    pub trait Sealed {}

    impl Sealed for f32 {}
    impl Sealed for [f32; 2] {}
    impl Sealed for [f32; 3] {}
}

/// An element type of per-point attributes: `f32`, `[f32; 2]` or `[f32; 3]`.
///
/// Only this crate implements it.
pub trait AttributeElement: sealed::Sealed + bytemuck::Pod {
    /// The attribute type whose elements are of this type.
    const TYPE: AttributeType;
}

impl AttributeElement for f32 {
    const TYPE: AttributeType = AttributeType::Float1;
}

impl AttributeElement for [f32; 2] {
    const TYPE: AttributeType = AttributeType::Float2;
}

impl AttributeElement for [f32; 3] {
    const TYPE: AttributeType = AttributeType::Float3;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_types_round_trip_through_data_types() {
        for attribute_type in [
            AttributeType::Float1,
            AttributeType::Float2,
            AttributeType::Float3,
        ] {
            assert_eq!(
                AttributeType::of_data_type(attribute_type.data_type()),
                Some(attribute_type)
            );
        }
    }

    #[test]
    fn element_types_match_their_data_types() {
        assert_eq!(f32::TYPE.data_type(), FLOAT1);
        assert_eq!(<[f32; 2]>::TYPE.data_type(), FLOAT2);
        assert_eq!(<[f32; 3]>::TYPE.data_type(), FLOAT3);
    }
}
