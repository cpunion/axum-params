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
struct Post {
    title: String,
    content: String,
    tags: Vec<String>,
    cover: UploadFile,
    attachments: Vec<Attachment>,
}

#[debug_handler]
async fn create_post_handler(Params(post, _): Params<Post>) -> impl IntoResponse {
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

## Examples

- [Basic Parameters](examples/basic_params.rs) - Handling path, query, and JSON parameters
- [File Upload](examples/file_upload.rs) - Basic file upload with metadata
- [Nested Parameters](examples/nested_params.rs) - Complex nested structures with multiple file uploads

## Rails-like Parameter Structure

Just like Rails, you can send nested parameters in various formats:

### Form Data
```
post[title]=My Post
post[content]=Hello World
post[tags][]=rust
post[tags][]=web
post[cover]=@file1.jpg
post[attachments][0][name]=Document
post[attachments][0][file]=@doc.pdf
post[attachments][1][name]=Image
post[attachments][1][file]=@image.png
```

### JSON Body
```json
{
  "title": "My Post",
  "content": "Hello World",
  "tags": ["rust", "web"],
  "name": "john",
  "extra": "additional info"
}
```

### Combined Parameters Example
```rust
// Route: "/users/:id"
let response = server
    .post("/users/123")                         // Path parameter
    .add_query_params(&[("extra", "query")])    // Query parameter
    .json(&json!({ "name": "test" }))           // JSON body
    .await;
```

### Multipart Form
The library automatically handles multipart form data, allowing you to upload files within nested structures. Files can be placed at any level in the parameter tree, and you can combine them with regular form fields.

## Running Examples

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

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

This project draws inspiration from Ruby on Rails' parameter handling system, adapting its elegant approach to parameter processing for the Rust and Axum ecosystem.
