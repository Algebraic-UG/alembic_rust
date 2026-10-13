const DIGEST_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Digest([u8; DIGEST_LEN]);

impl Digest {
    const PLACEHOLDER: Digest = Digest([0; DIGEST_LEN]);

    pub(crate) fn as_bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.0
    }
}

pub(crate) fn placeholder_sample_digest(_sample_bytes: &[u8]) -> Digest {
    Digest::PLACEHOLDER
}

pub(crate) struct ObjectHierarchyHashes {
    properties: Digest,
    children: Digest,
}

impl ObjectHierarchyHashes {
    pub(crate) fn placeholder() -> Self {
        ObjectHierarchyHashes {
            properties: Digest::PLACEHOLDER,
            children: Digest::PLACEHOLDER,
        }
    }

    pub(crate) fn encode(&self) -> [u8; 2 * DIGEST_LEN] {
        let mut encoded = [0; 2 * DIGEST_LEN];
        encoded[..DIGEST_LEN].copy_from_slice(self.properties.as_bytes());
        encoded[DIGEST_LEN..].copy_from_slice(self.children.as_bytes());
        encoded
    }
}
