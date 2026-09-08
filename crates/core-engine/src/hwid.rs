//! Hardware-bound licensing.
//!
//! Honest scope note (see the architecture review that preceded this
//! scaffold): this raises the bar against casual account sharing, it does
//! not "guarantee zero piracy" — a determined attacker can still patch the
//! binary or spoof the fingerprint. Treat it as a deterrent plus a support
//! workflow (grace period + self-service re-activation), not a silver
//! bullet, and don't market it as one.
//!
//! Production verification uses Ed25519. The client contains only the public
//! key; signing keys belong in a separate licensing service and must never be
//! committed, bundled, or accepted from the desktop client.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareFingerprint(pub String);

impl fmt::Display for HardwareFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LicenseError {
    #[error("could not read a stable machine identifier on this platform")]
    NoMachineId,
    #[error("activation signature is invalid")]
    BadSignature,
    #[error("activation license has expired")]
    Expired,
    #[error("activation license payload is invalid")]
    InvalidPayload,
}

/// Gathers a small set of low-churn hardware/OS identifiers and hashes them
/// into a single opaque fingerprint. Deliberately avoids anything that
/// changes on routine upgrades (RAM, a second monitor, ...); a full
/// component swap will still shift the machine-id sources below, which is
/// the honest tradeoff of any HWID scheme.
pub fn fingerprint() -> Result<HardwareFingerprint, LicenseError> {
    let raw = platform_machine_id().ok_or(LicenseError::NoMachineId)?;

    let mut sys = sysinfo::System::new();
    sys.refresh_all();
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();

    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hasher.update(cpu_brand.as_bytes());
    let digest = hasher.finalize();

    Ok(HardwareFingerprint(hex::encode(digest)))
}

#[cfg(target_os = "linux")]
fn platform_machine_id() -> Option<String> {
    // Stable across reboots, rotates on OS reinstall - the same tradeoff as
    // an SMBIOS UUID would give us, without needing dmidecode/root.
    std::fs::read_to_string("/etc/machine-id")
        .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(target_os = "macos")]
fn platform_machine_id() -> Option<String> {
    use std::process::Command;
    // IOPlatformUUID is the closest macOS equivalent of a motherboard UUID.
    let output = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find(|l| l.contains("IOPlatformUUID"))
        .and_then(|l| l.split('"').nth(3))
        .map(|s| s.to_string())
}

// `wmic csproduct get UUID` used to be the obvious way to read this, but
// Microsoft has been retiring the tool: it is disabled by default in
// Windows 11 23H2 and 24H2, removed on upgrade to 25H2, and slated to
// disappear entirely - not even available as a Feature on Demand - in the
// following feature update. Shelling out to it now fails on most current
// machines, and since `fingerprint()` is what licensing is built on, that
// failure took activation down with it. So: ask WMI through PowerShell,
// which is Microsoft's own recommended replacement, and keep the registry
// as a fallback.
#[cfg(target_os = "windows")]
fn platform_machine_id() -> Option<String> {
    smbios_uuid().or_else(machine_guid)
}

/// Preferred source: the SMBIOS/firmware UUID via WMI - the same value the
/// old `wmic` call returned, so fingerprints do not shift under existing
/// installs. Survives an OS reinstall; changes if the board is replaced,
/// which is the honest tradeoff behind the word "hardware-bound".
#[cfg(target_os = "windows")]
fn smbios_uuid() -> Option<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    // Keep a console window from flashing up when the GUI app calls this.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance -ClassName Win32_ComputerSystemProduct).UUID",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    let uuid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    is_usable_smbios_uuid(&uuid).then_some(uuid)
}

/// Fallback: the OS install's MachineGuid. Always present and needs no
/// subprocess, but it rotates on an OS reinstall - which is why it is
/// second choice rather than first.
#[cfg(target_os = "windows")]
fn machine_guid() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let guid: String = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        .ok()?
        .get_value("MachineGuid")
        .ok()?;

    let guid = guid.trim().to_string();
    (!guid.is_empty()).then_some(guid)
}

/// Some firmware reports a placeholder instead of a real SMBIOS UUID - all
/// zeroes, or all Fs. Those are worse than useless as a fingerprint, since
/// every machine of that model reports the identical value, so treat them
/// as absent and let the caller fall through to the registry.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_usable_smbios_uuid(uuid: &str) -> bool {
    let hex: String = uuid.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    hex.len() == 32
        && !hex.chars().all(|c| c == '0')
        && !hex.chars().all(|c| c.eq_ignore_ascii_case(&'f'))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_machine_id() -> Option<String> {
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LicenseClaims {
    pub seat_id: String,
    pub fingerprint: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedLicense {
    pub claims: LicenseClaims,
    /// Hex-encoded Ed25519 signature over the canonical JSON claims.
    pub signature: String,
}

fn claims_bytes(claims: &LicenseClaims) -> Result<Vec<u8>, LicenseError> {
    serde_json::to_vec(claims).map_err(|_| LicenseError::InvalidPayload)
}

pub fn serialize_claims(claims: &LicenseClaims) -> Result<Vec<u8>, LicenseError> {
    claims_bytes(claims)
}

pub fn parse_license_json(input: &[u8]) -> Result<SignedLicense, LicenseError> {
    serde_json::from_slice(input).map_err(|_| LicenseError::InvalidPayload)
}

/// Verify a license using a public key embedded in the application. A license
/// signer should call the Ed25519 SDK from a server-only service instead.
pub fn verify_license(
    public_key: &[u8; 32],
    license: &SignedLicense,
    expected_fingerprint: &HardwareFingerprint,
    now_unix: u64,
) -> Result<(), LicenseError> {
    if license.claims.fingerprint != expected_fingerprint.0 {
        return Err(LicenseError::BadSignature);
    }
    if license.claims.expires_at_unix <= now_unix {
        return Err(LicenseError::Expired);
    }

    let key = VerifyingKey::from_bytes(public_key).map_err(|_| LicenseError::BadSignature)?;
    let signature_bytes =
        hex::decode(&license.signature).map_err(|_| LicenseError::BadSignature)?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| LicenseError::BadSignature)?;
    key.verify(&claims_bytes(&license.claims)?, &signature)
        .map_err(|_| LicenseError::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_deterministic_within_a_run() {
        let a = fingerprint();
        let b = fingerprint();
        // Either both fail identically (no machine id available in this
        // sandbox) or both succeed with the same value - either way they
        // must agree with each other.
        match (a, b) {
            (Ok(a), Ok(b)) => assert_eq!(a, b),
            (Err(_), Err(_)) => {}
            _ => panic!("fingerprint() was non-deterministic across two calls"),
        }
    }

    #[test]
    fn rejects_placeholder_smbios_uuids() {
        // A real Dell/Lenovo-style SMBIOS UUID.
        assert!(is_usable_smbios_uuid("4C4C4544-0037-3810-8051-B4C04F573833"));
        // Firmware placeholders seen in the wild - identical on every unit.
        assert!(!is_usable_smbios_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(!is_usable_smbios_uuid("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"));
        assert!(!is_usable_smbios_uuid("ffffffff-ffff-ffff-ffff-ffffffffffff"));
        // Nothing at all, or PowerShell erroring out to stdout.
        assert!(!is_usable_smbios_uuid(""));
        assert!(!is_usable_smbios_uuid("not-a-uuid"));
    }

    #[test]
    fn activation_round_trips() {
        use ed25519_dalek::{Signer, SigningKey};

        let fp = HardwareFingerprint("test-fingerprint".into());
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let claims = LicenseClaims {
            seat_id: "seat-1".into(),
            fingerprint: fp.0.clone(),
            expires_at_unix: 2_000_000_000,
        };
        let payload = claims_bytes(&claims).expect("claims serialize");
        let license = SignedLicense {
            claims,
            signature: hex::encode(signing_key.sign(&payload).to_bytes()),
        };

        assert!(verify_license(
            signing_key.verifying_key().as_bytes(),
            &license,
            &fp,
            1_900_000_000
        )
        .is_ok());
        assert!(verify_license(
            signing_key.verifying_key().as_bytes(),
            &license,
            &fp,
            2_000_000_001
        )
        .is_err());

        let mut tampered = license.clone();
        tampered.claims.seat_id = "seat-2".into();
        assert!(verify_license(
            signing_key.verifying_key().as_bytes(),
            &tampered,
            &fp,
            1_900_000_000
        )
        .is_err());
    }
}
