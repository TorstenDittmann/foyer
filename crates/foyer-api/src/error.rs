//! The API's error type. Every handler returns `Result<_, AppError>`, which
//! renders as an [`ApiError`] JSON body with a matching status code.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use foyer_types::ApiError;

#[derive(Debug)]
pub enum AppError {
    /// 400: the request is malformed or a field is invalid.
    Validation(String),
    /// 401: no valid credentials.
    Unauthorized,
    /// 403: signed in, but not allowed.
    Forbidden(String),
    /// 404.
    NotFound(&'static str),
    /// 409 with a specific code, e.g. `slug_taken`.
    Conflict { code: &'static str, message: String },
    /// 409: chunks that are referenced but not uploaded.
    MissingChunks(Vec<String>),
    /// 503: object storage is not configured.
    StorageUnavailable,
    /// 500: anything unexpected. Logged, never shown to the client.
    Internal(anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn validation(message: impl Into<String>) -> Self {
        AppError::Validation(message.into())
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        AppError::Conflict {
            code,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        AppError::Forbidden(message.into())
    }
}

/// Errors from the database and other backends are internal: `?` on them
/// gives a 500.
macro_rules! internal_from {
    ($($ty:ty),* $(,)?) => {
        $(impl From<$ty> for AppError {
            fn from(err: $ty) -> Self {
                AppError::Internal(err.into())
            }
        })*
    };
}

internal_from!(
    anyhow::Error,
    libsql::Error,
    serde_json::Error,
    tokio::task::JoinError,
    reqwest::Error,
);

/// Most chunk hashes we list in one error body.
const MAX_MISSING_LISTED: usize = 1000;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error, message, missing) = match self {
            AppError::Validation(message) => (StatusCode::BAD_REQUEST, "validation", message, None),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "missing, invalid or expired token".to_string(),
                None,
            ),
            AppError::Forbidden(message) => (StatusCode::FORBIDDEN, "forbidden", message, None),
            AppError::NotFound(what) => (
                StatusCode::NOT_FOUND,
                "not_found",
                format!("{what} not found"),
                None,
            ),
            AppError::Conflict { code, message } => (StatusCode::CONFLICT, code, message, None),
            AppError::MissingChunks(mut hashes) => {
                let message = format!("{} chunks are not uploaded yet", hashes.len());
                hashes.truncate(MAX_MISSING_LISTED);
                (
                    StatusCode::CONFLICT,
                    "missing_chunks",
                    message,
                    Some(hashes),
                )
            }
            AppError::StorageUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                "object storage is not configured".to_string(),
                None,
            ),
            AppError::Internal(err) => {
                tracing::error!(error = ?err, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error".to_string(),
                    None,
                )
            }
        };
        let body = ApiError {
            error: error.to_string(),
            message,
            missing,
        };
        (status, Json(body)).into_response()
    }
}

/// Wraps axum's JSON, path and query extractors so their rejections use the
/// same error body as everything else.
#[derive(axum::extract::FromRequest)]
#[from_request(via(axum::Json), rejection(AppError))]
pub struct JsonBody<T>(pub T);

#[derive(axum::extract::FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(AppError))]
pub struct PathParam<T>(pub T);

#[derive(axum::extract::FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(AppError))]
pub struct QueryParams<T>(pub T);

impl From<JsonRejection> for AppError {
    fn from(rejection: JsonRejection) -> Self {
        AppError::Validation(rejection.body_text())
    }
}

impl From<PathRejection> for AppError {
    fn from(rejection: PathRejection) -> Self {
        AppError::Validation(rejection.body_text())
    }
}

impl From<QueryRejection> for AppError {
    fn from(rejection: QueryRejection) -> Self {
        AppError::Validation(rejection.body_text())
    }
}
