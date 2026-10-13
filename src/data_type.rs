#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Pod {
    Uint64 = 7,
    Float32 = 10,
    Float64 = 11,
}

impl Pod {
    pub(crate) fn byte_width(self) -> usize {
        match self {
            Pod::Float32 => 4,
            Pod::Uint64 | Pod::Float64 => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DataType {
    pub(crate) pod: Pod,
    pub(crate) extent: u8,
}

impl DataType {
    pub(crate) const fn new(pod: Pod, extent: u8) -> Self {
        DataType { pod, extent }
    }

    pub(crate) fn element_byte_width(self) -> usize {
        self.pod.byte_width() * usize::from(self.extent)
    }
}
