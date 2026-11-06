//! A filter's normal form as the filter keeps it.
//!
//! A `serde_json::Value` object is a B-tree whose every node takes about
//! 630 bytes, however few keys it holds: the normal form of a filter with one
//! clause, `{"$and": [{"input.key": {"$eq": "k"}}]}`, held three of them,
//! about 1.9 KB. [`Json`] holds the same JSON in boxed slices, under 200
//! bytes for that filter, and gives back the `Value` exactly: the same
//! numbers, the same keys in the same order, the same serialization.

use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::{Map, Number, Value};

/// A JSON value, read only, as compact as its parts allow.
#[derive(Debug)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(Number),
    String(Box<str>),
    Array(Box<[Json]>),
    /// In the order the `Value` object it was made from iterates its keys,
    /// each key once.
    Object(Box<[(Box<str>, Json)]>),
}

impl From<Value> for Json {
    fn from(value: Value) -> Self {
        match value {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(b),
            Value::Number(n) => Json::Number(n),
            Value::String(s) => Json::String(s.into_boxed_str()),
            Value::Array(items) => Json::Array(items.into_iter().map(Json::from).collect()),
            Value::Object(map) => Json::Object(
                map.into_iter()
                    .map(|(key, value)| (key.into_boxed_str(), Json::from(value)))
                    .collect(),
            ),
        }
    }
}

impl Json {
    /// The `Value` this was made from.
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Json::Null => Value::Null,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::Number(n.clone()),
            Json::String(s) => Value::String(s.to_string()),
            Json::Array(items) => Value::Array(items.iter().map(Json::to_value).collect()),
            Json::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_value()))
                    .collect::<Map<String, Value>>(),
            ),
        }
    }

    /// Whether `value` equals this JSON as `Value`'s `==` compares: numbers
    /// as `Number`s, objects whatever the order of their keys. Allocates
    /// nothing.
    pub(crate) fn eq_value(&self, value: &Value) -> bool {
        match (self, value) {
            (Json::Null, Value::Null) => true,
            (Json::Bool(a), Value::Bool(b)) => a == b,
            (Json::Number(a), Value::Number(b)) => a == b,
            (Json::String(a), Value::String(b)) => **a == **b,
            (Json::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.eq_value(b))
            }
            (Json::Object(a), Value::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(key, a)| b.get(&**key).is_some_and(|b| a.eq_value(b)))
            }
            _ => false,
        }
    }
}

/// As `Value`'s `==`: objects are equal whatever the order of their keys.
impl PartialEq for Json {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Json::Null, Json::Null) => true,
            (Json::Bool(a), Json::Bool(b)) => a == b,
            (Json::Number(a), Json::Number(b)) => a == b,
            (Json::String(a), Json::String(b)) => a == b,
            (Json::Array(a), Json::Array(b)) => a == b,
            (Json::Object(a), Json::Object(b)) => {
                a.len() == b.len()
                    && a.iter().enumerate().all(|(at, (key, a))| {
                        // Two objects made from maps that order their keys
                        // the same way hold each key at the same place.
                        let b = match b.get(at) {
                            Some((held, b)) if held == key => Some(b),
                            _ => b.iter().find(|(held, _)| held == key).map(|(_, b)| b),
                        };
                        b.is_some_and(|b| a == b)
                    })
            }
            _ => false,
        }
    }
}

/// Writes what `Value`'s `Serialize` writes for the value this was made
/// from: object keys in the order they were taken.
impl Serialize for Json {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => serializer.serialize_unit(),
            Json::Bool(b) => serializer.serialize_bool(*b),
            Json::Number(n) => n.serialize(serializer),
            Json::String(s) => serializer.serialize_str(s),
            Json::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items.iter() {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Json::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries.iter() {
                    map.serialize_entry(&**key, value)?;
                }
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn samples() -> Vec<Value> {
        vec![
            json!(null),
            json!(true),
            json!(0),
            json!(-7),
            json!(1.5),
            json!(1.0),
            json!(9007199254740993u64),
            json!("k\u{e9}y \"quoted\""),
            json!([]),
            json!({}),
            json!({ "$and": [{ "input.key": { "$eq": "key-1" } }] }),
            json!({ "b": [1, 2.5, { "c": null }], "a": { "z": "", "y": [true] } }),
        ]
    }

    #[test]
    fn gives_back_the_value_it_was_made_from() {
        for value in samples() {
            let json = Json::from(value.clone());
            assert_eq!(json.to_value(), value);
            assert!(json.eq_value(&value), "{value}");
            assert_eq!(
                serde_json::to_string(&json).unwrap(),
                serde_json::to_string(&value).unwrap()
            );
            assert_eq!(
                serde_json::to_string_pretty(&json).unwrap(),
                serde_json::to_string_pretty(&value).unwrap()
            );
        }
    }

    #[test]
    fn compares_as_values_compare() {
        let samples = samples();
        for a in &samples {
            for b in &samples {
                let (ja, jb) = (Json::from(a.clone()), Json::from(b.clone()));
                assert_eq!(ja == jb, a == b, "{a} == {b}");
                assert_eq!(ja.eq_value(b), a == b, "{a} == {b}");
            }
        }
        // An integer and the same number as a float are different values.
        assert!(!Json::from(json!(1)).eq_value(&json!(1.0)));
    }

    #[test]
    fn objects_are_equal_whatever_the_order_of_their_keys() {
        let ab = Json::Object(
            vec![
                ("a".into(), Json::Number(1.into())),
                ("b".into(), Json::Null),
            ]
            .into(),
        );
        let ba = Json::Object(
            vec![
                ("b".into(), Json::Null),
                ("a".into(), Json::Number(1.into())),
            ]
            .into(),
        );
        assert!(ab == ba);
        assert!(ba.eq_value(&json!({ "a": 1, "b": null })));
        let other = Json::Object(
            vec![
                ("b".into(), Json::Null),
                ("c".into(), Json::Number(1.into())),
            ]
            .into(),
        );
        assert!(ab != other);
    }
}
