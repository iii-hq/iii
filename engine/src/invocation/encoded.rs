// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! JSON encoded once and sent as is.
//!
//! The activations of one observed event all carry its payloads. A worker
//! connection receives each one as JSON text, so the engine encodes each
//! payload once per event and shares the text: an activation's argument is
//! the pieces it adds (its ids, the event's status) around the shared ones.

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

/// One JSON value as text, in pieces: the ones written for this value alone,
/// and the ones it shares with other values (an event's payload, encoded
/// once for all of its activations). Concatenated, the pieces are the
/// value's JSON text.
#[derive(Debug, Clone, Default)]
pub struct EncodedJson {
    pieces: Vec<Piece>,
    len: usize,
}

#[derive(Debug, Clone)]
enum Piece {
    Owned(String),
    Shared(Arc<str>),
}

impl Piece {
    fn as_str(&self) -> &str {
        match self {
            Piece::Owned(text) => text,
            Piece::Shared(text) => text,
        }
    }
}

impl EncodedJson {
    /// `text`, the JSON text of one value, shared.
    pub fn shared(text: Arc<str>) -> Self {
        Self {
            len: text.len(),
            pieces: vec![Piece::Shared(text)],
        }
    }

    /// `value`, encoded now; `null` if it does not encode.
    pub fn of(value: &impl Serialize) -> Self {
        let text = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
        Self {
            len: text.len(),
            pieces: vec![Piece::Owned(text)],
        }
    }

    /// The length of its text, in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends its text to `out`.
    pub fn write_to(&self, out: &mut String) {
        for piece in &self.pieces {
            out.push_str(piece.as_str());
        }
    }

    /// The value it encodes, decoded: for the rare reader that needs it as a
    /// [`Value`] (an observer of the function the text is sent to).
    pub fn to_value(&self) -> Value {
        let mut text = String::with_capacity(self.len);
        self.write_to(&mut text);
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }
}

/// Writes an [`EncodedJson`] object one field at a time, in the order given.
/// Keys are written as given: they must need no escaping.
pub struct ObjectWriter {
    pieces: Vec<Piece>,
    current: String,
    len: usize,
    empty: bool,
}

impl Default for ObjectWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectWriter {
    pub fn new() -> Self {
        Self {
            pieces: Vec::new(),
            current: String::from("{"),
            len: 0,
            empty: true,
        }
    }

    fn key(&mut self, key: &str) {
        debug_assert!(
            !key.contains(['"', '\\']) && !key.chars().any(char::is_control),
            "`{key}` needs escaping"
        );
        if !self.empty {
            self.current.push(',');
        }
        self.empty = false;
        self.current.push('"');
        self.current.push_str(key);
        self.current.push_str("\":");
    }

    /// `key`, with `value` encoded now (`null` if it does not encode).
    pub fn field(&mut self, key: &str, value: &impl Serialize) {
        self.key(key);
        match serde_json::to_string(value) {
            Ok(text) => self.current.push_str(&text),
            Err(_) => self.current.push_str("null"),
        }
    }

    /// `key`, with `text`, the JSON text of one value, shared.
    pub fn shared_field(&mut self, key: &str, text: &Arc<str>) {
        self.key(key);
        self.flush();
        self.len += text.len();
        self.pieces.push(Piece::Shared(text.clone()));
    }

    fn flush(&mut self) {
        if !self.current.is_empty() {
            let current = std::mem::take(&mut self.current);
            self.len += current.len();
            self.pieces.push(Piece::Owned(current));
        }
    }

    pub fn finish(mut self) -> EncodedJson {
        self.current.push('}');
        self.flush();
        EncodedJson {
            pieces: self.pieces,
            len: self.len,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(encoded: &EncodedJson) -> String {
        let mut out = String::new();
        encoded.write_to(&mut out);
        assert_eq!(out.len(), encoded.len());
        out
    }

    #[test]
    fn an_object_shares_the_pieces_it_was_given() {
        let payload: Arc<str> = Arc::from(json!({ "k": [1, "two", null] }).to_string());
        let mut writer = ObjectWriter::new();
        writer.field("id", &"o-1");
        writer.shared_field("input", &payload);
        writer.field("n", &1.5);
        writer.shared_field("output", &payload);
        let encoded = writer.finish();
        assert_eq!(
            serde_json::from_str::<Value>(&text(&encoded)).unwrap(),
            json!({
                "id": "o-1",
                "input": { "k": [1, "two", null] },
                "n": 1.5,
                "output": { "k": [1, "two", null] },
            })
        );
        assert_eq!(encoded.to_value()["output"]["k"][1], "two");
        assert_eq!(Arc::strong_count(&payload), 3);
    }

    #[test]
    fn an_empty_object_and_a_shared_value_are_valid_json() {
        assert_eq!(text(&ObjectWriter::new().finish()), "{}");
        let shared = EncodedJson::shared(Arc::from("[1,2]"));
        assert_eq!(shared.to_value(), json!([1, 2]));
        assert_eq!(
            EncodedJson::of(&json!({ "a": "\u{1}" })).to_value(),
            json!({ "a": "\u{1}" })
        );
    }
}
