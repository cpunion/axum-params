use std::collections::HashMap;

use actson::{
    feeder::{JsonFeeder, SliceJsonFeeder},
    JsonEvent, JsonParser,
};
use log::debug;

use crate::{Error, Number, Value};

#[derive(Debug)]
pub enum JsonError {
    SyntaxError(String),
    NoMoreInput,
    Other(String),
}

impl From<JsonError> for Error {
    fn from(err: JsonError) -> Self {
        match err {
            JsonError::SyntaxError(e) => Error::DecodeError(format!("Syntax error: {}", e)),
            JsonError::NoMoreInput => Error::DecodeError("Incomplete JSON input".to_string()),
            JsonError::Other(msg) => Error::DecodeError(msg),
        }
    }
}

impl From<&serde_json::Value> for Value {
    fn from(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(v) => Value::Bool(*v),
            serde_json::Value::Number(n) => {
                let n = n.as_f64().unwrap();
                if n.is_nan() {
                    Value::Null
                } else {
                    Value::Number(n.into())
                }
            }
            serde_json::Value::String(v) => Value::String(v.clone()),
            serde_json::Value::Array(v) => {
                Value::Array(v.iter().map(Value::from).collect::<Vec<Value>>())
            }
            serde_json::Value::Object(v) => Value::Object(
                v.iter()
                    .map(|(k, v)| (k.clone(), Value::from(v)))
                    .collect::<HashMap<String, Value>>(),
            ),
        }
    }
}

fn unescape_json_string(s: &str) -> Result<String, JsonError> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('/') => result.push('/'),
                Some('b') => result.push('\u{0008}'),
                Some('f') => result.push('\u{000C}'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('u') => {
                    let mut code = String::with_capacity(4);
                    for _ in 0..4 {
                        if let Some(hex) = chars.next() {
                            code.push(hex);
                        } else {
                            return Err(JsonError::SyntaxError("Missing hex digits".to_string()));
                        }
                    }
                    if let Ok(code) = u32::from_str_radix(&code, 16) {
                        if let Some(c) = char::from_u32(code) {
                            result.push(c);
                        } else {
                            return Err(JsonError::SyntaxError("Invalid Unicode".to_string()));
                        }
                    } else {
                        return Err(JsonError::SyntaxError("Invalid hex digits".to_string()));
                    }
                }
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => {
                    result.push('\\');
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    Ok(result)
}

fn json_event_to_value<T: JsonFeeder>(
    event: &JsonEvent,
    parser: &JsonParser<T>,
) -> Result<Value, JsonError> {
    match event {
        JsonEvent::ValueString => Ok(Value::String(unescape_json_string(
            parser.current_str().unwrap(),
        )?)),
        JsonEvent::ValueInt => Ok(Value::Number(Number::from(
            parser.current_str().unwrap().parse::<i64>().unwrap(),
        ))),
        JsonEvent::ValueFloat => Ok(Value::Number(Number::from(
            parser.current_str().unwrap().parse::<f64>().unwrap(),
        ))),
        JsonEvent::ValueTrue => Ok(Value::Bool(true)),
        JsonEvent::ValueFalse => Ok(Value::Bool(false)),
        JsonEvent::ValueNull => Ok(Value::Null),
        other => Err(JsonError::SyntaxError(format!(
            "Unexpected JSON event in parsing value: {:?}",
            other
        ))),
    }
}

pub fn parse_json(feeder: SliceJsonFeeder) -> Result<Value, JsonError> {
    let mut parser = JsonParser::new(feeder);

    let mut stack = vec![];
    let mut result = None;
    let mut current_key = None;

    while let Some(event) = parser
        .next_event()
        .map_err(|e| JsonError::SyntaxError(format!("parse error:{}", e)))?
    {
        debug!("JSON event: {:?}", event);
        match event {
            JsonEvent::NeedMoreInput => {}

            JsonEvent::StartObject | JsonEvent::StartArray => {
                let v = if event == JsonEvent::StartObject {
                    Value::Object(HashMap::new())
                } else {
                    Value::Array(vec![])
                };
                stack.push((current_key.take(), v));
            }

            JsonEvent::EndObject | JsonEvent::EndArray => {
                let v = stack.pop().unwrap();
                if let Some((_, top)) = stack.last_mut() {
                    match top {
                        Value::Object(o) => {
                            if let Some(key) = v.0 {
                                o.insert(key, v.1);
                            }
                        }
                        Value::Array(a) => {
                            a.push(v.1);
                        }
                        _ => {
                            return Err(JsonError::SyntaxError(
                                "Invalid JSON array end".to_string(),
                            ))
                        }
                    }
                } else {
                    result = Some(v.1);
                }
            }

            JsonEvent::FieldName => {
                let str_result = parser
                    .current_str()
                    .map_err(|e| JsonError::SyntaxError(format!("parse error:{}", e)))?;
                current_key = Some(str_result.to_string());
            }

            JsonEvent::ValueString
            | JsonEvent::ValueInt
            | JsonEvent::ValueFloat
            | JsonEvent::ValueTrue
            | JsonEvent::ValueFalse
            | JsonEvent::ValueNull => {
                let v = json_event_to_value(&event, &parser)?;
                if let Some((_, top)) = stack.last_mut() {
                    match top {
                        Value::Array(a) => {
                            a.push(v);
                        }
                        Value::Object(o) => {
                            if let Some(key) = current_key.take() {
                                o.insert(key, v);
                            } else {
                                return Err(JsonError::SyntaxError(
                                    "Invalid JSON object key".to_string(),
                                ));
                            }
                        }
                        other => {
                            return Err(JsonError::SyntaxError(format!(
                                "Unexpected JSON value in {}",
                                other.type_name()
                            )));
                        }
                    }
                } else if result.is_none() {
                    result = Some(v);
                } else {
                    return Err(JsonError::SyntaxError("Unexpected JSON value".to_string()));
                }
            }
        }
    }

    result.ok_or(JsonError::NoMoreInput)
}

#[cfg(test)]
mod tests {
    use actson::feeder::SliceJsonFeeder;

    use crate::{parse_json, Number, Value, N};

    #[test]
    fn test_parse_json_numbers() {
        // Test positive integers
        let json = r#"{"pos": 42, "zero": 0, "big": 9007199254740991}"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();
        if let Value::Object(map) = result {
            assert!(matches!(map["pos"], Value::Number(Number(N::PosInt(42)))));
            assert!(matches!(map["zero"], Value::Number(Number(N::PosInt(0)))));
            assert!(matches!(
                map["big"],
                Value::Number(Number(N::PosInt(9007199254740991)))
            ));
        } else {
            panic!("Expected object");
        }

        // Test negative integers
        let json = r#"{"neg": -42, "min": -9007199254740991}"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();
        if let Value::Object(map) = result {
            assert!(matches!(map["neg"], Value::Number(Number(N::NegInt(-42)))));
            assert!(matches!(
                map["min"],
                Value::Number(Number(N::NegInt(-9007199254740991)))
            ));
        } else {
            panic!("Expected object");
        }

        // Test floating point numbers
        let json = r#"{
            "float": 42.5,
            "neg_float": -42.5,
            "zero_float": 0.0,
            "exp": 1.23e5,
            "neg_exp": -1.23e-5
        }"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();
        if let Value::Object(map) = result {
            assert!(
                matches!(map["float"], Value::Number(Number(N::Float(v))) if (v - 42.5).abs() < f64::EPSILON)
            );
            assert!(
                matches!(map["neg_float"], Value::Number(Number(N::Float(v))) if (v - (-42.5)).abs() < f64::EPSILON)
            );
            assert!(
                matches!(map["zero_float"], Value::Number(Number(N::Float(v))) if v.abs() < f64::EPSILON)
            );
            assert!(
                matches!(map["exp"], Value::Number(Number(N::Float(v))) if (v - 123000.0).abs() < f64::EPSILON)
            );
            assert!(
                matches!(map["neg_exp"], Value::Number(Number(N::Float(v))) if (v - (-0.0000123)).abs() < f64::EPSILON)
            );
        } else {
            panic!("Expected object");
        }

        // Test array of numbers
        let json = r#"[42, -42, 42.5, 0, -0.0]"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();
        if let Value::Array(arr) = result {
            assert!(matches!(arr[0], Value::Number(Number(N::PosInt(42)))));
            assert!(matches!(arr[1], Value::Number(Number(N::NegInt(-42)))));
            assert!(
                matches!(arr[2], Value::Number(Number(N::Float(v))) if (v - 42.5).abs() < f64::EPSILON)
            );
            assert!(matches!(arr[3], Value::Number(Number(N::PosInt(0)))));
            assert!(matches!(arr[4], Value::Number(Number(N::Float(v))) if v.abs() < f64::EPSILON));
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn test_parse_json_mixed_types() {
        let json = r#"{
            "number": 42,
            "string": "hello",
            "bool": true,
            "null": null,
            "array": [1, "two", false],
            "nested": {"a": 1, "b": 2}
        }"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();
        if let Value::Object(map) = result {
            assert!(matches!(
                map["number"],
                Value::Number(Number(N::PosInt(42)))
            ));
            assert!(matches!(map["string"], Value::String(ref s) if s == "hello"));
            assert!(matches!(map["bool"], Value::Bool(b) if b));
            assert!(matches!(map["null"], Value::Null));

            if let Value::Array(arr) = &map["array"] {
                assert!(matches!(arr[0], Value::Number(Number(N::PosInt(1)))));
                assert!(matches!(arr[1], Value::String(ref s) if s == "two"));
                assert!(matches!(arr[2], Value::Bool(b) if !b));
            } else {
                panic!("Expected array");
            }

            if let Value::Object(nested) = &map["nested"] {
                assert!(matches!(nested["a"], Value::Number(Number(N::PosInt(1)))));
                assert!(matches!(nested["b"], Value::Number(Number(N::PosInt(2)))));
            } else {
                panic!("Expected nested object");
            }
        } else {
            panic!("Expected object");
        }
    }

    #[test]
    fn test_parse_json_escape_chars() {
        let json = r#"{
            "escaped_quotes": "hello \"world\"",
            "escaped_slash": "hello\/world",
            "escaped_backslash": "back\\slash",
            "escaped_controls": "\b\f\n\r\t",
            "escaped_unicode": "\u0041\u0042C",
            "mixed_escapes": "Hello\n\"World\"\\\u0021"
        }"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();

        if let Value::Object(map) = result {
            assert!(matches!(
                map["escaped_quotes"],
                Value::String(ref s) if s == "hello \"world\""
            ));
            assert!(matches!(
                map["escaped_slash"],
                Value::String(ref s) if s == "hello/world"
            ));
            assert!(matches!(
                map["escaped_backslash"],
                Value::String(ref s) if s == "back\\slash"
            ));
            assert!(matches!(
                map["escaped_controls"],
                Value::String(ref s) if s == "\u{0008}\u{000C}\n\r\t"
            ));
            assert!(matches!(
                map["escaped_unicode"],
                Value::String(ref s) if s == "ABC"
            ));
            assert!(matches!(
                map["mixed_escapes"],
                Value::String(ref s) if s == "Hello\n\"World\"\\!"
            ));
        } else {
            panic!("Expected object");
        }

        // Test invalid escape sequences
        let invalid_json = r#"{"invalid": "\z"}"#;
        assert!(parse_json(SliceJsonFeeder::new(invalid_json.as_bytes())).is_err());
    }
}
