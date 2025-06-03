use tokio::io::{AsyncRead, ReadBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::sync::Arc;
use indicatif::ProgressBar;

/// A wrapper around an `AsyncRead` that counts the number of bytes read and optionally updates a progress bar.
///
/// This reader maintains an internal counter of bytes read and can optionally update a progress bar
/// as data is read from the underlying reader. The progress bar updates are rate-limited by the
/// progress bar itself.
pub struct CountingReader<R> {
    inner: R,
    progress: Option<Arc<ProgressBar>>,
    count: u64,
}

impl<R: AsyncRead + Unpin> AsyncRead for CountingReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let pre = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);

        if let Poll::Ready(Ok(())) = &poll {
            let post = buf.filled().len();
            let delta = (post - pre) as u64;
            self.count += delta;
            if let Some(pb) = &self.progress {
                pb.inc(delta);
            }
        }

        poll
    }
}

impl<R> CountingReader<R> {
    /// Creates a new `CountingReader` that wraps the given reader.
    ///
    /// # Arguments
    ///
    /// * `inner` - The underlying reader to wrap
    /// * `progress` - An optional progress bar to update as bytes are read
    ///
    /// # Returns
    ///
    /// A new `CountingReader` instance
    pub fn new(inner: R, progress: Option<Arc<ProgressBar>>) -> Self {
        Self {
            inner,
            progress,
            count: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_counting_reader_basic() {
        let data = b"Hello, World!";
        let cursor = Cursor::new(data);
        let mut reader = CountingReader::new(cursor, None);

        let mut buf = vec![0; data.len()];
        let n = reader.read(&mut buf).await.unwrap();

        assert_eq!(n, data.len());
        assert_eq!(reader.count, data.len() as u64);
        assert_eq!(&buf, data);
    }

    #[tokio::test]
    async fn test_counting_reader_with_progress() {
        let data = b"Hello, World!";
        let cursor = Cursor::new(data);
        let progress = Arc::new(ProgressBar::new(data.len() as u64));
        let mut reader = CountingReader::new(cursor, Some(progress.clone()));

        let mut buf = vec![0; data.len()];
        let n = reader.read(&mut buf).await.unwrap();

        assert_eq!(n, data.len());
        assert_eq!(reader.count, data.len() as u64);
        assert_eq!(progress.position(), data.len() as u64);
        assert_eq!(&buf, data);
    }

    #[tokio::test]
    async fn test_counting_reader_partial_reads() {
        let data = b"Hello, World!";
        let cursor = Cursor::new(data);
        let mut reader = CountingReader::new(cursor, None);

        // Read first 5 bytes
        let mut buf = vec![0; 5];
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(n, 5);
        assert_eq!(reader.count, 5);

        // Read remaining bytes
        let mut buf = vec![0; data.len() - 5];
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(n, data.len() - 5);
        assert_eq!(reader.count, data.len() as u64);
    }
}

