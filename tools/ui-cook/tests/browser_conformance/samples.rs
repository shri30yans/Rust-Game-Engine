//! Builds the same sample bindings for engine layout and the browser fixture.

use std::collections::{BTreeMap, BTreeSet};

use engine_ui::doc::{Content, Node};
use engine_ui::{Bindings, ListBinding, ListRecord, MetricsKey, Template, TextMeasure, Value};
use serde::Deserialize;
use serde_json::{Map, Value as Json, json};

#[derive(Debug, Deserialize)]
pub(super) struct ScreenSample {
    rows: usize,
    options: usize,
    text: String,
    number: f32,
    flag: bool,
    #[serde(default)]
    values: BTreeMap<String, SampleValue>,
}

#[derive(Clone, Debug, Deserialize)]
enum SampleValue {
    Num(f32),
    Flag(bool),
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Num,
    Flag,
    Text,
    NumOrText,
}

#[derive(Default)]
struct ListNeed {
    repeat_max: Option<usize>,
    fields: BTreeMap<String, Kind>,
}

#[derive(Default)]
struct Requirements {
    globals: BTreeMap<String, Kind>,
    numbers: BTreeMap<String, f32>,
    lists: BTreeMap<String, ListNeed>,
}

pub(super) struct Mono;

impl TextMeasure for Mono {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(0)
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.5 * text.chars().count() as f32
    }
}

pub(super) fn sample_bindings(root: &Node, sample: &ScreenSample) -> (Bindings, Json) {
    let mut requirements = Requirements::default();
    collect_requirements(root, None, &mut requirements);
    let mut bindings = Bindings::new();
    for (name, kind) in &requirements.globals {
        let value = sample
            .values
            .get(name)
            .cloned()
            .unwrap_or_else(|| match kind {
                Kind::Num => SampleValue::Num(
                    requirements
                        .numbers
                        .get(name)
                        .copied()
                        .unwrap_or(sample.number),
                ),
                Kind::Flag => SampleValue::Flag(sample.flag),
                Kind::Text | Kind::NumOrText => SampleValue::Text(sample.text.clone()),
            });
        match (kind, value) {
            (Kind::Num, SampleValue::Num(value)) => bindings.set_num(name, value),
            (Kind::Flag, SampleValue::Flag(value)) => bindings.set_flag(name, value),
            (Kind::Text, SampleValue::Text(value)) => bindings.set_text(name, &value),
            (Kind::NumOrText, SampleValue::Num(value)) => bindings.set_num(name, value),
            (Kind::NumOrText, SampleValue::Text(value)) => bindings.set_text(name, &value),
            _ => panic!("sample binding {name} has the wrong type"),
        }
    }
    let unused = sample
        .values
        .keys()
        .filter(|name| !requirements.globals.contains_key(*name))
        .collect::<Vec<_>>();
    assert!(unused.is_empty(), "sample has unused bindings: {unused:?}");

    for (name, need) in &requirements.lists {
        let count = need
            .repeat_max
            .map_or(sample.options, |max| sample.rows.min(max));
        let records = (0..count)
            .map(|index| {
                let mut record = ListRecord::new();
                for (field, kind) in &need.fields {
                    let value = match kind {
                        Kind::Num => Value::Num(index as f32 + sample.number),
                        Kind::Flag => Value::Flag(sample.flag),
                        Kind::Text | Kind::NumOrText => {
                            Value::Text(format!("{} {}", sample.text, index + 1))
                        }
                    };
                    record.insert(field.clone(), value);
                }
                if need.repeat_max.is_none() {
                    let label = format!("{} {}", sample.text, index + 1);
                    record.insert("label".to_owned(), Value::Text(label.clone()));
                    record.insert("name".to_owned(), Value::Text(label));
                }
                record
            })
            .collect::<Vec<_>>();
        let mut list = ListBinding::with_capacity(count);
        list.replace(&records);
        bindings.set_list(name, list);
    }
    let browser_sample = sample_json(&bindings, &requirements);
    (bindings, browser_sample)
}

fn collect_requirements(
    node: &Node,
    row: Option<&mut BTreeMap<String, Kind>>,
    requirements: &mut Requirements,
) {
    let mut row = row;
    if let Some(condition) = &node.show_if {
        require(
            condition.trim_start_matches('!'),
            Kind::Flag,
            row.as_deref_mut(),
            requirements,
        );
    }
    match &node.content {
        Content::Text { text, .. } => require_template(text, row.as_deref_mut(), requirements),
        Content::Bar { value, .. } => require(value, Kind::Num, row.as_deref_mut(), requirements),
        Content::Button { held, .. } => require(held, Kind::Flag, row.as_deref_mut(), requirements),
        Content::Action {
            label, disabled, ..
        } => {
            require_template(label, row.as_deref_mut(), requirements);
            require_optional_flag(disabled, row.as_deref_mut(), requirements);
        }
        Content::Toggle {
            label,
            value,
            disabled,
            ..
        } => {
            require_template(label, row.as_deref_mut(), requirements);
            require(value, Kind::Flag, row.as_deref_mut(), requirements);
            require_optional_flag(disabled, row.as_deref_mut(), requirements);
        }
        Content::Slider {
            label,
            value,
            min,
            max,
            disabled,
            ..
        } => {
            require_template(label, row.as_deref_mut(), requirements);
            require(value, Kind::Num, row.as_deref_mut(), requirements);
            requirements
                .numbers
                .insert(value.clone(), (min + max) * 0.5);
            require_optional_flag(disabled, row.as_deref_mut(), requirements);
        }
        Content::Choice {
            label,
            options,
            selected,
            disabled,
            ..
        } => {
            require_template(label, row.as_deref_mut(), requirements);
            require(selected, Kind::Num, row.as_deref_mut(), requirements);
            requirements.numbers.insert(selected.clone(), 0.0);
            require_optional_flag(disabled, row.as_deref_mut(), requirements);
            require_options(options, requirements);
        }
        Content::Tabs {
            tabs,
            selected,
            disabled,
            ..
        } => {
            require(selected, Kind::Num, row.as_deref_mut(), requirements);
            requirements.numbers.insert(selected.clone(), 0.0);
            require_optional_flag(disabled, row.as_deref_mut(), requirements);
            require_options(tabs, requirements);
        }
        Content::Repeat {
            source,
            max,
            template,
        } => {
            let mut fields = BTreeMap::new();
            collect_requirements(template, Some(&mut fields), requirements);
            requirements.lists.insert(
                source.clone(),
                ListNeed {
                    repeat_max: Some(usize::from(*max)),
                    fields,
                },
            );
        }
        Content::None => {}
    }
    for child in &node.children {
        collect_requirements(child, row.as_deref_mut(), requirements);
    }
}

fn require_options(name: &str, requirements: &mut Requirements) {
    requirements
        .lists
        .entry(name.to_owned())
        .or_insert_with(|| ListNeed {
            repeat_max: None,
            fields: BTreeMap::from([
                ("label".to_owned(), Kind::Text),
                ("name".to_owned(), Kind::Text),
            ]),
        });
}

fn require_optional_flag(
    name: &Option<String>,
    row: Option<&mut BTreeMap<String, Kind>>,
    requirements: &mut Requirements,
) {
    if let Some(name) = name {
        require(name, Kind::Flag, row, requirements);
    }
}

fn require_template(
    template: &Template,
    mut row: Option<&mut BTreeMap<String, Kind>>,
    requirements: &mut Requirements,
) {
    let source = template.source();
    for name in template.variables() {
        let numeric = source.contains(&format!("{{{name}:"));
        require(
            name,
            if numeric { Kind::Num } else { Kind::NumOrText },
            row.as_deref_mut(),
            requirements,
        );
    }
}

fn require(
    name: &str,
    kind: Kind,
    row: Option<&mut BTreeMap<String, Kind>>,
    requirements: &mut Requirements,
) {
    if let Some(field) = name.strip_prefix("item.") {
        let fields = row.expect("row binding only appears inside a repeated template");
        if let Some(previous) = fields.get_mut(field) {
            *previous = merge_kind(*previous, kind, field);
        } else {
            fields.insert(field.to_owned(), kind);
        }
    } else if let Some(previous) = requirements.globals.get_mut(name) {
        *previous = merge_kind(*previous, kind, name);
    } else {
        requirements.globals.insert(name.to_owned(), kind);
    }
}

fn merge_kind(previous: Kind, next: Kind, name: &str) -> Kind {
    match (previous, next) {
        (Kind::NumOrText, other) | (other, Kind::NumOrText) => other,
        (left, right) => {
            assert_eq!(left, right, "binding {name} has conflicting types");
            left
        }
    }
}

pub(super) fn repeat_sources(root: &Node) -> BTreeSet<String> {
    let mut sources = BTreeSet::new();
    collect_repeat_sources(root, &mut sources);
    sources
}

fn collect_repeat_sources(node: &Node, out: &mut BTreeSet<String>) {
    if let Content::Repeat { template, .. } = &node.content {
        if let Some(source) = &node.source {
            out.insert(source.clone());
        }
        collect_repeat_sources(template, out);
    }
    for child in &node.children {
        collect_repeat_sources(child, out);
    }
}

fn sample_json(bindings: &Bindings, requirements: &Requirements) -> Json {
    let names = requirements
        .globals
        .keys()
        .chain(requirements.lists.keys())
        .cloned()
        .collect::<Vec<_>>();
    let values = names
        .into_iter()
        .filter_map(|name| bindings.get(&name).map(|value| (name, value_json(value))))
        .collect::<Map<_, _>>();
    Json::Object(values)
}

fn value_json(value: &Value) -> Json {
    match value {
        Value::Num(number) => json!(number),
        Value::Flag(flag) => json!(flag),
        Value::Text(text) => json!(text),
        Value::List(list) => Json::Array(
            list.records()
                .iter()
                .map(|record| {
                    Json::Object(
                        record
                            .iter()
                            .map(|(name, value)| (name.clone(), value_json(value)))
                            .collect(),
                    )
                })
                .collect(),
        ),
    }
}

pub(super) fn is_text_source(source: &str) -> bool {
    let segment = source.rsplit('>').next().unwrap_or(source);
    segment.starts_with("p.")
        || segment.starts_with("p:")
        || segment.starts_with("span.")
        || segment.starts_with("span:")
}
