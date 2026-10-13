use std::collections::HashSet;

use crate::error::{Error, Result};

const PATH_SEPARATOR: char = '/';

#[derive(Debug, Default)]
pub(crate) struct SiblingNames {
    taken: HashSet<String>,
}

impl SiblingNames {
    pub(crate) fn claim(&mut self, name: &str) -> Result<()> {
        if name.is_empty() || name.contains(PATH_SEPARATOR) {
            return Err(Error::InvalidName {
                name: name.to_owned(),
            });
        }
        if !self.taken.insert(name.to_owned()) {
            return Err(Error::DuplicateName {
                name: name.to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_slashed_and_duplicate_names() {
        let mut names = SiblingNames::default();
        assert!(matches!(names.claim(""), Err(Error::InvalidName { .. })));
        assert!(matches!(names.claim("a/b"), Err(Error::InvalidName { .. })));
        names.claim(".geom").unwrap();
        names.claim("P").unwrap();
        assert!(matches!(names.claim("P"), Err(Error::DuplicateName { .. })));
    }
}
