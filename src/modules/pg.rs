use anyhow::Result;
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Command, ChildStdout};
use std::process::Stdio;
use std::sync::Arc;
use indicatif::ProgressBar;
use tokio::sync::mpsc;
use bytes::Bytes;

use crate::utils::counting_reader::CountingReader;



#[async_trait]
pub trait PostgresTrait {
    async fn dump(&self) -> Result<Box<dyn AsyncRead + Send + Unpin>>;
}

pub struct PostgresClient {
    host: String,
    port: u16,
    user: String,
    password: String,
    dbname: String,
}

impl PostgresClient {
    pub fn new(host: String, port: u16, user: String, password: String, dbname: String) -> Self {
        Self {
            host,
            port,
            user,
            password,
            dbname,
        }
    }
}

#[async_trait]
impl PostgresTrait for PostgresClient {
    async fn dump(&self) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        let mut cmd = Command::new("pg_dump");
        cmd.env("PGPASSWORD", &self.password)
            .arg("--host").arg(&self.host)
            .arg("--port").arg(self.port.to_string())
            .arg("--username").arg(&self.user)
            .arg("--dbname").arg(&self.dbname)
            .arg("--no-password")
            .arg("--no-owner")
            .arg("--no-acl")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("Failed to get pg_dump stdout");
        let stderr = child.stderr.take().expect("Failed to get pg_dump stderr");

        // Spawn a background task to handle stderr
        tokio::spawn(async move {
            let mut stderr_buf = Vec::new();
            let mut stderr_reader = tokio::io::BufReader::new(stderr);
            stderr_reader.read_to_end(&mut stderr_buf).await.ok();
            if !stderr_buf.is_empty() {
                eprintln!("pg_dump stderr: {}", String::from_utf8_lossy(&stderr_buf));
            }
        });

        // Wrap stdout in a counting reader, regardless of whether a progress bar is provided
        let stdout = BufReader::new(stdout);
        let stdout = BufReader::new(stdout);
        Ok(Box::new(stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_pg_dump() {
        let client = PostgresClient::new(
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
            "postgres".to_string(),
            "postgres".to_string(),
        );

        // This test will fail if pg_dump is not installed or if the database is not accessible
        let result = client.dump(None).await;
        assert!(result.is_ok());

        if let Ok(mut reader) = result {
            let mut buffer = Vec::new();
            let read_result = reader.read_to_end(&mut buffer).await;
            assert!(read_result.is_ok() || !buffer.is_empty());
        }
    }
}
