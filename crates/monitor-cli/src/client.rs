use reqwest::Client;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Monitor HTTP client
// ---------------------------------------------------------------------------

pub struct MonitorClient {
    base_url: String,
    token: Option<String>,
    client: Client,
}

impl MonitorClient {
    pub fn new(base_url: String, token: Option<String>) -> Self {
        Self {
            base_url,
            token,
            client: Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    pub async fn post<T: Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<serde_json::Value, String> {
        let req = self.client.post(self.url(path)).json(body);
        let req = self.apply_auth(req);
        send(req).await
    }

    pub async fn get(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<serde_json::Value, String> {
        let req = self.client.get(self.url(path)).query(query);
        let req = self.apply_auth(req);
        send(req).await
    }

    pub async fn patch<T: Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<serde_json::Value, String> {
        let req = self.client.patch(self.url(path)).json(body);
        let req = self.apply_auth(req);
        send(req).await
    }
}

// ---------------------------------------------------------------------------
// Response handling
// ---------------------------------------------------------------------------

async fn send(req: reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("failed to read response body: {e}"))?;

    if status.is_success() {
        serde_json::from_str(&body).map_err(|e| format!("failed to parse response JSON: {e}"))
    } else {
        // Try to extract a nice error message from the response
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(msg) = val.get("message").and_then(|m| m.as_str()) {
                return Err(format!("server error ({status}): {msg}"));
            }
        }
        Err(format!("server error ({status}): {body}"))
    }
}
