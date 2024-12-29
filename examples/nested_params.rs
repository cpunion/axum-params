use axum::{routing::post, Json, Router};
use axum_params::{Error, Params, UploadFile};
use futures_util::future;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

// Complex nested structure with multiple files
#[derive(Debug, Serialize, Deserialize)]
struct CreatePostParams {
    title: String,
    content: String,
    tags: Vec<String>,
    cover: UploadFile,            // Single file
    attachments: Vec<Attachment>, // Array of files with metadata
}

#[derive(Debug, Serialize, Deserialize)]
struct Attachment {
    file: UploadFile,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AttachmentResponse {
    name: String,
    content_type: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UploadFileResponse {
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
async fn create_post_handler(
    Params(post, _): Params<CreatePostParams>,
) -> Result<Json<CreatePostResponse>, Error> {
    // Handle cover file
    let mut cover_file = post.cover.open().await.unwrap();
    let mut cover_content = String::new();
    cover_file.read_to_string(&mut cover_content).await.unwrap();

    // Handle attachments
    let attachments = future::join_all(post.attachments.into_iter().map(|a| async {
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

    Ok(Json(CreatePostResponse {
        title: post.title,
        content: post.content,
        tags: post.tags,
        cover: UploadFileResponse {
            name: post.cover.name,
            content_type: post.cover.content_type,
            content: cover_content,
        },
        attachments,
    }))
}

#[tokio::main]
async fn main() {
    // Build our application with a route
    let app = Router::new().route("/posts", post(create_post_handler));

    // Run it
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/*
Test with curl:

# Create post using form fields
curl -X POST http://localhost:3000/posts \
  -F "title=Rust Examples" \
  -F "content=Collection of axum-params examples" \
  -F "tags[]=rust" \
  -F "tags[]=axum" \
  -F "cover=@examples/nested_params.rs" \
  -F "attachments[0][name]=Basic Params" \
  -F "attachments[0][file]=@examples/basic_params.rs" \
  -F "attachments[1][name]=File Upload" \
  -F "attachments[1][file]=@examples/file_upload.rs"

# Or create post using JSON file for metadata and -F for files
# First, create a temporary JSON file:
cat > /tmp/post.json << 'EOF'
{
  "title": "Rust Examples",
  "content": "Collection of axum-params examples",
  "tags": ["rust", "axum"]
}
EOF

# Then use the JSON file in the request
curl -X POST http://localhost:3000/posts \
  -F "=@/tmp/post.json;type=application/json" \
  -F "cover=@examples/nested_params.rs" \
  -F "attachments[0][name]=Basic Params" \
  -F "attachments[0][file]=@examples/basic_params.rs" \
  -F "attachments[1][name]=File Upload" \
  -F "attachments[1][file]=@examples/file_upload.rs"

# Clean up
rm /tmp/post.json

Expected response:
{
  "title": "Rust Examples",
  "content": "Collection of axum-params examples",
  "tags": ["rust", "axum"],
  "cover": {
    "name": "nested_params.rs",
    "content_type": "text/x-rust",
    "content": "... content of nested_params.rs ..."
  },
  "attachments": [
    {
      "name": "Basic Params",
      "content_type": "text/x-rust",
      "content": "... content of basic_params.rs ..."
    },
    {
      "name": "File Upload",
      "content_type": "text/x-rust",
      "content": "... content of file_upload.rs ..."
    }
  ]
}
*/
