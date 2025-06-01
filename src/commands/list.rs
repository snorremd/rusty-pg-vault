use anyhow::Result;
use crate::utils;
use crate::{cli::ListOpts, modules::s3::S3ClientTrait};
use crate::modules::s3::S3Client;
use crate::utils::formatting::format_bytes;
use aws_sdk_s3::types::Object;
use chrono::DateTime;
use comfy_table::{Table, ContentArrangement, Cell, Color};
use indicatif::{ProgressBar, ProgressStyle};
use console::{style, Term};

pub async fn run(opts: &ListOpts) -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;
    
    let s3_client = S3Client::new(
        opts.s3.s3_region.clone(),
        opts.s3.aws_access_key_id.clone(),
        opts.s3.aws_secret_access_key.clone(),
        opts.s3.aws_endpoint_url.clone(),
        opts.s3.s3_bucket.clone(),
    );

    let prefix = if opts.s3.s3_prefix.is_empty() {
        None
    } else {
        Some(opts.s3.s3_prefix.clone())
    };

    // Create and configure the spinner
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap()
    );
    pb.set_message("Fetching backups from S3...");
    pb.enable_steady_tick(std::time::Duration::from_millis(120));

    let all_objects = s3_client.list_objects(prefix).await?;
    let total_objects = all_objects.len();

    // Finish the spinner
    pb.finish_and_clear();

    // Add some spacing
    println!();

    if all_objects.is_empty() {
        println!("{}", style(format!("Backups in bucket {}:", opts.s3.s3_bucket)).cyan().bold());
        println!();
        
        let mut table = Table::new();
        table
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new("Filename").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Size").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Last Modified").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
            ])
            .add_row(vec![
                Cell::new("No backups found").fg(Color::Yellow),
                Cell::new(""),
                Cell::new(""),
            ]);
        println!("{}", table);
    } else {
        println!("{}", style(format!("Backups in bucket {} ({} objects):", opts.s3.s3_bucket, total_objects)).cyan().bold());
        println!();
        
        let mut table = Table::new();
        table
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new("Filename").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Size").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Last Modified").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
            ]);

        for obj in all_objects {
            add_backup_to_table(&mut table, &obj);
        }

        println!("{}", table);
    }

    // Add some spacing at the end
    println!();

    Ok(())
}

fn add_backup_to_table(table: &mut Table, obj: &Object) {
    let key = obj.key().unwrap_or("unknown");
    let size = obj.size().unwrap_or(0) as u64;
    let last_modified = obj.last_modified()
        .map(|dt| {
            let secs = dt.as_secs_f64();
            let datetime = DateTime::from_timestamp(secs as i64, 0)
                .unwrap_or_else(|| DateTime::UNIX_EPOCH);
            datetime.format("%Y-%m-%d %H:%M:%S").to_string()
        })
        .unwrap_or_else(|| "unknown".to_string());

    table.add_row(vec![
        Cell::new(key).fg(Color::White),
        Cell::new(utils::formatting::format_bytes(size)).fg(Color::DarkGrey),
        Cell::new(last_modified).fg(Color::DarkGrey),
    ]);
} 