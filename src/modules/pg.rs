use anyhow::Result;
use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Command};

#[async_trait]
pub trait PostgresTrait {
    async fn dump(&self) -> Result<Box<dyn AsyncRead + Send + Unpin>>;
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
    pub fn new(host: String, port: u16, user: String, password: String, dbname: String) -> Self {
        Self {
            host,
            port,
            user,
            password,
            dbname,
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
        fn create_command(&self, _program: &str) -> Command {
            let mut cmd = Command::new("echo");
            cmd.arg("-n")
                .arg(String::from_utf8_lossy(&self.mock_output).to_string());
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
}
