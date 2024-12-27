use axum::{http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use axum_params::Params;
use serde::{Deserialize, Serialize};

// Simple parameters with path, query, and optional fields
#[derive(Debug, Deserialize, Serialize)]
struct TestParams {
    id: i32,      // Path parameter (/users/:id)
    name: String, // From JSON or form
    #[serde(default)]
    extra: Option<String>, // Optional query parameter
}

#[axum::debug_handler]
async fn test_params_handler(Params(test, _): Params<TestParams>) -> impl IntoResponse {
    // Access parameters naturally
    println!("ID: {}, Name: {}", test.id, test.name);
    (StatusCode::OK, Json(test))
}

#[tokio::main]
async fn main() {
    // Build our application with a route
    let app = Router::new().route("/users/:id", post(test_params_handler));

    // Run it
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/*
Test with curl:

# Combined path, query, and JSON parameters
curl -X POST "http://localhost:3000/users/123?extra=additional" \
  -H "Content-Type: application/json" \
  -d '{"name": "John Doe"}'

Expected response:
{
  "id": 123,
  "name": "John Doe",
  "extra": "additional"
}

# Form data instead of JSON
curl -X POST "http://localhost:3000/users/123?extra=additional" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "name=John%20Doe"

# Test using this source file as form data
curl -X POST "http://localhost:3000/users/123?extra=additional" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "name@examples/basic_params.rs"
*/
