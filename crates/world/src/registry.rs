//! Lookups by string id.
//!
//! The pack file registers shapes, models, and blocks here. The world and the
//! mesher ask the registry. An id that was never registered is absent.

/// Values stored under string ids, in the order they were registered.
#[derive(Clone, Debug)]
pub struct Registry<T> {
    ids: Vec<String>,
    values: Vec<T>,
}

impl<T> Registry<T> {
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Insert `id`. A second registration of the same id is an error.
    pub fn register(&mut self, id: impl Into<String>, value: T) -> Result<(), String> {
        let id = id.into();
        if self.contains(&id) {
            return Err(format!("duplicate id {id}"));
        }
        self.ids.push(id);
        self.values.push(value);
        Ok(())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|existing| existing == id)
    }

    pub fn get(&self, id: &str) -> Option<&T> {
        self.ids
            .iter()
            .position(|existing| existing == id)
            .map(|index| &self.values[index])
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut T> {
        self.ids
            .iter()
            .position(|existing| existing == id)
            .map(|index| &mut self.values[index])
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.ids.iter().map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.ids.iter().map(String::as_str).zip(self.values.iter())
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_look_up_by_id() {
        let mut shapes = Registry::new();
        shapes.register("cube", 1).unwrap();
        shapes.register("stair_north", 2).unwrap();
        assert_eq!(shapes.get("cube"), Some(&1));
        assert_eq!(shapes.get("stair_north"), Some(&2));
        assert!(shapes.get("missing").is_none());
        assert!(shapes.register("cube", 3).is_err());
        assert_eq!(shapes.ids().collect::<Vec<_>>(), ["cube", "stair_north"]);
    }
}
