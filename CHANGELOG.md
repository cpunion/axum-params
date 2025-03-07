# Changelog

## [0.4.1](https://github.com/cpunion/axum-params/compare/v0.4.0...v0.4.1) - 2025-03-07

### Other

- Update README.md
- fix workflow name
- add release workflow
- update GitHub Actions dependencies in CI workflow
- update changelog for v0.4.0

## v0.4.0 (2025-03-03)

### Changes
- Upgrade to axum@0.8.1
- Upgrade to Rust Edition 2024
- Update dependencies
- Remove unused trait ParamsReader

## v0.3.0 (2025-01-02)

### Breaking Changes
- Process JSON escaped characters correctly in JSON parser
  - Now properly handles all standard JSON escape sequences (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`)
  - Fixed handling of Unicode escape sequences
  - Improved error handling for invalid escape sequences

## v0.2.0 (2024-12-29)

### Changes
- Port [rack/query_parser.rb](https://github.com/rack/rack/blob/main/lib/rack/query_parser.rb) to make better compatible with rails/rack
- Port [rack/spec_utils.rb](https://github.com/rack/rack/blob/main/test/spec_utils.rb)
- Merge JSON with other structured data
- Rename `axum_params::ParamsValue` to `axum_params::Value`, doesn't need use it directly

## v0.1.0 (2024-12-25)

### Features

#### Unified Parameter Handling
- Path parameters
- Query parameters
- Form data
- Multipart form data
- JSON body
- All parameter types can be processed simultaneously
- Every parameter type supports structured data (arrays and objects)

#### Rails-like Tree-Structured Parameters
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
