use std::collections::HashMap;

use super::*;
use ::serde::{Deserialize, Serialize};
use axum::{
    body::Body,
    extract::{FromRequest, Request},
    http::StatusCode,
    http::{self, HeaderValue},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_test::{
    multipart::{MultipartForm, Part},
    TestServer,
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

    let attachments = futures_util::future::join_all(post.attachments.into_iter().map(|a| async {
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
    let app = Router::new().route("/users/:id", get(test_params_handler));
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
    let app = Router::new().route("/users/:id", post(test_params_handler));
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

#[test]
fn test_process_nested_params() {
    let mut input = HashMap::new();

    // Test simple key-value
    input.insert(
        "name".to_string(),
        vec![ParamsValue::Convertible("john".to_string())],
    );

    // Test nested object
    input.insert(
        "user[name]".to_string(),
        vec![ParamsValue::Convertible("mary".to_string())],
    );
    input.insert(
        "user[age]".to_string(),
        vec![ParamsValue::Convertible("25".to_string())],
    );
    input.insert(
        "user[address][city]".to_string(),
        vec![ParamsValue::Convertible("beijing".to_string())],
    );
    input.insert(
        "user[address][country]".to_string(),
        vec![ParamsValue::Convertible("china".to_string())],
    );

    // Test array
    input.insert(
        "colors[]".to_string(),
        vec![
            ParamsValue::Convertible("red".to_string()),
            ParamsValue::Convertible("blue".to_string()),
        ],
    );

    // Test indexed array
    input.insert(
        "numbers[0]".to_string(),
        vec![ParamsValue::Convertible("1".to_string())],
    );
    input.insert(
        "numbers[1]".to_string(),
        vec![ParamsValue::Convertible("2".to_string())],
    );
    input.insert(
        "numbers[2]".to_string(),
        vec![ParamsValue::Convertible("3".to_string())],
    );

    // Test array of objects
    input.insert(
        "users[0][name]".to_string(),
        vec![ParamsValue::Convertible("john".to_string())],
    );
    input.insert(
        "users[0][age]".to_string(),
        vec![ParamsValue::Convertible("20".to_string())],
    );
    input.insert(
        "users[1][name]".to_string(),
        vec![ParamsValue::Convertible("mary".to_string())],
    );
    input.insert(
        "users[1][age]".to_string(),
        vec![ParamsValue::Convertible("25".to_string())],
    );

    let result = process_nested_params(input);
    debug!("result: {:?}", result);

    // Verify the result
    if let ParamsValue::Object(map) = result {
        // Test simple key-value
        assert_eq!(
            map.get("name").unwrap(),
            &ParamsValue::Convertible("john".to_string())
        );

        // Test nested object
        if let ParamsValue::Object(user) = map.get("user").unwrap() {
            assert_eq!(
                user.get("name").unwrap(),
                &ParamsValue::Convertible("mary".to_string())
            );
            assert_eq!(
                user.get("age").unwrap(),
                &ParamsValue::Convertible("25".to_string())
            );

            if let ParamsValue::Object(address) = user.get("address").unwrap() {
                assert_eq!(
                    address.get("city").unwrap(),
                    &ParamsValue::Convertible("beijing".to_string())
                );
                assert_eq!(
                    address.get("country").unwrap(),
                    &ParamsValue::Convertible("china".to_string())
                );
            } else {
                panic!("address should be an object");
            }
        } else {
            panic!("user should be an object");
        }

        // Test array
        if let ParamsValue::Array(colors) = map.get("colors").unwrap() {
            assert_eq!(colors.len(), 2);
            assert_eq!(colors[0], ParamsValue::Convertible("red".to_string()));
            assert_eq!(colors[1], ParamsValue::Convertible("blue".to_string()));
        } else {
            panic!("colors should be an array");
        }

        // Test indexed array
        if let ParamsValue::Array(numbers) = map.get("numbers").unwrap() {
            assert_eq!(numbers.len(), 3);
            assert_eq!(numbers[0], ParamsValue::Convertible("1".to_string()));
            assert_eq!(numbers[1], ParamsValue::Convertible("2".to_string()));
            assert_eq!(numbers[2], ParamsValue::Convertible("3".to_string()));
        } else {
            panic!("numbers should be an array");
        }

        // Test array of objects
        if let ParamsValue::Array(users) = map.get("users").unwrap() {
            assert_eq!(users.len(), 2);

            if let ParamsValue::Object(user0) = &users[0] {
                assert_eq!(
                    user0.get("name").unwrap(),
                    &ParamsValue::Convertible("john".to_string())
                );
                assert_eq!(
                    user0.get("age").unwrap(),
                    &ParamsValue::Convertible("20".to_string())
                );
            } else {
                panic!("users[0] should be an object");
            }

            if let ParamsValue::Object(user1) = &users[1] {
                assert_eq!(
                    user1.get("name").unwrap(),
                    &ParamsValue::Convertible("mary".to_string())
                );
                assert_eq!(
                    user1.get("age").unwrap(),
                    &ParamsValue::Convertible("25".to_string())
                );
            } else {
                panic!("users[1] should be an object");
            }
        } else {
            panic!("users should be an array");
        }
    } else {
        panic!("result should be an object");
    }
}

#[test]
fn test_process_nested_with_empty_array() {
    let mut input = HashMap::new();

    // Test array with empty values
    input.insert(
        "colors[]".to_string(),
        vec![
            ParamsValue::Convertible("red".to_string()),
            ParamsValue::Convertible("blue".to_string()),
        ],
    );

    let result = process_nested_params(input);

    // Verify the result
    if let ParamsValue::Object(map) = result {
        // Test array
        if let ParamsValue::Array(colors) = map.get("colors").unwrap() {
            assert_eq!(colors.len(), 2);
            assert_eq!(colors[0], ParamsValue::Convertible("red".to_string()));
            assert_eq!(colors[1], ParamsValue::Convertible("blue".to_string()));
        } else {
            panic!("colors should be an array");
        }
    } else {
        panic!("result should be an object");
    }
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
                .add_text("attachments[0][name]", "First attachment")
                .add_part(
                    "attachments[0][file]",
                    Part::bytes(attachment1_content.to_vec())
                        .file_name("attachment1.txt")
                        .mime_type("text/plain"),
                )
                .add_text("attachments[1][name]", "Second attachment")
                .add_part(
                    "attachments[1][file]",
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
        "/posts/:category",
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
            "attachments[0][file]",
            Part::bytes(vec![1, 2, 3, 4])
                .file_name("test1.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part("attachments[0][name]", Part::text("Test Attachment 1"))
        .add_part(
            "attachments[0][description]",
            Part::text("First test attachment"),
        )
        // Add second attachment with file and metadata
        .add_part(
            "attachments[1][file]",
            Part::bytes(vec![5, 6, 7, 8, 9])
                .file_name("test2.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part("attachments[1][name]", Part::text("Test Attachment 2"))
        .add_part(
            "attachments[1][description]",
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
    let app = Router::new().route("/users/:user_id", post(complex_handler));
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
