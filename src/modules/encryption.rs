use age::{Encryptor, secrecy};
use anyhow::Result;
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use std::io::Write;
use tokio_util::compat::{TokioAsyncWriteCompatExt, Compat, FuturesAsyncWriteCompatExt};
use futures::AsyncWriteExt as FuturesAsyncWriteExt;
use std::io::{Cursor, Read};

#[async_trait]
pub trait EncryptionTrait {
    async fn encrypt<R, W>(&self, reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin;
}

pub struct AgeEncryption {
    passphrase: secrecy::SecretString,
}

impl AgeEncryption {
    pub fn new(passphrase: String) -> Self {
        Self { passphrase: secrecy::SecretString::new(passphrase.into()) }
    }
}

#[async_trait]
impl EncryptionTrait for AgeEncryption {
    async fn encrypt<R, W>(&self, mut reader: R, mut writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin,
    {
        // Read all data into memory
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).await?;
        
        // Create encryptor and encrypt the data
        let encryptor = Encryptor::with_user_passphrase(self.passphrase.clone());
        let mut encrypted_data = Vec::new();
        let mut encrypted_writer = encryptor.wrap_output(Cursor::new(&mut encrypted_data))?;
        encrypted_writer.write_all(&buffer)?;
        encrypted_writer.finish()?;

        // Write the encrypted data
        let mut cursor = Cursor::new(encrypted_data);
        let mut chunk = [0u8; 8192];

        loop {
            let n = Read::read(&mut cursor, &mut chunk).map_err(|e| anyhow::anyhow!("Failed to read from cursor: {}", e))?;
            if n == 0 {
                break;
            }
            writer.write_all(&chunk[..n]).await?;
        }

        writer.shutdown().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_encryption() {
        let passphrase = "test-passphrase";
        let encryption = AgeEncryption::new(passphrase.to_string());
        
        // Test data
        let original_data = b"Hello, this is a test message!";
        let mut encrypted_data = Vec::new();
        
        // Encrypt
        let reader = Cursor::new(original_data);
        let writer = Cursor::new(&mut encrypted_data);
        encryption.encrypt(reader, writer).await.unwrap();
        
        // Verify that encryption produced some output
        assert!(!encrypted_data.is_empty());
    }
}
