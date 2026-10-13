use std::collections::HashMap;

use crate::format::layout::{EMPTY_METADATA_INDEX, INLINE_METADATA_INDEX};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetaDataIndex {
    Empty,
    Dictionary(u8),
    Inline,
}

const DICTIONARY_CAPACITY: usize = 254;
const LONGEST_DICTIONARY_ENTRY: usize = 255;

impl MetaDataIndex {
    pub(crate) fn encoded(self) -> u8 {
        match self {
            MetaDataIndex::Empty => EMPTY_METADATA_INDEX,
            MetaDataIndex::Dictionary(index) => index,
            MetaDataIndex::Inline => INLINE_METADATA_INDEX,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct MetaDataDictionary {
    index_by_entry: HashMap<String, u8>,
    entries_in_index_order: Vec<String>,
}

impl MetaDataDictionary {
    pub(crate) fn index_of(&mut self, serialized: &str) -> MetaDataIndex {
        if serialized.is_empty() {
            return MetaDataIndex::Empty;
        }
        if serialized.len() > LONGEST_DICTIONARY_ENTRY {
            return MetaDataIndex::Inline;
        }
        if let Some(&index) = self.index_by_entry.get(serialized) {
            return MetaDataIndex::Dictionary(index);
        }
        if self.entries_in_index_order.len() == DICTIONARY_CAPACITY {
            return MetaDataIndex::Inline;
        }
        let first_dictionary_index = EMPTY_METADATA_INDEX + 1;
        let index = first_dictionary_index + self.entries_in_index_order.len() as u8;
        self.index_by_entry.insert(serialized.to_owned(), index);
        self.entries_in_index_order.push(serialized.to_owned());
        MetaDataIndex::Dictionary(index)
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        for entry in &self.entries_in_index_order {
            encoded.push(entry.len() as u8);
            encoded.extend_from_slice(entry.as_bytes());
        }
        encoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_dictionary_indices() {
        let mut dict = MetaDataDictionary::default();
        assert_eq!(dict.index_of(""), MetaDataIndex::Empty);
        assert_eq!(dict.index_of("a=1"), MetaDataIndex::Dictionary(1));
        assert_eq!(dict.index_of("b=2"), MetaDataIndex::Dictionary(2));
        assert_eq!(dict.index_of("a=1"), MetaDataIndex::Dictionary(1));
        assert_eq!(dict.index_of(&"x".repeat(256)), MetaDataIndex::Inline);
        assert_eq!(
            dict.index_of(&"x".repeat(255)),
            MetaDataIndex::Dictionary(3)
        );
        assert_eq!(dict.encode()[..8], *b"\x03a=1\x03b=2");
    }

    #[test]
    fn full_dictionary_falls_back_to_inline() {
        let mut dict = MetaDataDictionary::default();
        for i in 0..254 {
            assert_eq!(
                dict.index_of(&format!("k={i}")),
                MetaDataIndex::Dictionary(i as u8 + 1)
            );
        }
        assert_eq!(dict.index_of("k=new"), MetaDataIndex::Inline);
        assert_eq!(dict.index_of("k=0"), MetaDataIndex::Dictionary(1));
        assert_eq!(MetaDataIndex::Inline.encoded(), 255);
    }
}
