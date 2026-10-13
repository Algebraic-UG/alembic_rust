pub(crate) const MAGIC: &[u8; 5] = b"Ogawa";
pub(crate) const NOT_FROZEN: u8 = 0x00;
pub(crate) const FROZEN: u8 = 0xff;
pub(crate) const FORMAT_VERSION: [u8; 2] = [0, 1];
pub(crate) const HEADER_LEN: u64 = 16;
pub(crate) const FROZEN_BYTE_OFFSET: u64 = 5;
pub(crate) const ROOT_POSITION_OFFSET: u64 = 8;
pub(crate) const RECORD_LENGTH_FIELD: u64 = 8;
pub(crate) const CHILD_REFERENCE_SIZE: u64 = 8;

pub(crate) const DATA_REFERENCE_BIT: u64 = 0x8000_0000_0000_0000;
pub(crate) const INVALID_GROUP_POSITION: u64 = 0x7fff_ffff_ffff_ffff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GroupRef(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DataRef(pub(crate) u64);

impl GroupRef {
    pub(crate) const EMPTY: GroupRef = GroupRef(0);
}

impl DataRef {
    pub(crate) const EMPTY: DataRef = DataRef(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Child {
    Group(GroupRef),
    Data(DataRef),
}

impl Child {
    pub(crate) fn from_encoded_reference(reference: u64) -> Self {
        if reference & DATA_REFERENCE_BIT == 0 {
            Child::Group(GroupRef(reference))
        } else {
            Child::Data(DataRef(reference & !DATA_REFERENCE_BIT))
        }
    }

    pub(crate) fn encoded_reference(self) -> u64 {
        match self {
            Child::Group(GroupRef(position)) => position,
            Child::Data(DataRef(position)) => position | DATA_REFERENCE_BIT,
        }
    }
}

impl From<GroupRef> for Child {
    fn from(group: GroupRef) -> Self {
        Child::Group(group)
    }
}

impl From<DataRef> for Child {
    fn from(data: DataRef) -> Self {
        Child::Data(data)
    }
}
