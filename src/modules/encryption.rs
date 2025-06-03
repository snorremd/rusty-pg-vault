use age::{secrecy, Decryptor, Encryptor};
use anyhow::Result;
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::compat::{FuturesAsyncWriteCompatExt, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::cli::CryptoConfig;

#[async_trait]
pub trait EncryptionTrait {

    async fn encrypt_stream<R, W>(&self, reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin,
        W: AsyncWrite + Send + Unpin;
    
    async fn decrypt_stream<R, W>(&self, reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin,
        W: AsyncWrite + Send + Unpin;
}

pub struct AgeEncryption {
    passphrase: secrecy::SecretString,
}

impl AgeEncryption {
    pub fn new(config: CryptoConfig) -> Self {
        Self { passphrase: secrecy::SecretString::new(config.passphrase.into()) }
    }
}

#[async_trait]
impl EncryptionTrait for AgeEncryption {
    
    async fn encrypt_stream<R, W>(&self, mut reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin,
        W: AsyncWrite + Send + Unpin,
    {
        // Age library expects futures Reader and Writer, add compat layer to convert between tokio and futures
        let compat_writer = writer.compat_write();
        // Create an encryptor and wrap the writer in it
        let encryptor = Encryptor::with_user_passphrase(self.passphrase.clone());
        let encrypted_writer = encryptor.wrap_async_output(compat_writer).await?;
        let mut encrypted_compat = encrypted_writer.compat_write();
        // Start copying data from the reader (plaintext) into the writer (encrypted)
        tokio::io::copy(&mut reader, &mut encrypted_compat).await?;
        encrypted_compat.flush().await?;
        encrypted_compat.shutdown().await?;
        Ok(())
    }

    async fn decrypt_stream<R, W>(&self, reader: R, mut writer: W) -> Result<()>
    where
        R: AsyncRead + Send + Unpin,
        W: AsyncWrite + Send + Unpin,
    {
        let compat_reader = reader.compat();
        let decryptor = Decryptor::new_async(compat_reader).await?;
        let mut decrypted_reader = decryptor.decrypt_async(std::iter::once(&age::scrypt::Identity::new(self.passphrase.clone()) as _))?;

        let mut buf = [0u8; 8192];
        loop {
            let n = futures::AsyncReadExt::read(&mut decrypted_reader, &mut buf).await?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n]).await?;
        }
        writer.flush().await?;
        writer.shutdown().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use std::io::Cursor;

    #[tokio::test]
    async fn test_encryption_and_decryption() {
        println!("AGE_SCRYPT_PARAMS={:?}", std::env::var("AGE_SCRYPT_PARAMS"));
        let encryption = AgeEncryption::new(CryptoConfig { passphrase: "test-passphrase".to_string() });
        let original_data = b"Hello, this is a test message!";

        // Encrypt
        let mut encrypted_data = Vec::new();
        let mut reader = Cursor::new(original_data);
        let mut writer = Cursor::new(&mut encrypted_data);
        encryption.encrypt_stream(&mut reader, &mut writer).await.unwrap();

        // Verify that encryption produced some output
        assert!(!encrypted_data.is_empty());

        // Decrypt
        let mut decrypted_data = Vec::new();
        let mut encrypted_reader = Cursor::new(&encrypted_data);
        let mut decrypted_writer = Cursor::new(&mut decrypted_data);
        encryption.decrypt_stream(&mut encrypted_reader, &mut decrypted_writer).await.unwrap();

        // Verify round-trip
        assert_eq!(decrypted_data, original_data);
    }
}
