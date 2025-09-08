use std::io::Write;
use std::sync::Arc;
use tokio::io::AsyncWrite;
use indicatif::ProgressBar;

pub struct CountingWriter<W> {
    writer: W,
    progress_bar: Option<Arc<ProgressBar>>,
    bytes_written: usize,
}

impl<W> CountingWriter<W> {
    pub fn new(writer: W, progress_bar: Option<Arc<ProgressBar>>) -> Self {
        Self {
            writer,
            progress_bar,
            bytes_written: 0,
        }
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let bytes = self.writer.write(buf)?;
        self.bytes_written += bytes;
        if let Some(pb) = &self.progress_bar {
            pb.set_position(self.bytes_written as u64);
        }
        Ok(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for CountingWriter<W> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        let this = self.get_mut();
        let bytes = std::pin::Pin::new(&mut this.writer).poll_write(cx, buf);
        
        if let std::task::Poll::Ready(Ok(n)) = bytes {
            this.bytes_written += n;
            if let Some(pb) = &this.progress_bar {
                pb.set_position(this.bytes_written as u64);
            }
        }
        
        bytes
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::pin::Pin::new(&mut self.get_mut().writer).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::pin::Pin::new(&mut self.get_mut().writer).poll_shutdown(cx)
    }
} 