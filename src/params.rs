use crate::{parse_json, Error, QueryParser, UploadFile, Value};
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

        let parser = QueryParser::new(None);
        let mut merged_params = HashMap::new();

        // Extract path parameters
        if let Ok(Path(params)) =
            Path::<HashMap<String, String>>::from_request_parts(&mut parts, state).await
        {
            debug!("params: {:?}", params);

            for (key, value) in params {
                parser
                    .parse_nested_value(&mut merged_params, key.as_str(), Value::xstr(value))
                    .map_err(|e| {
                        Error::DecodeError(format!("Failed to parse path parameters: {}", e))
                    })?;
            }
        }

        debug!("merged path params: {:?}", merged_params);
        debug!("parts.uri: {:?}", parts.uri);
        debug!("parts.uri.query(): {:?}", parts.uri.query());

        // Extract query parameters from URI
        if let Some(query) = parts.uri.query() {
            parser
                .parse_nested_query_into(&mut merged_params, query)
                .map_err(|e| {
                    Error::DecodeError(format!("Failed to parse query parameters: {}", e))
                })?;
        }

        debug!("merged query params: {:?}", merged_params);

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
                        let value = parse_json(feeder)?;
                        debug!("parsed json: {:#?}", value);
                        merged_params = value.merge_into(merged_params).map_err(|e| {
                            debug!("Failed to merge JSON data: {e:?}");
                            Error::DecodeError(format!("Failed to merge JSON data: {e:?}"))
                        })?;
                        debug!("merged json: {:#?}", merged_params);
                    }
                    ct if ct.starts_with("application/x-www-form-urlencoded") => {
                        if !is_get_or_head {
                            let bytes = to_bytes(body, usize::MAX).await.map_err(|e| {
                                Error::ReadError(format!(
                                    "Failed to read form-urlencoded request body: {e}"
                                ))
                            })?;
                            parser
                                .parse_nested_query_into(
                                    &mut merged_params,
                                    String::from_utf8_lossy(&bytes).as_ref(),
                                )
                                .map_err(|e| {
                                    Error::DecodeError(format!(
                                        "Failed to parse form-urlencoded body: {}",
                                        e
                                    ))
                                })?
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
                                        "Failed to read JSON field bytes: {e}",
                                    ))
                                })?;
                                debug!(
                                    "JSON field bytes: {}",
                                    String::from_utf8(bytes.to_vec()).unwrap()
                                );
                                let feeder = SliceJsonFeeder::new(&bytes);
                                let value = parse_json(feeder)?;
                                debug!("Parsed JSON field: {:#?}", value);
                                let name = name.unwrap_or_default();
                                if name.is_empty() {
                                    merged_params =
                                        value.merge_into(merged_params).map_err(|e| {
                                            debug!("Failed to merge JSON field: {e:?}");
                                            Error::DecodeError(format!(
                                                "Failed to merge JSON field: {e:?}",
                                            ))
                                        })?;
                                } else {
                                    parser
                                        .parse_nested_value(
                                            &mut merged_params,
                                            name.as_str(),
                                            value,
                                        )
                                        .map_err(|e| {
                                            Error::DecodeError(format!(
                                                "Failed to parse JSON field: {}",
                                                e
                                            ))
                                        })?;
                                }

                                debug!("Merged JSON field: {:#?}", merged_params);
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

                                    let file = Value::UploadFile(UploadFile {
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
                                    });
                                    parser
                                        .parse_nested_value(&mut merged_params, name.as_str(), file)
                                        .map_err(|e| {
                                            Error::DecodeError(format!(
                                                "Failed to parse file upload field: {}",
                                                e
                                            ))
                                        })?;

                                    // Store the temp file
                                    temp_files.push(temp_file);
                                } else {
                                    // Handle text field
                                    let value = field.text().await.map_err(|e| {
                                        debug!("Failed to read text field: {}", e);
                                        Error::ReadError(format!("Failed to read text field: {e}",))
                                    })?;
                                    parser
                                        .parse_nested_value(
                                            &mut merged_params,
                                            name.as_str(),
                                            Value::xstr(value),
                                        )
                                        .map_err(|e| {
                                            Error::DecodeError(format!(
                                                "Failed to parse text field: {}",
                                                e
                                            ))
                                        })?;
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

        debug!("merged: {:?}", merged_params);
        T::deserialize(Value::Object(merged_params))
            .map_err(|e| Error::DecodeError(format!("Failed to deserialize parameters: {e}")))
            .map(|payload| Params(payload, temp_files))
    }
}
