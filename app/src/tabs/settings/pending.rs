use std::collections::{BTreeMap, BTreeSet};

use rhebok::config::check::FieldError;
use rhebok::dashboard::core::config::{ConfigCheck, ConfigSave};
use serde_json::{Map, Value as Json};
use toml::Value;

#[derive(Debug, Clone)]
pub struct ConfigState {
    view: Json,
}

impl ConfigState {
    pub fn new(view: Json) -> ConfigState {
        ConfigState { view }
    }

    pub fn view(&self) -> &Json {
        &self.view
    }

    pub fn path(&self) -> &str {
        self.view["path"].as_str().unwrap_or_default()
    }

    pub fn exists(&self) -> bool {
        self.view["exists"].as_bool().unwrap_or(false)
    }

    pub fn etag(&self) -> &str {
        self.view["etag"].as_str().unwrap_or_default()
    }

    pub fn file(&self) -> &Json {
        &self.view["file"]
    }

    pub fn effective(&self) -> &Json {
        &self.view["effective"]
    }

    pub fn defaults(&self) -> &Json {
        &self.view["defaults"]
    }

    pub fn unparseable(&self) -> bool {
        self.file().is_null()
    }

    pub fn errors(&self) -> Vec<FieldError> {
        errors_from(&self.view["errors"])
    }

    pub fn in_file(&self, path: &str) -> bool {
        in_file(self.file(), path)
    }

    pub fn default_of(&self, path: &str) -> Option<&Json> {
        at(self.defaults(), path)
    }

    pub fn override_for(&self, setting: &str) -> Option<String> {
        self.view["env_overrides"]
            .as_array()?
            .iter()
            .find(|o| o["setting"] == setting)
            .and_then(|o| o["variable"].as_str())
            .map(str::to_string)
    }

    pub fn secret_set(&self, name: &str) -> bool {
        self.view["secrets"][name] == "set"
    }

    pub fn endpoint(&self, field: &str) -> Option<String> {
        self.view["endpoint"][field].as_str().map(str::to_string)
    }

    pub fn remote(&self) -> bool {
        self.view["endpoint"]["backend"] == "remote"
    }

    pub fn surface_weight(&self, surface: &str, name: &str) -> Option<&Json> {
        self.view["surface_weights"][surface]
            .get(name)
            .filter(|v| !v.is_null())
    }

    pub fn embed_models(&self) -> Vec<Json> {
        self.view["embed_models"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn original(&self, path: &str) -> Option<&Json> {
        if self.unparseable() {
            return at(self.effective(), path);
        }
        at(self.file(), path).or_else(|| at(self.defaults(), path))
    }
}

pub fn errors_from(value: &Json) -> Vec<FieldError> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|e| FieldError {
                    path: e["path"].as_str().unwrap_or_default().to_string(),
                    message: e["message"].as_str().unwrap_or_default().to_string(),
                    line: e["line"].as_u64().map(|l| l as usize),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn at<'a>(root: &'a Json, path: &str) -> Option<&'a Json> {
    let mut node = root;
    for segment in path.split('.') {
        node = node.as_object()?.get(segment)?;
    }
    Some(node)
}

pub fn in_file(file: &Json, path: &str) -> bool {
    !file.is_null() && at(file, path).is_some()
}

pub fn js_number(n: &serde_json::Number) -> String {
    if n.is_i64() || n.is_u64() {
        return n.to_string();
    }
    let f = n.as_f64().unwrap_or(0.);
    let magnitude = f.abs();
    if magnitude != 0. && !(1e-6..1e21).contains(&magnitude) {
        let exponent = format!("{f:e}");
        return match exponent.split_once('e') {
            Some((mantissa, power)) if !power.starts_with('-') => {
                format!("{mantissa}e+{power}")
            }
            _ => exponent,
        };
    }
    if f == f.trunc() {
        return format!("{}", f as i128);
    }
    format!("{f}")
}

pub fn js_stringify(value: &Json) -> String {
    match value {
        Json::Null => "null".to_string(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => js_number(n),
        Json::String(s) => serde_json::to_string(s).unwrap_or_default(),
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(js_stringify).collect::<Vec<_>>().join(",")
        ),
        Json::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!(
                    "{}:{}",
                    serde_json::to_string(k).unwrap_or_default(),
                    js_stringify(v)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

pub fn describe(value: Option<&Json>) -> String {
    match value {
        None => "unset".to_string(),
        Some(Json::Null) => "none".to_string(),
        Some(v) => js_stringify(v),
    }
}

pub fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => x.as_f64() == y.as_f64(),
        (Json::Array(x), Json::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Json::Object(x), Json::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|other| same(v, other)))
        }
        _ => a == b,
    }
}

pub fn toml_to_json(value: &Value) -> Json {
    match value {
        Value::String(s) => Json::String(s.clone()),
        Value::Integer(i) => Json::from(*i),
        Value::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        Value::Boolean(b) => Json::Bool(*b),
        Value::Datetime(d) => Json::String(d.to_string()),
        Value::Array(items) => Json::Array(items.iter().map(toml_to_json).collect()),
        Value::Table(table) => Json::Object(
            table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect(),
        ),
    }
}

pub fn json_to_toml(value: &Json) -> Option<Value> {
    Some(match value {
        Json::Null => return None,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Float(n.as_f64()?),
        },
        Json::String(s) => Value::String(s.clone()),
        Json::Array(items) => Value::Array(items.iter().filter_map(json_to_toml).collect()),
        Json::Object(map) => Value::Table(
            map.iter()
                .filter_map(|(k, v)| Some((k.clone(), json_to_toml(v)?)))
                .collect(),
        ),
    })
}

const SAFE_INTEGER: f64 = 9_007_199_254_740_991.;

fn js_float(f: f64) -> Json {
    if f.is_finite() && f == f.trunc() && f.abs() <= SAFE_INTEGER {
        return Json::from(f as i64);
    }
    serde_json::Number::from_f64(f).map_or(Json::Null, Json::Number)
}

fn edit_value_to_json(value: &toml_edit::Value) -> Json {
    match value {
        toml_edit::Value::String(s) => Json::String(s.value().clone()),
        toml_edit::Value::Integer(i) => Json::from(*i.value()),
        toml_edit::Value::Float(f) => js_float(*f.value()),
        toml_edit::Value::Boolean(b) => Json::Bool(*b.value()),
        toml_edit::Value::Datetime(d) => Json::String(d.value().to_string()),
        toml_edit::Value::Array(items) => {
            Json::Array(items.iter().map(edit_value_to_json).collect())
        }
        toml_edit::Value::InlineTable(table) => Json::Object(
            table
                .iter()
                .map(|(k, v)| (k.to_string(), edit_value_to_json(v)))
                .collect(),
        ),
    }
}

fn edit_item_to_json(item: &toml_edit::Item) -> Json {
    match item {
        toml_edit::Item::None => Json::Null,
        toml_edit::Item::Value(value) => edit_value_to_json(value),
        toml_edit::Item::Table(table) => Json::Object(
            table
                .iter()
                .map(|(k, v)| (k.to_string(), edit_item_to_json(v)))
                .collect(),
        ),
        toml_edit::Item::ArrayOfTables(tables) => Json::Array(
            tables
                .iter()
                .map(|t| {
                    Json::Object(
                        t.iter()
                            .map(|(k, v)| (k.to_string(), edit_item_to_json(v)))
                            .collect(),
                    )
                })
                .collect(),
        ),
    }
}

pub fn array_index(key: &str) -> Option<u32> {
    let n: u32 = key.parse().ok()?;
    (n != u32::MAX && n.to_string() == key).then_some(n)
}

pub fn js_order<T>(entries: &mut [(String, T)]) {
    entries.sort_by_key(|(k, _)| array_index(k).map_or((1, 0), |n| (0, n)));
}

fn json_diff(before: &Json, after: &Json, prefix: &str) -> Vec<(String, Option<Json>)> {
    let join = |key: &str| {
        if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        }
    };
    let empty = Map::new();
    let old = before.as_object().unwrap_or(&empty);
    let new = after.as_object().unwrap_or(&empty);
    let mut entries: Vec<(String, &Json)> = new.iter().map(|(k, v)| (k.clone(), v)).collect();
    js_order(&mut entries);
    let mut out = Vec::new();
    for (key, value) in entries {
        let key = key.as_str();
        let path = join(key);
        match old.get(key) {
            Some(previous) if value.is_object() && previous.is_object() => {
                out.extend(json_diff(previous, value, &path))
            }
            Some(previous) if same(value, previous) => {}
            _ => out.push((path, Some(value.clone()))),
        }
    }
    let mut removed: Vec<(String, ())> = old
        .keys()
        .filter(|k| !new.contains_key(*k))
        .map(|k| (k.clone(), ()))
        .collect();
    js_order(&mut removed);
    out.extend(removed.into_iter().map(|(k, _)| (join(&k), None)));
    out
}

pub fn errors_for<'a>(errors: &'a [FieldError], path: &str) -> Vec<&'a FieldError> {
    let nested = format!("{path}.");
    errors
        .iter()
        .filter(|e| e.path == path || e.path.starts_with(&nested))
        .collect()
}

pub fn merge(a: &[FieldError], b: &[FieldError]) -> Vec<FieldError> {
    let mut out = a.to_vec();
    out.extend(
        b.iter()
            .filter(|e| !a.iter().any(|x| x.same_problem(e)))
            .cloned(),
    );
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffRow {
    pub path: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pending {
    pub set: BTreeMap<String, Value>,
    pub unset: BTreeSet<String>,
    order: Vec<String>,
}

impl Pending {
    pub fn len(&self) -> usize {
        self.paths().len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.unset.is_empty()
    }

    pub fn contains(&self, path: &str) -> bool {
        self.set.contains_key(path) || self.unset.contains(path)
    }

    pub fn paths(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .order
            .iter()
            .filter(|p| self.contains(p))
            .cloned()
            .collect();
        for path in self.set.keys().chain(self.unset.iter()) {
            if !out.contains(path) {
                out.push(path.clone());
            }
        }
        out
    }

    pub fn get(&self, path: &str) -> Option<Option<&Value>> {
        if let Some(value) = self.set.get(path) {
            return Some(Some(value));
        }
        self.unset.contains(path).then_some(None)
    }

    fn touch(&mut self, path: &str) {
        if !self.order.iter().any(|p| p == path) {
            self.order.push(path.to_string());
        }
    }

    fn remove(&mut self, path: &str) {
        self.set.remove(path);
        self.unset.remove(path);
        self.order.retain(|p| p != path);
    }

    pub fn put(&mut self, path: &str, value: Option<Value>) {
        self.touch(path);
        match value {
            Some(value) => {
                self.unset.remove(path);
                self.set.insert(path.to_string(), value);
            }
            None => {
                self.set.remove(path);
                self.unset.insert(path.to_string());
            }
        }
    }

    pub fn from_document(file: &Json, document: &toml_edit::DocumentMut) -> Pending {
        let after = edit_item_to_json(document.as_item());
        let mut out = Pending::default();
        for (path, value) in json_diff(file, &after, "") {
            out.put(&path, value.as_ref().and_then(json_to_toml));
        }
        out
    }

    pub fn with(&self, path: &str, value: Value) -> Pending {
        let mut next = self.clone();
        next.put(path, Some(value));
        next
    }

    pub fn change(&mut self, state: &ConfigState, path: &str, value: Option<Value>) {
        match value {
            None if state.in_file(path) => self.put(path, None),
            None => self.remove(path),
            Some(value) => {
                let unchanged = state.in_file(path)
                    && at(state.file(), path).is_some_and(|old| same(&toml_to_json(&value), old));
                if unchanged {
                    self.remove(path);
                } else {
                    self.put(path, Some(value));
                }
            }
        }
    }

    pub fn value_of(&self, state: &ConfigState, path: &str) -> Option<Json> {
        match self.get(path) {
            Some(Some(value)) => Some(toml_to_json(value)),
            Some(None) => state.default_of(path).cloned(),
            None => state.original(path).cloned(),
        }
    }

    pub fn is_default(&self, state: &ConfigState, path: &str) -> bool {
        match self.get(path) {
            Some(value) => value.is_none(),
            None => !state.in_file(path),
        }
    }

    pub fn needs_reindex(&self) -> bool {
        self.contains("embed.model") || self.contains("embed.dimensions")
    }

    fn patch(&self) -> (Map<String, Json>, Vec<String>) {
        let mut set = Map::new();
        let mut unset = Vec::new();
        for path in &self.paths() {
            match self.get(path) {
                Some(Some(value)) => {
                    set.insert(path.clone(), toml_to_json(value));
                }
                Some(None) => unset.push(path.clone()),
                None => {}
            }
        }
        (set, unset)
    }

    pub fn to_check(&self) -> ConfigCheck {
        let (set, unset) = self.patch();
        ConfigCheck { set, unset }
    }

    pub fn to_save(&self, etag: &str) -> ConfigSave {
        let (set, unset) = self.patch();
        ConfigSave {
            etag: etag.to_string(),
            set,
            unset,
        }
    }

    pub fn diff(&self, file: &Json, defaults: &Json) -> Vec<DiffRow> {
        let default_text = |path: &str| format!("default {}", describe(at(defaults, path)));
        self.paths()
            .iter()
            .map(|path| {
                let old = if in_file(file, path) {
                    describe(at(file, path))
                } else {
                    default_text(path)
                };
                let new = match self.get(path) {
                    Some(Some(value)) => js_stringify(&toml_to_json(value)),
                    _ => default_text(path),
                };
                DiffRow {
                    path: path.clone(),
                    old,
                    new,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use toml::Value;

    use super::{describe, js_stringify, same, ConfigState, DiffRow, Pending};

    fn state() -> ConfigState {
        ConfigState::new(json!({
            "path": "/c/config.toml",
            "exists": true,
            "etag": "e1",
            "file": { "sources": ["/notes"], "index_transcripts": false, "hook": { "threshold": 0.5 } },
            "effective": { "sources": ["/notes"], "index_transcripts": false },
            "defaults": {
                "sources": [],
                "index_transcripts": true,
                "index_transcripts_max_age_days": null,
                "hook": { "threshold": 0.66 }
            },
            "errors": []
        }))
    }

    #[test]
    fn applied_rows_follow_js_object_order() {
        let document: toml_edit::DocumentMut = "b = 1\n10 = 2\n2 = 3\n".parse().unwrap();
        let pending = Pending::from_document(&json!({ "z": 1, "7": 1 }), &document);
        assert_eq!(pending.paths(), ["2", "10", "b", "7", "z"]);
    }

    #[test]
    fn describe_reads_like_json_stringify() {
        assert_eq!(describe(None), "unset");
        assert_eq!(describe(Some(&json!(null))), "none");
        assert_eq!(describe(Some(&json!("a\"b"))), "\"a\\\"b\"");
        assert_eq!(js_stringify(&json!(1.0)), "1");
        assert_eq!(js_stringify(&json!(0.66)), "0.66");
        assert_eq!(js_stringify(&json!(1e21)), "1e+21");
        assert_eq!(js_stringify(&json!(["_templates"])), "[\"_templates\"]");
        assert!(same(&json!(1), &json!(1.0)));
    }

    #[test]
    fn a_change_back_to_the_file_value_is_not_pending() {
        let state = state();
        let mut pending = Pending::default();
        pending.change(&state, "index_transcripts", Some(Value::Boolean(true)));
        assert_eq!(pending.len(), 1);
        pending.change(&state, "index_transcripts", Some(Value::Boolean(false)));
        assert!(pending.is_empty());
        pending.change(&state, "index_transcripts_max_age_days", None);
        assert!(
            pending.is_empty(),
            "resetting a key the file lacks is not a change"
        );
        pending.change(&state, "hook.threshold", None);
        assert_eq!(pending.get("hook.threshold"), Some(None));
        assert!(pending.is_default(&state, "hook.threshold"));
        assert_eq!(
            pending.value_of(&state, "hook.threshold"),
            Some(json!(0.66))
        );
    }

    #[test]
    fn an_edit_written_straight_into_set_or_unset_still_counts() {
        let state = state();
        let mut pending = Pending::default();
        pending.change(&state, "index_transcripts", Some(Value::Boolean(true)));
        pending
            .set
            .insert("index_transcripts_max_age_days".into(), Value::Integer(9));
        pending.unset.insert("hook.threshold".into());
        assert_eq!(pending.len(), 3);
        assert!(!pending.is_empty());
        assert_eq!(
            pending.paths(),
            [
                "index_transcripts",
                "index_transcripts_max_age_days",
                "hook.threshold"
            ]
        );
        let check = pending.to_check();
        assert_eq!(check.set["index_transcripts_max_age_days"], json!(9));
        assert_eq!(check.unset, ["hook.threshold"]);
        assert_eq!(pending.diff(state.file(), state.defaults()).len(), 3);

        let mut direct = Pending::default();
        direct
            .set
            .insert("index_transcripts".into(), Value::Boolean(true));
        assert_eq!(direct.len(), 1);
        assert!(direct.to_save("e1").set.contains_key("index_transcripts"));
    }

    #[test]
    fn the_diff_names_old_and_new_values_in_edit_order() {
        let state = state();
        let mut pending = Pending::default();
        pending.change(
            &state,
            "index_transcripts_max_age_days",
            Some(Value::Integer(30)),
        );
        pending.change(&state, "hook.threshold", None);
        pending.change(&state, "index_transcripts", Some(Value::Boolean(true)));
        assert_eq!(
            pending.diff(state.file(), state.defaults()),
            [
                DiffRow {
                    path: "index_transcripts_max_age_days".into(),
                    old: "default none".into(),
                    new: "30".into(),
                },
                DiffRow {
                    path: "hook.threshold".into(),
                    old: "0.5".into(),
                    new: "default 0.66".into(),
                },
                DiffRow {
                    path: "index_transcripts".into(),
                    old: "false".into(),
                    new: "true".into(),
                },
            ]
        );
        let check = pending.to_check();
        assert_eq!(check.unset, ["hook.threshold"]);
        assert_eq!(check.set["index_transcripts_max_age_days"], json!(30));
    }
}
