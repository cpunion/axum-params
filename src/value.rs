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

pub trait IntoNumber {
    fn into_number(self) -> Number;
}

impl IntoNumber for u64 {
    fn into_number(self) -> Number {
        Number::from(self)
    }
}

impl IntoNumber for i64 {
    fn into_number(self) -> Number {
        Number::from(self)
    }
}

impl IntoNumber for f64 {
    fn into_number(self) -> Number {
        Number::from(self)
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    XStr(String),
    Object(HashMap<String, Value>),
    Array(Vec<Value>),
    UploadFile(UploadFile),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::XStr(a), b) => match b {
                Self::XStr(b) => a == b,
                Self::String(b) => a == b,
                _ => false,
            },
            (a, Self::XStr(b)) => match a {
                Self::XStr(a) => a == b,
                Self::String(a) => a == b,
                _ => false,
            },
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::UploadFile(a), Self::UploadFile(b)) => a == b,
            _ => false,
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

impl Value {
    pub fn merge(self, other: Value) -> Result<Value, Error> {
        match (self, other) {
            // Object + Object = Merged object
            (Value::Object(mut a), Value::Object(b)) => {
                a.extend(b);
                Ok(Value::Object(a))
            }
            // Array + Array = Combined array
            (Value::Array(mut a), Value::Array(b)) => {
                a.extend(b);
                Ok(Value::Array(a))
            }
            // Array + Any = Array with new element
            (Value::Array(mut a), other) => {
                a.push(other);
                Ok(Value::Array(a))
            }
            // Any + Array = Array with new element at start
            (value, Value::Array(mut arr)) => {
                arr.insert(0, value);
                Ok(Value::Array(arr))
            }
            // Null + Any = Any
            (Value::Null, other) => Ok(other),
            // Any + Null = Any
            (value, Value::Null) => Ok(value),
            // Incompatible types
            (a, b) => Err(Error::MergeError(format!(
                "Cannot merge {} with {}",
                a.type_name(),
                b.type_name()
            ))),
        }
    }

    pub fn merge_into(
        self,
        mut a: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>, Error> {
        match self {
            Value::Object(b) => {
                a.extend(b);
                Ok(a)
            }
            _ => Err(Error::MergeError(format!(
                "Cannot merge {} with object",
                self.type_name()
            ))),
        }
    }

    pub fn xstr<T: Into<String>>(v: T) -> Value {
        Value::XStr(v.into())
    }

    pub fn xstr_opt<T: Into<String>>(v: Option<T>) -> Value {
        match v {
            Some(v) => Value::XStr(v.into()),
            None => Value::Null,
        }
    }

    pub fn number<T: IntoNumber>(v: T) -> Value {
        Value::Number(v.into_number())
    }

    pub fn bool(v: bool) -> Value {
        Value::Bool(v)
    }

    pub fn null() -> Value {
        Value::Null
    }

    pub fn array(v: Vec<Value>) -> Value {
        Value::Array(v)
    }

    pub fn object(v: HashMap<String, Value>) -> Value {
        Value::Object(v)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Object(_) => "object",
            Value::Array(_) => "array",
            Value::XStr(_) => "string",
            Value::UploadFile(_) => "file",
        }
    }
}

fn event_to_params_value<T: JsonFeeder>(event: &JsonEvent, parser: &JsonParser<T>) -> Value {
    match event {
        JsonEvent::ValueString => Value::XStr(parser.current_str().unwrap().to_string()),
        JsonEvent::ValueInt => Value::Number(Number::from(
            parser.current_str().unwrap().parse::<i64>().unwrap(),
        )),
        JsonEvent::ValueFloat => Value::Number(Number::from(
            parser.current_str().unwrap().parse::<f64>().unwrap(),
        )),
        JsonEvent::ValueTrue => Value::XStr("true".to_string()),
        JsonEvent::ValueFalse => Value::XStr("false".to_string()),
        JsonEvent::ValueNull => Value::Null,
        _ => unreachable!(),
    }
}

pub fn parse_json(feeder: SliceJsonFeeder) -> Result<Value, JsonError> {
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
                        Value::Array(a) => {
                            a.push(v);
                        }
                        Value::Object(o) => {
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
            assert!(matches!(map["string"], Value::XStr(ref s) if s == "hello"));
            assert!(matches!(map["bool"], Value::XStr(ref s) if s == "true"));
            assert!(matches!(map["null"], Value::Null));

            if let Value::Array(arr) = &map["array"] {
                assert!(matches!(arr[0], Value::Number(Number(N::PosInt(1)))));
                assert!(matches!(arr[1], Value::XStr(ref s) if s == "two"));
                assert!(matches!(arr[2], Value::XStr(ref s) if s == "false"));
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
}
