use std::sync::Arc;

use crate::cli::BackupOpts;
use crate::modules::compression::{CompressionTrait, ZstdCompression};
use crate::modules::encryption::{AgeEncryption, EncryptionTrait};
use crate::modules::pg::{PostgresClient, PostgresTrait};
use crate::modules::s3::{S3Client, S3ClientTrait};
use crate::utils::counting_reader::CountingReader;
use crate::utils::counting_writer::CountingWriter;
use anyhow::Result;
use chrono::Utc;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::io::duplex;

pub async fn run(opts: &BackupOpts) -> Result<()> {
    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S");
    let object_key = format!("{}_{}.sql.zst.age", opts.pg.dbname, timestamp);

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
    let (zstd_writer, zstd_reader) = duplex(64 * 1024);
    let (age_writer, age_reader) = duplex(64 * 1024);

    // Create a postgres client
    let pg_client = PostgresClient::new(opts.pg.clone());

    // We wrap the pg_dump output in a counting reader to get progress bar updates
    let pg_reader = CountingReader::new(pg_client.dump().await?, Some(pg_bar.clone()));

    // Spawn compression task, read from pg dump buffered reader and write it to the zstd writer
    // When it is done copying data we can shut down the encoder
    let compression_opts_clone = opts.compression.clone();
    let comp_bar_clone = comp_bar.clone();
    let compression_task = tokio::spawn(async move {
        let compression = ZstdCompression::new(compression_opts_clone);
        let compression_writer = CountingWriter::new(zstd_writer, Some(comp_bar_clone));
        compression
            .compress_stream(pg_reader, compression_writer)
            .await
    });

    // Spawn encryption task with symmetric encryption using passphrase from cli
    let encryption_opts_clone = opts.crypto.clone();
    let enc_bar_clone = enc_bar.clone();
    let encryption_task = tokio::spawn(async move {
        let encryption = AgeEncryption::new(encryption_opts_clone);
        encryption
            .encrypt_stream(zstd_reader, CountingWriter::new(age_writer, Some(enc_bar_clone)))
            .await
    });

    let object_key = object_key.clone();
    let s3_bar_clone = s3_bar.clone();
    let s3_opts_clone = opts.s3.clone();

    let s3_upload_task = tokio::spawn(async move {
        let s3_client = S3Client::new(s3_opts_clone);

        s3_client
            .upload_to_s3_streaming(
                CountingReader::new(age_reader, Some(s3_bar_clone)),
                &object_key,
            )
            .await?;
        Ok::<(), anyhow::Error>(())
    });

    // Wait for all tasks to complete
    let (compression_result, encryption_result, s3_result) =
        tokio::join!(compression_task, encryption_task, s3_upload_task);

    // Check results in order
    compression_result??;
    encryption_result??;
    s3_result??;

    // Keep progress bars visible after completion with final state
    pg_bar.abandon();
    comp_bar.abandon();
    enc_bar.abandon();
    s3_bar.abandon();

    Ok(())
}
