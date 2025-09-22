use preconfirmation_gateway::config::{Config, DatabaseConfig, LoggingConfig, ServerConfig, ValidationConfig};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_config_load_from_existing_file() {
	let config = Config::load_from_file("config.toml").expect("Should load existing config file");

	assert_eq!(config.server.host, "127.0.0.1");
	assert_eq!(config.server.port, 8080);
	assert_eq!(config.database.url, "postgresql://localhost/preconfirmation_gateway");
	assert_eq!(config.logging.level, "info");
	assert_eq!(config.logging.enable_method_tracing, true);
	assert_eq!(config.logging.traced_methods, vec!["commitmentRequest", "commitmentResult", "slots", "fee"]);
}

#[test]
fn test_config_load_nonexistent_file_uses_defaults() {
	let config = Config::load_from_file("nonexistent.toml").expect("Should use defaults when file doesn't exist");

	let expected_default = Config::default();
	assert_eq!(config.server.host, expected_default.server.host);
	assert_eq!(config.server.port, expected_default.server.port);
	assert_eq!(config.database.url, expected_default.database.url);
	assert_eq!(config.logging.level, expected_default.logging.level);
	assert_eq!(config.logging.enable_method_tracing, expected_default.logging.enable_method_tracing);
	assert_eq!(config.logging.traced_methods, expected_default.logging.traced_methods);
}

#[test]
fn test_config_load_invalid_toml() {
	let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
	temp_file.write_all(b"invalid toml content [[[").expect("Failed to write to temp file");

	let result = Config::load_from_file(temp_file.path());
	assert!(result.is_err());
	assert!(result.unwrap_err().to_string().contains("Failed to parse configuration file"));
}

#[test]
fn test_config_load_empty_file() {
	let temp_file = NamedTempFile::new().expect("Failed to create temp file");

	let result = Config::load_from_file(temp_file.path());
	assert!(result.is_err());
}

#[test]
fn test_config_load_partial_server_config() {
	let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
	temp_file.write_all(b"[server]\nhost = \"0.0.0.0\"\n").expect("Failed to write to temp file");

	let result = Config::load_from_file(temp_file.path());
	assert!(result.is_err());
}

#[test]
fn test_config_load_complete_valid_config() {
	let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
	let config_content = r#"
[server]
host = "0.0.0.0"
port = 9090

[database]
url = "postgresql://test/testdb"

[logging]
level = "debug"
enable_method_tracing = false
traced_methods = ["test_method"]

[validation]
slasher_address = "0x0000000000000000000000000000000000000000"
"#;
	temp_file.write_all(config_content.as_bytes()).expect("Failed to write to temp file");

	let config = Config::load_from_file(temp_file.path()).expect("Should load valid config");

	assert_eq!(config.server.host, "0.0.0.0");
	assert_eq!(config.server.port, 9090);
	assert_eq!(config.database.url, "postgresql://test/testdb");
	assert_eq!(config.logging.level, "debug");
	assert_eq!(config.logging.enable_method_tracing, false);
	assert_eq!(config.logging.traced_methods, vec!["test_method"]);
}

#[test]
fn test_config_default_values() {
	let config = Config::default();

	assert_eq!(config.server.host, "127.0.0.1");
	assert_eq!(config.server.port, 8080);
	assert_eq!(config.database.url, "postgresql://localhost/preconfirmation_gateway");
	assert_eq!(config.logging.level, "info");
	assert_eq!(config.logging.enable_method_tracing, true);
	assert_eq!(config.logging.traced_methods, vec!["commitmentRequest", "commitmentResult", "slots", "fee"]);
}

#[test]
fn test_server_config_default() {
	let server_config = ServerConfig::default();
	assert_eq!(server_config.host, "127.0.0.1");
	assert_eq!(server_config.port, 8080);
}

#[test]
fn test_database_config_default() {
	let db_config = DatabaseConfig::default();
	assert_eq!(db_config.url, "postgresql://localhost/preconfirmation_gateway");
}

#[test]
fn test_logging_config_default() {
	let logging_config = LoggingConfig::default();
	assert_eq!(logging_config.level, "info");
	assert_eq!(logging_config.enable_method_tracing, true);
	assert_eq!(logging_config.traced_methods, vec!["commitmentRequest", "commitmentResult", "slots", "fee"]);
}

#[test]
fn test_config_database_url_accessor() {
	let config = Config::default();
	assert_eq!(config.database_url(), "postgresql://localhost/preconfirmation_gateway");

	let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
	let config_content = r#"
[server]
host = "127.0.0.1"
port = 8080

[database]
url = "postgresql://custom/db"

[logging]
level = "info"
enable_method_tracing = true
traced_methods = []

[validation]
slasher_address = "0x0000000000000000000000000000000000000000"
"#;
	temp_file.write_all(config_content.as_bytes()).expect("Failed to write to temp file");

	let config = Config::load_from_file(temp_file.path()).expect("Should load config");
	assert_eq!(config.database_url(), "postgresql://custom/db");
}

#[test]
fn test_config_load_with_missing_sections() {
	let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
	let config_content = r#"
[server]
host = "127.0.0.1"
port = 8080
"#;
	temp_file.write_all(config_content.as_bytes()).expect("Failed to write to temp file");

	let result = Config::load_from_file(temp_file.path());
	assert!(result.is_err());
}

#[test]
fn test_config_load_with_extra_fields() {
	let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
	let config_content = r#"
[server]
host = "127.0.0.1"
port = 8080
extra_field = "ignored"

[database]
url = "postgresql://localhost/preconfirmation_gateway"

[logging]
level = "info"
enable_method_tracing = true
traced_methods = ["fee"]

[validation]
slasher_address = "0x0000000000000000000000000000000000000000"
"#;
	temp_file.write_all(config_content.as_bytes()).expect("Failed to write to temp file");

	let config = Config::load_from_file(temp_file.path()).expect("Should load config ignoring extra fields");
	assert_eq!(config.server.host, "127.0.0.1");
	assert_eq!(config.server.port, 8080);
}

#[test]
fn test_validation_config_default() {
	let validation_config = ValidationConfig::default();
	assert_eq!(validation_config.slasher_address, "0x0000000000000000000000000000000000000000");
}

#[test]
fn test_config_with_custom_slasher_address() {
	let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
	let config_content = r#"
[server]
host = "127.0.0.1"
port = 8080

[database]
url = "postgresql://localhost/preconfirmation_gateway"

[logging]
level = "info"
enable_method_tracing = true
traced_methods = ["commitmentRequest"]

[validation]
slasher_address = "0x1234567890123456789012345678901234567890"
"#;
	temp_file.write_all(config_content.as_bytes()).expect("Failed to write to temp file");

	let config = Config::load_from_file(temp_file.path()).expect("Should load config with custom slasher");
	assert_eq!(config.validation.slasher_address, "0x1234567890123456789012345678901234567890");
}
