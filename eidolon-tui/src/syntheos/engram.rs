use reqwest::Client;
use reqwest::Url;
use serde_json::{json, Value};
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone)]
pub struct EngramClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl EngramClient {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self, String> {
        validate_base_url(base_url)?;

        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        })
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.api_key)
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<String>, String> {
        let url = format!("{}/search", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", self.auth())
            .json(&json!({"query": query, "limit": limit}))
            .send()
            .await
            .map_err(|e| format!("Engram search failed: {}", e))?;
        let resp = resp
            .error_for_status()
            .map_err(|e| format!("Engram search failed: {}", e))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Engram search parse error: {}", e))?;

        let results = body
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        v.get("content")
                            .and_then(|c| c.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(results)
    }

    pub async fn store(&self, content: &str, source: &str, category: &str) -> Result<(), String> {
        let url = format!("{}/store", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", self.auth())
            .json(&json!({"content": content, "source": source, "category": category}))
            .send()
            .await
            .map_err(|e| format!("Engram store failed: {}", e))?;
        let _ = resp
            .error_for_status()
            .map_err(|e| format!("Engram store failed: {}", e))?;
        Ok(())
    }

    /// Legacy URL-builder shims used by prompt_generator.rs
    pub fn build_search_request(&self, query: &str, limit: u32) -> (String, String) {
        let url = format!("{}/search", self.base_url);
        let body = serde_json::json!({"query": query, "limit": limit}).to_string();
        (url, body)
    }

    pub fn build_store_request(
        &self,
        content: &str,
        source: &str,
        category: &str,
    ) -> (String, String) {
        let url = format!("{}/store", self.base_url);
        let body = serde_json::json!({"content": content, "source": source, "category": category})
            .to_string();
        (url, body)
    }

    pub fn auth_header(&self) -> String {
        self.auth()
    }

    pub async fn context(&self, query: &str) -> Result<String, String> {
        let url = format!("{}/context", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", self.auth())
            .json(&json!({"query": query}))
            .send()
            .await
            .map_err(|e| format!("Engram context failed: {}", e))?;
        let resp = resp
            .error_for_status()
            .map_err(|e| format!("Engram context failed: {}", e))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Engram context parse error: {}", e))?;

        Ok(body
            .get("context")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string())
    }
}

fn validate_base_url(base_url: &str) -> Result<(), String> {
    let url = Url::parse(base_url).map_err(|e| format!("Invalid Engram URL: {}", e))?;

    if !url.username().is_empty() || url.password().is_some() {
        return Err("Engram URL must not include credentials".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("Engram URL must not include query or fragment components".to_string());
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err("Engram URL must not include a path".to_string());
    }

    match (url.scheme(), url.host_str()) {
        ("https", Some(_)) => Ok(()),
        ("http", Some(host)) if host_allows_plain_http(host) => Ok(()),
        ("http", Some(_)) => Err("Engram URL must use https for non-local hosts".to_string()),
        (_, Some(_)) => Err("Engram URL must use http or https".to_string()),
        (_, None) => Err("Engram URL must include a host".to_string()),
    }
}

fn host_allows_plain_http(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return ip.is_loopback() || ip.is_private() || ip.is_link_local() || is_cgnat(ip);
    }
    if let Ok(ip) = host.parse::<Ipv6Addr>() {
        return ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local();
    }

    // Single-label hostnames (no dots) are internal network names
    if !host.contains('.') {
        return true;
    }
    // .local mDNS names
    if host.ends_with(".local") {
        return true;
    }

    false
}

fn is_cgnat(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}
