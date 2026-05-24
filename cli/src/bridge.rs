use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub iface: String,
    pub queue_id: u32,
    pub zero_copy: bool,
    pub num_frames: u32,
    pub frame_size: u32,
    pub batch_size: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_size_min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_size_max: Option<u32>,
    pub num_workers: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_threshold: Option<u32>,
    #[serde(default)]
    pub fill_adaptive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_adapt_factor: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_ema_alpha: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_max: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_addr: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteUpdate {
    pub dest_id: [u8; 32],
    pub next_hop_id: [u8; 32],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_hop_mac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUpdate {
    pub session_id: [u8; 16],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_label: Option<u32>,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeatureFlags {
    pub stub_mode: bool,
    pub enable_metrics: bool,
    pub enable_adaptive_fill: bool,
    pub enable_zero_copy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlRequest {
    pub runtime_config: RuntimeConfig,
    #[serde(default)]
    pub route_updates: Vec<RouteUpdate>,
    #[serde(default)]
    pub session_updates: Vec<SessionUpdate>,
    pub feature_flags: FeatureFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwarderStats {
    pub received: usize,
    pub forwarded: usize,
    pub encrypted: usize,
    pub route_misses: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueueStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_target: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_actual: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryResponse {
    pub health_state: String,
    pub forwarder_stats: ForwarderStats,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_stats: Option<QueueStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub timestamp: String,
}

impl ControlRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.runtime_config.iface.is_empty() {
            return Err("bridge: runtime_config.iface is required".into());
        }
        if self.runtime_config.num_frames == 0 {
            return Err("bridge: runtime_config.num_frames must be positive".into());
        }
        if self.runtime_config.frame_size == 0 {
            return Err("bridge: runtime_config.frame_size must be positive".into());
        }
        if self.runtime_config.batch_size == 0 {
            return Err("bridge: runtime_config.batch_size must be positive".into());
        }
        if let (Some(min), Some(max)) = (self.runtime_config.batch_size_min, self.runtime_config.batch_size_max) {
            if min > max {
                return Err("bridge: runtime_config.batch_size_min cannot exceed batch_size_max".into());
            }
        }
        Ok(())
    }
}

impl FeatureFlags {
}

impl SessionUpdate {
    pub fn secret_bytes(&self) -> Option<Vec<u8>> {
        self.secret.as_ref().and_then(|value| general_purpose::STANDARD.decode(value).ok())
    }

    pub fn info_bytes(&self) -> Option<Vec<u8>> {
        self.info.as_ref().and_then(|value| general_purpose::STANDARD.decode(value).ok())
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BridgeArtifactDigest {
    path: String,
    sha256: String,
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BridgeManifest {
    contract: String,
    version: String,
    schema: BridgeArtifactDigest,
    example: BridgeArtifactDigest,
}

#[cfg(test)]
fn bridge_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("bridge")
}

#[cfg(test)]
fn workspace_root() -> PathBuf {
    bridge_root()
        .parent()
        .expect("bridge root has a workspace parent")
        .to_path_buf()
}

#[cfg(test)]
fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{bridge_root, sha256_hex, workspace_root, BridgeManifest, ControlRequest};
    use serde_json::Value;
    use std::fs;

    #[test]
    fn bridge_contract_artifacts_round_trip() {
        let schema_path = bridge_root().join("bridge_contract.schema.json");
        let schema_data = fs::read_to_string(&schema_path).expect("read schema");
        let schema: Value = serde_json::from_str(&schema_data).expect("parse schema");
        assert_eq!(schema.get("title").and_then(Value::as_str), Some("Mohawk Go-Rust Bridge Contract"));

        let fixture_path = bridge_root().join("examples").join("control_request.example.json");
        let fixture_data = fs::read_to_string(&fixture_path).expect("read example");
        let request: ControlRequest = serde_json::from_str(&fixture_data).expect("parse example");
        request.validate().expect("validate example");

        let encoded = serde_json::to_value(&request).expect("encode request");
        let decoded: ControlRequest = serde_json::from_value(encoded).expect("decode request");
        assert_eq!(decoded.runtime_config.iface, "eth0");
        assert_eq!(decoded.route_updates.len(), 1);
        assert_eq!(decoded.session_updates.len(), 1);
    }

    #[test]
    fn bridge_contract_manifest_matches_artifacts() {
        let manifest_path = bridge_root().join("bridge_contract.manifest.json");
        let manifest_data = fs::read_to_string(&manifest_path).expect("read manifest");
        let manifest: BridgeManifest = serde_json::from_str(&manifest_data).expect("parse manifest");

        assert_eq!(manifest.contract, "Mohawk Go-Rust Bridge Contract");
        assert_eq!(manifest.version, "bridge_contract.manifest.v1");

        let root = workspace_root();
        let schema_data = fs::read(root.join(&manifest.schema.path)).expect("read schema");
        let example_data = fs::read(root.join(&manifest.example.path)).expect("read example");

        assert_eq!(sha256_hex(&schema_data), manifest.schema.sha256);
        assert_eq!(sha256_hex(&example_data), manifest.example.sha256);
    }
}
