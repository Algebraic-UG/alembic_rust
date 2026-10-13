#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Pod {
    Uint8 = 1,
    Uint64 = 7,
    Float32 = 10,
    Float64 = 11,
}

impl Pod {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Pod::Uint8 => "uint8_t",
            Pod::Uint64 => "uint64_t",
            Pod::Float32 => "float32_t",
            Pod::Float64 => "float64_t",
        }
    }

    pub(crate) fn byte_width(self) -> usize {
        match self {
            Pod::Uint8 => 1,
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
