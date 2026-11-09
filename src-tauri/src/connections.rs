//! Profil koneksi: disimpan di app config (tanpa password).
//! Password hanya di macOS Keychain (service `dev.selisih.app`, akun = id profil).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use selisih_core::live::ConnParams;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SERVICE: &str = "dev.selisih.app";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub use_tls: bool,
    pub accept_invalid_certs: bool,
    /// Hanya untuk UI: ada password di Keychain.
    #[serde(skip_deserializing)]
    pub has_password: bool,
}

fn store_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("connections.json"))
}

pub fn load(app: &AppHandle) -> Result<Vec<ConnectionProfile>, String> {
    let p = store_path(app)?;
    if !p.exists() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let mut v: Vec<ConnectionProfile> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    for c in &mut v {
        c.has_password = get_password(&c.id).is_some();
    }
    Ok(v)
}

fn save_all(app: &AppHandle, v: &[ConnectionProfile]) -> Result<(), String> {
    let p = store_path(app)?;
    let raw = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(p, raw).map_err(|e| e.to_string())
}

fn entry(id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, id).map_err(|e| e.to_string())
}

pub fn get_password(id: &str) -> Option<String> {
    entry(id).ok()?.get_password().ok()
}

fn set_password(id: &str, pw: &str) -> Result<(), String> {
    let e = entry(id)?;
    if pw.is_empty() {
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    } else {
        e.set_password(pw).map_err(|e| e.to_string())
    }
}

/// Simpan profil. `password = None` -> password lama dipertahankan.
pub fn upsert(app: &AppHandle, mut p: ConnectionProfile, password: Option<String>) -> Result<ConnectionProfile, String> {
    let mut all = load(app)?;
    if p.id.is_empty() {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        p.id = format!("c{n:x}");
    }
    if p.port == 0 {
        p.port = 3306;
    }
    if let Some(pw) = password {
        set_password(&p.id, &pw)?;
    }
    p.has_password = get_password(&p.id).is_some();
    match all.iter_mut().find(|x| x.id == p.id) {
        Some(x) => *x = p.clone(),
        None => all.push(p.clone()),
    }
    save_all(app, &all)?;
    Ok(p)
}

pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
    let mut all = load(app)?;
    all.retain(|c| c.id != id);
    save_all(app, &all)?;
    let _ = set_password(id, "");
    Ok(())
}

pub fn find(app: &AppHandle, id: &str) -> Result<ConnectionProfile, String> {
    load(app)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "Koneksi tidak ditemukan".to_string())
}

pub fn params(p: &ConnectionProfile, password: Option<String>) -> ConnParams {
    ConnParams {
        host: p.host.clone(),
        port: p.port,
        user: p.user.clone(),
        password: password.or_else(|| get_password(&p.id)).unwrap_or_default(),
        database: None,
        use_tls: p.use_tls,
        accept_invalid_certs: p.accept_invalid_certs,
    }
}
