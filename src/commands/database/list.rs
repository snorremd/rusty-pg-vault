use anyhow::Result;
use crate::cli::DatabasesOpts;
use comfy_table::{Table, ContentArrangement, Cell, Color};
use indicatif::{ProgressBar, ProgressStyle};
use console::{style, Term};
use tokio::process::Command;
use std::io::{self, IsTerminal};

#[derive(Debug)]
struct DatabaseInfo {
    name: String,
    owner: String,
    encoding: String,
    collate: String,
    ctype: String,
    size: String,
}

pub async fn run(opts: &DatabasesOpts) -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;

    // Create and configure the spinner
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap()
    );
    pb.set_message("Fetching databases from PostgreSQL...");
    pb.enable_steady_tick(std::time::Duration::from_millis(120));

    // Query to list all databases with sizes
    let query = r#"
        SELECT 
            d.datname as name,
            pg_get_userbyid(d.datdba) as owner,
            pg_encoding_to_char(d.encoding) as encoding,
            d.datcollate as collate,
            d.datctype as ctype,
            pg_size_pretty(pg_database_size(d.datname)) as size
        FROM pg_database d
        WHERE d.datistemplate = false
        ORDER BY pg_database_size(d.datname) DESC, d.datname;
    "#;

    let output = Command::new("psql")
        .env("PGPASSWORD", &opts.pg.password)
        .arg("--host")
        .arg(&opts.pg.host)
        .arg("--port")
        .arg(opts.pg.port.to_string())
        .arg("--username")
        .arg(&opts.pg.user)
        .arg("--dbname")
        .arg(&opts.pg.dbname)
        .arg("--no-password")
        .arg("--tuples-only")
        .arg("--no-align")
        .arg("--field-separator")
        .arg("|")
        .arg("-c")
        .arg(query)
        .output()
        .await?;

    // Finish the spinner
    pb.finish_and_clear();

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Failed to query databases: {}", error));
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let databases = parse_database_output(&output_str);

    // Determine if we should use simple formatting
    let use_simple = opts.simple || !io::stdout().is_terminal();

    // Add some spacing
    println!();

    if databases.is_empty() {
        println!("{}", style(format!("Databases on {}:{}:", opts.pg.host, opts.pg.port)).cyan().bold());
        println!();
        
        let mut table = Table::new();
        table
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new("Name").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Owner").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Encoding").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Collate").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Ctype").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Size").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
            ])
            .add_row(vec![
                Cell::new("No databases found").fg(Color::Yellow),
                Cell::new(""),
                Cell::new(""),
                Cell::new(""),
                Cell::new(""),
                Cell::new(""),
            ]);
        println!("{}", table);
    } else {
        println!("{}", style(format!("Databases on {}:{} ({} databases):", opts.pg.host, opts.pg.port, databases.len())).cyan().bold());
        println!();

        if use_simple {
            // Simple output for shell scripts
            for db in &databases {
                println!("{}", db.name);
            }
        } else {
            // Table output
            let mut table = Table::new();
            table
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    Cell::new("Name").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Owner").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Encoding").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Collate").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Ctype").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                    Cell::new("Size").fg(Color::Cyan).add_attribute(comfy_table::Attribute::Bold),
                ]);

            for db in &databases {
                table.add_row(vec![
                    Cell::new(&db.name).fg(Color::White),
                    Cell::new(&db.owner).fg(Color::DarkGrey),
                    Cell::new(&db.encoding).fg(Color::DarkGrey),
                    Cell::new(&db.collate).fg(Color::DarkGrey),
                    Cell::new(&db.ctype).fg(Color::DarkGrey),
                    Cell::new(&db.size).fg(Color::Green),
                ]);
            }

            println!("{}", table);
        }
    }

    // Add some spacing at the end
    println!();

    Ok(())
}

fn parse_database_output(output: &str) -> Vec<DatabaseInfo> {
    let mut databases = Vec::new();
    
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 6 {
            databases.push(DatabaseInfo {
                name: parts[0].trim().to_string(),
                owner: parts[1].trim().to_string(),
                encoding: parts[2].trim().to_string(),
                collate: parts[3].trim().to_string(),
                ctype: parts[4].trim().to_string(),
                size: parts[5].trim().to_string(),
            });
        }
    }

    databases
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{DatabasesOpts, PostgresConfig};

    fn create_test_opts() -> DatabasesOpts {
        DatabasesOpts {
            pg: PostgresConfig {
                host: "localhost".to_string(),
                port: 5432,
                user: "testuser".to_string(),
                password: "testpass".to_string(),
                dbname: "postgres".to_string(),
                databases: None,
            },
            simple: false,
        }
    }



    #[test]
    fn test_parse_database_output_empty() {
        let output = "";
        let databases = parse_database_output(output);
        assert_eq!(databases.len(), 0);
    }

    #[test]
    fn test_parse_database_output_single_database() {
        let output = "testdb|testuser|UTF8|en_US.UTF-8|en_US.UTF-8|15 MB";
        let databases = parse_database_output(output);
        
        assert_eq!(databases.len(), 1);
        let db = &databases[0];
        assert_eq!(db.name, "testdb");
        assert_eq!(db.owner, "testuser");
        assert_eq!(db.encoding, "UTF8");
        assert_eq!(db.collate, "en_US.UTF-8");
        assert_eq!(db.ctype, "en_US.UTF-8");
        assert_eq!(db.size, "15 MB");
    }

    #[test]
    fn test_parse_database_output_multiple_databases() {
        let output = "testdb1|testuser|UTF8|en_US.UTF-8|en_US.UTF-8|15 MB\ntestdb2|testuser|UTF8|en_US.UTF-8|en_US.UTF-8|25 MB";
        let databases = parse_database_output(output);
        
        assert_eq!(databases.len(), 2);
        assert_eq!(databases[0].name, "testdb1");
        assert_eq!(databases[0].size, "15 MB");
        assert_eq!(databases[1].name, "testdb2");
        assert_eq!(databases[1].size, "25 MB");
    }

    #[test]
    fn test_parse_database_output_with_whitespace() {
        let output = "  testdb  |  testuser  |  UTF8  |  en_US.UTF-8  |  en_US.UTF-8  |  15 MB  ";
        let databases = parse_database_output(output);
        
        assert_eq!(databases.len(), 1);
        let db = &databases[0];
        assert_eq!(db.name, "testdb");
        assert_eq!(db.owner, "testuser");
        assert_eq!(db.encoding, "UTF8");
        assert_eq!(db.collate, "en_US.UTF-8");
        assert_eq!(db.ctype, "en_US.UTF-8");
        assert_eq!(db.size, "15 MB");
    }

    #[test]
    fn test_parse_database_output_with_empty_lines() {
        let output = "\n\ntestdb|testuser|UTF8|en_US.UTF-8|en_US.UTF-8|15 MB\n\n";
        let databases = parse_database_output(output);
        
        assert_eq!(databases.len(), 1);
        assert_eq!(databases[0].name, "testdb");
    }

    #[test]
    fn test_parse_database_output_invalid_format() {
        let output = "testdb|testuser|UTF8"; // Missing fields
        let databases = parse_database_output(output);
        assert_eq!(databases.len(), 0);
    }

    #[test]
    fn test_parse_database_output_complex_sizes() {
        let output = "small_db|user1|UTF8|en_US.UTF-8|en_US.UTF-8|1.5 MB\nlarge_db|user2|UTF8|en_US.UTF-8|en_US.UTF-8|2.3 GB";
        let databases = parse_database_output(output);
        
        assert_eq!(databases.len(), 2);
        assert_eq!(databases[0].size, "1.5 MB");
        assert_eq!(databases[1].size, "2.3 GB");
    }

    #[test]
    fn test_database_info_debug() {
        let db = DatabaseInfo {
            name: "testdb".to_string(),
            owner: "testuser".to_string(),
            encoding: "UTF8".to_string(),
            collate: "en_US.UTF-8".to_string(),
            ctype: "en_US.UTF-8".to_string(),
            size: "15 MB".to_string(),
        };
        
        let debug_str = format!("{:?}", db);
        assert!(debug_str.contains("testdb"));
        assert!(debug_str.contains("testuser"));
        assert!(debug_str.contains("15 MB"));
    }

    #[test]
    fn test_databases_opts_simple_flag() {
        let mut opts = create_test_opts();
        assert_eq!(opts.simple, false);
        
        opts.simple = true;
        assert_eq!(opts.simple, true);
    }

    #[test]
    fn test_postgres_config_fields() {
        let pg_config = PostgresConfig {
            host: "localhost".to_string(),
            port: 5432,
            user: "testuser".to_string(),
            password: "testpass".to_string(),
            dbname: "postgres".to_string(),
            databases: None,
        };
        
        assert_eq!(pg_config.host, "localhost");
        assert_eq!(pg_config.port, 5432);
        assert_eq!(pg_config.user, "testuser");
        assert_eq!(pg_config.password, "testpass");
        assert_eq!(pg_config.dbname, "postgres");
        assert_eq!(pg_config.databases, None);
    }

    #[test]
    fn test_postgres_config_with_databases() {
        let pg_config = PostgresConfig {
            host: "localhost".to_string(),
            port: 5432,
            user: "testuser".to_string(),
            password: "testpass".to_string(),
            dbname: "postgres".to_string(),
            databases: Some(vec!["db1".to_string(), "db2".to_string()]),
        };
        
        assert_eq!(pg_config.databases, Some(vec!["db1".to_string(), "db2".to_string()]));
    }

    #[test]
    fn test_parse_database_output_with_special_characters() {
        let output = "test-db_123|user@domain|UTF8|en_US.UTF-8|en_US.UTF-8|1.2 GB";
        let databases = parse_database_output(output);
        
        assert_eq!(databases.len(), 1);
        let db = &databases[0];
        assert_eq!(db.name, "test-db_123");
        assert_eq!(db.owner, "user@domain");
        assert_eq!(db.size, "1.2 GB");
    }

    #[test]
    fn test_parse_database_output_with_extra_fields() {
        let output = "testdb|testuser|UTF8|en_US.UTF-8|en_US.UTF-8|15 MB|extra_field";
        let databases = parse_database_output(output);
        
        assert_eq!(databases.len(), 1);
        let db = &databases[0];
        assert_eq!(db.name, "testdb");
        assert_eq!(db.size, "15 MB");
    }

    #[test]
    fn test_parse_database_output_mixed_formats() {
        let output = "valid_db|user|UTF8|en_US.UTF-8|en_US.UTF-8|10 MB\ninvalid_line\nanother_valid|user2|UTF8|en_US.UTF-8|en_US.UTF-8|5 MB";
        let databases = parse_database_output(output);
        
        assert_eq!(databases.len(), 2);
        assert_eq!(databases[0].name, "valid_db");
        assert_eq!(databases[1].name, "another_valid");
    }
} 