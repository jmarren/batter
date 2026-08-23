use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

use crate::util;

struct WriteJob {
    path: PathBuf,
    data: Vec<u8>,
}

enum Task {
    Write(WriteJob),
    Shutdown,
}

pub struct Writer {
    base_path: PathBuf,
    tx: mpsc::UnboundedSender<Task>,
    // the background consumer task; taken and awaited by join(). wrapped so
    // join() can work from &self - only ever accessed by whichever call to
    // join() gets there first, since every other write()/join() caller only
    // ever needs the sender.
    consumer: Mutex<Option<JoinHandle<()>>>,
}

impl Writer {
    pub fn new(base_path: impl Into<PathBuf>, deadline: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let consumer = tokio::spawn(run_consumer(rx, deadline));

        Self {
            base_path: base_path.into(),
            tx,
            consumer: Mutex::new(Some(consumer)),
        }
    }

    pub fn write(&self, path: impl Into<PathBuf>, data: &[u8]) -> Result<()> {
        let full_path = self.base_path.join(path.into());

        let job = WriteJob {
            path: full_path,
            data: data.to_vec(),
        };

        self.tx
            .send(Task::Write(job))
            .map_err(|_| anyhow!("writer's background task is no longer running"))
    }

    pub fn write_js(&self, path: impl Into<PathBuf>, data: &[u8]) -> Result<()> {
        self.write_ext(path, data, "js")
    }

    pub fn write_txt(&self, path: impl Into<PathBuf>, data: &[u8]) -> Result<()> {
        self.write_ext(path, data, "txt")
    }

    pub fn write_html(&self, path: impl Into<PathBuf>, data: &[u8]) -> Result<()> {
        self.write_ext(path, data, "html")
    }

    pub fn write_ext(&self, _path: impl Into<PathBuf>, data: &[u8], ext: &str) -> Result<()> {
        let mut path: PathBuf = _path.into();
        // set js extension
        path.set_extension(ext);
        // write
        self.write(path, data)
    }

    // signals the background writer to stop accepting new jobs and waits for
    // it to finish (or hit its deadline, at which point remaining writes are
    // aborted and logged). safe to call more than once - only the first
    // caller actually awaits the consumer; any write() attempted after this
    // simply gets a "no longer running" error back, since the channel send
    // fails once the consumer has read the Shutdown task and exited.
    pub async fn join(&self) {
        // only fails if the consumer has already exited (e.g. panicked, or a
        // previous join() already ran), in which case there's nothing left
        // to signal
        let _ = self.tx.send(Task::Shutdown);

        // take() ensures only the first caller actually awaits the handle;
        // later callers see None and return immediately
        let handle = self.consumer.lock().unwrap().take();

        if let Some(handle) = handle
            && let Err(err) = handle.await
        {
            tracing::error!("writer background task panicked: {err}");
        }
    }
}

// receives write jobs and fans each one out into a JoinSet so writes happen
// concurrently; once the channel is closed, drains whatever's left in the
// JoinSet, bounded by `deadline` - aborting and logging any stragglers
async fn run_consumer(mut rx: mpsc::UnboundedReceiver<Task>, deadline: Duration) {
    let mut join_set: JoinSet<Result<()>> = JoinSet::new();

    loop {
        tokio::select! {
            job = rx.recv() => {
                match job {
                    Some(Task::Write(job)) => {
                        join_set.spawn(async move { util::write_file(job.path, &job.data).await });
                    },
                    Some(Task::Shutdown) => {
                        break;
                    },
                    None => break,
                }
            }
            Some(result) = join_set.join_next(), if !join_set.is_empty() => {
                log_write_result(result);
            }
        }
    }

    let pending = join_set.len();

    // if the timeout fires, the future driving join_all (and the JoinSet it
    // owns) is dropped, which aborts every remaining task automatically
    match tokio::time::timeout(deadline, join_set.join_all()).await {
        Ok(results) => {
            for result in results {
                log_write_result(Ok(result));
            }
        }
        Err(_) => {
            tracing::error!(
                "writer deadline of {:?} exceeded; aborted {pending} pending write(s)",
                deadline
            );
        }
    }
}

fn log_write_result(result: std::result::Result<Result<()>, tokio::task::JoinError>) {
    match result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => tracing::error!("failed to write file: {err:#}"),
        Err(err) => tracing::error!("write task panicked: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("batter-writer-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn writes_are_flushed_to_disk_by_join() {
        let dir = scratch_dir("flush");
        let writer = Writer::new(&dir, Duration::from_secs(5));

        writer.write("a.txt", b"hello").unwrap();
        writer.write("nested/b.txt", b"world").unwrap();

        writer.join().await;

        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(dir.join("nested/b.txt")).unwrap(), b"world");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn join_is_safe_to_call_more_than_once() {
        let dir = scratch_dir("double-join");
        let writer = Writer::new(&dir, Duration::from_secs(5));

        writer.write("a.txt", b"hello").unwrap();

        writer.join().await;
        writer.join().await;

        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"hello");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn write_after_join_returns_an_error() {
        let dir = scratch_dir("write-after-join");
        let writer = Writer::new(&dir, Duration::from_secs(5));

        writer.join().await;

        assert!(writer.write("a.txt", b"hello").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn join_returns_promptly_with_a_short_deadline() {
        let dir = scratch_dir("deadline");
        let writer = Writer::new(&dir, Duration::from_millis(50));

        // queue a large number of writes so at least some are still pending
        // when join() is called
        for i in 0..500 {
            writer.write(format!("{i}.txt"), b"hello").unwrap();
        }

        let started = std::time::Instant::now();
        writer.join().await;
        let elapsed = started.elapsed();

        // regardless of how many writes finished in time, join() itself must
        // never block past roughly its configured deadline
        assert!(
            elapsed < Duration::from_secs(2),
            "join() took too long: {elapsed:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
