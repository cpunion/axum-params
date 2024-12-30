// Port from: https://github.com/rack/rack/blob/main/lib/rack/query_parser.rb

use form_urlencoded;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::Value;

const DEFAULT_PARAM_DEPTH_LIMIT: usize = 100;

#[derive(Debug)]
pub enum QueryParserError {
    ParameterTypeError(String),
    InvalidParameterError(String),
    ParamsTooDeepError(String),
}

impl fmt::Display for QueryParserError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            QueryParserError::ParameterTypeError(msg) => write!(f, "Parameter type error: {}", msg),
            QueryParserError::InvalidParameterError(msg) => write!(f, "Invalid parameter: {}", msg),
            QueryParserError::ParamsTooDeepError(msg) => write!(f, "Parameters too deep: {}", msg),
        }
    }
}

impl Error for QueryParserError {}

pub struct QueryParser {
    param_depth_limit: usize,
}

impl QueryParser {
    pub fn new(param_depth_limit: Option<usize>) -> Self {
        Self {
            param_depth_limit: param_depth_limit.unwrap_or(DEFAULT_PARAM_DEPTH_LIMIT),
        }
    }

    pub fn parse_nested_query<'a>(
        &self,
        qs: impl Into<Option<&'a str>>,
    ) -> Result<HashMap<String, Value>, QueryParserError> {
        let mut params = HashMap::new();
        self.parse_nested_query_into(&mut params, qs)?;
        Ok(params)
    }

    pub fn parse_nested_query_into<'a>(
        &self,
        params: &mut HashMap<String, Value>,
        qs: impl Into<Option<&'a str>>,
    ) -> Result<(), QueryParserError> {
        let qs = qs.into().unwrap_or("");

        if qs.is_empty() {
            return Ok(());
        }

        for pair in qs.split('&') {
            if pair.is_empty() {
                continue;
            }

            let (key, value) = match pair.split_once('=') {
                Some((k, v)) => {
                    let k = form_urlencoded::parse(k.as_bytes())
                        .next()
                        .map(|(k, _)| k.into_owned())
                        .unwrap_or_default();
                    let v = form_urlencoded::parse(v.as_bytes())
                        .next()
                        .map(|(v, _)| v.into_owned())
                        .unwrap_or_default();
                    (k, Some(v))
                }
                None => {
                    let k = form_urlencoded::parse(pair.as_bytes())
                        .next()
                        .map(|(k, _)| k.into_owned())
                        .unwrap_or_default();
                    (k, None)
                }
            };

            let value = Value::xstr_opt(value);
            self._normalize_params(params, &key, value, 0)?;
        }

        Ok(())
    }

    pub fn parse_nested_value<'a>(
        &self,
        params: &mut HashMap<String, Value>,
        key: impl Into<Option<&'a str>>,
        value: Value,
    ) -> Result<(), QueryParserError> {
        let key = key.into().unwrap_or("");

        if key.is_empty() {
            return Ok(());
        }

        self._normalize_params(params, key, value, 0)?;
        Ok(())
    }

    fn _normalize_params(
        &self,
        params: &mut HashMap<String, Value>,
        name: &str,
        v: Value,
        depth: usize,
    ) -> Result<Value, QueryParserError> {
        if depth >= self.param_depth_limit {
            return Err(QueryParserError::ParamsTooDeepError(
                "Parameters nested too deep".to_string(),
            ));
        }

        let (k, after) = if name.is_empty() {
            ("", "")
        } else if depth == 0 {
            if let Some(start) = name[1..].find('[') {
                let start = start + 1;
                (&name[..start], &name[start..])
            } else {
                (name, "")
            }
        } else if let Some(stripped) = name.strip_prefix("[]") {
            ("[]", stripped)
        } else if let Some(stripped) = name.strip_prefix("[") {
            if let Some(start) = stripped.find(']') {
                (&stripped[..start], &stripped[start + 1..])
            } else {
                (name, "")
            }
        } else {
            (name, "")
        };

        if k.is_empty() {
            return Ok(Value::Null);
        }

        if after.is_empty() {
            if k == "[]" && depth != 0 {
                return Ok(Value::Array(vec![v]));
            }
            params.insert(k.to_string(), v);
        } else if after == "[" {
            params.insert(name.to_string(), v);
        } else if after == "[]" {
            let entry = params
                .entry(k.to_string())
                .or_insert_with(|| Value::Array(Vec::new()));

            if let Value::Array(vec) = entry {
                vec.push(v);
            } else {
                return Err(QueryParserError::ParameterTypeError(format!(
                    "expected Array (got {}) for param `{}`",
                    entry.type_name(),
                    k
                )));
            }
        } else if let Some(after) = after.strip_prefix("[]") {
            // Recognize x[][y] (hash inside array) parameters
            let child_key = if !after.starts_with('[')
                || !after.ends_with(']')
                || after[1..after.len() - 1].contains('[')
                || after[1..after.len() - 1].contains(']')
                || after[1..after.len() - 1].is_empty()
            {
                after
            } else {
                &after[1..after.len() - 1]
            };

            let entry = params
                .entry(k.to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Value::Array(vec) = entry {
                let mut new_params = HashMap::new();
                if let Some(Value::Object(hash)) = vec.last_mut() {
                    if !params_hash_has_key(hash, child_key) {
                        let _ = self._normalize_params(&mut *hash, child_key, v.clone(), depth + 1);
                    } else {
                        let normalized = self._normalize_params(
                            &mut new_params,
                            child_key,
                            v.clone(),
                            depth + 1,
                        )?;
                        vec.push(normalized);
                    }
                } else {
                    let normalized =
                        self._normalize_params(&mut new_params, child_key, v.clone(), depth + 1)?;
                    vec.push(normalized);
                }
            } else {
                return Err(QueryParserError::ParameterTypeError(format!(
                    "expected Array (got {}) for param `{}`",
                    entry.type_name(),
                    k
                )));
            }
        } else {
            let entry = params
                .entry(k.to_string())
                .or_insert_with(|| Value::Object(HashMap::new()));

            if let Value::Object(hash) = entry {
                self._normalize_params(hash, after, v, depth + 1)?;
            } else {
                return Err(QueryParserError::ParameterTypeError(format!(
                    "expected Object (got {}) for param `{}`",
                    entry.type_name(),
                    k
                )));
            }
        }

        Ok(Value::Object(params.to_owned()))
    }
}

fn params_hash_has_key(hash: &HashMap<String, Value>, key: &str) -> bool {
    if key.contains("[]") {
        return false;
    }
    let parts: Vec<&str> = key
        .split(['[', ']'])
        .filter(|&part| !part.is_empty())
        .collect();

    let mut current = hash;
    for part in parts {
        if let Some(next) = current.get(part) {
            if let Value::Object(map) = next {
                current = map;
            } else {
                return true;
            }
        } else {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    // Port from: https://github.com/rack/rack/blob/main/test/spec_utils.rb

    use crate::query_parser::{QueryParser, Value, DEFAULT_PARAM_DEPTH_LIMIT};
    use maplit::hashmap;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    trait ParseTest {
        fn should_be(&self, expected: &str);
    }

    impl<'a> ParseTest for &'a str {
        fn should_be(&self, expected: &str) {
            let parser = QueryParser::new(None);
            assert_eq!(
                Value::Object(parser.parse_nested_query(*self).unwrap()),
                convert(expected)
            );
        }
    }

    fn convert(json: &str) -> Value {
        let json: serde_json::Value = serde_json::from_str(json).unwrap();
        Value::from(&json)
    }

    fn setup() {
        let _ = env_logger::builder().is_test(true).try_init();
    }

    #[test]
    fn parse_nil_as_an_empty_query_string() {
        let parser = QueryParser::new(None);
        assert_eq!(parser.parse_nested_query(None).unwrap(), HashMap::new());
    }

    #[test]
    fn raise_an_exception_if_the_params_are_too_deep() {
        let parser = QueryParser::new(Some(DEFAULT_PARAM_DEPTH_LIMIT));
        let deep_string = "[a]".repeat(DEFAULT_PARAM_DEPTH_LIMIT);
        let query_string = format!("foo{}=bar", deep_string);
        let result = parser.parse_nested_query(&*query_string);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_nested_query_strings_correctly() {
        setup();

        "foo".should_be(r#"{"foo": null}"#);
        "foo=".should_be(r#"{"foo": ""}"#);
        "foo=bar".should_be(r#"{"foo": "bar"}"#);
        "foo=\"bar\"".should_be(r#"{"foo": "\"bar\""}"#);

        "foo=bar&foo=quux".should_be(r#"{"foo": "quux"}"#);
        "foo&foo=".should_be(r#"{"foo": ""}"#);
        "foo=1&bar=2".should_be(r#"{"foo": "1", "bar": "2"}"#);
        "&foo=1&&bar=2".should_be(r#"{"foo": "1", "bar": "2"}"#);
        "foo&bar=".should_be(r#"{"foo": null, "bar": ""}"#);
        "foo=bar&baz=".should_be(r#"{"foo": "bar", "baz": ""}"#);
        "&foo=1&&bar=2".should_be(r#"{"foo": "1", "bar": "2"}"#);
        "foo&bar=".should_be(r#"{"foo": null, "bar": ""}"#);
        "foo=bar&baz=".should_be(r#"{"foo": "bar", "baz": ""}"#);
        "my+weird+field=q1%212%22%27w%245%267%2Fz8%29%3F"
            .should_be(r#"{"my weird field": "q1!2\"'w$5&7/z8)?"}"#);

        "a=b&pid%3D1234=1023".should_be(r#"{"pid=1234": "1023", "a": "b"}"#);

        "foo[]".should_be(r#"{"foo": [null]}"#);
        "foo[]=".should_be(r#"{"foo": [""]}"#);
        "foo[]=bar".should_be(r#"{"foo": ["bar"]}"#);
        "foo[]=bar&foo".should_be(r#"{"foo": null}"#);
        "foo[]=bar&foo[".should_be(r#"{"foo": ["bar"], "foo[": null}"#);
        "foo[]=bar&foo[=baz".should_be(r#"{"foo": ["bar"], "foo[": "baz"}"#);
        "foo[]=bar&foo[]".should_be(r#"{"foo": ["bar", null]}"#);
        "foo[]=bar&foo[]=".should_be(r#"{"foo": ["bar", ""]}"#);

        "foo[]=1&foo[]=2".should_be(r#"{"foo": ["1", "2"]}"#);
        "foo=bar&baz[]=1&baz[]=2&baz[]=3".should_be(r#"{"foo": "bar", "baz": ["1", "2", "3"]}"#);
        "foo[]=bar&baz[]=1&baz[]=2&baz[]=3"
            .should_be(r#"{"foo": ["bar"], "baz": ["1", "2", "3"]}"#);

        "x[y][z]".should_be(r#"{"x": { "y": { "z": null } }}"#);
        "x[y][z]=1".should_be(r#"{"x": { "y": { "z": "1"} }}"#);
        "x[y][z][]=1".should_be(r#"{"x": { "y": { "z": ["1"] } }}"#);
        "x[y][z]=1&x[y][z]=2".should_be(r#"{"x": { "y": { "z": "2"} }}"#);
        "x[y][z][]=1&x[y][z][]=2".should_be(r#"{"x": { "y": { "z": ["1", "2"] } }}"#);

        "x[y][][z]=1".should_be(r#"{"x": { "y": [{ "z": "1" }] }}"#);
        "x[y][][z][]=1".should_be(r#"{"x": { "y": [{ "z": ["1"] }] }}"#);
        "x[y][][z]=1&x[y][][w]=2".should_be(r#"{"x": { "y": [{ "z": "1", "w": "2" }] }}"#);

        "x[y][][v][w]=1".should_be(r#"{"x": { "y": [{ "v": { "w": "1" } }] }}"#);
        "x[y][][z]=1&x[y][][v][w]=2"
            .should_be(r#"{"x": { "y": [{ "z": "1", "v": { "w": "2" } }] }}"#);

        "x[y][][z]=1&x[y][][z]=2".should_be(r#"{"x": { "y": [{ "z": "1" }, { "z": "2" }] }}"#);
        "x[y][][z]=1&x[y][][w]=a&x[y][][z]=2&x[y][][w]=3"
            .should_be(r#"{"x": { "y": [{ "z": "1", "w": "a" }, { "z": "2", "w": "3" }] }}"#);

        "x[][y]=1&x[][z][w]=a&x[][y]=2&x[][z][w]=b".should_be(
            r#"{"x": [{ "y": "1", "z": { "w": "a" } }, { "y": "2", "z": { "w": "b" } }]}"#,
        );
        "x[][z][w]=a&x[][y]=1&x[][z][w]=b&x[][y]=2".should_be(
            r#"{"x": [{ "y": "1", "z": { "w": "a" } }, { "y": "2", "z": { "w": "b" } }]}"#,
        );

        "data[books][][data][page]=1&data[books][][data][page]=2".should_be(
            r#"{"data": { "books": [{ "data": { "page": "1" } }, { "data": { "page": "2" } }] }}"#,
        )
    }

    #[test]
    fn test_parse_empty() {
        let parser = QueryParser::new(None);
        assert_eq!(parser.parse_nested_query("").unwrap(), HashMap::new());
        assert_eq!(parser.parse_nested_query(None).unwrap(), HashMap::new());
    }

    #[test]
    fn test_parse_empty_key_value() {
        let parser = QueryParser::new(None);

        // Test empty key with value
        assert_eq!(parser.parse_nested_query("=value").unwrap(), hashmap! {});

        // Test key with empty value
        assert_eq!(
            parser.parse_nested_query("key=").unwrap(),
            hashmap! {
                "key".to_string() => Value::xstr("")
            }
        );

        // Test empty key-value pair
        assert_eq!(parser.parse_nested_query("=").unwrap(), hashmap! {});

        // Test key without value
        assert_eq!(
            parser.parse_nested_query("&key&").unwrap(),
            hashmap! {
                "key".to_string() => Value::Null
            }
        );
    }

    #[test]
    fn test_parse_duplicate_keys() {
        let parser = QueryParser::new(None);

        // Test duplicate keys (last value wins)
        assert_eq!(
            parser.parse_nested_query("foo=bar&foo=quux").unwrap(),
            hashmap! {
                "foo".to_string() => Value::xstr("quux")
            }
        );

        // Test key without value followed by key with value
        assert_eq!(
            parser.parse_nested_query("foo&foo=").unwrap(),
            hashmap! {
                "foo".to_string() => Value::xstr("")
            }
        );

        // Test key with value followed by key without value
        assert_eq!(
            parser.parse_nested_query("foo=bar&foo").unwrap(),
            hashmap! {
                "foo".to_string() => Value::Null
            }
        );
    }

    #[test]
    fn test_parse_array_edge_cases() {
        setup();
        let parser = QueryParser::new(None);

        // Test array followed by plain key
        assert_eq!(
            parser.parse_nested_query("foo[]=bar&foo").unwrap(),
            hashmap! {
                "foo".to_string() => Value::Null
            }
        );

        // Test array followed by incomplete array syntax
        assert_eq!(
            parser.parse_nested_query("foo[]=bar&foo[").unwrap(),
            hashmap! {
                "foo".to_string() => Value::Array(vec![Value::xstr("bar")]),
                "foo[".to_string() => Value::Null
            }
        );

        // Test array followed by incomplete array with value
        assert_eq!(
            parser.parse_nested_query("foo[]=bar&foo[=baz").unwrap(),
            hashmap! {
                "foo".to_string() => Value::Array(vec![Value::xstr("bar")]),
                "foo[".to_string() => Value::xstr("baz")
            }
        );
    }

    #[test]
    // can parse a query string with a key that has invalid UTF-8 encoded bytes
    fn test_parse_invalid_utf8() {
        let parser = QueryParser::new(None);
        let result = parser.parse_nested_query("foo%81E=1").unwrap_or_default();
        assert_eq!(result.len(), 1);
        let key = result.keys().next().unwrap().as_bytes();
        assert_eq!(key, b"foo\xEF\xBF\xBDE");
    }

    #[test]
    fn only_moves_to_a_new_array_when_the_full_key_has_been_seen() {
        "x[][y][][z]=1&x[][y][][w]=2".should_be(r#"{"x": [{ "y": [{ "z": "1", "w": "2" }] }]}"#);
        "x[][id]=1&x[][y][a]=5&x[][y][b]=7&x[][z][id]=3&x[][z][w]=0&x[][id]=2&x[][y][a]=6&x[][y][b]=8&x[][z][id]=4&x[][z][w]=0"
            .should_be(
                r#"
            {
                "x": [
                    { "id": "1", "y": { "a": "5", "b": "7" }, "z": { "id": "3", "w": "0" } },
                    { "id": "2", "y": { "a": "6", "b": "8" }, "z": { "id": "4", "w": "0" } }
                ]
            }"#,
            );
    }

    #[test]
    fn handles_unexpected_use_of_brackets_in_parameter_keys_as_normal_characters() {
        "[]=1&[a]=2&b[=3&c]=4".should_be(r#"{"[]": "1", "[a]": "2", "b[": "3", "c]": "4"}"#);
        "d[[]=5&e][]=6&f[[]]=7"
            .should_be(r#"{"d": {"[":  "5"}, "e]":  ["6"], "f":  { "[":  { "]":  "7" } }}"#);
        "g[h]i=8&j[k]l[m]=9"
            .should_be(r#"{"g": { "h": { "i":  "8" } }, "j":  { "k":  { "l[m]": "9" } }}"#);
        "l[[[[[[[[]]]]]]]=10".should_be(r#"{"l": {"[[[[[[[": {"]]]]]]": "10"}}}"#);
    }
}
