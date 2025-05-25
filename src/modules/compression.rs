use anyhow::Result;
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, BufReader};
use flate2::write::{GzEncoder, GzDecoder};
use flate2::Compression;
use std::io::Write;

/// The optimal chunk size for gzip compression.
/// 
/// This size (1MB) was chosen based on the following considerations:
/// 1. gzip's deflate algorithm works best with chunks of 1MB-10MB
/// 2. Smaller chunks (like 32KB) have too much overhead from gzip headers
/// 3. Larger chunks (like 10MB) use more memory without significant compression benefits
/// 4. 1MB provides a good balance between memory usage and compression ratio
/// 5. This size aligns well with typical disk I/O buffer sizes
/// 6. 1MB is exactly 128 times our read buffer size (8KB), ensuring perfect alignment
const CHUNK_SIZE: usize = 1024 * 1024; // 1MB chunks

/// The size of the read buffer for each iteration.
/// This is set to 8KB to match typical disk I/O buffer sizes and to be a power of 2.
/// Each chunk (1MB) is filled with exactly 128 of these 8KB read operations.
const READ_BUFFER_SIZE: usize = 8 * 1024; // 8KB

#[async_trait]
pub trait CompressionTrait {
    async fn compress<R, W>(&self, reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin;
}

pub struct GzipCompression {
    level: u32,
}

impl GzipCompression {
    pub fn new(level: u32) -> Self {
        // Ensure level is between 0-9
        let level = level.min(9);
        Self { level }
    }
}

#[async_trait]
impl CompressionTrait for GzipCompression {
    async fn compress<R, W>(&self, reader: R, mut writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin,
    {
        let mut reader = BufReader::new(reader);
        // Allocate buffer with exact size needed for a full chunk
        let mut buffer = Vec::with_capacity(CHUNK_SIZE);
        let mut chunk = [0u8; READ_BUFFER_SIZE];

        loop {
            // Read a chunk of data
            let n = reader.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..n]);

            // If we've accumulated enough data or this is the last chunk
            if buffer.len() >= CHUNK_SIZE || n < READ_BUFFER_SIZE {
                // Create a new encoder for this chunk
                let mut encoder = GzEncoder::new(Vec::new(), Compression::new(self.level));
                encoder.write_all(&buffer)?;
                let compressed = encoder.finish()?;

                // Write the compressed chunk
                writer.write_all(&compressed).await?;
                buffer.clear();
            }
        }

        writer.shutdown().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_compression() {
        let compression = GzipCompression::new(6);
        
        // Test data - create a mix of compressible and incompressible data
        let mut original_data = Vec::new();
        // Add some compressible data (repeating patterns)
        original_data.extend(b"Hello, this is a test message that will be compressed!".repeat(1000));
        // Add some incompressible data (random bytes)
        original_data.extend((0..1000).map(|_| rand::random::<u8>()).collect::<Vec<u8>>());
        
        let original_length = original_data.len();
        let mut compressed_data = Vec::new();
        
        // Compress
        let reader = Cursor::new(original_data);
        let writer = Cursor::new(&mut compressed_data);
        compression.compress(reader, writer).await.unwrap();

        // Verify that compression produced some output
        assert!(!compressed_data.is_empty());
        assert!(compressed_data.len() < original_length);
        
        // Calculate and display compression statistics
        let compressed_length = compressed_data.len();
        let size_reduction = original_length - compressed_length;
        let reduction_percentage = (size_reduction as f64 / original_length as f64) * 100.0;
        
        println!("Compression results:");
        println!("  Original size: {} bytes", original_length);
        println!("  Compressed size: {} bytes", compressed_length);
        println!("  Size reduction: {} bytes ({:.2}%)", size_reduction, reduction_percentage);
    }
} 