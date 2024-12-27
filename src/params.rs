use crate::Error;
use ::serde::{de::DeserializeOwned, Deserialize};
use axum::{
    async_trait,
    body::to_bytes,
    extract::{FromRequest, FromRequestParts, Path, Request},
    http::{self},
};
use log::debug;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use tempfile::NamedTempFile;
use tokio::fs::File;
use url::form_urlencoded;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UploadFile {
    pub name: String,
    pub content_type: String,
    pub(crate) temp_file_path: String,
}

impl PartialEq for UploadFile {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.content_type == other.content_type
    }
}

impl UploadFile {
    pub async fn open(&self) -> Result<File, std::io::Error> {
        File::open(&self.temp_file_path).await
    }
}

#[derive(Debug, Clone)]
pub enum ParamsValue {
    Object(HashMap<String, ParamsValue>),
    Array(Vec<ParamsValue>),
    Json(Value),
    Convertable(String),
    UploadFile(UploadFile),
}

impl PartialEq for ParamsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Json(a), Self::Json(b)) => a == b,
            (Self::Convertable(a), Self::Convertable(b)) => a == b,
            (Self::UploadFile(a), Self::UploadFile(b)) => a == b,
            _ => false,
        }
    }
}

impl From<Value> for ParamsValue {
    fn from(value: Value) -> Self {
        ParamsValue::Json(value)
    }
}

#[derive(Debug, Default)]
pub struct Params<T>(pub T, pub Vec<NamedTempFile>);

#[async_trait]
impl<T, S> FromRequest<S> for Params<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = crate::Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let is_get_or_head =
            req.method() == http::Method::GET || req.method() == http::Method::HEAD;
        let (mut parts, body) = req.into_parts();

        // Start with empty vec to preserve multiple values for the same key
        let mut merged: HashMap<String, Vec<ParamsValue>> = HashMap::new();

        // Extract path parameters
        if let Ok(Path(params)) =
            Path::<HashMap<String, String>>::from_request_parts(&mut parts, state).await
        {
            debug!("params: {:?}", params);
            for (key, value) in params {
                // Remove query string from path parameter if present
                let value = if let Some(pos) = value.find('?') {
                    value[..pos].to_string()
                } else {
                    value
                };
                merged
                    .entry(key)
                    .or_default()
                    .push(ParamsValue::Convertable(value));
            }
        }

        debug!("merged path params: {:?}", merged);
        debug!("parts.uri: {:?}", parts.uri);
        debug!("parts.uri.query(): {:?}", parts.uri.query());

        // Extract query parameters from URI
        if let Some(query) = parts.uri.query() {
            let params: Vec<_> = form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect();
            debug!("query params: {:?}", params);
            for (key, value) in params {
                merged
                    .entry(key)
                    .or_default()
                    .push(ParamsValue::Convertable(value));
            }
        }

        debug!("merged query params: {:?}", merged);

        let mut temp_files = Vec::new();
        debug!(
            "Content-Type: {:?}",
            parts.headers.get(http::header::CONTENT_TYPE)
        );
        if let Some(content_type) = parts.headers.get(http::header::CONTENT_TYPE) {
            debug!("Content-Type: {:?}", content_type);
            if let Ok(content_type) = content_type.to_str() {
                match content_type {
                    ct if ct.starts_with("application/json") => {
                        let bytes = to_bytes(body, usize::MAX).await.map_err(|e| {
                            debug!("Failed to read JSON request body: {}", e);
                            Error::DecodeError(format!("Failed to read JSON request body: {}", e))
                        })?;
                        if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&bytes) {
                            for (k, v) in map {
                                merged.entry(k).or_default().push(ParamsValue::from(v));
                            }
                        } else {
                            debug!(
                                "Failed to parse JSON from request body: {:?}",
                                String::from_utf8_lossy(&bytes)
                            );
                        }
                    }
                    ct if ct.starts_with("application/x-www-form-urlencoded") => {
                        if !is_get_or_head {
                            let bytes = to_bytes(body, usize::MAX).await.map_err(|e| {
                                Error::ReadError(format!(
                                    "Failed to read form-urlencoded request body: {e}"
                                ))
                            })?;
                            if let Ok(map) =
                                serde_urlencoded::from_bytes::<HashMap<String, String>>(&bytes)
                                    .map_err(|err| -> Error {
                                        debug!(
                                            "Failed to deserialize form-urlencoded data: {}",
                                            err
                                        );
                                        Error::DecodeError(format!(
                                            "Failed to deserialize form: {err}",
                                        ))
                                    })
                            {
                                for (k, v) in map {
                                    merged
                                        .entry(k)
                                        .or_default()
                                        .push(ParamsValue::Convertable(v));
                                }
                            }
                        }
                    }
                    ct if ct.starts_with("multipart/form-data") => {
                        let boundary = multer::parse_boundary(content_type).map_err(|e| {
                            debug!("Failed to parse multipart boundary: {}", e);
                            Error::DecodeError(format!("Failed to parse multipart boundary: {e}"))
                        })?;
                        let mut multipart =
                            multer::Multipart::new(body.into_data_stream(), boundary);

                        while let Some(mut field) = multipart.next_field().await.map_err(|e| {
                            debug!("Failed to read multipart field: {}", e);
                            Error::ReadError(format!("Failed to read multipart field: {e}"))
                        })? {
                            let content_type = field
                                .content_type()
                                .map(|ct| ct.to_string())
                                .unwrap_or_else(|| "application/octet-stream".to_string());
                            if content_type == "application/json" {
                                // TODO: parse JSON into merged
                            }
                            if let Some(name) = field.name() {
                                let name = name.to_string();

                                // Check if this is a file upload field
                                if field.file_name().is_some() {
                                    // Handle file upload
                                    let temp_file = NamedTempFile::new().map_err(|e| {
                                        Error::IOError(format!("Failed to create temp file: {e}",))
                                    })?;
                                    debug!("Created temp file at: {:?}", temp_file.path());

                                    let mut file = tokio::fs::OpenOptions::new()
                                        .write(true)
                                        .open(temp_file.path())
                                        .await
                                        .map_err(|e| {
                                            debug!("Failed to open temp file for writing: {}", e);
                                            Error::IOError(
                                                format!("Failed to open temp file: {e}",),
                                            )
                                        })?;

                                    let mut total_bytes = 0;
                                    while let Some(chunk) = field.chunk().await.map_err(|e| {
                                        debug!("Failed to read multipart field chunk: {}", e);
                                        Error::ReadError(format!(
                                            "Failed to read multipart field chunk: {e}",
                                        ))
                                    })? {
                                        total_bytes += chunk.len();
                                        debug!("Writing chunk of size {} bytes", chunk.len());
                                        tokio::io::copy(&mut &*chunk, &mut file).await.map_err(
                                            |e| {
                                                debug!("Failed to write chunk to temp file: {}", e);
                                                Error::IOError(format!(
                                                    "Failed to write to temp file: {e}",
                                                ))
                                            },
                                        )?;
                                    }

                                    // Sync the file to disk
                                    file.sync_all().await.map_err(|e| {
                                        debug!("Failed to sync temp file: {}", e);
                                        Error::IOError(format!("Failed to sync temp file: {e}",))
                                    })?;

                                    debug!("Total bytes written to file: {}", total_bytes);

                                    merged
                                        .entry(name)
                                        .or_default()
                                        .push(ParamsValue::UploadFile(UploadFile {
                                            name: field.file_name().unwrap().to_string(),
                                            content_type: field
                                                .content_type()
                                                .map(|ct| ct.to_string())
                                                .unwrap_or_else(|| {
                                                    "application/octet-stream".to_string()
                                                }),
                                            temp_file_path: temp_file
                                                .path()
                                                .to_string_lossy()
                                                .to_string(),
                                        }));

                                    // Store the temp file
                                    temp_files.push(temp_file);
                                } else {
                                    // Handle text field
                                    let value = field.text().await.map_err(|e| {
                                        debug!("Failed to read text field: {}", e);
                                        Error::ReadError(format!("Failed to read text field: {e}",))
                                    })?;
                                    merged
                                        .entry(name)
                                        .or_default()
                                        .push(ParamsValue::Convertable(value));
                                }
                            }
                        }
                    }
                    ct => {
                        debug!("Unhandled content type: {}", ct);
                    }
                }
            }
        }
        let merged = process_nested_params(merged);
        debug!("merged: {:?}", merged);
        T::deserialize(merged)
            .map_err(|e| Error::DecodeError(format!("Failed to deserialize parameters: {e}")))
            .map(|payload| Params(payload, temp_files))
    }
}

pub fn process_nested_params(grouped: HashMap<String, Vec<ParamsValue>>) -> ParamsValue {
    debug!("Starting process_nested_params with input: {:?}", grouped);
    let mut result = HashMap::new();

    // Process each group
    for (key, values) in grouped {
        let parts = parse_key_parts(&key);
        if parts.is_empty() {
            continue;
        }

        // Get the value from insert_nested_values and store it in the result
        let value = insert_nested_values(&mut result, &parts, values);
        if parts.len() == 1 {
            result.insert(parts[0].clone(), value);
        }
    }

    ParamsValue::Object(result)
}

fn insert_nested_values(
    map: &mut HashMap<String, ParamsValue>,
    parts: &[String],
    values: Vec<ParamsValue>,
) -> ParamsValue {
    if parts.is_empty() {
        return values
            .into_iter()
            .next()
            .unwrap_or_else(|| ParamsValue::Object(HashMap::new()));
    }

    let key = &parts[0];
    if parts.len() == 1 {
        let value = if values.len() == 1 {
            values.into_iter().next().unwrap()
        } else {
            ParamsValue::Array(values)
        };
        return value;
    }

    // Check if next part indicates an array
    let is_array = parts
        .get(1)
        .map(|p| p.is_empty() || p.parse::<usize>().is_ok())
        .unwrap_or(false);

    let entry = map.entry(key.clone()).or_insert_with(|| {
        if is_array {
            ParamsValue::Array(Vec::new())
        } else {
            ParamsValue::Object(HashMap::new())
        }
    });

    match entry {
        ParamsValue::Object(nested_map) => {
            let value = insert_nested_values(nested_map, &parts[1..], values);
            if parts.len() == 2 {
                nested_map.insert(parts[1].clone(), value.clone());
            }
            ParamsValue::Object(nested_map.clone())
        }
        ParamsValue::Array(vec) => {
            if parts.get(1).map(|p| p.is_empty()).unwrap_or(false) {
                vec.extend(values);
            } else if let Some(Ok(index)) = parts.get(1).map(|p| p.parse::<usize>()) {
                while vec.len() <= index {
                    vec.push(ParamsValue::Object(HashMap::new()));
                }

                if parts.len() == 2 {
                    if let Some(value) = values.into_iter().next() {
                        vec[index] = value;
                    }
                } else if let ParamsValue::Object(nested_map) = &mut vec[index] {
                    let value = insert_nested_values(nested_map, &parts[2..], values);
                    if parts.len() == 3 {
                        nested_map.insert(parts[2].clone(), value);
                    }
                }
            }
            ParamsValue::Array(vec.clone())
        }
        _ => values
            .into_iter()
            .next()
            .unwrap_or_else(|| ParamsValue::Object(HashMap::new())),
    }
}

fn parse_key_parts(key: &str) -> Vec<String> {
    debug!("Parsing key parts for: {}", key);
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_brackets = false;

    for c in key.chars() {
        match c {
            '[' => {
                if !current.is_empty() {
                    debug!("Adding part before bracket: {}", current);
                    parts.push(current.clone());
                    current.clear();
                }
                in_brackets = true;
            }
            ']' => {
                if in_brackets {
                    if current.is_empty() {
                        debug!("Found empty brackets");
                        parts.push(String::new());
                    } else {
                        debug!("Adding part from bracket: {}", current);
                        parts.push(current.clone());
                    }
                    current.clear();
                }
                in_brackets = false;
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        debug!("Adding remaining part: {}", current);
        parts.push(current);
    }

    debug!("Final parsed parts: {:?}", parts);
    parts
}
