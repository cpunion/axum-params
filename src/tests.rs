use std::collections::HashMap;

use super::*;
use ::serde::{Deserialize, Serialize};
use axum::http::HeaderValue;
use axum::Json;
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use axum_test::{
    multipart::{MultipartForm, Part},
    TestServer,
};
use log::debug;
use serde_json::json;
use tokio::io::AsyncReadExt;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct TestParams {
    id: i32,
    name: String,
    #[serde(default)]
    extra: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
struct UploadFileResponse {
    name: String,
    content_type: String,
    content: String,
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
        vec![ParamsValue::Convertable("john".to_string())],
    );

    // Test nested object
    input.insert(
        "user[name]".to_string(),
        vec![ParamsValue::Convertable("mary".to_string())],
    );
    input.insert(
        "user[age]".to_string(),
        vec![ParamsValue::Convertable("25".to_string())],
    );
    input.insert(
        "user[address][city]".to_string(),
        vec![ParamsValue::Convertable("beijing".to_string())],
    );
    input.insert(
        "user[address][country]".to_string(),
        vec![ParamsValue::Convertable("china".to_string())],
    );

    // Test array
    input.insert(
        "colors[]".to_string(),
        vec![
            ParamsValue::Convertable("red".to_string()),
            ParamsValue::Convertable("blue".to_string()),
        ],
    );

    // Test indexed array
    input.insert(
        "numbers[0]".to_string(),
        vec![ParamsValue::Convertable("1".to_string())],
    );
    input.insert(
        "numbers[1]".to_string(),
        vec![ParamsValue::Convertable("2".to_string())],
    );
    input.insert(
        "numbers[2]".to_string(),
        vec![ParamsValue::Convertable("3".to_string())],
    );

    // Test array of objects
    input.insert(
        "users[0][name]".to_string(),
        vec![ParamsValue::Convertable("john".to_string())],
    );
    input.insert(
        "users[0][age]".to_string(),
        vec![ParamsValue::Convertable("20".to_string())],
    );
    input.insert(
        "users[1][name]".to_string(),
        vec![ParamsValue::Convertable("mary".to_string())],
    );
    input.insert(
        "users[1][age]".to_string(),
        vec![ParamsValue::Convertable("25".to_string())],
    );

    let result = process_nested_params(input);
    debug!("result: {:?}", result);

    // Verify the result
    if let ParamsValue::Object(map) = result {
        // Test simple key-value
        assert_eq!(
            map.get("name").unwrap(),
            &ParamsValue::Convertable("john".to_string())
        );

        // Test nested object
        if let ParamsValue::Object(user) = map.get("user").unwrap() {
            assert_eq!(
                user.get("name").unwrap(),
                &ParamsValue::Convertable("mary".to_string())
            );
            assert_eq!(
                user.get("age").unwrap(),
                &ParamsValue::Convertable("25".to_string())
            );

            if let ParamsValue::Object(address) = user.get("address").unwrap() {
                assert_eq!(
                    address.get("city").unwrap(),
                    &ParamsValue::Convertable("beijing".to_string())
                );
                assert_eq!(
                    address.get("country").unwrap(),
                    &ParamsValue::Convertable("china".to_string())
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
            assert_eq!(colors[0], ParamsValue::Convertable("red".to_string()));
            assert_eq!(colors[1], ParamsValue::Convertable("blue".to_string()));
        } else {
            panic!("colors should be an array");
        }

        // Test indexed array
        if let ParamsValue::Array(numbers) = map.get("numbers").unwrap() {
            assert_eq!(numbers.len(), 3);
            assert_eq!(numbers[0], ParamsValue::Convertable("1".to_string()));
            assert_eq!(numbers[1], ParamsValue::Convertable("2".to_string()));
            assert_eq!(numbers[2], ParamsValue::Convertable("3".to_string()));
        } else {
            panic!("numbers should be an array");
        }

        // Test array of objects
        if let ParamsValue::Array(users) = map.get("users").unwrap() {
            assert_eq!(users.len(), 2);

            if let ParamsValue::Object(user0) = &users[0] {
                assert_eq!(
                    user0.get("name").unwrap(),
                    &ParamsValue::Convertable("john".to_string())
                );
                assert_eq!(
                    user0.get("age").unwrap(),
                    &ParamsValue::Convertable("20".to_string())
                );
            } else {
                panic!("users[0] should be an object");
            }

            if let ParamsValue::Object(user1) = &users[1] {
                assert_eq!(
                    user1.get("name").unwrap(),
                    &ParamsValue::Convertable("mary".to_string())
                );
                assert_eq!(
                    user1.get("age").unwrap(),
                    &ParamsValue::Convertable("25".to_string())
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
            ParamsValue::Convertable("red".to_string()),
            ParamsValue::Convertable("blue".to_string()),
        ],
    );

    let result = process_nested_params(input);

    // Verify the result
    if let ParamsValue::Object(map) = result {
        // Test array
        if let ParamsValue::Array(colors) = map.get("colors").unwrap() {
            assert_eq!(colors.len(), 2);
            assert_eq!(colors[0], ParamsValue::Convertable("red".to_string()));
            assert_eq!(colors[1], ParamsValue::Convertable("blue".to_string()));
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

    // First attachment
    assert_eq!(body.attachments[0].name, "First attachment");
    assert_eq!(body.attachments[0].content_type, "text/plain");
    assert_eq!(
        body.attachments[0].content,
        String::from_utf8_lossy(attachment1_content)
    );

    // Second attachment
    assert_eq!(body.attachments[1].name, "Second attachment");
    assert_eq!(body.attachments[1].content_type, "text/plain");
    assert_eq!(
        body.attachments[1].content,
        String::from_utf8_lossy(attachment2_content)
    );
}
