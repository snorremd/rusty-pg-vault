use clap::{Parser, Subcommand, Args, arg};
use crate::modules::compression::{CompressionLevel};
use std::str::FromStr;

/// Main CLI parser
#[derive(Parser, Debug)]
#[command(name = "rusty-pg-vault", author, version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Backup(BackupOpts),
    Restore(RestoreOpts),
    List(ListOpts),
}

/// Common config for S3
#[derive(Args, Debug, Clone)]
pub struct S3Config {
    #[arg(long, env = "S3_BUCKET")]
    pub s3_bucket: String,

    #[arg(long, env = "S3_REGION", default_value = "us-east-1")]
    pub s3_region: String,

    #[arg(long, env = "S3_PREFIX", default_value = "")]
    pub s3_prefix: String,

    #[arg(long, env = "AWS_ACCESS_KEY_ID")]
    pub aws_access_key_id: String,

    #[arg(long, env = "AWS_SECRET_ACCESS_KEY")]
    pub aws_secret_access_key: String,

    #[arg(long, env = "AWS_ENDPOINT_URL")]
    pub aws_endpoint_url: String,
}

/// PostgreSQL connection options (matches official env vars)
#[derive(Args, Debug, Clone)]
pub struct PostgresConfig {
    #[arg(long = "pg-host", env = "PGHOST", required = true)]
    pub host: String,

    #[arg(long = "pg-port", env = "PGPORT", default_value = "5432")]
    pub port: u16,

    #[arg(long = "pg-user", env = "PGUSER", required = true)]
    pub user: String,

    #[arg(long = "pg-password", env = "PGPASSWORD", required = true)]
    pub password: String,

    #[arg(long = "pg-database", env = "PGDATABASE", required = true)]
    pub dbname: String,

    /// Comma-separated list of databases to backup. If not specified, only the main database will be backed up.
    #[arg(long = "pg-databases", env = "PG_DATABASES", value_delimiter = ',')]
    pub databases: Option<Vec<String>>,
}

/// Common encryption input
#[derive(Args, Debug, Clone)]
pub struct CryptoConfig {
    #[arg(long, env = "BACKUP_PASSPHRASE")]
    pub passphrase: String,
}


/// Compression options
#[derive(Args, Debug, Clone)]
pub struct CompressionConfig {
    /// Enable compression (default: true)
    #[arg(long = "compression-enabled", env = "BACKUP_COMPRESS", default_value = "true")]
    pub enabled: bool,

    /// Compression level
    #[arg(long = "compression-level", env = "BACKUP_COMPRESSION_LEVEL", default_value = "FASTEST", value_parser = CompressionLevel::from_str)]
    pub level: CompressionLevel,
}


/// Backup options
#[derive(Args, Debug, Clone)]
pub struct BackupOpts {
    #[command(flatten)]
    pub pg: PostgresConfig,

    #[command(flatten)]
    pub s3: S3Config,

    #[command(flatten)]
    pub crypto: CryptoConfig,

    #[command(flatten)]
    pub compression: CompressionConfig,
}

/// Restore options
#[derive(Args, Debug)]
pub struct RestoreOpts {
    #[arg(long)]
    pub s3_key: String,

    #[command(flatten)]
    pub pg: PostgresConfig,

    #[command(flatten)]
    pub s3: S3Config,

    #[command(flatten)]
    pub crypto: CryptoConfig,
}

/// List backups options
#[derive(Args, Debug)]
pub struct ListOpts {
    #[command(flatten)]
    pub s3: S3Config,

    /// Filter backups by prefix (overrides s3-prefix)
    #[arg(long = "prefix")]
    pub prefix: Option<String>,
}


#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_crypto_config() {
        let config = CryptoConfig {
            passphrase: "secret".to_string(),
        };
        assert_eq!(config.passphrase, "secret");
    }

    #[test]
    fn test_backup_opts() {
        let opts = BackupOpts {
            pg: PostgresConfig {
                user: "user".to_string(),
                password: "pass".to_string(),
                host: "host".to_string(),
                port: 5432,
                dbname: "db".to_string(),
                databases: None,
            },
            s3: S3Config {
                s3_bucket: "bucket".to_string(),
                s3_region: "region".to_string(),
                s3_prefix: "".to_string(),
                aws_access_key_id: "access_key_id".to_string(),
                aws_secret_access_key: "secret_access_key".to_string(),
                aws_endpoint_url: "endpoint_url".to_string(),
            },
            crypto: CryptoConfig {
                passphrase: "secret".to_string(),
            },
            compression: CompressionConfig {
                enabled: true,
                level: CompressionLevel::Precise(6),
            },
        };

        assert_eq!(opts.pg.user, "user");
        assert_eq!(opts.s3.s3_bucket, "bucket");
        assert_eq!(opts.crypto.passphrase, "secret");
    }

    #[test]
    fn test_restore_opts() {
        let opts = RestoreOpts {
            s3_key: "backup.sql.gpg".to_string(),
            pg: PostgresConfig {
                user: "user".to_string(),
                password: "pass".to_string(),
                host: "host".to_string(),
                port: 5432,
                dbname: "db".to_string(),
                databases: None,
            },
            s3: S3Config {
                s3_bucket: "bucket".to_string(),
                s3_region: "region".to_string(),
                s3_prefix: "".to_string(),
                aws_access_key_id: "access_key_id".to_string(),
                aws_secret_access_key: "secret_access_key".to_string(),
                aws_endpoint_url: "endpoint_url".to_string(),
            },
            crypto: CryptoConfig {
                passphrase: "secret".to_string(),
            },
        };

        assert_eq!(opts.s3_key, "backup.sql.gpg");
        assert_eq!(opts.pg.dbname, "db");
        assert_eq!(opts.s3.s3_region, "region");
    }

    #[test]
    fn test_list_opts() {
        let opts = ListOpts {
            s3: S3Config {
                s3_bucket: "bucket".to_string(),
                s3_region: "region".to_string(),
                s3_prefix: "".to_string(),
                aws_access_key_id: "access_key_id".to_string(),
                aws_secret_access_key: "secret_access_key".to_string(),
                aws_endpoint_url: "endpoint_url".to_string(),
            },
            prefix: None,
        };

        assert_eq!(opts.s3.s3_bucket, "bucket");
        assert_eq!(opts.s3.s3_region, "region");
    }
}
