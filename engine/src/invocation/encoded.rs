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
//! The `invokefunction` message that carries it keeps the pieces too, until
//! the connection's writer sends them (see [`crate::protocol::invoke_frame`]).

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

/// A piece of an [`EncodedJson`]'s text.
#[derive(Debug, Clone)]
pub enum Piece {
    /// Written for this value alone.
    Owned(String),
    /// Shared with other values.
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

    /// `text`, the JSON text of one value, in one piece it owns.
    pub fn owned(text: String) -> Self {
        Self {
            len: text.len(),
            pieces: vec![Piece::Owned(text)],
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

    /// Its text, joined; its one piece as it is when that piece is its own.
    pub fn into_text(mut self) -> String {
        match self.pieces.as_mut_slice() {
            [Piece::Owned(text)] => std::mem::take(text),
            _ => {
                let mut text = String::with_capacity(self.len);
                self.write_to(&mut text);
                text
            }
        }
    }

    /// Its pieces, in order.
    pub fn into_pieces(self) -> std::vec::IntoIter<Piece> {
        self.pieces.into_iter()
    }

    /// Its text between `head` and `tail`: a message that carries the value,
    /// sharing what the value shares. `head` and `tail` join the pieces next
    /// to them when those are the value's own, so the message is in no more
    /// pieces than the value, and the pieces it owns hold no spare capacity
    /// at its ends.
    pub fn enclosed(mut self, head: String, tail: &str) -> Self {
        self.len += head.len() + tail.len();
        match self.pieces.as_mut_slice() {
            [Piece::Owned(only)] => {
                *only = joined(&[head.as_str(), only.as_str(), tail]);
                return self;
            }
            [Piece::Owned(first), ..] => *first = joined(&[head.as_str(), first.as_str()]),
            _ => self.pieces.insert(0, Piece::Owned(head)),
        }
        match self.pieces.last_mut() {
            Some(Piece::Owned(last)) => *last = joined(&[last.as_str(), tail]),
            _ => self.pieces.push(Piece::Owned(tail.to_string())),
        }
        self
    }

    /// The value it encodes, decoded: for the rare reader that needs it as a
    /// [`Value`] (an observer of the function the text is sent to).
    pub fn to_value(&self) -> Value {
        let mut text = String::with_capacity(self.len);
        self.write_to(&mut text);
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }
}

/// `parts`, in order, in a string of exactly their length.
fn joined(parts: &[&str]) -> String {
    let mut text = String::with_capacity(parts.iter().map(|part| part.len()).sum());
    for part in parts {
        text.push_str(part);
    }
    text
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

    /// A value enclosed in a message keeps sharing its pieces: the head and
    /// tail join the owned pieces at its ends.
    #[test]
    fn an_enclosed_value_shares_its_pieces() {
        let payload: Arc<str> = Arc::from("[1,2]");
        let mut writer = ObjectWriter::new();
        writer.field("id", &"o-1");
        writer.shared_field("input", &payload);
        let message = writer.finish().enclosed("{\"data\":".to_string(), "}");
        assert_eq!(text(&message), r#"{"data":{"id":"o-1","input":[1,2]}}"#);
        assert_eq!(Arc::strong_count(&payload), 2);
        let pieces: Vec<Piece> = message.clone().into_pieces().collect();
        assert!(matches!(
            pieces.as_slice(),
            [Piece::Owned(head), Piece::Shared(shared), Piece::Owned(tail)]
                if head == r#"{"data":{"id":"o-1","input":"#
                    && head.capacity() == head.len()
                    && Arc::ptr_eq(shared, &payload)
                    && tail == "}}"
                    && tail.capacity() == tail.len()
        ));
        assert_eq!(
            message.into_text(),
            r#"{"data":{"id":"o-1","input":[1,2]}}"#
        );

        // A value that starts or ends with a shared piece, or has none.
        let shared = EncodedJson::shared(payload.clone()).enclosed("[".to_string(), "]");
        assert_eq!(text(&shared), "[[1,2]]");
        assert_eq!(shared.into_pieces().count(), 3);
        let owned = EncodedJson::of(&1).enclosed("[".to_string(), "]");
        assert_eq!(owned.clone().into_pieces().count(), 1);
        assert_eq!(owned.into_text(), "[1]");
        let empty = EncodedJson::default().enclosed("[".to_string(), "]");
        assert_eq!(text(&empty), "[]");
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
