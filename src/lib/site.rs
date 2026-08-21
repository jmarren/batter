use anyhow::Result;

pub async fn fetch_url(url: &str) -> Result<Vec<u8>> {
    let response = reqwest::get(format!("https://{}", url))
        .await?
        .error_for_status()?;
    let bytes = response.bytes().await?;

    Ok(bytes.to_vec())
}
