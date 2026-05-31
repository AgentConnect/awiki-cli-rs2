use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerJoinError {
    detail: String,
}

impl WorkerJoinError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for WorkerJoinError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for WorkerJoinError {}

pub(crate) async fn run_blocking<F, T>(operation: F) -> Result<T, WorkerJoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|err| WorkerJoinError::new(format!("blocking worker join failed: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_blocking_returns_result() {
        let result = run_blocking(|| 41 + 1).await.unwrap();

        assert_eq!(result, 42);
    }
}
