use std::collections::HashMap;

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
}
