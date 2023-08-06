use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Cfg {
    pub address: String,
    pub auth_endpoint: Url,
}

// sane defaults
impl Default for Cfg {
    fn default() -> Self {
        Self {
            address: "http://localhost:8082/".parse().unwrap(),
            auth_endpoint: Url::parse("http://localhost:8081/").unwrap(),
        }
    }
}
