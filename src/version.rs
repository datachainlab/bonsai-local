use anyhow::{anyhow, Context, Result};
use std::process::Command;

pub fn check_docker() -> Result<()> {
    // Check if docker command exists
    let output = Command::new("docker").arg("--version").output().context(
        "Failed to execute 'docker --version'. Make sure Docker is installed and in PATH",
    )?;

    if !output.status.success() {
        return Err(anyhow!(
            "docker command failed with status: {}",
            output.status
        ));
    }

    Ok(())
}

pub fn check_r0vm_version(required_version: &str) -> Result<()> {
    // Check if r0vm command exists and get its version
    let output = Command::new("r0vm")
        .arg("--version")
        .output()
        .context("Failed to execute 'r0vm --version'. Make sure r0vm is installed and in PATH")?;

    if !output.status.success() {
        return Err(anyhow!(
            "r0vm command failed with status: {}",
            output.status
        ));
    }

    let version_output =
        String::from_utf8(output.stdout).context("Failed to parse r0vm version output as UTF-8")?;

    // Extract version from output
    // Assuming format like "r0vm 1.0.0" or "r0vm version 1.0.0"
    let version = extract_version(&version_output)?;

    // Check if version matches the required major.minor
    if !version_matches(&version, required_version)? {
        return Err(anyhow!(
            "r0vm version mismatch: found {}, required {}",
            version,
            required_version
        ));
    }

    Ok(())
}

fn extract_version(output: &str) -> Result<String> {
    // Try to find version pattern in the output
    // Looking for patterns like "1.0.0", "1.0", "v1.0.0", etc.
    let trimmed = output.trim();

    // Split by whitespace and look for version-like strings
    for part in trimmed.split_whitespace() {
        // Remove 'v' prefix if present
        let cleaned = part.trim_start_matches('v');

        // Check if this looks like a version (starts with a digit and contains a dot)
        if cleaned.chars().next().is_some_and(|c| c.is_ascii_digit()) && cleaned.contains('.') {
            return Ok(cleaned.to_string());
        }
    }

    Err(anyhow!(
        "Could not extract version from r0vm output: {}",
        output
    ))
}

fn version_matches(actual: &str, required: &str) -> Result<bool> {
    // Parse versions to compare major.minor parts
    let actual_parts: Vec<&str> = actual.split('.').collect();
    let required_parts: Vec<&str> = required.split('.').collect();

    if required_parts.len() < 2 {
        return Err(anyhow!(
            "Invalid required version format: {}. Expected format: <major>.<minor>",
            required
        ));
    }

    if actual_parts.len() < 2 {
        return Err(anyhow!("Invalid actual version format: {}", actual));
    }

    // Compare major and minor versions
    let actual_major = actual_parts[0];
    let actual_minor = actual_parts[1];
    let required_major = required_parts[0];
    let required_minor = required_parts[1];

    Ok(actual_major == required_major && actual_minor == required_minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_version() {
        assert_eq!(extract_version("r0vm 1.0.0").unwrap(), "1.0.0");
        assert_eq!(extract_version("r0vm version 1.0.0").unwrap(), "1.0.0");
        assert_eq!(extract_version("v1.0.0").unwrap(), "1.0.0");
        assert_eq!(extract_version("r0vm 1.0").unwrap(), "1.0");
        assert_eq!(extract_version("  1.2.3  ").unwrap(), "1.2.3");
    }

    #[test]
    fn test_version_matches() {
        assert!(version_matches("1.0.0", "1.0").unwrap());
        assert!(version_matches("1.0.5", "1.0").unwrap());
        assert!(!version_matches("1.1.0", "1.0").unwrap());
        assert!(!version_matches("2.0.0", "1.0").unwrap());
        assert!(version_matches("1.2.3", "1.2").unwrap());
    }
}
