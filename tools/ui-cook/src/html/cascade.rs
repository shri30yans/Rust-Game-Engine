//! Selector matching and the cascade: which declarations apply to one element, and in what order.
//! Invariant: a later declaration wins on a tie, and an inline `style` attribute wins over rules.

use std::collections::BTreeMap;

use crate::css::{Declaration, Rule, Selector, WidgetState, parse_declarations};
use crate::error::CookError;

use super::props::PropMap;

fn matches(selector: &Selector, tag: &str, id: Option<&str>, classes: &[&str]) -> bool {
    match selector {
        Selector::Universal => true,
        Selector::Tag(t) => t == tag,
        Selector::Class(c) => classes.contains(&c.as_str()),
        Selector::Id(i) => id == Some(i.as_str()),
        Selector::State { base, .. } => matches(base, tag, id, classes),
    }
}

/// The declarations that apply to an element, keyed by property name. Widget-state declarations
/// keep a `state-<name>-` prefix so they can be told apart from the base style.
pub(super) fn cascade(
    rules: &[Rule],
    tag: &str,
    attrs: &BTreeMap<String, String>,
) -> Result<PropMap, CookError> {
    let id = attrs.get("id").map(String::as_str);
    let classes: Vec<&str> = attrs
        .get("class")
        .map(|c| c.split_whitespace().collect())
        .unwrap_or_default();
    let mut applied: Vec<(u8, Option<String>, &Declaration)> = Vec::new();
    for rule in rules {
        let best = rule
            .selectors
            .iter()
            .filter(|s| matches(s, tag, id, &classes))
            .map(Selector::specificity)
            .max();
        if let Some(weight) = best {
            for selector in &rule.selectors {
                if !matches(selector, tag, id, &classes) {
                    continue;
                }
                let prefix = match selector {
                    Selector::State { state, .. } => Some(match state {
                        WidgetState::Hover => "state-hover-",
                        WidgetState::FocusVisible => "state-focus-",
                        WidgetState::Active => "state-pressed-",
                        WidgetState::Disabled => "state-disabled-",
                    }),
                    _ => None,
                };
                for declaration in &rule.declarations {
                    applied.push((weight, prefix.map(|prefix| prefix.to_owned()), declaration));
                }
            }
        }
    }
    // Stable sort keeps source order among equal weights.
    applied.sort_by_key(|(weight, _, _)| *weight);
    let mut props = PropMap::new();
    for (_, prefix, d) in applied {
        let name = prefix.map_or_else(|| d.name.clone(), |prefix| format!("{prefix}{}", d.name));
        props.insert(name, d.values.clone());
    }
    if let Some(style) = attrs.get("style") {
        for d in parse_declarations(style)? {
            props.insert(d.name, d.values);
        }
    }
    Ok(props)
}
