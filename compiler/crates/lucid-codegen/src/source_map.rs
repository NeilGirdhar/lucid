//! Source map integration for debugging support

use crate::SourceMap;
use std::path::Path;

/// Complete source map with export capabilities
#[derive(Debug)]
pub struct SourceMapIntegration {
    pub source_map: SourceMap,
    pub current_line: usize,
}

impl SourceMapIntegration {
    pub fn new() -> Self {
        Self {
            source_map: SourceMap::new(),
            current_line: 0,
        }
    }

    /// Record a line of generated code with source location
    pub fn record_line(&mut self, source_file: String, source_line: usize, source_column: usize) {
        self.source_map
            .add_mapping(self.current_line, source_file, source_line, source_column);
        self.current_line += 1;
    }

    /// Increment line counter (for generated code without source mapping)
    pub fn next_line(&mut self) {
        self.current_line += 1;
    }

    /// Export source map to JSON and write to file
    pub fn export_to_file(&self, path: &Path) -> std::io::Result<()> {
        self.source_map.write_to_file(path)
    }

    /// Get JSON representation
    pub fn to_json(&self) -> String {
        self.source_map.to_json()
    }

    /// Get statistics about source map coverage
    pub fn coverage_stats(&self) -> (usize, usize, f64) {
        let total_lines = self.current_line;
        let mapped_lines = self.source_map.entries().len();
        let coverage = if total_lines > 0 {
            (mapped_lines as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        };
        (total_lines, mapped_lines, coverage)
    }
}

impl Default for SourceMapIntegration {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_map_integration() {
        let mut integration = SourceMapIntegration::new();

        integration.record_line("test.lucid".to_string(), 1, 0);
        integration.record_line("test.lucid".to_string(), 2, 0);
        integration.next_line();
        integration.record_line("test.lucid".to_string(), 3, 0);

        let (total, mapped, coverage) = integration.coverage_stats();
        assert_eq!(total, 4);
        assert_eq!(mapped, 3);
        assert!(coverage > 70.0 && coverage < 80.0);
    }

    #[test]
    fn test_source_map_json_export() {
        let mut integration = SourceMapIntegration::new();
        integration.record_line("file.lucid".to_string(), 10, 5);

        let json = integration.to_json();
        assert!(json.contains("\"version\": 3"));
        assert!(json.contains("file.lucid"));
        assert!(json.contains("\"original_line\": 10"));
    }
}
