pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_bytes() {
        assert_eq!(format_bytes(0), "0.00 B");
    }

    #[test]
    fn test_single_byte() {
        assert_eq!(format_bytes(1), "1.00 B");
    }

    #[test]
    fn test_bytes_under_1kb() {
        assert_eq!(format_bytes(512), "512.00 B");
        assert_eq!(format_bytes(1023), "1023.00 B");
    }

    #[test]
    fn test_exactly_1kb() {
        assert_eq!(format_bytes(1024), "1.00 KB");
    }

    #[test]
    fn test_kilobytes() {
        assert_eq!(format_bytes(2048), "2.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048575), "1024.00 KB");
    }

    #[test]
    fn test_exactly_1mb() {
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
    }

    #[test]
    fn test_megabytes() {
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.00 MB");
        assert_eq!(format_bytes(1572864), "1.50 MB"); // 1.5 MB
    }

    #[test]
    fn test_gigabytes() {
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.00 GB");
    }

    #[test]
    fn test_terabytes() {
        assert_eq!(format_bytes(1024u64.pow(4)), "1.00 TB");
        assert_eq!(format_bytes(2 * 1024u64.pow(4)), "2.00 TB");
    }

    #[test]
    fn test_petabytes() {
        assert_eq!(format_bytes(1024u64.pow(5)), "1.00 PB");
        assert_eq!(format_bytes(2 * 1024u64.pow(5)), "2.00 PB");
    }

    #[test]
    fn test_large_values() {
        // Test the maximum value that fits in PB range
        assert_eq!(format_bytes(1024u64.pow(6) - 1), "1024.00 PB");
        // Test exactly 1 PB
        assert_eq!(format_bytes(1024u64.pow(5)), "1.00 PB");
    }

    #[test]
    fn test_decimal_precision() {
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1572864), "1.50 MB");
        assert_eq!(format_bytes(1610612736), "1.50 GB");
    }

    #[test]
    fn test_boundary_values() {
        // Just under 1 KB
        assert_eq!(format_bytes(1023), "1023.00 B");
        // Just under 1 MB
        assert_eq!(format_bytes(1024 * 1024 - 1), "1024.00 KB");
        // Just under 1 GB
        assert_eq!(format_bytes(1024u64.pow(3) - 1), "1024.00 MB");
    }
} 