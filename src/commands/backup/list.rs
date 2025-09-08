use anyhow::Result;
use crate::utils;
use crate::{cli::ListOpts, modules::s3::S3ClientTrait};
use crate::modules::s3::S3Client;
use aws_sdk_s3::types::Object;
use chrono::DateTime;
use comfy_table::{Table, ContentArrangement, Cell, Color};
use indicatif::{ProgressBar, ProgressStyle};
use console::{style, Term};
use std::io::{self, IsTerminal};

pub async fn run(opts: &ListOpts) -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;
    
    let s3_client = S3Client::new(opts.s3.clone());

    // Use the s3-prefix from config for filtering
    let prefix = if !opts.s3.s3_prefix.is_empty() {
        Some(opts.s3.s3_prefix.clone())
    } else {
        // No prefix filtering
        None
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

    // Determine if we should use simple formatting
    let use_simple = opts.simple || !io::stdout().is_terminal();

    // Add some spacing
    println!();

    if all_objects.is_empty() {
        if !use_simple {
            let filter_info = if !opts.s3.s3_prefix.is_empty() {
                format!(" (filtered by prefix: {})", opts.s3.s3_prefix)
            } else {
                String::new()
            };
            println!("{}", style(format!("Backups in bucket {}{}:", opts.s3.s3_bucket, filter_info)).cyan().bold());
            println!();
            
            let mut table = Table::new();
            table
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    Cell::new("Database").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Size").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Last Modified").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Full Path").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                ])
                .add_row(vec![
                    Cell::new("No backups found").fg(Color::Yellow),
                    Cell::new(""),
                    Cell::new(""),
                    Cell::new(""),
                ]);
            println!("{}", table);
        }
    } else {
        if use_simple {
            // Simple output for shell scripts
            let filter_info = if !opts.s3.s3_prefix.is_empty() {
                format!(" (filtered by prefix: {})", opts.s3.s3_prefix)
            } else {
                String::new()
            };
            println!("{}", style(format!("Backups in bucket {}{} ({} objects):", opts.s3.s3_bucket, filter_info, total_objects)).cyan().bold());
            println!();
            for obj in &all_objects {
                println!("{}", obj.key().unwrap_or("unknown"));
            }
        } else {
            // Table output
            let filter_info = if !opts.s3.s3_prefix.is_empty() {
                format!(" (filtered by prefix: {})", opts.s3.s3_prefix)
            } else {
                String::new()
            };
            println!("{}", style(format!("Backups in bucket {}{} ({} objects):", opts.s3.s3_bucket, filter_info, total_objects)).cyan().bold());
            println!();
            
            let mut table = Table::new();
            table
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    Cell::new("Database").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Size").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Last Modified").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Full Path").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                ]);

            for obj in &all_objects {
                add_backup_to_table(&mut table, &obj);
            }

            println!("{}", table);
        }
    }

    // Add some spacing at the end
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{ListOpts, S3Config};

    #[test]
    fn test_extract_database_from_key() {
        // Test with S3 prefix
        assert_eq!(
            extract_database_from_key("backups/mydb/2024-01-15_10-30-00.sql.zst.age"),
            "mydb"
        );
        
        // Test without S3 prefix
        assert_eq!(
            extract_database_from_key("mydb/2024-01-15_10-30-00.sql.zst.age"),
            "mydb"
        );
        
        // Test with deeper prefix
        assert_eq!(
            extract_database_from_key("prod/backups/mydb/2024-01-15_10-30-00.sql.zst.age"),
            "mydb"
        );
        
        // Test edge cases
        assert_eq!(extract_database_from_key(""), "unknown");
        assert_eq!(extract_database_from_key("invalid"), "unknown");
        assert_eq!(extract_database_from_key("just/one/part"), "one");
    }

    #[test]
    fn test_s3_prefix_usage() {
        // Test with no S3 prefix
        let opts = ListOpts {
            s3: S3Config {
                s3_bucket: "bucket".to_string(),
                s3_region: "region".to_string(),
                s3_prefix: "".to_string(),
                aws_access_key_id: "key".to_string(),
                aws_secret_access_key: "secret".to_string(),
                aws_endpoint_url: "url".to_string(),
            },
            simple: false,
        };
        
        // This would be the prefix logic from the run function
        let prefix = if !opts.s3.s3_prefix.is_empty() {
            Some(opts.s3.s3_prefix.clone())
        } else {
            None
        };
        
        assert_eq!(prefix, None);
        
        // Test with S3 prefix
        let opts_with_s3_prefix = ListOpts {
            s3: S3Config {
                s3_bucket: "bucket".to_string(),
                s3_region: "region".to_string(),
                s3_prefix: "demo_1gb/".to_string(),
                aws_access_key_id: "key".to_string(),
                aws_secret_access_key: "secret".to_string(),
                aws_endpoint_url: "url".to_string(),
            },
            simple: false,
        };
        
        let prefix_with_s3 = if !opts_with_s3_prefix.s3.s3_prefix.is_empty() {
            Some(opts_with_s3_prefix.s3.s3_prefix.clone())
        } else {
            None
        };
        
        assert_eq!(prefix_with_s3, Some("demo_1gb/".to_string()));
    }
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

    // Extract database name from key path
    let database_name = extract_database_from_key(key);

    table.add_row(vec![
        Cell::new(database_name).fg(Color::Green),
        Cell::new(utils::formatting::format_bytes(size)).fg(Color::DarkGrey),
        Cell::new(last_modified).fg(Color::DarkGrey),
        Cell::new(key).fg(Color::Yellow),
    ]);
}

fn extract_database_from_key(key: &str) -> String {
    // Key format: {s3-prefix}/{database}/{timestamp}.sql.zst.age
    // We want to extract the database name from the path
    let parts: Vec<&str> = key.split('/').collect();
    
    if parts.len() >= 2 {
        // If we have a prefix, database is the second-to-last part
        // If no prefix, database is the first part
        if parts.len() >= 3 {
            parts[parts.len() - 2].to_string()
        } else {
            parts[0].to_string()
        }
    } else {
        "unknown".to_string()
    }
} 