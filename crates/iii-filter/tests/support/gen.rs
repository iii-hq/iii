//! Seeded random filters and documents, shared by `tests/normalize_props.rs`
//! and `examples/diffgen.rs`. Deterministic for a given seed.
//!
//! Filters use every operator, nesting up to the depth limit, `$any` in both
//! forms, escaped keys, index segments and numbers near the integer / float
//! boundaries (literals stay within ±2^53). Documents are built from the
//! filter, so that its paths are usually present and its operands usually
//! nearby, with random noise, absent paths and mixed types.

#![allow(dead_code)]

use iii_filter::Filter;
use serde_json::{Map, Number, Value, json};

/// xorshift64*, seeded through splitmix64.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng(if z == 0 { 0x2545_F491_4F6C_DD1D } else { z })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const ROOTS: &[&str] = &[
    "input", "input", "input", "input", "input", "output", "output", "error", "caller", "caller",
    "metadata",
];

const KEYS: &[&str] = &[
    "a",
    "b",
    "id",
    "op",
    "n",
    "s",
    "tags",
    "items",
    "status",
    "parent",
    "worker_name",
    "0",
    "1",
    "2",
    "01",
    "x.y",
    "back\\slash",
    "é",
    "@",
    "length",
    "constructor",
];

const OPERATORS: &[&str] = &[
    "$eq", "$eq", "$ne", "$in", "$nin", "$gt", "$gte", "$lt", "$lte", "$exists", "$prefix", "$any",
    "$any",
];

const STRINGS: &[&str] = &[
    "", "a", "b", "ab", "B", "insert", "update", "paid", "é", "e\u{301}", "😀", "\u{ff61}",
    "\u{e000}", "env:prod", "/api/",
];

const INTEGERS: &[i64] = &[
    0,
    1,
    -1,
    2,
    100,
    9_007_199_254_740_992,
    -9_007_199_254_740_992,
    9_007_199_254_740_991,
    -9_007_199_254_740_991,
];

const FLOATS: &[f64] = &[
    0.0,
    -0.0,
    1.0,
    0.5,
    2.5,
    -2.5,
    0.1,
    0.3,
    1e-9,
    123.456,
    4_503_599_627_370_495.5,
    9_007_199_254_740_991.0,
];

/// Numbers that only appear in documents: beyond ±2^53, compared as f64.
fn payload_extreme(rng: &mut Rng) -> Value {
    match rng.below(5) {
        0 => Value::from(9_007_199_254_740_993u64),
        1 => Value::from(u64::MAX),
        2 => Value::from(i64::MIN),
        3 => Value::from(-9_007_199_254_740_993i64),
        _ => float(1e300),
    }
}

fn float(f: f64) -> Value {
    Number::from_f64(f).map_or(Value::Null, Value::Number)
}

/// A random filter (it can occasionally exceed a limit; callers retry).
pub fn filter(rng: &mut Rng) -> Value {
    let mut generator = FilterGen { rng, and_depth: 0 };
    generator.filter(1, false)
}

struct FilterGen<'r> {
    rng: &'r mut Rng,
    and_depth: usize,
}

impl FilterGen<'_> {
    fn filter(&mut self, depth: usize, in_any: bool) -> Value {
        let mut map = Map::new();
        let clauses = match self.rng.below(100) {
            0..5 => 0,
            5..50 => 1,
            50..80 => 2,
            _ => 3,
        };
        for _ in 0..clauses {
            let roll = self.rng.below(100);
            if depth < 8 && roll < 12 {
                let children = (0..1 + self.rng.below(3))
                    .map(|_| self.filter(depth + 1, in_any))
                    .collect();
                map.insert("$or".to_owned(), Value::Array(children));
            } else if depth < 8 && roll < 20 {
                let child = self.filter(depth + 1, in_any);
                map.insert("$not".to_owned(), child);
            } else if roll < 26 && self.and_depth < 3 {
                self.and_depth += 1;
                let children = (0..1 + self.rng.below(2))
                    .map(|_| self.filter(depth, in_any))
                    .collect();
                self.and_depth -= 1;
                map.insert("$and".to_owned(), Value::Array(children));
            } else {
                let path = self.path(in_any);
                let ops = self.ops(depth);
                map.insert(path, ops);
            }
        }
        Value::Object(map)
    }

    fn path(&mut self, in_any: bool) -> String {
        let mut path = if in_any {
            "@".to_owned()
        } else {
            (*self.rng.pick(ROOTS)).to_owned()
        };
        let segments = match self.rng.below(100) {
            0..15 => 0,
            15..60 => 1,
            60..90 => 2,
            _ => 3,
        };
        for _ in 0..segments {
            path.push('.');
            for c in self.rng.pick(KEYS).chars() {
                if c == '.' || c == '\\' {
                    path.push('\\');
                }
                path.push(c);
            }
        }
        path
    }

    /// An operator object at `depth` (the level of the filter holding it).
    fn ops(&mut self, depth: usize) -> Value {
        let mut map = Map::new();
        let count = if self.rng.chance(25) { 2 } else { 1 };
        while map.len() < count {
            let name = *self.rng.pick(OPERATORS);
            let operand = match name {
                "$eq" | "$ne" => literal(self.rng, 2),
                "$in" | "$nin" => Value::Array(
                    (0..self.rng.below(5))
                        .map(|_| literal(self.rng, 1))
                        .collect(),
                ),
                "$gt" | "$gte" | "$lt" | "$lte" => {
                    if self.rng.chance(60) {
                        number_literal(self.rng)
                    } else {
                        Value::from(*self.rng.pick(STRINGS))
                    }
                }
                "$exists" => Value::Bool(self.rng.chance(60)),
                "$prefix" => Value::from(*self.rng.pick(STRINGS)),
                _ if depth + 1 > 8 => continue,
                _ if self.rng.chance(50) => self.ops(depth + 1),
                _ => self.filter(depth + 1, true),
            };
            map.insert(name.to_owned(), operand);
        }
        Value::Object(map)
    }
}

fn number_literal(rng: &mut Rng) -> Value {
    if rng.chance(50) {
        Value::from(*rng.pick(INTEGERS))
    } else {
        float(*rng.pick(FLOATS))
    }
}

/// A literal operand (numbers within ±2^53).
fn literal(rng: &mut Rng, depth: usize) -> Value {
    match rng.below(100) {
        0..35 => number_literal(rng),
        35..70 => Value::from(*rng.pick(STRINGS)),
        70..78 => Value::Null,
        78..86 => Value::Bool(rng.chance(50)),
        86..93 if depth > 0 => {
            Value::Array((0..rng.below(4)).map(|_| literal(rng, depth - 1)).collect())
        }
        93.. if depth > 0 => {
            let mut map = Map::new();
            for _ in 0..rng.below(4) {
                map.insert((*rng.pick(KEYS)).to_owned(), literal(rng, depth - 1));
            }
            Value::Object(map)
        }
        _ => Value::from(*rng.pick(STRINGS)),
    }
}

/// Any JSON value, payload extremes included.
fn random_value(rng: &mut Rng, depth: usize) -> Value {
    match rng.below(100) {
        0..20 => number_literal(rng),
        20..25 => payload_extreme(rng),
        25..50 => Value::from(*rng.pick(STRINGS)),
        50..60 => Value::Null,
        60..70 => Value::Bool(rng.chance(50)),
        70..85 if depth > 0 => Value::Array(
            (0..rng.below(4))
                .map(|_| random_value(rng, depth - 1))
                .collect(),
        ),
        85.. if depth > 0 => {
            let mut map = Map::new();
            for _ in 0..rng.below(4) {
                map.insert((*rng.pick(KEYS)).to_owned(), random_value(rng, depth - 1));
            }
            Value::Object(map)
        }
        _ => Value::Null,
    }
}

/// A document shaped by `filter`: most of its paths are set to values near
/// its operands, some are left absent, some get unrelated values.
pub fn doc(rng: &mut Rng, filter: &Value) -> Value {
    let mut doc = Value::Object(Map::new());
    if rng.chance(15) {
        let root = *rng.pick(&["input", "output", "error", "caller", "metadata"]);
        let value = random_value(rng, 2);
        set(rng, &mut doc, &[root.to_owned()], value);
    }
    apply(rng, filter, &mut doc);
    if rng.chance(5) {
        let root = *rng.pick(&["input", "output", "error", "caller", "metadata"]);
        let value = random_value(rng, 0);
        set(rng, &mut doc, &[root.to_owned()], value);
    }
    doc
}

fn apply(rng: &mut Rng, filter: &Value, target: &mut Value) {
    let Value::Object(map) = filter else { return };
    for (key, value) in map {
        match key.as_str() {
            "$and" | "$or" => {
                for child in value.as_array().into_iter().flatten() {
                    if rng.chance(75) {
                        apply(rng, child, target);
                    }
                }
            }
            "$not" => {
                if rng.chance(60) {
                    apply(rng, value, target);
                }
            }
            path => {
                if !rng.chance(85) {
                    continue;
                }
                let v = if rng.chance(40) {
                    value_for_all(rng, value)
                } else {
                    value_for(rng, value)
                };
                let Some(v) = v else {
                    continue;
                };
                let mut segments = split_path(path);
                if segments.first().map(String::as_str) == Some("@") {
                    segments.remove(0);
                }
                set(rng, target, &segments, v);
            }
        }
    }
}

/// A value for a path, aimed at one of its operators. `None` leaves it absent.
fn value_for(rng: &mut Rng, ops: &Value) -> Option<Value> {
    let Value::Object(ops) = ops else {
        return Some(random_value(rng, 2));
    };
    if ops.is_empty() || rng.chance(12) {
        return Some(random_value(rng, 2));
    }
    let (name, operand) = ops.iter().nth(rng.below(ops.len()))?;
    Some(match name.as_str() {
        "$eq" | "$ne" => variant(rng, operand),
        "$in" | "$nin" => match operand.as_array() {
            Some(items) if !items.is_empty() && rng.chance(75) => {
                let item = rng.pick(items).clone();
                variant(rng, &item)
            }
            _ => random_value(rng, 1),
        },
        "$gt" | "$gte" | "$lt" | "$lte" => near(rng, operand),
        "$exists" => {
            if operand == &Value::Bool(true) || rng.chance(30) {
                random_value(rng, 1)
            } else {
                return None;
            }
        }
        "$prefix" => {
            let prefix = operand.as_str().unwrap_or_default();
            match rng.below(4) {
                0 => Value::from(prefix),
                1 => Value::from(format!("{prefix}{}", rng.pick(STRINGS))),
                2 => Value::from(prefix.chars().skip(1).collect::<String>()),
                _ => random_value(rng, 0),
            }
        }
        "$any" => {
            let elements = (0..rng.below(4))
                .map(|_| element_for(rng, operand))
                .collect();
            Value::Array(elements)
        }
        _ => random_value(rng, 1),
    })
}

/// A value meeting every operator of `ops` when one of a few tries does
/// (`None` is absent, which `$ne` / `$nin` / `$exists: false` accept).
fn value_for_all(rng: &mut Rng, ops: &Value) -> Option<Value> {
    let Ok(all) = Filter::parse(&json!({ "input": ops })) else {
        return value_for(rng, ops);
    };
    let mut last = None;
    for _ in 0..8 {
        let candidate = value_for(rng, ops);
        let doc = match &candidate {
            Some(v) => json!({ "input": v }),
            None => json!({}),
        };
        if all.matches_value(&doc) {
            return candidate;
        }
        last = candidate;
    }
    last
}

fn element_for(rng: &mut Rng, operand: &Value) -> Value {
    let is_ops = operand.as_object().is_some_and(|m| {
        !m.is_empty()
            && m.keys()
                .all(|k| k.starts_with('$') && !matches!(k.as_str(), "$and" | "$or" | "$not"))
    });
    if is_ops {
        // Some elements satisfy every operator, others one of them: `$any`
        // needs all operators to hold for the same element.
        let value = if rng.chance(50) {
            value_for_all(rng, operand)
        } else {
            value_for(rng, operand)
        };
        return value.unwrap_or_else(|| random_value(rng, 0));
    }
    let mut element = if rng.chance(85) {
        Value::Object(Map::new())
    } else {
        random_value(rng, 1)
    };
    apply(rng, operand, &mut element);
    element
}

/// The same value, respelled (1 ↔ 1.0), or a near one of the same type.
fn variant(rng: &mut Rng, value: &Value) -> Value {
    match value {
        Value::Number(_) | Value::String(_) if rng.chance(40) => near(rng, value),
        Value::Number(n) if rng.chance(30) => {
            let f = n.as_f64().unwrap_or_default();
            if n.is_f64() && f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_992.0 {
                Value::from(f as i64)
            } else {
                float(f)
            }
        }
        _ if rng.chance(10) => random_value(rng, 1),
        other => other.clone(),
    }
}

fn near(rng: &mut Rng, value: &Value) -> Value {
    match value {
        Value::Number(n) => {
            if rng.chance(8) {
                return payload_extreme(rng);
            }
            // Around ±2^53: the payload integers next to it, which compare
            // as f64 (2^53 + 1 equals 2^53).
            if let Some(i) = n.as_i64().filter(|i| i.unsigned_abs() >= (1 << 53) - 1) {
                if rng.chance(50) {
                    return Value::from(i + *rng.pick(&[-1, 0, 1, 2]) * i.signum());
                }
            }
            let x = n.as_f64().unwrap_or_default();
            let candidates = [x, x + 1.0, x - 1.0, x + 0.5, x - 0.5, -x, x * 2.0];
            let y = *rng.pick(&candidates);
            if y.fract() == 0.0 && y.abs() <= 9_007_199_254_740_992.0 && rng.chance(50) {
                Value::from(y as i64)
            } else {
                float(y)
            }
        }
        Value::String(s) => match rng.below(5) {
            0 => Value::from(s.as_str()),
            1 => Value::from(format!("{s}a")),
            2 => Value::from(
                s.chars()
                    .take(s.chars().count().saturating_sub(1))
                    .collect::<String>(),
            ),
            3 => Value::from(format!("{s}😀")),
            _ => Value::from(*rng.pick(STRINGS)),
        },
        other => other.clone(),
    }
}

/// Splits a path on unescaped dots and unescapes `\.` / `\\`.
fn split_path(path: &str) -> Vec<String> {
    let mut segments = vec![String::new()];
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    segments.last_mut().unwrap().push(next);
                }
            }
            '.' => segments.push(String::new()),
            c => segments.last_mut().unwrap().push(c),
        }
    }
    segments
}

fn small_index(segment: &str) -> Option<usize> {
    let canonical = segment == "0"
        || (segment.starts_with(|c: char| ('1'..='9').contains(&c))
            && segment.chars().all(|c| c.is_ascii_digit()));
    if canonical {
        segment.parse().ok().filter(|i| *i < 8)
    } else {
        None
    }
}

/// Sets `value` at `segments` under `target`, creating containers on the way
/// (arrays for small index segments half of the time).
fn set(rng: &mut Rng, target: &mut Value, segments: &[String], value: Value) {
    let Some((segment, rest)) = segments.split_first() else {
        *target = value;
        return;
    };
    let index = small_index(segment);
    match target {
        Value::Object(map) => {
            let key_follows = rest.first().is_some_and(|next| small_index(next).is_none());
            if !map.contains_key(segment) && key_follows && rng.chance(12) {
                // An array where the path expects an object: the value lands
                // in an element, which the path does not reach (no fan-out).
                let mut element = Value::Object(Map::new());
                set(rng, &mut element, rest, value);
                let mut items = vec![element];
                if rng.chance(50) {
                    items.insert(0, random_value(rng, 1));
                }
                map.insert(segment.clone(), Value::Array(items));
                return;
            }
            if !map.contains_key(segment) {
                let container = new_container(rng, rest.first());
                map.insert(segment.clone(), container);
            }
            let child = map.get_mut(segment).expect("just inserted");
            set(rng, child, rest, value);
        }
        Value::Array(items) if index.is_some() => {
            let i = index.unwrap_or_default();
            while items.len() <= i {
                items.push(random_value(rng, 0));
            }
            set(rng, &mut items[i], rest, value);
        }
        _ => {
            if rng.chance(80) {
                *target = if index.is_some() && rng.chance(50) {
                    Value::Array(Vec::new())
                } else {
                    Value::Object(Map::new())
                };
                set(rng, target, segments, value);
            }
        }
    }
}

fn new_container(rng: &mut Rng, next: Option<&String>) -> Value {
    match next {
        None => Value::Null,
        Some(segment) if small_index(segment).is_some() && rng.chance(50) => {
            Value::Array(Vec::new())
        }
        Some(_) => Value::Object(Map::new()),
    }
}

/// Documents tried per case to land on the wanted outcome.
const ATTEMPTS: usize = 64;

/// One `diffgen` run: `{"seed":N,"cases":[{"filters":…,"doc":…,"expect":bool}]}`
/// (no trailing newline). Each case first draws the outcome (match or not,
/// 50/50), then a filter and a document the Rust evaluator gives that outcome
/// for; a filter that cannot reach it within ATTEMPTS documents is dropped.
pub fn diff_run(seed: u64, count: usize) -> String {
    let mut rng = Rng::new(seed);
    let mut cases = Vec::with_capacity(count);
    while cases.len() < count {
        let want = rng.chance(50);
        let (src, doc) = loop {
            let src = filter(&mut rng);
            let Ok(compiled) = Filter::parse(&src) else {
                continue;
            };
            let found = (0..ATTEMPTS)
                .map(|_| doc(&mut rng, &src))
                .find(|doc| compiled.matches_value(doc) == want);
            if let Some(doc) = found {
                break (src, doc);
            }
        };
        cases.push(json!({ "filters": src, "doc": doc, "expect": want }));
    }
    // Written by hand to keep `seed` first (serde_json sorts object keys).
    format!("{{\"seed\":{seed},\"cases\":{}}}", Value::Array(cases))
}
