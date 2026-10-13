use std::collections::{BTreeMap, HashMap};

use crate::error::{Error, Result};

const PAIR_SEPARATOR: char = ';';
const ASSIGNMENT_SEPARATOR: char = '=';

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaData {
    entries_sorted_by_key: BTreeMap<String, String>,
}

impl MetaData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> Result<()> {
        let (key, value) = (key.into(), value.into());
        reject_separators("key", &key)?;
        reject_separators("value", &value)?;
        self.entries_sorted_by_key.insert(key, value);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries_sorted_by_key.get(key).map(String::as_str)
    }

    pub(crate) fn deserialize(serialized: &str) -> MetaData {
        let mut metadata = MetaData::new();
        let mut rest = serialized;
        loop {
            let pair_end = rest.find(PAIR_SEPARATOR);
            let assignment = rest.find(ASSIGNMENT_SEPARATOR);
            let assignment_belongs_to_a_later_pair = match (assignment, pair_end) {
                (Some(assignment), Some(pair_end)) => assignment > pair_end,
                (None, Some(_)) => true,
                _ => false,
            };
            if assignment_belongs_to_a_later_pair {
                return metadata;
            }
            if let Some(assignment) = assignment {
                let key = &rest[..assignment];
                let value = &rest[assignment + 1..pair_end.unwrap_or(rest.len())];
                metadata
                    .entries_sorted_by_key
                    .entry(key.to_owned())
                    .or_insert_with(|| value.to_owned());
            }
            match pair_end {
                Some(pair_end) => rest = &rest[pair_end + 1..],
                None => return metadata,
            }
        }
    }

    pub(crate) fn serialize(&self) -> String {
        let mut serialized = String::new();
        let non_empty_entries = self
            .entries_sorted_by_key
            .iter()
            .filter(|(_, value)| !value.is_empty());
        for (key, value) in non_empty_entries {
            if !serialized.is_empty() {
                serialized.push(PAIR_SEPARATOR);
            }
            serialized.push_str(key);
            serialized.push(ASSIGNMENT_SEPARATOR);
            serialized.push_str(value);
        }
        serialized
    }
}

fn reject_separators(what: &'static str, text: &str) -> Result<()> {
    if text.contains([PAIR_SEPARATOR, ASSIGNMENT_SEPARATOR]) {
        return Err(Error::InvalidMetaData {
            what,
            text: text.to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetaDataIndex {
    Empty,
    Dictionary(u8),
    Inline,
}

const EMPTY_INDEX: u8 = 0;
pub(crate) const INLINE_INDEX: u8 = 255;
const DICTIONARY_CAPACITY: usize = 254;
const LONGEST_DICTIONARY_ENTRY: usize = 255;

impl MetaDataIndex {
    pub(crate) fn encoded(self) -> u8 {
        match self {
            MetaDataIndex::Empty => EMPTY_INDEX,
            MetaDataIndex::Dictionary(index) => index,
            MetaDataIndex::Inline => INLINE_INDEX,
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
        let first_dictionary_index = EMPTY_INDEX + 1;
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
    fn serializes_sorted_and_skips_empty_values() {
        let mut md = MetaData::new();
        md.set("schema", "AbcGeom_Points_v1").unwrap();
        md.set("empty", "").unwrap();
        md.set("geoScope", "var").unwrap();
        assert_eq!(md.serialize(), "geoScope=var;schema=AbcGeom_Points_v1");
        assert_eq!(MetaData::new().serialize(), "");
    }

    #[test]
    fn deserializes_like_upstream() {
        let md = MetaData::deserialize("b=2;a=1;eq=x=y;empty=;b=dup");
        assert_eq!(md.get("a"), Some("1"));
        assert_eq!(md.get("b"), Some("2"));
        assert_eq!(md.get("eq"), Some("x=y"));
        assert_eq!(md.get("empty"), Some(""));
        assert_eq!(MetaData::deserialize(""), MetaData::new());
        assert_eq!(MetaData::deserialize("a=1;"), MetaData::deserialize("a=1"));
    }

    #[test]
    fn deserializing_stops_at_a_pair_without_assignment() {
        let md = MetaData::deserialize("a=1;broken;c=3");
        assert_eq!(md.get("a"), Some("1"));
        assert_eq!(md.get("c"), None);
        assert_eq!(MetaData::deserialize("broken"), MetaData::new());
    }

    #[test]
    fn deserialize_reverses_serialize() {
        let mut md = MetaData::new();
        md.set("schema", "AbcGeom_Points_v1").unwrap();
        md.set("geoScope", "var").unwrap();
        assert_eq!(MetaData::deserialize(&md.serialize()), md);
    }

    #[test]
    fn rejects_separators() {
        let mut md = MetaData::new();
        assert!(md.set("a;b", "x").is_err());
        assert!(md.set("a", "x=y").is_err());
        assert!(md.get("a").is_none());
    }

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
