use std::collections::HashMap;

use actson::{
    feeder::{JsonFeeder, SliceJsonFeeder},
    JsonEvent, JsonParser,
};
use log::debug;

use crate::{Error, UploadFile};

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum N {
    PosInt(u64),
    /// Always less than zero.
    NegInt(i64),
    /// Always finite.
    Float(f64),
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Number(pub(crate) N);

impl From<u64> for Number {
    fn from(v: u64) -> Self {
        Number(N::PosInt(v))
    }
}

impl From<i64> for Number {
    fn from(v: i64) -> Self {
        if v >= 0 {
            Number(N::PosInt(v as u64))
        } else {
            Number(N::NegInt(v))
        }
    }
}

impl From<f64> for Number {
    fn from(v: f64) -> Self {
        Number(N::Float(v))
    }
}

#[derive(Debug, Clone)]
pub enum ParamsValue {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Convertible(String),
    Object(HashMap<String, ParamsValue>),
    Array(Vec<ParamsValue>),
    UploadFile(UploadFile),
}

impl PartialEq for ParamsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Convertible(a), Self::Convertible(b)) => a == b,
            (Self::UploadFile(a), Self::UploadFile(b)) => a == b,
            _ => false,
        }
    }
}

fn event_to_params_value<T: JsonFeeder>(event: &JsonEvent, parser: &JsonParser<T>) -> ParamsValue {
    match event {
        JsonEvent::ValueString => {
            ParamsValue::Convertible(parser.current_str().unwrap().to_string())
        }
        JsonEvent::ValueInt => ParamsValue::Number(Number::from(
            parser.current_str().unwrap().parse::<i64>().unwrap(),
        )),
        JsonEvent::ValueFloat => ParamsValue::Number(Number::from(
            parser.current_str().unwrap().parse::<f64>().unwrap(),
        )),
        JsonEvent::ValueTrue => ParamsValue::Convertible("true".to_string()),
        JsonEvent::ValueFalse => ParamsValue::Convertible("false".to_string()),
        JsonEvent::ValueNull => ParamsValue::Null,
        _ => unreachable!(),
    }
}

pub fn parse_json(feeder: SliceJsonFeeder) -> Result<ParamsValue, JsonError> {
    let mut parser = JsonParser::new(feeder);

    let mut stack = vec![];
    let mut result = None;
    let mut current_key = None;

    while let Some(event) = parser.next_event().unwrap() {
        debug!("JSON event: {:?}", event);
        match event {
            JsonEvent::NeedMoreInput => {}

            JsonEvent::StartObject | JsonEvent::StartArray => {
                let v = if event == JsonEvent::StartObject {
                    ParamsValue::Object(HashMap::new())
                } else {
                    ParamsValue::Array(vec![])
                };
                stack.push((current_key.take(), v));
            }

            JsonEvent::EndObject | JsonEvent::EndArray => {
                let v = stack.pop().unwrap();
                if let Some((_, top)) = stack.last_mut() {
                    match top {
                        ParamsValue::Object(o) => {
                            if let Some(key) = v.0 {
                                o.insert(key, v.1);
                            }
                        }
                        ParamsValue::Array(a) => {
                            a.push(v.1);
                        }
                        _ => return Err(JsonError::SyntaxError),
                    }
                } else {
                    result = Some(v.1);
                }
            }

            JsonEvent::FieldName => {
                let str_result = parser.current_str().map_err(|_| JsonError::SyntaxError)?;
                current_key = Some(str_result.to_string());
            }

            JsonEvent::ValueString
            | JsonEvent::ValueInt
            | JsonEvent::ValueFloat
            | JsonEvent::ValueTrue
            | JsonEvent::ValueFalse
            | JsonEvent::ValueNull => {
                let v = event_to_params_value(&event, &parser);
                if let Some((_, top)) = stack.last_mut() {
                    match top {
                        ParamsValue::Array(a) => {
                            a.push(v);
                        }
                        ParamsValue::Object(o) => {
                            if let Some(key) = current_key.take() {
                                o.insert(key, v);
                            } else {
                                return Err(JsonError::SyntaxError);
                            }
                        }
                        _ => {
                            return Err(JsonError::SyntaxError);
                        }
                    }
                } else if result.is_none() {
                    result = Some(v);
                } else {
                    return Err(JsonError::SyntaxError);
                }
            }
        }
    }

    result.ok_or(JsonError::NoMoreInput)
}

#[derive(Debug)]
pub enum JsonError {
    SyntaxError,
    NoMoreInput,
    Other(String),
}

impl From<JsonError> for Error {
    fn from(err: JsonError) -> Self {
        match err {
            JsonError::SyntaxError => Error::DecodeError("JSON syntax error".to_string()),
            JsonError::NoMoreInput => Error::DecodeError("Incomplete JSON input".to_string()),
            JsonError::Other(msg) => Error::DecodeError(msg),
        }
    }
}

pub fn merge_json(
    feeder: SliceJsonFeeder,
    merged: &mut HashMap<String, Vec<ParamsValue>>,
) -> Result<(), JsonError> {
    let value = parse_json(feeder)?;
    debug!("Parsed JSON value: {:#?}", value);
    match value {
        ParamsValue::Object(obj) => {
            for (key, value) in obj {
                merged.insert(key, vec![value]);
            }
        }
        _ => {
            merged.insert("".to_string(), vec![value]);
        }
    }
    debug!("Final merged map: {:#?}", merged);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_from_u64() {
        let n = Number::from(42u64);
        assert!(matches!(n.0, N::PosInt(42)));

        let n = Number::from(0u64);
        assert!(matches!(n.0, N::PosInt(0)));

        let n = Number::from(u64::MAX);
        assert!(matches!(n.0, N::PosInt(u64::MAX)));
    }

    #[test]
    fn test_number_from_i64() {
        let n = Number::from(42i64);
        assert!(matches!(n.0, N::PosInt(42)));

        let n = Number::from(0i64);
        assert!(matches!(n.0, N::PosInt(0)));

        let n = Number::from(-42i64);
        assert!(matches!(n.0, N::NegInt(-42)));

        let n = Number::from(i64::MIN);
        assert!(matches!(n.0, N::NegInt(i64::MIN)));
    }

    #[test]
    fn test_number_from_f64() {
        let n = Number::from(42.0);
        assert!(matches!(n.0, N::Float(v) if v == 42.0));

        let n = Number::from(0.0);
        assert!(matches!(n.0, N::Float(v) if v == 0.0));

        let n = Number::from(-42.5);
        assert!(matches!(n.0, N::Float(v) if v == -42.5));

        let n = Number::from(f64::MIN_POSITIVE);
        assert!(matches!(n.0, N::Float(v) if v == f64::MIN_POSITIVE));

        let n = Number::from(f64::MAX);
        assert!(matches!(n.0, N::Float(v) if v == f64::MAX));
    }

    #[test]
    fn test_number_equality() {
        // Same type comparisons
        assert_eq!(Number::from(42u64), Number::from(42u64));
        assert_eq!(Number::from(-42i64), Number::from(-42i64));
        assert_eq!(Number::from(42.0), Number::from(42.0));

        // Different values
        assert_ne!(Number::from(42u64), Number::from(43u64));
        assert_ne!(Number::from(-42i64), Number::from(-43i64));
        assert_ne!(Number::from(42.0), Number::from(42.5));
    }

    #[test]
    fn test_parse_json_numbers() {
        // Test positive integers
        let json = r#"{"pos": 42, "zero": 0, "big": 9007199254740991}"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();
        if let ParamsValue::Object(map) = result {
            assert!(matches!(
                map["pos"],
                ParamsValue::Number(Number(N::PosInt(42)))
            ));
            assert!(matches!(
                map["zero"],
                ParamsValue::Number(Number(N::PosInt(0)))
            ));
            assert!(matches!(
                map["big"],
                ParamsValue::Number(Number(N::PosInt(9007199254740991)))
            ));
        } else {
            panic!("Expected object");
        }

        // Test negative integers
        let json = r#"{"neg": -42, "min": -9007199254740991}"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();
        if let ParamsValue::Object(map) = result {
            assert!(matches!(
                map["neg"],
                ParamsValue::Number(Number(N::NegInt(-42)))
            ));
            assert!(matches!(
                map["min"],
                ParamsValue::Number(Number(N::NegInt(-9007199254740991)))
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
        if let ParamsValue::Object(map) = result {
            assert!(
                matches!(map["float"], ParamsValue::Number(Number(N::Float(v))) if (v - 42.5).abs() < f64::EPSILON)
            );
            assert!(
                matches!(map["neg_float"], ParamsValue::Number(Number(N::Float(v))) if (v - (-42.5)).abs() < f64::EPSILON)
            );
            assert!(
                matches!(map["zero_float"], ParamsValue::Number(Number(N::Float(v))) if v.abs() < f64::EPSILON)
            );
            assert!(
                matches!(map["exp"], ParamsValue::Number(Number(N::Float(v))) if (v - 123000.0).abs() < f64::EPSILON)
            );
            assert!(
                matches!(map["neg_exp"], ParamsValue::Number(Number(N::Float(v))) if (v - (-0.0000123)).abs() < f64::EPSILON)
            );
        } else {
            panic!("Expected object");
        }

        // Test array of numbers
        let json = r#"[42, -42, 42.5, 0, -0.0]"#;
        let result = parse_json(SliceJsonFeeder::new(json.as_bytes())).unwrap();
        if let ParamsValue::Array(arr) = result {
            assert!(matches!(arr[0], ParamsValue::Number(Number(N::PosInt(42)))));
            assert!(matches!(
                arr[1],
                ParamsValue::Number(Number(N::NegInt(-42)))
            ));
            assert!(
                matches!(arr[2], ParamsValue::Number(Number(N::Float(v))) if (v - 42.5).abs() < f64::EPSILON)
            );
            assert!(matches!(arr[3], ParamsValue::Number(Number(N::PosInt(0)))));
            assert!(
                matches!(arr[4], ParamsValue::Number(Number(N::Float(v))) if v.abs() < f64::EPSILON)
            );
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
        if let ParamsValue::Object(map) = result {
            assert!(matches!(
                map["number"],
                ParamsValue::Number(Number(N::PosInt(42)))
            ));
            assert!(matches!(map["string"], ParamsValue::Convertible(ref s) if s == "hello"));
            assert!(matches!(map["bool"], ParamsValue::Convertible(ref s) if s == "true"));
            assert!(matches!(map["null"], ParamsValue::Null));

            if let ParamsValue::Array(arr) = &map["array"] {
                assert!(matches!(arr[0], ParamsValue::Number(Number(N::PosInt(1)))));
                assert!(matches!(arr[1], ParamsValue::Convertible(ref s) if s == "two"));
                assert!(matches!(arr[2], ParamsValue::Convertible(ref s) if s == "false"));
            } else {
                panic!("Expected array");
            }

            if let ParamsValue::Object(nested) = &map["nested"] {
                assert!(matches!(
                    nested["a"],
                    ParamsValue::Number(Number(N::PosInt(1)))
                ));
                assert!(matches!(
                    nested["b"],
                    ParamsValue::Number(Number(N::PosInt(2)))
                ));
            } else {
                panic!("Expected nested object");
            }
        } else {
            panic!("Expected object");
        }
    }
}
