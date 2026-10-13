use std::collections::BTreeMap;

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

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries_sorted_by_key
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    pub(crate) fn deserialize(serialized: &str) -> MetaData {
        let entries_sorted_by_key = serialized
            .split(PAIR_SEPARATOR)
            .filter_map(|pair| pair.split_once(ASSIGNMENT_SEPARATOR))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        MetaData {
            entries_sorted_by_key,
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
}
