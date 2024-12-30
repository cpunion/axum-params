use crate::{Number, N};

use super::Value;
use log::debug;
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use std::collections::HashMap;

struct ParamsValueVisitor;

impl<'de> Visitor<'de> for ParamsValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("any valid JSON value or upload file")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(v)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(v)))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(v)))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Value::XStr(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(Value::XStr(v))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut vec = Vec::new();
        while let Some(elem) = seq.next_element()? {
            vec.push(elem);
        }
        Ok(Value::Array(vec))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut values = HashMap::new();
        while let Some((key, value)) = map.next_entry()? {
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(ParamsValueVisitor)
    }
}

struct MapAccessor {
    map: std::collections::hash_map::IntoIter<String, Value>,
    current_value: Option<Value>,
}

impl MapAccessor {
    fn new(map: HashMap<String, Value>) -> Self {
        MapAccessor {
            map: map.into_iter(),
            current_value: None,
        }
    }
}

impl<'de> MapAccess<'de> for MapAccessor {
    type Error = serde::de::value::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        match self.map.next() {
            Some((key, value)) => {
                self.current_value = Some(value);
                seed.deserialize(key.into_deserializer()).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        match self.current_value.take() {
            Some(value) => seed.deserialize(value),
            None => Err(de::Error::custom("value is missing")),
        }
    }
}

struct SeqAccessor {
    seq: std::vec::IntoIter<Value>,
}

impl<'de> SeqAccess<'de> for SeqAccessor {
    type Error = serde::de::value::Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.seq.next() {
            Some(value) => seed.deserialize(value).map(Some),
            None => Ok(None),
        }
    }
}

impl<'de> Deserializer<'de> for Value {
    type Error = serde::de::value::Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(b),
            Value::Number(Number(n)) => match n {
                N::PosInt(i) => visitor.visit_u64(i),
                N::NegInt(i) => visitor.visit_i64(i),
                N::Float(f) => visitor.visit_f64(f),
            },
            Value::String(s) => visitor.visit_string(s),
            Value::Object(map) => visitor.visit_map(MapAccessor::new(map)),
            Value::Array(vec) => visitor.visit_seq(SeqAccessor {
                seq: vec.into_iter(),
            }),
            Value::XStr(s) => visitor.visit_string(s),
            Value::UploadFile(file) => {
                let map = HashMap::from([
                    ("name".to_string(), Value::String(file.name.clone())),
                    (
                        "content_type".to_string(),
                        Value::String(file.content_type.clone()),
                    ),
                    (
                        "temp_file_path".to_string(),
                        Value::String(file.temp_file_path.to_string()),
                    ),
                ]);
                visitor.visit_map(MapAccessor::new(map))
            }
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => match s.to_lowercase().as_str() {
                "true" | "1" | "on" | "yes" => visitor.visit_bool(true),
                "false" | "0" | "off" | "no" => visitor.visit_bool(false),
                _ => Err(de::Error::custom("invalid boolean value")),
            },
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_i8(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_i16(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_i32(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        debug!("deserialize_i64 self: {:?}", self);
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_i64(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_u8(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_u16(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_u32(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_u64(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        debug!("deserialize_f32 self: {:?}", self);
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_f32(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        debug!("deserialize_f64 self: {:?}", self);
        match self {
            Value::XStr(s) => s
                .parse()
                .map_err(de::Error::custom)
                .and_then(|v| visitor.visit_f64(v)),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::XStr(s) => {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => visitor.visit_char(c),
                    _ => Err(de::Error::custom("invalid char value")),
                }
            }
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    serde::forward_to_deserialize_any! {
        str string bytes byte_buf unit newtype_struct seq tuple
        tuple_struct map enum unit_struct struct identifier ignored_any
    }
}

pub use serde::de::{DeserializeSeed, IntoDeserializer};
