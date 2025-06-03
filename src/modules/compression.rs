use async_compression::{tokio::{bufread::ZstdDecoder, write::ZstdEncoder}, Level};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use anyhow::Result;
use std::str::FromStr;

use crate::cli::CompressionConfig;

#[derive(Debug, Clone)]
pub enum CompressionLevel {
    Fastest,
    Best,
    Default,
    Precise(i32),
}

/// We use our own enum for compression level in case we ever want to switch out the library we use for compression.
/// Then we're not forced to change the CLI options.
impl FromStr for CompressionLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "FASTEST" => Ok(CompressionLevel::Fastest),
            "BEST" => Ok(CompressionLevel::Best),
            "DEFAULT" => Ok(CompressionLevel::Default),
            s => match s.parse::<i32>() {
                Ok(level) => Ok(CompressionLevel::Precise(level)),
                Err(_) => Err(format!("Invalid compression level: {}", s)),
            },
        }
    }
}

impl From<&CompressionLevel> for Level {
    fn from(level: &CompressionLevel) -> Self {
        match level {
            CompressionLevel::Fastest => Level::Fastest,
            CompressionLevel::Best => Level::Best, 
            CompressionLevel::Default => Level::Default,
            CompressionLevel::Precise(n) => Level::Precise(*n),
        }
    }
}


#[async_trait]
pub trait CompressionTrait {
    async fn compress_stream<R,W>(&self, reader: R,writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin,
        W: AsyncWrite + Send + Unpin;

    async fn decompress_stream<R,W>(&self, reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin,
        W: AsyncWrite + Send + Unpin;
}

pub struct ZstdCompression {
    level: CompressionLevel,
}

impl ZstdCompression {
    pub fn new(config: CompressionConfig) -> Self {
        
        Self { level: config.level.into() }  
    }
}

#[async_trait]
impl CompressionTrait for ZstdCompression {
    async fn compress_stream<R,W>(&self, mut reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin,
        W: AsyncWrite + Send + Unpin,
    {        
        // We need an encoder that we can pass to tokio::io::copy
        let mut encoder = ZstdEncoder::with_quality(writer, Level::from(&self.level));

        tokio::io::copy(&mut reader, &mut encoder).await?;
        encoder.shutdown().await?;
        Ok(())
    }

    async fn decompress_stream<R,W>(&self, reader: R, mut writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin,
        W: AsyncWrite + Send + Unpin,
    {
        let mut decoder = ZstdDecoder::new(BufReader::new(reader));
        tokio::io::copy(&mut decoder, &mut writer).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn test_compression_level_parsing() {
        assert!(matches!(CompressionLevel::from_str("FASTEST").unwrap(), CompressionLevel::Fastest));
        assert!(matches!(CompressionLevel::from_str("BEST").unwrap(), CompressionLevel::Best));
        assert!(matches!(CompressionLevel::from_str("DEFAULT").unwrap(), CompressionLevel::Default));
        assert!(matches!(CompressionLevel::from_str("3").unwrap(), CompressionLevel::Precise(3)));
        assert!(matches!(CompressionLevel::from_str("-1").unwrap(), CompressionLevel::Precise(-1)));
        assert!(CompressionLevel::from_str("INVALID").is_err());
    }

    #[test]
    fn test_compression_level_conversion() {
        assert!(matches!(Level::from(&CompressionLevel::Fastest), Level::Fastest));
        assert!(matches!(Level::from(&CompressionLevel::Best), Level::Best));
        assert!(matches!(Level::from(&CompressionLevel::Default), Level::Default));
        assert!(matches!(Level::from(&CompressionLevel::Precise(3)), Level::Precise(3)));
    }

    #[tokio::test]
    async fn test_compression_and_decompression() {
        let compression = ZstdCompression::new(CompressionConfig { enabled: true, level: CompressionLevel::Default });
        
        // Test data
        let original_data = b"Hello, this is a test string that we will compress and then decompress!";
        
        // Create in-memory buffers for compression
        let mut compressed_data = Vec::new();
        let mut decompressed_data = Vec::new();
        
        // Compress the data
        compression.compress_stream(
            Cursor::new(original_data),
            &mut compressed_data
        ).await.unwrap();
        
        // Verify that the compressed data is different from original
        assert_ne!(compressed_data, original_data);
        
        // Decompress the data
        compression.decompress_stream(
            Cursor::new(&compressed_data),
            &mut decompressed_data
        ).await.unwrap();
        
        // Verify that the decompressed data matches the original
        assert_eq!(decompressed_data, original_data);
    }

    #[tokio::test]
    async fn test_compression_with_different_levels() {
        let test_data = b"This is a test string that we will compress with different levels";
        
        // Test with different compression levels
        let levels = [
            CompressionLevel::Fastest,
            CompressionLevel::Best,
            CompressionLevel::Default,
            CompressionLevel::Precise(3),
        ];
        
        for level in levels {
            let compression = ZstdCompression::new(CompressionConfig { enabled: true, level: level });
            let mut compressed_data = Vec::new();
            let mut decompressed_data = Vec::new();
            
            // Compress
            compression.compress_stream(
                Cursor::new(test_data),
                &mut compressed_data
            ).await.unwrap();
            
            // Decompress
            compression.decompress_stream(
                Cursor::new(&compressed_data),
                &mut decompressed_data
            ).await.unwrap();
            
            // Verify round-trip
            assert_eq!(decompressed_data, test_data);
        }
    }

    #[tokio::test]
    async fn test_compression_levels_roundtrip() {
        // Create test data with some patterns to make compression meaningful
        let test_data = b"This is a test string that we will repeat multiple times to create a larger dataset for testing compression. ".repeat(50);
        
        // Test with different compression levels
        let levels = [
            CompressionLevel::Fastest,
            CompressionLevel::Default,
            CompressionLevel::Precise(10),
            CompressionLevel::Best,
        ];
        
        for level in levels {
            let compression = ZstdCompression::new(CompressionConfig { enabled: true, level: level.clone() });
            let mut compressed_data = Vec::new();
            let mut decompressed_data = Vec::new();
            
            // Compress
            compression.compress_stream(
                Cursor::new(&test_data),
                &mut compressed_data
            ).await.unwrap();
            
            // Calculate compression ratio
            let ratio = (compressed_data.len() as f64 / test_data.len() as f64) * 100.0;
            println!("Compression level {:?}: {} bytes -> {} bytes ({}%)", 
                level, test_data.len(), compressed_data.len(), ratio);
            
            // For Fastest level, we might not get compression, but that's okay
            if !matches!(level, CompressionLevel::Fastest) {
                assert!(compressed_data.len() < test_data.len(), 
                    "Compression level {:?} did not reduce data size (original: {}, compressed: {})", 
                    level, test_data.len(), compressed_data.len());
            }
            
            // Decompress
            compression.decompress_stream(
                Cursor::new(&compressed_data),
                &mut decompressed_data
            ).await.unwrap();
            
            // Verify round-trip
            assert_eq!(decompressed_data, test_data,
                "Compression level {:?} failed to preserve data integrity", level);
        }
    }
}