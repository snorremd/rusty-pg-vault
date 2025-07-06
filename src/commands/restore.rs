use std::sync::Arc;
use std::io::{self, IsTerminal};

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::io::duplex;
use crate::cli::RestoreOpts;
use crate::modules::compression::{CompressionTrait, ZstdCompression};
use crate::modules::encryption::{AgeEncryption, EncryptionTrait};
use crate::modules::pg::{PostgresClient, PostgresTrait};
use crate::modules::s3::{S3Client, S3ClientTrait};
use crate::utils::counting_reader::CountingReader;
use crate::utils::counting_writer::CountingWriter;


pub async fn run(opts: &RestoreOpts) -> Result<()> {
    // Determine if we should use simple output
    let use_simple = opts.simple || !io::stdout().is_terminal();

    if use_simple {
        println!("Starting restore to database: {}", opts.pg.dbname);
        println!("Backup file: {}", opts.s3_key);
    }

    // Create progress bars only for interactive mode
    let (_multi_progress, pg_bar, comp_bar, enc_bar, s3_bar) = if use_simple {
        (None, Arc::new(ProgressBar::hidden()), Arc::new(ProgressBar::hidden()), Arc::new(ProgressBar::hidden()), Arc::new(ProgressBar::hidden()))
    } else {
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
        s3_bar.set_message("Downloading from S3...");
        enc_bar.set_message("Decrypting...");
        comp_bar.set_message("Decompressing...");
        pg_bar.set_message("Restoring PostgreSQL...");

        (Some(multi_progress), pg_bar, comp_bar, enc_bar, s3_bar)
    };

    // We need duplex channels for the pipeline with larger buffers
    let (age_reader, age_writer) = duplex(1024 * 1024); // 1MB buffer
    let (zstd_reader, zstd_writer) = duplex(1024 * 1024); // 1MB buffer

    // Start S3 download task
    let s3_opts_clone = opts.s3.clone();
    let s3_key_clone = opts.s3_key.clone();
    let s3_reader = {
        let s3_client = S3Client::new(s3_opts_clone);
        let reader = s3_client.download_from_s3_streaming(&s3_key_clone).await?;
        Box::new(CountingReader::new(reader, Some(s3_bar)))
    };

    // Start decryption task
    let encryption_opts_clone = opts.crypto.clone();
    let decryption_task = tokio::spawn(async move {
        let encryption = AgeEncryption::new(encryption_opts_clone);
        encryption.decrypt_stream(s3_reader, CountingWriter::new(age_writer, Some(enc_bar))).await
    });

    // Start decompression task
    let compression_opts_clone = opts.compression.clone();
    let decompression_task = tokio::spawn(async move {
        let compression = ZstdCompression::new(compression_opts_clone);
        compression.decompress_stream(age_reader, CountingWriter::new(zstd_writer, Some(comp_bar))).await
    });

    // Start PostgreSQL restore task
    let pg_client = PostgresClient::new(opts.pg.clone());
    let pg_task = tokio::spawn(async move {
        pg_client.restore(Box::new(CountingReader::new(zstd_reader, Some(pg_bar)))).await
    });

    // Wait for all tasks to complete
    let (dec_result, decomp_result, pg_result) = tokio::join!(
        decryption_task,
        decompression_task,
        pg_task
    );

    // Check results
    dec_result??;
    decomp_result??;
    pg_result??;

    if use_simple {
        println!("Restore completed successfully to database: {}", opts.pg.dbname);
    }

    Ok(())
} 