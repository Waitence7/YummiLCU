use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::Semaphore;

// One outstanding OS operation per resource, even after its async caller times
// out or is cancelled. A stalled syscall cannot consume the whole blocking pool
// or race a newer write after a reconnect.
pub(crate) struct BlockingIo(OnceLock<Arc<Semaphore>>);

impl BlockingIo {
    pub(crate) const fn new() -> Self {
        Self(OnceLock::new())
    }

    pub(crate) async fn run<T: Send + 'static>(
        &self,
        limit: Duration,
        operation: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, String> {
        let gate = self.0.get_or_init(|| Arc::new(Semaphore::new(1)));
        let permit = Arc::clone(gate)
            .try_acquire_owned()
            .map_err(|_| "blocking I/O still busy".to_owned())?;
        let task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            operation()
        });
        tokio::time::timeout(limit, task)
            .await
            .map_err(|_| "blocking I/O deadline exceeded".to_owned())?
            .map_err(|error| format!("blocking I/O task failed: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn cancelling_caller_does_not_release_running_io() {
        let io = Arc::new(BlockingIo::new());
        let worker_io = Arc::clone(&io);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let caller = tokio::spawn(async move {
            worker_io
                .run(Duration::from_secs(30), move || {
                    let _ = entered_tx.send(());
                    let _ = release_rx.recv();
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert!(io
            .run(Duration::from_secs(1), || panic!("overlapping I/O"))
            .await
            .unwrap_err()
            .contains("busy"));
        release_tx.send(()).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn timeout_keeps_gate_until_os_operation_finishes() {
        let io = BlockingIo::new();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let result = io
            .run(Duration::from_millis(25), move || {
                let _ = release_rx.recv();
                let _ = done_tx.send(());
            })
            .await;
        assert!(result.unwrap_err().contains("deadline"));
        let second = io
            .run(Duration::from_secs(1), || {
                panic!("must not start a second operation")
            })
            .await;
        assert!(second.unwrap_err().contains("busy"));
        release_tx.send(()).unwrap();
        done_rx.await.unwrap();
        // The notification precedes releasing the permit by a few instructions.
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match io.run(Duration::from_secs(1), || 42).await {
                    Ok(value) => {
                        assert_eq!(value, 42);
                        break;
                    }
                    Err(error) if error.contains("busy") => tokio::task::yield_now().await,
                    Err(error) => panic!("{error}"),
                }
            }
        })
        .await
        .unwrap();
    }
}
