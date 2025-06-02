use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tokio::io::{duplex};
use indicatif::{ProgressBar, ProgressStyle, MultiProgress};
use tokio_util::compat::{TokioAsyncWriteCompatExt, FuturesAsyncWriteCompatExt};
use crate::cli::BackupOpts;
use crate::modules::compression::{CompressionTrait, ZstdCompression};
use crate::modules::encryption::{AgeEncryption, EncryptionTrait};
use crate::modules::pg::{PostgresClient, PostgresTrait};
use crate::modules::s3::{S3Client, S3ClientTrait};
use crate::utils::counting_reader::CountingReader;
use crate::utils::counting_writer::CountingWriter;

pub async fn run(opts: &BackupOpts) -> Result<()> {

    

    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S");
    let object_key = format!("{}_{}.sql.zst.age", 
        opts.pg.dbname, 
        timestamp
    );

    // Create a single MultiProgress instance
    let multi_progress = MultiProgress::new();
    let style = ProgressStyle::with_template("{spinner:.green} {msg} {bytes}")
        .unwrap()
        .progress_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");

    // Create progress bars for each stage
    let pg_bar = Arc::new(multi_progress.add(ProgressBar::new(0)));
    let comp_bar = Arc::new(multi_progress.add(ProgressBar::new(0)));
    let enc_bar = Arc::new(multi_progress.add(ProgressBar::new(0)));
    let s3_bar = Arc::new(multi_progress.add(ProgressBar::new(0)));

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
    pg_bar.set_message("Dumping PostgreSQL...");
    comp_bar.set_message("Compressing...");
    enc_bar.set_message("Encrypting...");
    s3_bar.set_message("Uploading to S3...");

    // We need a few duplex pipes to pass data between tasks
    let (zstd_writer, mut zstd_reader) = duplex(64 * 1024);
    let (age_writer, age_reader) = duplex(64 * 1024);

    // Create a postgres client
    let pg_client = PostgresClient::new(
        opts.pg.host.clone(),
        opts.pg.port,
        opts.pg.user.clone(),
        opts.pg.password.clone(),
        opts.pg.dbname.clone(),
    );

    // We wrap the pg_dump output in a counting reader to get progress bar updates
    let pg_reader = CountingReader::new(pg_client.dump().await?, Some(pg_bar));

    let compression_level = opts.compression.level.clone();

    // Spawn compression task, read from pg dump buffered reader and write it to the zstd writer
    // When it is done copying data we can shut down the encoder
    let compression_task: tokio::task::JoinHandle<std::result::Result<(), anyhow::Error>> = tokio::spawn(async move {
        let compression = ZstdCompression::new(compression_level);
        let compression_writer = CountingWriter::new(zstd_writer, Some(comp_bar));
        compression.compress_stream(pg_reader, compression_writer).await?;
        Ok(())
    });

    // Spawn encryption task with symmetric encryption using passphrase from cli
    let passphrase = opts.crypto.passphrase.clone();
    let encryption_task = tokio::spawn(async move {
        let encryption = AgeEncryption::new(passphrase);
        encryption.encrypt_stream(zstd_reader, CountingWriter::new(age_writer, Some(enc_bar))).await;
    });

    let s3_config = Arc::new(opts.s3.clone());
    let object_key = object_key.clone();
    let s3_bar_clone = s3_bar.clone();

    let s3_upload_task = tokio::spawn(async move {
        let s3_client = S3Client::new(
            s3_config.s3_region.clone(),
            s3_config.aws_access_key_id.clone(),
            s3_config.aws_secret_access_key.clone(),
            s3_config.aws_endpoint_url.clone(),
            s3_config.s3_bucket.clone(),
        );

        s3_client.upload_to_s3_streaming(CountingReader::new(age_reader, Some(s3_bar_clone)), &object_key).await?;
        Ok::<(), anyhow::Error>(())
    });

    // Wait for all tasks to complete
    let (comp_result, encrypt_result, s3_result) = tokio::join!(compression_task, encryption_task, s3_upload_task);

    // Check results in order
    comp_result??;
    encrypt_result??;
    s3_result??;

    Ok(())
}