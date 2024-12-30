# Axum Params

A powerful parameter handling library for [Axum](https://github.com/tokio-rs/axum) web framework, inspired by [Ruby on Rails](https://rubyonrails.org/)' parameter system. Seamlessly handles multiple parameter sources and tree-structured data with file uploads.


[![Build Status](https://github.com/cpunion/axum-params/actions/workflows/ci.yml/badge.svg)](https://github.com/cpunion/axum-params/actions/workflows/ci.yml)
[![codecov](https://codecov.io/github/cpunion/axum-params/graph/badge.svg?token=uATQa0RzPL)](https://codecov.io/github/cpunion/axum-params)
[![crate](https://img.shields.io/crates/v/axum-params.svg)](https://crates.io/crates/axum-params)
[![docs](https://docs.rs/axum-params/badge.svg)](https://docs.rs/axum-params)
[![GitHub commits](https://badgen.net/github/commits/cpunion/axum-params)](https://GitHub.com/Naereen/cpunion/axum-params/commit/)
[![GitHub release](https://img.shields.io/github/v/tag/cpunion/axum-params.svg?label=release)](https://github.com/cpunion/axum-params/releases)


## Features

- **Unified Parameter Handling**
  - Path parameters
  - Query parameters
  - Form data
  - Multipart form data
  - JSON body
  - All parameter types can be processed simultaneously
  - Every parameter type supports structured data (arrays and objects)

- **Rails-like Tree-Structured Parameters**
  - Nested parameter handling similar to Rails' strong parameters
  - Support for deeply nested structures with arrays and objects
  - Files can be placed at any position in the parameter tree, e.g. `post[attachments][][file]`
  - Seamlessly mix files with other data types in the same request
  - Automatic parameter parsing and type conversion
  - Handle complex forms with multiple file uploads in nested structures

Example structure:

```rust
post: {
    title: String,
    content: String,
    tags: Vec<String>,
    cover: UploadFile,
    attachments: Vec<{
        file: UploadFile,
        description: String
    }>
}
```


## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
axum-params = "0.1"
```

## Quick Start

```rust
use axum::{routing::post, Router};
use axum_params::Params;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Attachment {
    file: UploadFile,
    description: String,
}

#[derive(Serialize, Deserialize)]
struct CreatePost {
    title: String,
    content: String,
    tags: Vec<String>,
    cover: UploadFile,
    attachments: Vec<Attachment>,
}

#[debug_handler]
async fn create_post_handler(Params(post, _): Params<CreatePost>) -> impl IntoResponse {
    // Handle cover file
    let mut cover_file = post.cover.open().await.unwrap();
    // process file
    // Handle attachments
    for attachment in post.attachments {
        let mut file = attachment.file.open().await.unwrap();
        // process file
    }
}
```

## Rails-like Parameter Structure

Just like Rails, you can send nested parameters in various formats:

### Combined Parameters Example
```bash
# Combining path parameters, query parameters, and form data
curl -X POST "http://localhost:3000/posts/123?draft=true" \
  -F "post[title]=My First Post" \
  -F "post[content]=Hello World from Axum" \
  -F "post[tags][]=rust" \
  -F "post[cover]=@cover.jpg"
```

### JSON Body
```bash
# Sending JSON data (note: file uploads not possible in pure JSON)
curl -X POST http://localhost:3000/posts \
  -H "Content-Type: application/json" \
  -d '{
    "post": {
      "title": "My First Post",
      "content": "Hello World from Axum",
      "tags": ["rust", "web", "axum"]
    }
  }'
```

### Form Data
```bash
# Basic form data with nested parameters and file uploads
curl -X POST http://localhost:3000/posts \
  -F "post[title]=My First Post" \
  -F "post[content]=Hello World from Axum" \
  -F "post[tags][]=rust" \
  -F "post[tags][]=web" \
  -F "post[tags][]=axum" \
  -F "post[cover]=@cover.jpg" \
  -F "post[attachments][][file]=@document.pdf" \
  -F "post[attachments][][description]=Project Documentation" \
  -F "post[attachments][][file]=@diagram.png" \
  -F "post[attachments][][description]=Architecture Diagram"
```

### Multipart Form
The library automatically handles multipart form data, allowing you to upload files within nested structures. Files can be placed at any level in the parameter tree, and you can combine them with regular form fields.

```bash
# Complex multipart form matching the Post struct example
curl -X POST http://localhost:3000/posts \
  -F "post[title]=My First Post" \
  -F "post[content]=Hello World from Axum" \
  -F "post[tags][]=rust" \
  -F "post[tags][]=web" \
  -F "post[tags][]=axum" \
  -F "post[cover]=@cover.jpg" \
  -F "post[attachments][][file]=@document.pdf" \
  -F "post[attachments][][description]=Project Documentation" \
  -F "post[attachments][][file]=@diagram.png" \
  -F "post[attachments][][description]=Architecture Diagram" \
  -F "post[attachments][][file]=@screenshot.jpg" \
  -F "post[attachments][][description]=Application Screenshot"
```

This example demonstrates how the multipart form maps to the Rust struct:
- Single field (`title`, `content`)
- Array field (`tags[]`)
- Single file field (`cover`)
- Nested array with files (`attachments[]` with `file` and `description`)

## Examples

- [Basic Parameters](examples/basic_params.rs) - Handling path, query, and JSON parameters
- [File Upload](examples/file_upload.rs) - Basic file upload with metadata
- [Nested Parameters](examples/nested_params.rs) - Complex nested structures with multiple file uploads

### Running Examples

```bash
# Run basic parameters example
cargo run --example basic_params

# Run file upload example
cargo run --example file_upload

# Run nested parameters example
cargo run --example nested_params
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Acknowledgments

- The parameter parsing implementation is ported from [Rack's QueryParser](https://github.com/rack/rack), which provides robust and battle-tested parameter parsing capabilities.
- This project draws inspiration from Ruby on Rails' parameter handling system, adapting its elegant approach to parameter processing for the Rust and Axum ecosystem.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
