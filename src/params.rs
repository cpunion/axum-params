use crate::{Error, UploadFile, Value, parse_json, query_parser::QueryParser};
use ::serde::de::DeserializeOwned;
use actson::feeder::SliceJsonFeeder;
use axum::{
    body::to_bytes,
    extract::{FromRequest, FromRequestParts, Path, Request},
    http::{self},
};
use log::debug;
use std::collections::HashMap;
use tempfile::NamedTempFile;

#[derive(Debug, Default)]
pub struct Params<T>(pub T, pub Vec<NamedTempFile>);

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use ::serde::{Deserialize, Serialize};
    use axum::{
        Json, Router,
        body::Body,
        extract::{FromRequest, Request},
        http::StatusCode,
        http::{self, HeaderValue},
        response::IntoResponse,
        routing::{get, post},
    };
    use axum_test::{
        TestServer,
        multipart::{MultipartForm, Part},
    };
    use log::debug;
    use serde_json::json;
    use tokio::io::AsyncReadExt;

    pub fn setup() {
        let _ = env_logger::builder().is_test(true).try_init();
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct TestParams {
        id: i32,
        name: String,
        #[serde(default)]
        extra: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct UploadFileResponse {
        name: String,
        content_type: String,
        content: String,
    }

    #[axum::debug_handler]
    async fn test_params_handler(Params(test, _): Params<TestParams>) -> impl IntoResponse {
        (StatusCode::OK, serde_json::to_string(&test).unwrap())
    }

    #[derive(Debug, Deserialize)]
    struct FileUploadParams {
        title: String,
        description: Option<String>,
        file: UploadFile,
    }

    #[axum::debug_handler]
    async fn file_upload_handler(Params(upload, _): Params<FileUploadParams>) -> impl IntoResponse {
        let mut temp_file = upload.file.open().await.unwrap();
        debug!(
            "Reading file from: {:?}",
            temp_file.metadata().await.unwrap()
        );
        let mut content = String::new();
        temp_file.read_to_string(&mut content).await.unwrap();
        debug!("Read {} bytes from file", content.len());
        debug!("File content: {:?}", content);

        let response = json!({
            "title": upload.title,
            "description": upload.description,
            "file_name": upload.file.name,
            "file_content": content,
        });
        (StatusCode::OK, serde_json::to_string(&response).unwrap())
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Attachment {
        file: UploadFile,
        name: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct CreatePostParams {
        title: String,
        content: String,
        tags: Vec<String>,
        cover: UploadFile,
        attachments: Vec<Attachment>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct AttachmentResponse {
        name: String,
        content_type: String,
        content: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct CreatePostResponse {
        title: String,
        content: String,
        tags: Vec<String>,
        cover: UploadFileResponse,
        attachments: Vec<AttachmentResponse>,
    }

    #[axum::debug_handler]
    async fn test_nested_params_handler(
        Params(post, _): Params<CreatePostParams>,
    ) -> Result<Json<CreatePostResponse>, Error> {
        let mut cover_file = post.cover.open().await.unwrap();
        let mut cover_content = String::new();
        cover_file.read_to_string(&mut cover_content).await.unwrap();

        let response = CreatePostResponse {
            title: post.title,
            content: post.content,
            tags: post.tags,
            cover: UploadFileResponse {
                name: post.cover.name,
                content_type: post.cover.content_type,
                content: cover_content,
            },
            attachments: Vec::new(),
        };

        let attachments =
            futures_util::future::join_all(post.attachments.into_iter().map(|a| async {
                let mut file = a.file.open().await.unwrap();
                let mut content = String::new();
                file.read_to_string(&mut content).await.unwrap();

                AttachmentResponse {
                    name: a.name,
                    content_type: a.file.content_type,
                    content,
                }
            }))
            .await;

        let mut response = response;
        response.attachments = attachments;

        Ok(Json(response))
    }

    #[tokio::test]
    async fn test_path_params() {
        let app = Router::new().route("/users/{id}", get(test_params_handler));
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/users/123")
            .add_query_params(&[("name", "test")])
            .await;
        println!("response: {:?}", response);
        assert_eq!(response.status_code(), StatusCode::OK);

        let body = response.text();
        let params: TestParams = serde_json::from_str(&body).unwrap();
        assert_eq!(params.id, 123);
        assert_eq!(params.name, "test");
        assert_eq!(params.extra, None);
    }

    #[tokio::test]
    async fn test_json_body() {
        let app = Router::new().route("/api/test", post(test_params_handler));
        let server = TestServer::new(app).unwrap();

        let json_data = json!({
            "id": 123,
            "name": "test",
            "extra": "data"
        });

        let response = server.post("/api/test").json(&json_data).await;
        assert_eq!(response.status_code(), StatusCode::OK);

        let body = response.text();
        let params: TestParams = serde_json::from_str(&body).unwrap();
        assert_eq!(params.id, 123);
        assert_eq!(params.name, "test");
        assert_eq!(params.extra, Some("data".to_string()));
    }

    #[tokio::test]
    async fn test_form_data() {
        let app = Router::new().route("/api/test", post(test_params_handler));
        let server = TestServer::new(app).unwrap();

        let response = server
            .post("/api/test")
            .form(&[("id", "123"), ("name", "test"), ("extra", "form_data")])
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);

        let body = response.text();
        let params: TestParams = serde_json::from_str(&body).unwrap();
        assert_eq!(params.id, 123);
        assert_eq!(params.name, "test");
        assert_eq!(params.extra, Some("form_data".to_string()));
    }

    #[tokio::test]
    async fn test_multipart_form() {
        let app = Router::new().route("/api/upload", post(file_upload_handler));
        let server = TestServer::new(app).unwrap();

        let test_content = b"Hello, World!";
        let test_content_str = String::from_utf8_lossy(test_content).to_string();

        let response = server
            .post("/api/upload")
            .add_header(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("multipart/form-data; boundary=X-BOUNDARY"),
            )
            .multipart(
                MultipartForm::new()
                    .add_text("title", "Test Upload")
                    .add_text("description", "A test file upload")
                    .add_part(
                        "file",
                        Part::bytes(test_content.to_vec()).file_name("test.txt"),
                    ),
            )
            .await;

        assert_eq!(response.status_code(), StatusCode::OK);
        let body = response.text();
        let result: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(result["title"], "Test Upload");
        assert_eq!(result["description"], "A test file upload");
        assert_eq!(result["file_name"], "test.txt");
        assert_eq!(result["file_content"], test_content_str);
    }

    #[tokio::test]
    async fn test_combined_params() {
        let app = Router::new().route("/users/{id}", post(test_params_handler));
        let server = TestServer::new(app).unwrap();

        let json_data = json!({
            "name": "test"
        });

        let response = server
            .post("/users/123")
            .add_query_params(&[("extra", "query_param")])
            .json(&json_data)
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);

        let body = response.text();
        let params: TestParams = serde_json::from_str(&body).unwrap();
        assert_eq!(params.id, 123);
        assert_eq!(params.name, "test");
        assert_eq!(params.extra, Some("query_param".to_string()));
    }

    #[tokio::test]
    async fn test_nested_params_with_file_upload() {
        let app = Router::new().route("/api/posts", post(test_nested_params_handler));
        let server = TestServer::new(app).unwrap();

        // Create test file content
        let cover_content = b"Cover image content";
        let attachment1_content = b"Attachment 1 content";
        let attachment2_content = b"Attachment 2 content";

        // Create multipart form
        let response = server
            .post("/api/posts")
            .add_header(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("multipart/form-data; boundary=X-BOUNDARY"),
            )
            .multipart(
                MultipartForm::new()
                    .add_text("title", "Test Post")
                    .add_text("content", "This is a test post content")
                    .add_text("tags[]", "tag1")
                    .add_text("tags[]", "tag2")
                    .add_text("tags[]", "tag3")
                    .add_part(
                        "cover",
                        Part::bytes(cover_content.to_vec())
                            .file_name("cover.jpg")
                            .mime_type("image/jpeg"),
                    )
                    .add_text("attachments[][name]", "First attachment")
                    .add_part(
                        "attachments[][file]",
                        Part::bytes(attachment1_content.to_vec())
                            .file_name("attachment1.txt")
                            .mime_type("text/plain"),
                    )
                    .add_text("attachments[][name]", "Second attachment")
                    .add_part(
                        "attachments[][file]",
                        Part::bytes(attachment2_content.to_vec())
                            .file_name("attachment2.txt")
                            .mime_type("text/plain"),
                    ),
            )
            .await;

        assert_eq!(response.status_code(), StatusCode::OK);

        let body: CreatePostResponse = serde_json::from_str(&response.text()).unwrap();

        // Verify the response
        assert_eq!(body.title, "Test Post");
        assert_eq!(body.content, "This is a test post content");
        assert_eq!(body.tags, vec!["tag1", "tag2", "tag3"]);

        // Verify cover file
        assert_eq!(body.cover.name, "cover.jpg");
        assert_eq!(body.cover.content_type, "image/jpeg");
        assert_eq!(body.cover.content, String::from_utf8_lossy(cover_content));

        // Verify attachments
        assert_eq!(body.attachments.len(), 2);

        let attachment1 = &body.attachments[0];
        assert_eq!(attachment1.name, "First attachment");
        assert_eq!(attachment1.content_type, "text/plain");
        assert_eq!(
            attachment1.content,
            String::from_utf8_lossy(attachment1_content)
        );

        let attachment2 = &body.attachments[1];
        assert_eq!(attachment2.name, "Second attachment");
        assert_eq!(attachment2.content_type, "text/plain");
        assert_eq!(
            attachment2.content,
            String::from_utf8_lossy(attachment2_content)
        );
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct MixedAttachment {
        name: String,
        description: String,
        file: Option<UploadFile>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct MixedPostParams {
        title: String,
        content: String,
        tags: Option<Vec<String>>,
        attachments: Option<Vec<MixedAttachment>>,
        author: Option<String>,
        status: Option<String>,
        metadata: Option<HashMap<String, String>>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct MixedAttachmentResponse {
        name: String,
        description: String,
        file_size: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct MixedPostResponse {
        message: String,
        title: String,
        content: String,
        tags: Vec<String>,
        author: String,
        status: String,
        metadata: HashMap<String, String>,
        attachments: Vec<MixedAttachmentResponse>,
    }

    #[tokio::test]
    async fn test_mixed_create_post() {
        setup();
        use tokio::io::AsyncReadExt;

        let app = Router::new().route(
            "/posts/{category}",
            post(|params: Params<MixedPostParams>| async move {
                debug!("params: {:#?}", params);
                let MixedPostParams {
                    title,
                    content,
                    tags,
                    attachments,
                    author,
                    status,
                    metadata,
                } = params.0;

                let mut response = MixedPostResponse {
                    message: "Success".to_string(),
                    title,
                    content,
                    tags: tags.unwrap_or_default(),
                    author: author.unwrap_or_default(),
                    status: status.unwrap_or_default(),
                    metadata: metadata.unwrap_or_default(),
                    attachments: Vec::new(),
                };

                if let Some(attachments) = attachments {
                    response.attachments =
                        futures_util::future::join_all(attachments.into_iter().map(|a| async {
                            let size = if let Some(file) = a.file {
                                let mut f = file.open().await.unwrap();
                                let mut content = Vec::new();
                                f.read_to_end(&mut content).await.unwrap();
                                content.len() as u64
                            } else {
                                0
                            };

                            MixedAttachmentResponse {
                                name: a.name,
                                description: a.description,
                                file_size: size,
                            }
                        }))
                        .await;
                }

                Ok::<Json<MixedPostResponse>, Error>(Json(response))
            }),
        );

        let server = TestServer::new(app).unwrap();

        // Prepare base post data in empty-named JSON part
        let base_json = r#"{
        "title": "Mixed Test Post",
        "content": "This is a test post with mixed data sources",
        "tags": ["rust", "axum", "test"]
    }"#;

        // Create multipart form with mixed data
        let form = MultipartForm::new()
            // Add base data as empty-named JSON part
            .add_part("", Part::text(base_json).mime_type("application/json"))
            // Add metadata fields individually
            .add_part("metadata[version]", Part::text("2.0"))
            .add_part("metadata[visibility]", Part::text("public"))
            .add_part("metadata[created_at]", Part::text("2024-12-29"))
            // Add first attachment with file and metadata
            .add_part(
                "attachments[][file]",
                Part::bytes(vec![1, 2, 3, 4])
                    .file_name("test1.bin")
                    .mime_type("application/octet-stream"),
            )
            .add_part("attachments[][name]", Part::text("Test Attachment 1"))
            .add_part(
                "attachments[][description]",
                Part::text("First test attachment"),
            )
            // Add second attachment with file and metadata
            .add_part(
                "attachments[][file]",
                Part::bytes(vec![5, 6, 7, 8, 9])
                    .file_name("test2.bin")
                    .mime_type("application/octet-stream"),
            )
            .add_part("attachments[][name]", Part::text("Test Attachment 2"))
            .add_part(
                "attachments[][description]",
                Part::text("Second test attachment"),
            );

        // Add headers for author and status
        let response = server
            .post("/posts/tech?author=test_user&status=draft")
            .add_header(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("multipart/form-data"),
            )
            .multipart(form)
            .await;

        debug!("Request URL: {}", response.request_url());
        debug!("Request headers: {:?}", response.headers());
        debug!("Response status: {}", response.status_code());
        debug!(
            "Response body: {}",
            String::from_utf8_lossy(&response.as_bytes())
        );

        assert_eq!(response.status_code(), StatusCode::OK);

        let body: MixedPostResponse = response.json();

        // Verify base data from empty-named JSON part
        assert_eq!(body.title, "Mixed Test Post");
        assert_eq!(body.content, "This is a test post with mixed data sources");
        assert_eq!(body.tags, vec!["rust", "axum", "test"]);

        // Verify data from query parameters
        assert_eq!(body.author, "test_user");
        assert_eq!(body.status, "draft");

        // Verify metadata from named JSON part
        assert_eq!(body.metadata.get("version").unwrap(), "2.0");
        assert_eq!(body.metadata.get("visibility").unwrap(), "public");
        assert_eq!(body.metadata.get("created_at").unwrap(), "2024-12-29");

        // Verify attachments from multipart form
        assert_eq!(body.attachments.len(), 2);

        let attachment1 = &body.attachments[0];
        assert_eq!(attachment1.name, "Test Attachment 1");
        assert_eq!(attachment1.description, "First test attachment");
        assert_eq!(attachment1.file_size, 4);

        let attachment2 = &body.attachments[1];
        assert_eq!(attachment2.name, "Test Attachment 2");
        assert_eq!(attachment2.description, "Second test attachment");
        assert_eq!(attachment2.file_size, 5);
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct ComplexParams {
        // Path params
        user_id: i32,
        // Query params
        filter: String,
        page: Option<i32>,
        // JSON body
        data: ComplexData,
        // Multipart form
        avatar: UploadFile,
        profile: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct ComplexData {
        title: String,
        tags: Vec<String>,
        version_number: f64,
        metadata: HashMap<String, String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct ComplexResponse {
        message: String,
        user_id: i32,
        filter: String,
        page: Option<i32>,
        title: String,
        tags: Vec<String>,
        version_number: f64,
        metadata: HashMap<String, String>,
        avatar_size: u64,
        profile: Option<String>,
    }

    async fn complex_handler(
        Params(params, _): Params<ComplexParams>,
    ) -> Result<Json<ComplexResponse>, Error> {
        let avatar_file = params
            .avatar
            .open()
            .await
            .map_err(|e| Error::ReadError(format!("Failed to open avatar file: {}", e)))?;
        let avatar_size = avatar_file
            .metadata()
            .await
            .map_err(|e| Error::ReadError(format!("Failed to get avatar metadata: {}", e)))?
            .len();

        Ok(Json(ComplexResponse {
            message: "Success".to_string(),
            user_id: params.user_id,
            filter: params.filter,
            page: params.page,
            title: params.data.title,
            tags: params.data.tags,
            version_number: params.data.version_number,
            metadata: params.data.metadata,
            avatar_size,
            profile: params.profile,
        }))
    }

    #[tokio::test]
    async fn test_complex_params() {
        setup();
        let app = Router::new().route("/users/{user_id}", post(complex_handler));
        let server = TestServer::new(app).unwrap();

        // Prepare multipart form data
        let avatar_content = "avatar data".as_bytes();
        let form = MultipartForm::new()
            .add_part(
                "avatar",
                Part::bytes(avatar_content.to_vec())
                    .file_name("avatar.jpg")
                    .mime_type("image/jpeg"),
            )
            .add_text("profile", "Test profile")
            .add_part(
                "data",
                Part::text(
                    r#"{
            "title": "Test Post",
            "tags": ["rust", "axum"],
            "version_number": 1.0,
            "metadata": {
                "version": "1.0",
                "author": "test"
            }
        }"#,
                )
                .mime_type("application/json"),
            );

        // Build the request with all parameter types
        let user_id = 123;
        let response = server
            .post(&format!("/users/{}", user_id))
            .add_query_param("filter", "active")
            .add_query_param("page", "1")
            .multipart(form)
            .await;
        debug!("Request URL: {}", response.request_url());
        debug!("Request headers: {:?}", response.headers());
        debug!("Response status: {}", response.status_code());
        debug!(
            "Response body: {}",
            String::from_utf8_lossy(&response.as_bytes())
        );
        assert_eq!(response.status_code(), StatusCode::OK);

        let body: ComplexResponse = response.json();
        assert_eq!(body.message, "Success");
        assert_eq!(body.user_id, user_id);
        assert_eq!(body.filter, "active");
        assert_eq!(body.page, Some(1));
        assert_eq!(body.title, "Test Post");
        assert_eq!(body.tags, vec!["rust", "axum"]);
        assert_eq!(body.metadata.get("version").unwrap(), "1.0");
        assert_eq!(body.metadata.get("author").unwrap(), "test");
        assert_eq!(body.avatar_size, avatar_content.len() as u64);
        assert_eq!(body.profile, Some("Test profile".to_string()));
    }

    #[tokio::test]
    async fn test_json_part() {
        setup();
        let app = Router::new().route("/test", post(complex_handler));
        let server = TestServer::new(app).unwrap();

        // Prepare multipart form data with empty field name
        let form = MultipartForm::new()
            .add_part(
                "",
                Part::text(
                    r#"{
                    "data": {
                        "title": "Test Post",
                        "tags": ["rust", "axum"],
                        "version_number": 1.0,
                        "metadata": {
                            "version": "1.0",
                            "author": "test"
                        }
                    },
                    "profile": "Test profile",
                    "filter": "active",
                    "page": 1,
                    "user_id": 123
                }"#,
                )
                .mime_type("application/json"),
            )
            .add_part(
                "avatar",
                Part::bytes(vec![1, 2, 3, 4])
                    .file_name("test-avatar.bin")
                    .mime_type("application/octet-stream"),
            );

        let response = server.post("/test").multipart(form).await;
        debug!("Request URL: {}", response.request_url());
        debug!("Request headers: {:?}", response.headers());
        debug!("Response status: {}", response.status_code());
        debug!(
            "Response body: {}",
            String::from_utf8_lossy(&response.as_bytes())
        );
        assert_eq!(response.status_code(), StatusCode::OK);

        let body: ComplexResponse = response.json();
        assert_eq!(body.message, "Success");
        assert_eq!(body.user_id, 123);
        assert_eq!(body.filter, "active");
        assert_eq!(body.page, Some(1));
        assert_eq!(body.title, "Test Post");
        assert_eq!(body.tags, vec!["rust", "axum"]);
        assert_eq!(body.version_number, 1.0);
        assert_eq!(body.metadata.get("version").unwrap(), "1.0");
        assert_eq!(body.metadata.get("author").unwrap(), "test");
        assert_eq!(body.profile, Some("Test profile".to_string()));
        assert_eq!(body.avatar_size, 4);
    }

    #[derive(Debug, Deserialize)]
    struct TestNumbers {
        pos_int: u64,
        neg_int: i64,
        float: f64,
        zero: i64,
        big_num: u64,
        small_float: f64,
        exp_num: f64,
    }

    #[derive(Debug, Deserialize)]
    struct TestMixed {
        number: i64,
        text: String,
        boolean: bool,
        opt_val: Option<String>,
        numbers: Vec<f64>,
        nested: TestNested,
    }

    #[derive(Debug, Deserialize)]
    struct TestNested {
        id: u64,
        name: String,
    }

    #[tokio::test]
    async fn test_json_numbers() {
        setup();
        let json = r#"{
        "pos_int": 42,
        "neg_int": -42,
        "float": 42.5,
        "zero": 0,
        "big_num": 9007199254740991,
        "small_float": 0.0000123,
        "exp_num": 1.23e5
    }"#;

        let req = Request::builder()
            .method(http::Method::POST)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(json))
            .unwrap();

        let params = Params::<TestNumbers>::from_request(req, &()).await.unwrap();
        assert_eq!(params.0.pos_int, 42);
        assert_eq!(params.0.neg_int, -42);
        assert!((params.0.float - 42.5).abs() < f64::EPSILON);
        assert_eq!(params.0.zero, 0);
        assert_eq!(params.0.big_num, 9007199254740991);
        assert!((params.0.small_float - 0.0000123).abs() < f64::EPSILON);
        assert!((params.0.exp_num - 123000.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_json_mixed_types() {
        setup();
        let json = r#"{
        "number": 42,
        "text": "hello world",
        "boolean": true,
        "opt_val": null,
        "numbers": [1.1, 2.2, 3.3],
        "nested": {
            "id": 1,
            "name": "test"
        }
    }"#;

        let req = Request::builder()
            .method(http::Method::POST)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(json))
            .unwrap();

        let params = Params::<TestMixed>::from_request(req, &()).await.unwrap();
        assert_eq!(params.0.number, 42);
        assert_eq!(params.0.text, "hello world");
        assert!(params.0.boolean);
        assert!(params.0.opt_val.is_none());
        assert_eq!(params.0.numbers.len(), 3);
        assert!((params.0.numbers[0] - 1.1).abs() < f64::EPSILON);
        assert!((params.0.numbers[1] - 2.2).abs() < f64::EPSILON);
        assert!((params.0.numbers[2] - 3.3).abs() < f64::EPSILON);
        assert_eq!(params.0.nested.id, 1);
        assert_eq!(params.0.nested.name, "test");
    }

    #[tokio::test]
    async fn test_form_urlencoded_numbers() {
        setup();
        let form_data = "pos_int=42&neg_int=-42&float=42.5&zero=0&big_num=9007199254740991&small_float=0.0000123&exp_num=123000";

        let req = Request::builder()
            .method(http::Method::POST)
            .header(
                http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(Body::from(form_data))
            .unwrap();

        let params = Params::<TestNumbers>::from_request(req, &()).await.unwrap();
        assert_eq!(params.0.pos_int, 42);
        assert_eq!(params.0.neg_int, -42);
        assert!((params.0.float - 42.5).abs() < f64::EPSILON);
        assert_eq!(params.0.zero, 0);
        assert_eq!(params.0.big_num, 9007199254740991);
        assert!((params.0.small_float - 0.0000123).abs() < f64::EPSILON);
        assert!((params.0.exp_num - 123000.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_query_params_numbers() {
        setup();
        let req = Request::builder()
        .method(http::Method::GET)
        .uri("/test?pos_int=42&neg_int=-42&float=42.5&zero=0&big_num=9007199254740991&small_float=0.0000123&exp_num=123000")
        .body(Body::empty())
        .unwrap();

        let params = Params::<TestNumbers>::from_request(req, &()).await.unwrap();
        assert_eq!(params.0.pos_int, 42);
        assert_eq!(params.0.neg_int, -42);
        assert!((params.0.float - 42.5).abs() < f64::EPSILON);
        assert_eq!(params.0.zero, 0);
        assert_eq!(params.0.big_num, 9007199254740991);
        assert!((params.0.small_float - 0.0000123).abs() < f64::EPSILON);
        assert!((params.0.exp_num - 123000.0).abs() < f64::EPSILON);
    }

    #[derive(Debug, Deserialize)]
    struct TestEncodedParams {
        #[serde(rename = "foo=1")]
        foo: Option<String>,
        baz: Option<String>,
    }

    #[tokio::test]
    async fn test_encoded_path_params() {
        setup();

        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/test?foo%3D1=bar&baz=qux%3D2")
            .body(Body::empty())
            .unwrap();

        let Params(params, _) = Params::<TestEncodedParams>::from_request(req, &())
            .await
            .unwrap();
        assert_eq!(params.foo, Some("bar".to_string()));
        assert_eq!(params.baz, Some("qux=2".to_string()));
    }

    #[tokio::test]
    async fn test_json_params() {
        setup();
        let req = Request::builder()
            .method(http::Method::POST)
            .header(http::header::CONTENT_TYPE, "application/json")
            .uri("/test")
            .body(Body::new(
                json!({
                    "foo=1": "bar",
                    "baz": "qux=2"
                })
                .to_string(),
            ))
            .unwrap();

        let Params(params, _) = Params::<TestEncodedParams>::from_request(req, &())
            .await
            .unwrap();
        assert_eq!(params.foo, Some("bar".to_string()));
        assert_eq!(params.baz, Some("qux=2".to_string()));
    }

    #[tokio::test]
    async fn test_json_params_dont_decode() {
        setup();
        let req = Request::builder()
            .method(http::Method::POST)
            .header(http::header::CONTENT_TYPE, "application/json")
            .uri("/test")
            .body(Body::new(
                json!({
                    "foo%3D1": "bar",
                    "baz": "qux%3D2"
                })
                .to_string(),
            ))
            .unwrap();

        let Params(params, _) = Params::<TestEncodedParams>::from_request(req, &())
            .await
            .unwrap();
        assert_eq!(params.foo, None);
        assert_eq!(params.baz, Some("qux%3D2".to_string()));
    }

    #[tokio::test]
    async fn test_encoded_form_params() {
        setup();
        let req = Request::builder()
            .method(http::Method::POST)
            .header(
                http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .uri("/test")
            .body(Body::new("foo%3D1=bar&baz=qux%3D2".to_string()))
            .unwrap();

        let Params(params, _) = Params::<TestEncodedParams>::from_request(req, &())
            .await
            .unwrap();
        assert_eq!(params.foo, Some("bar".to_string()));
        assert_eq!(params.baz, Some("qux=2".to_string()));
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    pub struct OrderId(String);

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    pub enum CurrencyCode {
        #[serde(rename = "usd")]
        Usd,
        #[serde(rename = "gbp")]
        Gbp,
        #[serde(rename = "cad")]
        Cad,
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct PaymentRequest {
        order_id: OrderId,
        amount: f64,
        currency: CurrencyCode,
        description: Option<String>,
    }

    #[axum::debug_handler]
    async fn payment_handler(Params(payment, _): Params<PaymentRequest>) -> impl IntoResponse {
        let response = json!({
            "order_id": payment.order_id.0,
            "amount": payment.amount,
            "currency": payment.currency,
            "description": payment.description,
            "processed": true
        });

        (StatusCode::OK, serde_json::to_string(&response).unwrap())
    }

    // Test for JSON body
    #[tokio::test]
    async fn test_currency_code_json() {
        setup();

        let json = r#"{
        "order_id": "1234567890",
        "amount": 99.99,
        "currency": "usd",
        "description": "Test payment"
    }"#;

        let req = Request::builder()
            .method(http::Method::POST)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(json))
            .unwrap();

        let params = Params::<PaymentRequest>::from_request(req, &())
            .await
            .unwrap();
        assert_eq!(params.0.order_id.0, "1234567890");
        assert_eq!(params.0.amount, 99.99);
        assert_eq!(params.0.currency, CurrencyCode::Usd);
        assert_eq!(params.0.description, Some("Test payment".to_string()));
    }

    // Test for request parameters (query params)
    #[tokio::test]
    async fn test_currency_code_query_params() {
        setup();

        let app = Router::new().route("/payment", get(payment_handler));
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/payment")
            .add_query_params(&[
                ("order_id", "1234567890"),
                ("amount", "199.99"),
                ("currency", "gbp"),
                ("description", "Query payment"),
            ])
            .await;

        assert_eq!(response.status_code(), StatusCode::OK);

        let body: serde_json::Value = response.json();
        assert_eq!(body["order_id"], "1234567890");
        assert_eq!(body["amount"], 199.99);
        assert_eq!(body["currency"], "gbp");
        assert_eq!(body["description"], "Query payment");
        assert_eq!(body["processed"], true);
    }

    // Test for path parameters
    #[tokio::test]
    async fn test_currency_code_path_params() {
        setup();

        // Define a handler that extracts currency from path
        async fn path_handler(
            Path(currency): Path<CurrencyCode>,
            Params(payment, _): Params<PaymentRequest>,
        ) -> impl IntoResponse {
            let response = json!({
                "order_id": payment.order_id.0,
                "amount": payment.amount,
                "currency": currency,
                "description": payment.description,
                "processed": true
            });

            (StatusCode::OK, serde_json::to_string(&response).unwrap())
        }

        let app = Router::new().route("/payment/{currency}", post(path_handler));
        let server = TestServer::new(app).unwrap();

        let json_data = json!({
            "order_id": "1234567890",
            "amount": 299.99,
            "description": "Path payment"
        });

        let response = server.post("/payment/cad").json(&json_data).await;

        assert_eq!(response.status_code(), StatusCode::OK);

        let body: serde_json::Value = response.json();
        assert_eq!(body["order_id"], "1234567890");
        assert_eq!(body["amount"], 299.99);
        assert_eq!(body["currency"], "cad");
        assert_eq!(body["description"], "Path payment");
        assert_eq!(body["processed"], true);
    }

    // Test for form data
    #[tokio::test]
    async fn test_currency_code_form_data() {
        setup();

        let app = Router::new().route("/payment", post(payment_handler));
        let server = TestServer::new(app).unwrap();

        let response = server
            .post("/payment")
            .form(&[
                ("order_id", "1234567890"),
                ("amount", "399.99"),
                ("currency", "gbp"),
                ("description", "Form payment"),
            ])
            .await;

        assert_eq!(response.status_code(), StatusCode::OK);

        let body: serde_json::Value = response.json();
        assert_eq!(body["order_id"], "1234567890");
        assert_eq!(body["amount"], 399.99);
        assert_eq!(body["currency"], "gbp");
        assert_eq!(body["description"], "Form payment");
        assert_eq!(body["processed"], true);
    }
}
