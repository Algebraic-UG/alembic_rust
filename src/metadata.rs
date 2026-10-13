use std::collections::{BTreeMap, HashMap};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaData {
    entries: BTreeMap<String, String>,
}

impl MetaData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> Result<()> {
        let (key, value) = (key.into(), value.into());
        check("key", &key)?;
        check("value", &value)?;
        self.entries.insert(key, value);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub(crate) fn serialize(&self) -> String {
        let mut out = String::new();
        for (key, value) in self.entries.iter().filter(|(_, v)| !v.is_empty()) {
            if !out.is_empty() {
                out.push(';');
            }
            out.push_str(key);
            out.push('=');
            out.push_str(value);
        }
        out
    }
}

fn check(what: &'static str, text: &str) -> Result<()> {
    if text.contains([';', '=']) {
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
    Indexed(u8),
    Inline,
}

impl MetaDataIndex {
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            MetaDataIndex::Empty => 0,
            MetaDataIndex::Indexed(i) => i,
            MetaDataIndex::Inline => 255,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct MetaDataMap {
    indices: HashMap<String, u8>,
    in_order: Vec<String>,
}

const MAX_ENTRIES: usize = 254;
const MAX_LEN: usize = 255;

impl MetaDataMap {
    pub(crate) fn index(&mut self, serialized: &str) -> MetaDataIndex {
        if serialized.is_empty() {
            return MetaDataIndex::Empty;
        }
        if serialized.len() > MAX_LEN {
            return MetaDataIndex::Inline;
        }
        if let Some(&i) = self.indices.get(serialized) {
            return MetaDataIndex::Indexed(i);
        }
        if self.in_order.len() < MAX_ENTRIES {
            let i = (self.in_order.len() + 1) as u8;
            self.indices.insert(serialized.to_owned(), i);
            self.in_order.push(serialized.to_owned());
            return MetaDataIndex::Indexed(i);
        }
        MetaDataIndex::Inline
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for entry in &self.in_order {
            out.push(entry.len() as u8);
            out.extend_from_slice(entry.as_bytes());
        }
        out
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
    fn rejects_separators() {
        let mut md = MetaData::new();
        assert!(md.set("a;b", "x").is_err());
        assert!(md.set("a", "x=y").is_err());
        assert!(md.get("a").is_none());
    }

    #[test]
    fn dictionary_indices() {
        let mut map = MetaDataMap::default();
        assert_eq!(map.index(""), MetaDataIndex::Empty);
        assert_eq!(map.index("a=1"), MetaDataIndex::Indexed(1));
        assert_eq!(map.index("b=2"), MetaDataIndex::Indexed(2));
        assert_eq!(map.index("a=1"), MetaDataIndex::Indexed(1));
        assert_eq!(map.index(&"x".repeat(256)), MetaDataIndex::Inline);
        assert_eq!(map.index(&"x".repeat(255)), MetaDataIndex::Indexed(3));
        assert_eq!(map.encode()[..8], *b"\x03a=1\x03b=2");
    }

    #[test]
    fn dictionary_fills_up() {
        let mut map = MetaDataMap::default();
        for i in 0..254 {
            assert_eq!(
                map.index(&format!("k={i}")),
                MetaDataIndex::Indexed(i as u8 + 1)
            );
        }
        assert_eq!(map.index("k=new"), MetaDataIndex::Inline);
        assert_eq!(map.index("k=0"), MetaDataIndex::Indexed(1));
    }
}
