use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, Clone)]
pub enum Error {
    DecodeError(String),
    ReadError(String),
    IOError(String),
    MergeError(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(format!("{:?}", self).into())
            .unwrap()
    }
}
