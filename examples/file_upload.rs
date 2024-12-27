use axum::{response::IntoResponse, routing::post, Json, Router};
use axum_params::{Params, UploadFile};
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncReadExt;

// File upload with metadata
#[derive(Debug, Deserialize)]
struct FileUploadParams {
    title: String,
    description: Option<String>,
    file: UploadFile,
}

#[axum::debug_handler]
async fn file_upload_handler(Params(upload, _): Params<FileUploadParams>) -> impl IntoResponse {
    let mut temp_file = upload.file.open().await.unwrap();
    let mut content = String::new();
    temp_file.read_to_string(&mut content).await.unwrap();

    Json(json!({
        "title": upload.title,
        "description": upload.description,
        "file_name": upload.file.name,
        "file_content": content,
    }))
}

#[tokio::main]
async fn main() {
    // Build our application with a route
    let app = Router::new().route("/upload", post(file_upload_handler));

    // Run it
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/*
Test with curl:

# Upload this source file with metadata using multipart form
curl -X POST http://localhost:3000/upload \
  -F "title=File Upload Example" \
  -F "description=Source code of the file upload example" \
  -F "file=@examples/file_upload.rs"

Expected response:
{
  "title": "File Upload Example",
  "description": "Source code of the file upload example",
  "file_name": "file_upload.rs",
  "file_content": "... content of this file ..."
}

# Upload without optional description
curl -X POST http://localhost:3000/upload \
  -F "title=File Upload Example" \
  -F "file=@examples/file_upload.rs"
*/
