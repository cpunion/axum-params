use crate::{merge_json, Error, ParamsValue, UploadFile};
use ::serde::de::DeserializeOwned;
use actson::feeder::SliceJsonFeeder;
use axum::{
    async_trait,
    body::to_bytes,
    extract::{FromRequest, FromRequestParts, Path, Request},
    http::{self},
};
use log::debug;
use std::collections::HashMap;
use tempfile::NamedTempFile;
use url::form_urlencoded;

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
                    .push(ParamsValue::Convertible(value));
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
                    .push(ParamsValue::Convertible(value));
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
                        let feeder = SliceJsonFeeder::new(&bytes);
                        merge_json(feeder, &mut merged)?;
                        debug!("merged json: {:#?}", merged);
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
                                        .push(ParamsValue::Convertible(v));
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
                            Error::ReadError(format!("Failed to read multipart field: {e}",))
                        })? {
                            let content_type = field
                                .content_type()
                                .map(|ct| ct.to_string())
                                .unwrap_or_else(|| "application/octet-stream".to_string());
                            if content_type == "application/json" {
                                let name = field.name().map(|s| s.to_string());
                                let bytes = field.bytes().await.map_err(|e| {
                                    debug!("Failed to read JSON field bytes: {}", e);
                                    Error::ReadError(format!(
                                        "Failed to read JSON field bytes: {e}"
                                    ))
                                })?;
                                debug!(
                                    "JSON field bytes: {}",
                                    String::from_utf8(bytes.to_vec()).unwrap()
                                );
                                let feeder = SliceJsonFeeder::new(&bytes);
                                let mut temp_map = HashMap::new();
                                merge_json(feeder, &mut temp_map)?;
                                debug!("Parsed JSON field: {:#?}", temp_map);
                                let name = name.unwrap_or_default();
                                if name.is_empty() {
                                    // If no field name, clear all existing data and merge only the JSON data
                                    for (key, values) in temp_map {
                                        merged.insert(key, values);
                                    }
                                    debug!("Merged JSON field: {:#?}", merged);
                                    continue;
                                }

                                // If we have a single value in the map with key "", use it as the value
                                if let Some(values) = temp_map.get("") {
                                    if values.len() == 1 {
                                        merged.insert(name, values.clone());
                                        continue;
                                    }
                                }

                                // Otherwise, process the map as nested parameters
                                let value = process_nested_params(temp_map);
                                merged.insert(name, vec![value]);

                                debug!("Merged JSON field: {:#?}", merged);
                                continue;
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
                                        .push(ParamsValue::Convertible(value));
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
        debug!("Processing key: {} with values: {:?}", key, values);
        let parts = parse_key_parts(&key);
        debug!("Parsed parts: {:?}", parts);
        if parts.is_empty() {
            continue;
        }

        // For single-part keys, directly add the value
        if parts.len() == 1 {
            let value = if values.len() == 1 {
                values.into_iter().next().unwrap()
            } else {
                ParamsValue::Array(values)
            };
            debug!(
                "Adding single-part key: {} with value: {:?}",
                parts[0], value
            );
            result.insert(parts[0].clone(), value);
            continue;
        }

        // Get the value from insert_nested_values and store it in the result
        let value = insert_nested_values(&mut result, &parts, values);
        if parts.len() == 1 {
            debug!("Adding nested key: {} with value: {:?}", parts[0], value);
            result.insert(parts[0].clone(), value);
        }
    }

    debug!("Final result: {:?}", result);
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
