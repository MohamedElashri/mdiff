//! Shared Git admission control. Background work can occupy only one of four slots,
//! leaving capacity for interactive reads even while a large graph is being built.
use std::future::Future;
use std::sync::LazyLock;
use tokio::sync::{Semaphore, SemaphorePermit};
tokio::task_local! { static BACKGROUND: bool; }
static ALL: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(4));
static LOW: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));
pub struct Permit {
    _all: SemaphorePermit<'static>,
    _low: Option<SemaphorePermit<'static>>,
}
pub async fn acquire() -> Permit {
    let low = if BACKGROUND.try_with(|value| *value).unwrap_or(false) {
        Some(LOW.acquire().await.expect("scheduler open"))
    } else {
        None
    };
    Permit { _all: ALL.acquire().await.expect("scheduler open"), _low: low }
}
pub async fn background<T>(job: impl Future<Output = T>) -> T {
    BACKGROUND.scope(true, job).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn background_work_leaves_interactive_capacity() {
        let low = background(acquire()).await;
        let interactive = tokio::time::timeout(std::time::Duration::from_secs(1), acquire()).await.unwrap();
        assert!(tokio::time::timeout(std::time::Duration::from_millis(10), background(acquire())).await.is_err());
        drop((low, interactive));
        assert!(tokio::time::timeout(std::time::Duration::from_secs(1), background(acquire())).await.is_ok());
    }
}
