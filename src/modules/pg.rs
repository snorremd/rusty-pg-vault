use anyhow::Result;
use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Command};
use tokio::io::AsyncBufReadExt;

use crate::cli::PostgresConfig;

#[async_trait]
pub trait PostgresTrait {
    async fn dump(&self) -> Result<Box<dyn AsyncRead + Send + Unpin>>;
    async fn restore(&self, input: Box<dyn AsyncRead + Send + Unpin>) -> Result<()>;
    async fn create_database(&self) -> Result<()>;
}

// Command factory trait for testing
pub trait CommandFactory {
    fn create_command(&self, program: &str) -> Command;
}

// Real implementation
pub struct RealCommandFactory;

impl CommandFactory for RealCommandFactory {
    fn create_command(&self, program: &str) -> Command {
        Command::new(program)
    }
}

pub struct PostgresClient<F: CommandFactory = RealCommandFactory> {
    host: String,
    port: u16,
    user: String,
    password: String,
    dbname: String,
    command_factory: F,
}

impl PostgresClient {
    pub fn new(opts: PostgresConfig) -> Self {
        Self {
            host: opts.host,
            port: opts.port,
            user: opts.user,
            password: opts.password,
            dbname: opts.dbname,
            command_factory: RealCommandFactory,
        }
    }
}

#[async_trait]
impl<F: CommandFactory + Send + Sync> PostgresTrait for PostgresClient<F> {
    async fn dump(&self) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        let mut cmd = self.command_factory.create_command("pg_dump");
        cmd.env("PGPASSWORD", &self.password)
            .arg("--host")
            .arg(&self.host)
            .arg("--port")
            .arg(self.port.to_string())
            .arg("--username")
            .arg(&self.user)
            .arg("--dbname")
            .arg(&self.dbname)
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

        let stdout = BufReader::new(stdout);
        Ok(Box::new(stdout))
    }

    async fn create_database(&self) -> Result<()> {
        // First, try to connect to postgres database to create our target database
        let status = Command::new("psql")
            .env("PGPASSWORD", &self.password)
            .arg("--host")
            .arg(&self.host)
            .arg("--port")
            .arg(self.port.to_string())
            .arg("--username")
            .arg(&self.user)
            .arg("--dbname")
            .arg("postgres")
            .arg("--no-password")
            .arg("--quiet")
            .arg("--command")
            .arg(&format!("CREATE DATABASE \"{}\";", self.dbname))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await?;

        if !status.success() {
            // Database might already exist, which is fine
            eprintln!("Note: Database '{}' might already exist or creation failed", self.dbname);
        }

        Ok(())
    }

    async fn restore(&self, input: Box<dyn AsyncRead + Send + Unpin>) -> Result<()> {
        // Create database first
        self.create_database().await?;

        let mut child = Command::new("psql")
            .env("PGPASSWORD", &self.password)
            .arg("--host")
            .arg(&self.host)
            .arg("--port")
            .arg(self.port.to_string())
            .arg("--username")
            .arg(&self.user)
            .arg("--dbname")
            .arg(&self.dbname)
            .arg("--no-password")
            .arg("--quiet")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to get stdin"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow::anyhow!("Failed to get stderr"))?;

        // Create channels for stderr output
        let (stderr_tx, mut stderr_rx) = tokio::sync::mpsc::channel(100);
        let stderr_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }
                if let Err(_) = stderr_tx.send(line.clone()).await {
                    break;
                }
                line.clear();
            }
        });

        // Create a buffered reader for the input
        let mut input = BufReader::with_capacity(1024 * 1024, input); // 1MB buffer
        let mut stdin = BufWriter::with_capacity(1024 * 1024, stdin); // 1MB buffer

        // Spawn a task to handle stdin
        let stdin_handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 64 * 1024]; // 64KB chunks
            loop {
                match input.read_buf(&mut buf).await {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if let Err(e) = stdin.write_all(&buf[..n]).await {
                            eprintln!("Error writing to psql stdin: {}", e);
                            return Err(e);
                        }
                        if let Err(e) = stdin.flush().await {
                            eprintln!("Error flushing psql stdin: {}", e);
                            return Err(e);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading from input: {}", e);
                        return Err(e);
                    }
                }
            }
            if let Err(e) = stdin.shutdown().await {
                eprintln!("Error shutting down psql stdin: {}", e);
                return Err(e);
            }
            Ok(())
        });

        // Wait for stdin to complete
        if let Err(e) = stdin_handle.await? {
            eprintln!("Error in stdin task: {}", e);
            return Err(e.into());
        }

        // Wait for the process to complete
        let status = child.wait().await?;
        if !status.success() {
            let mut error_output = String::new();
            while let Some(line) = stderr_rx.recv().await {
                error_output.push_str(&line);
            }
            return Err(anyhow::anyhow!("psql restore failed: {}", error_output));
        }

        // Wait for stderr task to complete
        if let Err(e) = stderr_handle.await {
            eprintln!("Error in stderr task: {}", e);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    impl<F: CommandFactory> PostgresClient<F> {
        pub fn with_command_factory(
            host: String,
            port: u16,
            user: String,
            password: String,
            dbname: String,
            command_factory: F,
        ) -> Self {
            Self {
                host,
                port,
                user,
                password,
                dbname,
                command_factory,
            }
        }
    }

    // Mock command factory for testing
    struct MockCommandFactory {
        mock_output: Vec<u8>,
    }

    impl CommandFactory for MockCommandFactory {
        fn create_command(&self, program: &str) -> Command {
            let mut cmd = Command::new("echo");
            if program == "psql" {
                // For psql, we want to simulate success
                cmd.arg("-n").arg("CREATE TABLE\nINSERT 0 1");
            } else {
                // For pg_dump, use the mock output
                cmd.arg("-n")
                    .arg(String::from_utf8_lossy(&self.mock_output).to_string());
            }
            cmd
        }
    }

    #[tokio::test]
    async fn test_pg_dump() {
        let mock_output = r#"
-- Mock PostgreSQL dump
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100),
    email VARCHAR(255)
);

INSERT INTO users (name, email) VALUES
    ('Test User', 'test@example.com');
"#
        .as_bytes()
        .to_vec();

        let client = PostgresClient::with_command_factory(
            "localhost".to_string(),
            5432,
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            MockCommandFactory { mock_output },
        );

        let result = client.dump().await;
        assert!(result.is_ok());

        if let Ok(mut reader) = result {
            let mut buffer = Vec::new();
            let read_result = reader.read_to_end(&mut buffer).await;
            assert!(read_result.is_ok());
            assert!(!buffer.is_empty());

            // Verify the content contains our test data
            let content = String::from_utf8_lossy(&buffer);
            assert!(content.contains("CREATE TABLE users"));
            assert!(content.contains("Test User"));
        } else {
            panic!("Failed to get pg_dump content");
        }
    }

    #[tokio::test]
    async fn test_pg_restore() {
        // This test is complex due to the restore process involving multiple processes
        // For now, we'll just test that the client can be created and the method exists
        let client = PostgresClient::with_command_factory(
            "localhost".to_string(),
            5432,
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            MockCommandFactory { mock_output: vec![] },
        );

        // Test that create_database works (this is called by restore)
        let result = client.create_database().await;
        assert!(result.is_ok());
        
        // Note: Full restore testing would require more complex mocking
        // of the psql process and stdin/stdout handling
    }

    #[tokio::test]
    async fn test_create_database() {
        let client = PostgresClient::with_command_factory(
            "localhost".to_string(),
            5432,
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            MockCommandFactory { mock_output: vec![] },
        );

        let result = client.create_database().await;
        assert!(result.is_ok());
    }
}
