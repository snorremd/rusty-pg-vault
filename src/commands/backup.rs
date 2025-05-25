use anyhow::Result;
use chrono::Utc;
use tokio::io::{BufReader, AsyncWriteExt, AsyncReadExt, AsyncWrite, duplex};
use tokio_util::compat::{TokioAsyncWriteCompatExt, Compat};
use indicatif::{ProgressBar, ProgressStyle, MultiProgress};
use console::style;
use console::Term;
use tracing::{info, error};
use std::io::Write;
use std::sync::Arc;
use flate2::write::GzEncoder;
use flate2::Compression;
use crate::cli::BackupOpts;
use crate::modules::encryption::{EncryptionTrait, AgeEncryption};
use crate::modules::pg::{PostgresClient, PostgresTrait};
use crate::modules::s3::{S3Client, S3ClientTrait};
use crate::modules::compression::{CompressionTrait, GzipCompression};
use crate::utils::{self, counting_reader::CountingReader};

pub async fn run(opts: &BackupOpts) -> Result<()> {
    let pg_client = PostgresClient::new(
        opts.pg.host.clone(),
        opts.pg.port,
        opts.pg.user.clone(),
        opts.pg.password.clone(),
        opts.pg.dbname.clone(),
    );
    
    let encryption = AgeEncryption::new(opts.crypto.passphrase.clone());
    
    let s3_client = S3Client::new(
        opts.s3.s3_region.clone(),
        opts.s3.aws_access_key_id.clone(),
        opts.s3.aws_secret_access_key.clone(),
        opts.s3.aws_endpoint_url.clone(),
        opts.s3.s3_bucket.clone(),
    );

    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S");
    let object_key = format!("{}_{}.sql.gz.age", 
        opts.pg.dbname, 
        timestamp
    );

    // Create a single MultiProgress instance
    let multi_progress = MultiProgress::new();
    let style = ProgressStyle::with_template("{spinner:.green} {msg} {bytes}")
        .unwrap()
        .progress_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");

    // Add a header
    let header = multi_progress.add(ProgressBar::new_spinner());
    header.set_style(style.clone());
    header.set_message("Starting backup process...");
    header.println("");

    // Create progress bars for each stage
    let pg_bar = multi_progress.add(ProgressBar::new(0));
    let comp_bar = multi_progress.add(ProgressBar::new(0));
    let enc_bar = multi_progress.add(ProgressBar::new(0));
    let s3_bar = multi_progress.add(ProgressBar::new(0));

    // Set styles and initial messages
    for bar in [&pg_bar, &comp_bar, &enc_bar, &s3_bar] {
        bar.set_style(style.clone());
    }

    // Enable rate limiting for all progress bars
    let refresh_rate = std::time::Duration::from_millis(100);
    pg_bar.enable_steady_tick(refresh_rate);
    comp_bar.enable_steady_tick(refresh_rate);
    enc_bar.enable_steady_tick(refresh_rate);
    s3_bar.enable_steady_tick(refresh_rate);

    // Set initial messages
    pg_bar.set_message("Waiting to start pg_dump...");
    comp_bar.set_message("Waiting to start compression...");
    enc_bar.set_message("Waiting to start encryption...");
    s3_bar.set_message("Waiting to start S3 upload...");

    // Create pipes for data flow
    let (pg_writer, pg_reader) = duplex(1024 * 1024); // 1MB buffer
    let (comp_writer, comp_reader) = duplex(1024 * 1024); // 1MB buffer
    let (enc_writer, enc_reader) = duplex(1024 * 1024); // 1MB buffer

    // Spawn the task for pg_dump
    let pg_task = {
        let pg_reader = pg_client.dump().await?;
        let mut pg_writer = pg_writer;
        let pg_bar = pg_bar.clone();
        
        tokio::spawn(async move {
            pg_bar.set_message("Running pg_dump...");
            let reader = CountingReader::new(pg_reader, Some(Arc::new(pg_bar.clone())));
            let mut reader = BufReader::new(reader);
            
            let result = tokio::io::copy_buf(&mut reader, &mut pg_writer).await
                .map_err(|e| anyhow::anyhow!("Failed to copy data: {}", e))
                .map(|_| ());
            
            if result.is_ok() {
                let bytes = pg_bar.position();
                pg_bar.finish_with_message(format!("✓ pg_dump completed ({} dumped)", utils::format_bytes(bytes)));
            } else {
                let error_msg = result.as_ref().err().map_or("pg_dump failed".to_string(), |e| e.to_string());
                pg_bar.finish_with_message(format!("✗ pg_dump failed: {}", error_msg));
            }
            result
        })
    };

    // Spawn the compression task
    let compression_task = {
        let reader = CountingReader::new(pg_reader, Some(Arc::new(comp_bar.clone())));
        let reader = BufReader::new(reader);
        let compression_level = opts.compression.level;
        let comp_bar = comp_bar.clone();
        let compression = GzipCompression::new(compression_level);
        
        tokio::spawn(async move {
            comp_bar.set_message("Compressing data...");
            
            let result = compression.compress(reader, comp_writer).await;
            
            if result.is_ok() {
                let bytes = comp_bar.position();
                comp_bar.finish_with_message(format!("✓ Compression completed ({} processed)", utils::format_bytes(bytes)));
            } else {
                let error_msg = result.as_ref().err().map_or("Compression failed".to_string(), |e| e.to_string());
                comp_bar.finish_with_message(format!("✗ Compression failed: {}", error_msg));
            }
            result
        })
    };

    // Spawn the encryption task
    let encryption_task = {
        let reader = CountingReader::new(comp_reader, Some(Arc::new(enc_bar.clone())));
        let reader = BufReader::new(reader);
        let enc_writer = enc_writer;
        let enc_bar = enc_bar.clone();
        
        tokio::spawn(async move {
            enc_bar.set_message("Encrypting data...");
            
            let result = encryption.encrypt(reader, enc_writer).await;
            
            if result.is_ok() {
                let bytes = enc_bar.position();
                enc_bar.finish_with_message(format!("✓ Encryption completed ({} processed)", utils::format_bytes(bytes)));
            } else {
                let error_msg = result.as_ref().err().map_or("Encryption failed".to_string(), |e| e.to_string());
                enc_bar.finish_with_message(format!("✗ Encryption failed: {}", error_msg));
            }
            result
        })
    };

    // Spawn S3 upload task
    let s3_key = format!("{}/{}", opts.s3.s3_prefix, object_key);
    let s3_upload = {
        let reader = CountingReader::new(enc_reader, Some(Arc::new(s3_bar.clone())));
        let reader = BufReader::new(reader);
        let s3_bar = s3_bar.clone();
        
        tokio::spawn(async move {
            s3_bar.set_message("Uploading to S3...");
            
            let result = s3_client.upload_stream(&s3_key, reader).await;
            
            if result.is_ok() {
                let bytes = s3_bar.position();
                s3_bar.finish_with_message(format!("✓ S3 upload completed ({} uploaded)", utils::format_bytes(bytes)));
            } else {
                let error_msg = result.as_ref().err().map_or("S3 upload failed".to_string(), |e| e.to_string());
                s3_bar.finish_with_message(format!("✗ S3 upload failed: {}", error_msg));
            }
            result
        })
    };

    // Wait for all tasks to complete
    let (pg_result, comp_result, encrypt_result, s3_result): (
        Result<Result<(), anyhow::Error>, tokio::task::JoinError>,
        Result<Result<(), anyhow::Error>, tokio::task::JoinError>,
        Result<Result<(), anyhow::Error>, tokio::task::JoinError>,
        Result<Result<(), anyhow::Error>, tokio::task::JoinError>
    ) = tokio::join!(pg_task, compression_task, encryption_task, s3_upload);

    // Check results in order
    pg_result??;
    comp_result??;
    encrypt_result??;
    s3_result??;

    println!("\n{}", console::style("Backup Summary:").green().bold());
    println!("Database: {}", console::style(&opts.pg.dbname).yellow());
    println!("Backup file: {}", console::style(&object_key).yellow());
    println!("Timestamp: {}", console::style(&timestamp.to_string()).yellow());
    println!("Status: {}", console::style("Success").green());

    Ok(())
} 

#[async_trait::async_trait]
pub trait ProgressReporter: Send + Sync {
    fn start_backup(&self, dbname: &str, bucket: &str, key: &str);
    fn initialize(&self);
    fn start_pg_dump(&self);
    fn pg_dump_complete(&self, success: bool, error: Option<&anyhow::Error>);
    fn start_compression(&self);
    fn compression_complete(&self, success: bool, error: Option<&anyhow::Error>);
    fn start_encryption(&self);
    fn encryption_complete(&self, success: bool, error: Option<&anyhow::Error>);
    fn start_s3_upload(&self);
    fn s3_upload_complete(&self, success: bool, error: Option<&anyhow::Error>);
    fn backup_complete(&self, dbname: &str, key: &str, timestamp: &str);
    fn get_pg_dump_bar(&self) -> Option<ProgressBar>;
    fn get_compression_bar(&self) -> Option<ProgressBar>;
    fn get_encryption_bar(&self) -> Option<ProgressBar>;
    fn get_s3_bar(&self) -> Option<ProgressBar>;
}

pub struct InteractiveProgress {
    pg_bar: ProgressBar,
    compression_bar: ProgressBar,
    encrypt_bar: ProgressBar,
    s3_bar: ProgressBar,
}

impl InteractiveProgress {
    fn new() -> Self {
        let multi_progress = MultiProgress::new();
        let pg_bar = multi_progress.add(ProgressBar::new(0));
        let compression_bar = multi_progress.add(ProgressBar::new(0));
        let encrypt_bar = multi_progress.add(ProgressBar::new(0));
        let s3_bar = multi_progress.add(ProgressBar::new(0));

        let style = ProgressStyle::with_template("{spinner:.green} {msg} {bytes}")
            .unwrap()
            .progress_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");
        
        for bar in [&pg_bar, &compression_bar, &encrypt_bar, &s3_bar] {
            bar.set_style(style.clone());
        }

        // Enable rate limiting for all progress bars
        let refresh_rate = std::time::Duration::from_millis(100);
        pg_bar.enable_steady_tick(refresh_rate);
        compression_bar.enable_steady_tick(refresh_rate);
        encrypt_bar.enable_steady_tick(refresh_rate);
        s3_bar.enable_steady_tick(refresh_rate);

        Self {
            pg_bar,
            compression_bar,
            encrypt_bar,
            s3_bar,
        }
    }
}

#[async_trait::async_trait]
impl ProgressReporter for InteractiveProgress {
    fn start_backup(&self, dbname: &str, bucket: &str, key: &str) {
        println!("{}", style("Starting backup process...").cyan().bold());
        println!("Database: {}", style(dbname).yellow());
        println!("Destination: {}", style(format!("s3://{}/{}", bucket, key)).yellow());
    }

    fn initialize(&self) {
        // No need to set messages here as they're set in the main backup function
    }

    fn start_pg_dump(&self) {
        self.pg_bar.set_message("Running pg_dump...");
    }

    fn pg_dump_complete(&self, success: bool, error: Option<&anyhow::Error>) {
        if success {
            let bytes = self.pg_bar.position();
            self.pg_bar.finish_with_message(format!("✓ pg_dump completed ({} dumped)", utils::format_bytes(bytes)));
        } else {
            let error_msg = error.map_or("pg_dump failed".to_string(), |e| e.to_string());
            self.pg_bar.finish_with_message(format!("✗ pg_dump failed: {}", error_msg));
        }
    }

    fn start_compression(&self) {
        self.compression_bar.set_message("Compressing data...");
    }

    fn compression_complete(&self, success: bool, error: Option<&anyhow::Error>) {
        if success {
            let bytes = self.compression_bar.position();
            self.compression_bar.finish_with_message(format!("✓ Compression completed ({} processed)", utils::format_bytes(bytes)));
        } else {
            let error_msg = error.map_or("Compression failed".to_string(), |e| e.to_string());
            self.compression_bar.finish_with_message(format!("✗ Compression failed: {}", error_msg));
        }
    }

    fn start_encryption(&self) {
        self.encrypt_bar.set_message("Encrypting data...");
    }

    fn encryption_complete(&self, success: bool, error: Option<&anyhow::Error>) {
        if success {
            let bytes = self.encrypt_bar.position();
            self.encrypt_bar.finish_with_message(format!("✓ Encryption completed ({} processed)", utils::format_bytes(bytes)));
        } else {
            let error_msg = error.map_or("Encryption failed".to_string(), |e| e.to_string());
            self.encrypt_bar.finish_with_message(format!("✗ Encryption failed: {}", error_msg));
        }
    }

    fn start_s3_upload(&self) {
        self.s3_bar.set_message("Uploading to S3...");
    }

    fn s3_upload_complete(&self, success: bool, error: Option<&anyhow::Error>) {
        if success {
            let bytes = self.s3_bar.position();
            self.s3_bar.finish_with_message(format!("✓ S3 upload completed ({} uploaded)", utils::format_bytes(bytes)));
        } else {
            let error_msg = error.map_or("S3 upload failed".to_string(), |e| e.to_string());
            self.s3_bar.finish_with_message(format!("✗ S3 upload failed: {}", error_msg));
        }
    }

    fn backup_complete(&self, dbname: &str, key: &str, timestamp: &str) {
        println!("\n{}", style("Backup Summary:").green().bold());
        println!("Database: {}", style(dbname).yellow());
        println!("Backup file: {}", style(key).yellow());
        println!("Timestamp: {}", style(timestamp).yellow());
        println!("Status: {}", style("Success").green());
    }

    fn get_pg_dump_bar(&self) -> Option<ProgressBar> {
        Some(self.pg_bar.clone())
    }

    fn get_compression_bar(&self) -> Option<ProgressBar> {
        Some(self.compression_bar.clone())
    }

    fn get_encryption_bar(&self) -> Option<ProgressBar> {
        Some(self.encrypt_bar.clone())
    }

    fn get_s3_bar(&self) -> Option<ProgressBar> {
        Some(self.s3_bar.clone())
    }
}

pub struct NonInteractiveProgress;

#[async_trait::async_trait]
impl ProgressReporter for NonInteractiveProgress {
    fn start_backup(&self, dbname: &str, bucket: &str, key: &str) {
        info!(
            database = %dbname,
            bucket = %bucket,
            key = %key,
            "Starting backup process"
        );
    }

    fn initialize(&self) {
        info!("Initializing backup process");
    }

    fn start_pg_dump(&self) {
        info!("Starting pg_dump");
    }

    fn pg_dump_complete(&self, success: bool, error: Option<&anyhow::Error>) {
        if success {
            info!("pg_dump completed successfully");
        } else if let Some(e) = error {
            error!(error = %e, "pg_dump failed");
        }
    }

    fn start_compression(&self) {
        info!("Starting compression");
    }

    fn compression_complete(&self, success: bool, error: Option<&anyhow::Error>) {
        if success {
            info!("Compression completed successfully");
        } else if let Some(e) = error {
            error!(error = %e, "Compression failed");
        }
    }

    fn start_encryption(&self) {
        info!("Starting encryption");
    }

    fn encryption_complete(&self, success: bool, error: Option<&anyhow::Error>) {
        if success {
            info!("Encryption completed successfully");
        } else if let Some(e) = error {
            error!(error = %e, "Encryption failed");
        }
    }

    fn start_s3_upload(&self) {
        info!("Starting S3 upload");
    }

    fn s3_upload_complete(&self, success: bool, error: Option<&anyhow::Error>) {
        if success {
            info!("S3 upload completed successfully");
        } else if let Some(e) = error {
            error!(error = %e, "S3 upload failed");
        }
    }

    fn backup_complete(&self, dbname: &str, key: &str, timestamp: &str) {
        info!(
            database = %dbname,
            backup_file = %key,
            timestamp = %timestamp,
            status = "success",
            "Backup completed successfully"
        );
    }

    fn get_pg_dump_bar(&self) -> Option<ProgressBar> {
        None
    }

    fn get_compression_bar(&self) -> Option<ProgressBar> {
        None
    }

    fn get_encryption_bar(&self) -> Option<ProgressBar> {
        None
    }

    fn get_s3_bar(&self) -> Option<ProgressBar> {
        None
    }
}
