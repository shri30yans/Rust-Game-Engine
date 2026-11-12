//! The values a game hands to a UI document each frame: numbers, flags and text, by name.
//!
//! Invariants: a name keeps one kind of value for its whole life (setting a name to another kind
//! replaces it). Updating an existing name allocates nothing; only the first `set_*` of a new name
//! does, so a game binds everything once at start-up and then only updates.

use std::collections::BTreeMap;

/// One bound value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A number (health, ammo, a bar fraction).
    Num(f32),
    /// A flag (a button is held, a panel is shown).
    Flag(bool),
    /// A piece of text.
    Text(String),
    /// A list of named records for choices and repeated templates.
    List(ListBinding),
}

/// One named row in a [`ListBinding`].
pub type ListRecord = BTreeMap<String, Value>;

/// Typed record rows bound to a document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListBinding {
    records: Vec<ListRecord>,
}

impl ListBinding {
    /// Creates an empty list with storage for up to `capacity` rows.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            records: Vec::with_capacity(capacity),
        }
    }
    /// Rows in document order.
    #[must_use]
    pub fn records(&self) -> &[ListRecord] {
        &self.records
    }
    /// Replaces the rows, retaining capacity when it is sufficient.
    pub fn replace(&mut self, records: &[ListRecord]) {
        self.records.clear();
        self.records.extend_from_slice(records);
    }
}

/// Named values a document reads.
#[derive(Clone, Debug, Default)]
pub struct Bindings {
    map: BTreeMap<String, BoundValue>,
    generation: u64,
}

#[derive(Clone, Debug)]
struct BoundValue {
    value: Value,
    generation: u64,
}

enum IncomingValue<'a> {
    Num(f32),
    Flag(bool),
    Text(&'a str),
    List(ListBinding),
}

impl Bindings {
    /// An empty set of bindings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `name` to a number.
    pub fn set_num(&mut self, name: &str, value: f32) {
        self.set(name, IncomingValue::Num(value));
    }

    /// Binds `name` to a flag.
    pub fn set_flag(&mut self, name: &str, value: bool) {
        self.set(name, IncomingValue::Flag(value));
    }

    /// Binds `name` to text, reusing the old string's storage when `name` already holds text.
    pub fn set_text(&mut self, name: &str, value: &str) {
        self.set(name, IncomingValue::Text(value));
    }

    /// Binds a typed list of records.
    pub fn set_list(&mut self, name: &str, value: ListBinding) {
        self.set(name, IncomingValue::List(value));
    }

    /// The value bound to `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.map.get(name).map(|bound| &bound.value)
    }

    /// The generation of a named value, or zero when the name has never been bound.
    #[must_use]
    pub(crate) fn generation(&self, name: &str) -> u64 {
        self.map.get(name).map_or(0, |bound| bound.generation)
    }

    /// The generation of the complete binding set.
    #[must_use]
    pub(crate) fn global_generation(&self) -> u64 {
        self.generation
    }

    fn set(&mut self, name: &str, value: IncomingValue<'_>) {
        if self
            .map
            .get(name)
            .is_some_and(|bound| match (&bound.value, &value) {
                (Value::Num(current), IncomingValue::Num(next)) => current == next,
                (Value::Flag(current), IncomingValue::Flag(next)) => current == next,
                (Value::Text(current), IncomingValue::Text(next)) => current == next,
                (Value::List(current), IncomingValue::List(next)) => current == next,
                _ => false,
            })
        {
            return;
        }
        self.bump_generation();
        let generation = self.generation;
        if let Some(bound) = self.map.get_mut(name) {
            match (&mut bound.value, value) {
                (Value::Text(current), IncomingValue::Text(next)) => {
                    current.clear();
                    current.push_str(next);
                }
                (slot, IncomingValue::Num(next)) => *slot = Value::Num(next),
                (slot, IncomingValue::Flag(next)) => *slot = Value::Flag(next),
                (slot, IncomingValue::Text(next)) => *slot = Value::Text(next.to_owned()),
                (slot, IncomingValue::List(next)) => *slot = Value::List(next),
            }
            bound.generation = generation;
        } else {
            let value = match value {
                IncomingValue::Num(number) => Value::Num(number),
                IncomingValue::Flag(flag) => Value::Flag(flag),
                IncomingValue::Text(text) => Value::Text(text.to_owned()),
                IncomingValue::List(list) => Value::List(list),
            };
            self.map
                .insert(name.to_owned(), BoundValue { value, generation });
        }
    }

    fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_name_holds_the_last_value_set() {
        let mut b = Bindings::new();
        b.set_num("ammo", 30.0);
        b.set_num("ammo", 29.0);
        assert_eq!(b.get("ammo"), Some(&Value::Num(29.0)));
    }

    #[test]
    fn setting_another_kind_replaces_the_value() {
        let mut b = Bindings::new();
        b.set_num("x", 1.0);
        b.set_flag("x", true);
        assert_eq!(b.get("x"), Some(&Value::Flag(true)));
        b.set_text("x", "hi");
        assert_eq!(b.get("x"), Some(&Value::Text("hi".to_owned())));
    }

    #[test]
    fn an_unbound_name_is_none() {
        assert_eq!(Bindings::new().get("nope"), None);
    }

    #[test]
    fn generations_change_only_when_a_value_changes() {
        let mut bindings = Bindings::new();
        assert_eq!(bindings.generation("score"), 0);
        assert_eq!(bindings.global_generation(), 0);
        bindings.set_num("score", 7.0);
        let first = bindings.generation("score");
        let global_first = bindings.global_generation();
        bindings.set_num("score", 7.0);
        assert_eq!(bindings.generation("score"), first);
        assert_eq!(bindings.global_generation(), global_first);
        bindings.set_num("score", 8.0);
        assert_ne!(bindings.generation("score"), first);
        assert_ne!(bindings.global_generation(), global_first);
        assert_eq!(bindings.generation("unread"), 0);
    }
}
