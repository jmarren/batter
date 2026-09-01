use std::collections::HashSet;

use anyhow::{Result, anyhow};
use tokio::process::Command;

// discovers subdomains of `domain` by shelling out to `subfinder -d <domain>
// -silent`, which queries a number of passive sources and prints one
// subdomain per line with no other output. requires subfinder to already be
// installed and on PATH.
pub async fn enumerate_subdomains(domain: &str) -> Result<HashSet<String>> {
    let output = Command::new("subfinder")
        .arg("-d")
        .arg(domain)
        .arg("-silent")
        .arg("-all")
        .output()
        .await
        .map_err(|err| anyhow!("failed to run subfinder (is it installed and on PATH?): {err}"))?;

    if !output.status.success() {
        return Err(anyhow!(
            "subfinder exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| anyhow!("subfinder produced non-utf8 output: {err}"))?;

    Ok(parse_subfinder_output(&stdout))
}

fn parse_subfinder_output(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .map(|line| line.trim().to_lowercase())
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_subdomain_per_line() {
        let output = "api.example.com\nwww.example.com\n";

        assert_eq!(
            parse_subfinder_output(output),
            HashSet::from(["api.example.com".to_string(), "www.example.com".to_string()])
        );
    }

    #[test]
    fn ignores_blank_lines() {
        let output = "api.example.com\n\n\nwww.example.com\n";

        assert_eq!(
            parse_subfinder_output(output),
            HashSet::from(["api.example.com".to_string(), "www.example.com".to_string()])
        );
    }

    #[test]
    fn dedupes_and_lowercases() {
        let output = "API.example.com\napi.example.com\n";

        assert_eq!(
            parse_subfinder_output(output),
            HashSet::from(["api.example.com".to_string()])
        );
    }

    #[test]
    fn empty_output_yields_empty_set() {
        assert!(parse_subfinder_output("").is_empty());
    }
}
