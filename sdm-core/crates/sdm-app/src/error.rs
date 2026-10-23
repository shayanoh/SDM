use serde::Serialize;
use specta::Type;

/// What a command reports to the UI. `kind` is stable; `message` is for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AppError {
    #[error("invalid {argument}: {message}")]
    InvalidArgument { argument: String, message: String },
    #[error("{message}")]
    Failed { message: String },
    #[error("internal error: {message}")]
    Internal { message: String },
}

impl AppError {
    pub fn invalid(argument: &str, message: impl Into<String>) -> Self {
        AppError::InvalidArgument {
            argument: argument.to_owned(),
            message: message.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        AppError::Failed {
            message: message.into(),
        }
    }
}
