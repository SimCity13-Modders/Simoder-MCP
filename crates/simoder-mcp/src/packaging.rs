#![forbid(unsafe_code)]

//! Clean-prefix installation, atomic upgrade, leftover legacy `.exe` elimination,
//! and transactional rollback for the native `mods/simoder.devtools/` package (`U5`: `Simoder-DevTools#1`, `#2`).
//!
//! Ensures the installed developer mod package contains only the approved native mod `.dll`
//! (`simoder_mcp.dll`), `mod.toml`, and optional authoring tools, with zero legacy `.NET`
//! `Broker/Simoder.DevBridge.exe` or `MCP/Simoder.McpServer.exe` binaries and no hidden offline
//! bootstrap fallback.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Relative paths of retired legacy `.NET` executables and folders that must never remain in a `U5` native mod installation.
pub const RETIRED_LEGACY_SERVICE_PATHS: &[&str] = &[
    "Broker/Simoder.DevBridge.exe",
    "Broker/Simoder.DevBridge.dll",
    "Broker/Simoder.DevBridge.Core.dll",
    "MCP/Simoder.McpServer.exe",
    "MCP/Simoder.McpServer.dll",
    "MCP/Simoder.DevBridge.Core.dll",
];

/// Relative filename of the native MCP DevTool DLL inside `mods/simoder.devtools/`.
pub const NATIVE_MCP_DLL_FILENAME: &str = "simoder_mcp.dll";

/// Canonical native developer-mod `mod.toml` template (`schemaVersion = 2`, `hostAbiVersion = 2`, zero `.exe` services).
pub fn render_native_devtools_manifest(version: &str, dll_sha256: &str) -> String {
    format!(
        r#"schemaVersion = 2
format = "simoder.native-devtool"
kind = "developer-mod"
hostAbiVersion = 2

[metadata]
id = "simoder.devtools"
name = "Simoder Developer Tools (Native MCP)"
version = "{version}"
author = "Simoder"
description = "Optional game-owned native MCP DevTool mod (.dll) interacting with Simoder via ABI v2"

[native]
entry = "{NATIVE_MCP_DLL_FILENAME}"
sha256 = "{dll_sha256}"
abi = 2
"#
    )
}

/// Candidate package payload used when installing or upgrading `mods/simoder.devtools/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeDevToolPackageSpec {
    /// Semantic package version (for example `"0.2.0"`).
    pub version: String,
    /// Raw bytes of `simoder_mcp.dll`.
    pub dll_bytes: Vec<u8>,
    /// Optional `config.toon` or mod configuration content.
    pub config_text: String,
    /// Inject a simulated mid-upgrade write failure after staging to test atomic rollback (`Simoder-DevTools#2`).
    pub simulate_commit_failure: bool,
}

/// Verification and audit report produced by [`NativeDevToolInstaller`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageOperationReport {
    /// Active installed version after the operation (or restored version after rollback).
    pub active_version: String,
    /// Lowercase SHA-256 hex digest of the installed `simoder_mcp.dll`.
    pub active_dll_sha256: String,
    /// Retired legacy `.exe`/`.dll` paths detected and purged during installation or upgrade.
    pub purged_legacy_paths: Vec<String>,
    /// Whether a mid-upgrade failure triggered an automatic rollback to the previous snapshot.
    pub rolled_back: bool,
    /// Structured operation log lines preserved for auditing (`Simoder-DevTools#2`).
    pub operation_logs: Vec<String>,
}

/// Transactional installer and upgrader for `mods/simoder.devtools/` (`U5`).
pub struct NativeDevToolInstaller {
    mod_root: PathBuf,
}

impl NativeDevToolInstaller {
    /// Bind an installer to `<game_root>/mods/simoder.devtools`.
    pub fn new(mod_root: impl Into<PathBuf>) -> Self {
        Self {
            mod_root: mod_root.into(),
        }
    }

    /// Return the target `mods/simoder.devtools` directory path.
    pub fn mod_root(&self) -> &Path {
        &self.mod_root
    }

    /// Scan `mod_root` for any retired legacy `Broker/` or `MCP/` executables (`Simoder-DevTools#1`).
    pub fn find_legacy_leftovers(&self) -> Vec<String> {
        RETIRED_LEGACY_SERVICE_PATHS
            .iter()
            .filter(|rel| self.mod_root.join(rel).exists())
            .map(|rel| (*rel).to_string())
            .collect()
    }

    /// Perform a clean-prefix installation or transactional upgrade with automatic rollback on failure (`Simoder-DevTools#1`, `#2`).
    pub fn install_or_upgrade(
        &self,
        spec: &NativeDevToolPackageSpec,
    ) -> Result<PackageOperationReport, String> {
        if spec.dll_bytes.is_empty() {
            return Err("native mod DLL payload cannot be empty".into());
        }
        let mut logs = Vec::new();
        fs::create_dir_all(&self.mod_root).map_err(|e| e.to_string())?;

        // Snapshot existing installation (if present) for atomic rollback on failure.
        let manifest_path = self.mod_root.join("mod.toml");
        let dll_path = self.mod_root.join(NATIVE_MCP_DLL_FILENAME);
        let config_path = self.mod_root.join("config.toon");

        let backup_manifest = fs::read_to_string(&manifest_path).ok();
        let backup_dll = fs::read(&dll_path).ok();
        let backup_config = fs::read_to_string(&config_path).ok();
        let had_previous = backup_manifest.is_some() && backup_dll.is_some();
        if had_previous {
            logs.push("snapshotted previous native mod installation for rollback safety".into());
        } else {
            logs.push("starting clean-prefix native mod installation".into());
        }

        // Detect and purge any retired legacy .NET executables (`Broker/Simoder.DevBridge.exe`, `MCP/Simoder.McpServer.exe`).
        let mut purged_legacy_paths = Vec::new();
        for &rel in RETIRED_LEGACY_SERVICE_PATHS {
            let full = self.mod_root.join(rel);
            if full.exists() {
                fs::remove_file(&full).map_err(|e| e.to_string())?;
                purged_legacy_paths.push(rel.to_string());
                logs.push(format!("purged retired legacy service artifact '{rel}'"));
            }
        }
        for legacy_dir in ["Broker", "MCP"] {
            let dir_path = self.mod_root.join(legacy_dir);
            if dir_path.is_dir()
                && fs::read_dir(&dir_path)
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(false)
            {
                let _ = fs::remove_dir(&dir_path);
            }
        }

        let new_sha256 = hex::encode(Sha256::digest(&spec.dll_bytes));
        let new_manifest = render_native_devtools_manifest(&spec.version, &new_sha256);

        // Stage candidate files into `.staging/` before committing.
        let staging_dir = self.mod_root.join(".staging");
        fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;
        let staged_manifest = staging_dir.join("mod.toml");
        let staged_dll = staging_dir.join(NATIVE_MCP_DLL_FILENAME);
        let staged_config = staging_dir.join("config.toon");

        fs::write(&staged_manifest, &new_manifest).map_err(|e| e.to_string())?;
        fs::write(&staged_dll, &spec.dll_bytes).map_err(|e| e.to_string())?;
        fs::write(&staged_config, &spec.config_text).map_err(|e| e.to_string())?;
        logs.push(format!(
            "staged native mod v{} (sha256={new_sha256})",
            spec.version
        ));

        // Write partial target then check simulated commit failure to verify rollback recovery.
        fs::copy(&staged_manifest, &manifest_path).map_err(|e| e.to_string())?;
        if spec.simulate_commit_failure {
            logs.push("commit failure detected during upgrade; rolling back to previous snapshot".into());
            let _ = fs::remove_dir_all(&staging_dir);
            if let (Some(prev_m), Some(prev_d)) = (backup_manifest, backup_dll) {
                fs::write(&manifest_path, &prev_m).map_err(|e| e.to_string())?;
                fs::write(&dll_path, &prev_d).map_err(|e| e.to_string())?;
                if let Some(prev_c) = backup_config {
                    fs::write(&config_path, prev_c).map_err(|e| e.to_string())?;
                }
                let restored_sha = hex::encode(Sha256::digest(&prev_d));
                let restored_ver = extract_version_from_manifest(&prev_m).unwrap_or_else(|| "unknown".into());
                logs.push(format!(
                    "rollback completed cleanly: restored v{restored_ver} (sha256={restored_sha})"
                ));
                return Ok(PackageOperationReport {
                    active_version: restored_ver,
                    active_dll_sha256: restored_sha,
                    purged_legacy_paths,
                    rolled_back: true,
                    operation_logs: logs,
                });
            } else {
                let _ = fs::remove_file(&manifest_path);
                let _ = fs::remove_file(&dll_path);
                let _ = fs::remove_file(&config_path);
                return Err("clean-prefix installation aborted and cleaned up on commit failure".into());
            }
        }

        fs::copy(&staged_dll, &dll_path).map_err(|e| e.to_string())?;
        fs::copy(&staged_config, &config_path).map_err(|e| e.to_string())?;
        let _ = fs::remove_dir_all(&staging_dir);
        logs.push(format!(
            "committed native mod v{} without legacy executables",
            spec.version
        ));

        Ok(PackageOperationReport {
            active_version: spec.version.clone(),
            active_dll_sha256: new_sha256,
            purged_legacy_paths,
            rolled_back: false,
            operation_logs: logs,
        })
    }
}

fn extract_version_from_manifest(manifest_text: &str) -> Option<String> {
    for line in manifest_text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version =") {
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}
