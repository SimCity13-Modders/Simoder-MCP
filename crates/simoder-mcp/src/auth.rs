#![forbid(unsafe_code)]

//! Client authentication, explicit agent-access consent, fine-grained permission grants,
//! runtime epoch binding, and live revocation for `simoder-mcp` (`Simoder-DevTools-MCP#3`).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_with::DeserializeFromStr;
use sha2::{Digest, Sha256};
use strum::{EnumString, IntoStaticStr, VariantArray};

use crate::types::McpError;

/// Built-in permission grant categories enforced by `simoder-mcp` before tool execution (`ADR-005`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    DeserializeFromStr,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
#[strum(ascii_case_insensitive)]
pub enum McpGrant {
    /// Permission to read startup state, capabilities, service info, regions, and native session status.
    #[strum(to_string = "session.read")]
    #[serde(rename = "session.read")]
    SessionRead,
    /// Permission to launch, enter menu, skip intro, create Sandbox regions, claim cities, load cities, or request normal exit.
    #[strum(to_string = "session.mutate")]
    #[serde(rename = "session.mutate")]
    SessionMutate,
    /// Permission to inspect loaded Sandbox world units (`simcity_get_world_units`, `simcity_get_world_unit`).
    #[strum(to_string = "world.inspect")]
    #[serde(rename = "world.inspect")]
    WorldInspect,
    /// Permission to inspect or toggle mods (`simoder_get_runtime_state`, `simoder_get_logs`, `simoder_set_mod_enabled`).
    #[strum(to_string = "mods.control")]
    #[serde(rename = "mods.control")]
    ModsControl,
    /// Permission to list game modules, read bounded 32-bit process memory, or export PE code for REA.
    #[strum(to_string = "memory.analysis")]
    #[serde(rename = "memory.analysis")]
    MemoryAnalysis,
    /// Permission to capture windows and send capture-bound clicks or allowlisted keys (`Escape`, `Enter`, `Space`).
    #[strum(to_string = "ui.input")]
    #[serde(rename = "ui.input")]
    UiInput,
    /// Permission to invoke `Simoder-OpenSCP` debug-tools unlock, 14-channel `EAWebKit` bridge telemetry, and `SLDelta` v4 decoders.
    #[strum(to_string = "openscp.diagnostics")]
    #[serde(rename = "openscp.diagnostics")]
    OpenScpDiagnostics,
}

impl McpGrant {
    /// Return the canonical dot-separated grant identifier.
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Authenticated MCP client session bound to a specific client identity, token digest, runtime epoch,
/// explicit user consent flag, and active permission grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSession {
    /// Host-issued session identifier.
    pub session_id: String,
    /// Verified client identity (for example `"codex-cli"` or `"antigravity-mcp-client"`).
    pub client_id: String,
    /// SHA-256 digest of the bearer/session token (raw secret is never stored).
    pub token_sha256: String,
    /// Runtime epoch at the time the token was issued (stale tokens from previous epochs are rejected).
    pub issued_epoch: u64,
    /// Whether the user has granted explicit agent-access consent for this client session.
    pub user_consent_approved: bool,
    /// Active built-in and custom mod-namespace permission grants (for example `"session.read"` or `"mod.traffic.read"`).
    pub grants: BTreeSet<String>,
    /// Whether this session has been explicitly revoked.
    pub revoked: bool,
}

/// Security gate managing client identities, explicit agent consent, permission grants,
/// token epoch freshness, and live mid-session revocation (`Simoder-DevTools-MCP#3`).
#[derive(Debug)]
pub struct McpAuthManager {
    current_epoch: u64,
    global_agent_opt_in: bool,
    sessions: BTreeMap<String, ClientSession>,
}

impl Default for McpAuthManager {
    fn default() -> Self {
        Self::new(1)
    }
}

impl McpAuthManager {
    /// Construct a new [`McpAuthManager`] initialized at `initial_epoch` (`>= 1`).
    pub fn new(initial_epoch: u64) -> Self {
        Self {
            current_epoch: initial_epoch.max(1),
            global_agent_opt_in: true,
            sessions: BTreeMap::new(),
        }
    }

    /// Return the current runtime security epoch.
    pub const fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Advance the runtime epoch (for example on mod reload or city/session reset), invalidating all prior tokens.
    pub fn advance_epoch(&mut self) -> u64 {
        self.current_epoch = self.current_epoch.saturating_add(1);
        self.current_epoch
    }

    /// Enable or disable global user opt-in for MCP agent access.
    pub fn set_global_agent_opt_in(&mut self, enabled: bool) {
        self.global_agent_opt_in = enabled;
    }

    /// Provision an authenticated [`ClientSession`] with explicit consent and an initial grant set.
    pub fn register_session(
        &mut self,
        session_id: impl Into<String>,
        client_id: impl Into<String>,
        raw_token: &str,
        user_consent_approved: bool,
        grants: impl IntoIterator<Item = String>,
    ) -> ClientSession {
        let session_id = session_id.into();
        let token_sha256 = hex::encode(Sha256::digest(raw_token.as_bytes()));
        let session = ClientSession {
            session_id: session_id.clone(),
            client_id: client_id.into(),
            token_sha256,
            issued_epoch: self.current_epoch,
            user_consent_approved,
            grants: grants.into_iter().collect(),
            revoked: false,
        };
        self.sessions.insert(session_id, session.clone());
        session
    }

    /// Provision a fully consented session holding all 7 built-in [`McpGrant`] permissions.
    pub fn register_full_session(
        &mut self,
        session_id: impl Into<String>,
        client_id: impl Into<String>,
        raw_token: &str,
    ) -> ClientSession {
        let all_grants = McpGrant::VARIANTS
            .iter()
            .map(|g| g.as_str().to_string())
            .collect::<Vec<_>>();
        self.register_session(session_id, client_id, raw_token, true, all_grants)
    }

    /// Update explicit user consent for `session_id` (allows live consent revocation during an active session).
    pub fn set_user_consent(&mut self, session_id: &str, approved: bool) -> bool {
        let Some(session) = self.sessions.get_mut(session_id) else {
            return false;
        };
        session.user_consent_approved = approved;
        true
    }

    /// Grant an additional permission identifier (built-in [`McpGrant`] or mod namespace grant) to `session_id`.
    pub fn grant_permission(&mut self, session_id: &str, grant: impl Into<String>) -> bool {
        let Some(session) = self.sessions.get_mut(session_id) else {
            return false;
        };
        session.grants.insert(grant.into());
        true
    }

    /// Revoke a specific permission grant from `session_id` in real time.
    pub fn revoke_permission(&mut self, session_id: &str, grant: &str) -> bool {
        let Some(session) = self.sessions.get_mut(session_id) else {
            return false;
        };
        session.grants.remove(grant)
    }

    /// Permanently revoke an entire client session (`session_id`).
    pub fn revoke_session(&mut self, session_id: &str) -> bool {
        let Some(session) = self.sessions.get_mut(session_id) else {
            return false;
        };
        session.revoked = true;
        true
    }

    /// Verify client identity, token secret, epoch freshness, explicit user consent, and required grants.
    pub fn authorize_call(
        &self,
        session_id: &str,
        raw_token: &str,
        required_grants: &[String],
    ) -> Result<&ClientSession, McpError> {
        if !self.global_agent_opt_in {
            return Err(McpError::AuthorizationDenied(
                "global agent-access opt-in is disabled".into(),
            ));
        }
        let Some(session) = self.sessions.get(session_id) else {
            return Err(McpError::AuthorizationDenied(format!(
                "unknown client session '{session_id}'"
            )));
        };
        if session.revoked {
            return Err(McpError::AuthorizationDenied(format!(
                "client session '{session_id}' has been revoked"
            )));
        }
        let provided_hash = hex::encode(Sha256::digest(raw_token.as_bytes()));
        if provided_hash != session.token_sha256 {
            return Err(McpError::AuthorizationDenied(
                "invalid session bearer token".into(),
            ));
        }
        if session.issued_epoch != self.current_epoch {
            return Err(McpError::AuthorizationDenied(format!(
                "stale session token epoch {} (current runtime epoch is {})",
                session.issued_epoch, self.current_epoch
            )));
        }
        if !session.user_consent_approved {
            return Err(McpError::AuthorizationDenied(format!(
                "explicit user consent is not approved for client '{}'",
                session.client_id
            )));
        }
        for req in required_grants {
            if !session.grants.contains(req) {
                return Err(McpError::AuthorizationDenied(format!(
                    "missing required permission grant '{req}' for client '{}'",
                    session.client_id
                )));
            }
        }
        Ok(session)
    }
}
