use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct McConfig {
    #[allow(dead_code)]
    pub version: String,
    pub aliases: HashMap<String, AliasConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AliasConfig {
    pub url: String,
    #[serde(rename = "accessKey")]
    pub access_key: String,
    #[serde(rename = "secretKey")]
    pub secret_key: String,
    #[allow(dead_code)]
    pub api: Option<String>,
    #[allow(dead_code)]
    pub path: Option<String>,
}

impl McConfig {
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::config_path()?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
        let config: McConfig = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse mc config: {}", e))?;
        Ok(config)
    }

    fn config_path() -> anyhow::Result<PathBuf> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;

        // Try ~/.mc/config.json first (standard mc location)
        let mc_path = home.join(".mc").join("config.json");
        if mc_path.exists() {
            return Ok(mc_path);
        }

        // Try ~/.mcli/config.json (alternative location)
        let mcli_path = home.join(".mcli").join("config.json");
        if mcli_path.exists() {
            return Ok(mcli_path);
        }

        anyhow::bail!(
            "MinIO client config not found.\n\
             Searched:\n  {}\n  {}\n\
             Run 'mc alias set <name> <url> <access_key> <secret_key>' to create one.",
            mc_path.display(),
            mcli_path.display()
        )
    }
}
