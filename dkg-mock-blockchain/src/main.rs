#[tokio::main]
async fn main() {
	let config_path = env!("MOCK_BLOCKCHAIN_CONFIG_PATH");
	let data = tokio::fs::read_to_string(config_path).await?;
	let config: crate::MockBlockchainConfig = toml::from_str(&data)?;
	MockBlockchain::new(config).await?.execute().await
}
