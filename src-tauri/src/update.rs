//! "Check for updates" (Yash, 2026-10-08). Ivy goes online for this only when the user presses the button in
//! Settings: one request to GitHub for the latest release, compared with the running version. "Download &
//! install" fetches that release's setup, checks it against the release's SHA256SUMS.txt, starts it in
//! passive mode with /R (it removes the old version but keeps data, model and settings, then reopens Ivy:
//! see installer/installer.nsi) and quits Ivy so the files can be replaced. Nothing ever runs by itself.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;

const LATEST: &str = "https://api.github.com/repos/raj-7676/IVY-/releases/latest";
const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current: String,
    latest: String,
    newer: bool,
    page: String,
}

fn agent() -> Result<ureq::Agent, String> {
    Ok(ureq::AgentBuilder::new()
        // Windows' own TLS and certificate store, like the model download (model.rs).
        .tls_connector(Arc::new(native_tls::TlsConnector::new().map_err(|e| e.to_string())?))
        .try_proxy_from_env(true)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(30))
        .user_agent(concat!("Ivy/", env!("CARGO_PKG_VERSION")))
        .build())
}

fn plain(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("GitHub answered {code}"),
        ureq::Error::Transport(_) => "no connection to GitHub".into(),
    }
}

/// "0.2.10" > "0.2.9": numeric parts, a leading "v" ignored. Anything unparsable is never "newer".
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect() };
    match (parse(latest), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

struct Release {
    version: String,
    page: String,
    setup_url: String,
    setup_name: String,
    sums_url: String,
}

fn latest_release() -> Result<Release, String> {
    let body = agent()?
        .get(LATEST)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(plain)?
        .into_string()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| "GitHub sent something unexpected".to_string())?;
    let version = v["tag_name"].as_str().unwrap_or_default().trim_start_matches('v').to_string();
    let setup_name = format!("Ivy_{version}_x64-setup.exe");
    let asset = |name: &str| {
        v["assets"].as_array().into_iter().flatten()
            .find(|a| a["name"].as_str() == Some(name))
            .and_then(|a| a["browser_download_url"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    Ok(Release {
        page: v["html_url"].as_str().unwrap_or("https://github.com/raj-7676/IVY-/releases/latest").to_string(),
        setup_url: asset(&setup_name),
        sums_url: asset("SHA256SUMS.txt"),
        setup_name,
        version,
    })
}

#[tauri::command]
pub fn check_for_update() -> Result<UpdateInfo, String> {
    let r = latest_release()?;
    Ok(UpdateInfo { newer: is_newer(&r.version, CURRENT), current: CURRENT.into(), latest: r.version, page: r.page })
}

/// Downloads the newer setup, checks it, starts it and quits Ivy. Progress: `ivy://update-progress` (0-100).
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    let path = tauri::async_runtime::spawn_blocking(move || download_setup(&handle))
        .await
        .map_err(|e| e.to_string())??;
    std::process::Command::new(&path).args(["/P", "/R"]).spawn().map_err(|e| format!("could not start setup: {e}"))?;
    app.exit(0);
    Ok(())
}

fn download_setup(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let r = latest_release()?;
    if !is_newer(&r.version, CURRENT) {
        return Err("Ivy is already up to date".into());
    }
    if r.setup_url.is_empty() || r.sums_url.is_empty() {
        return Err("that release has no setup file yet".into());
    }
    let agent = agent()?;
    let sums = agent.get(&r.sums_url).call().map_err(plain)?.into_string().map_err(|e| e.to_string())?;
    let expected = sums
        .lines()
        .find_map(|l| l.split_once("  ").filter(|(_, name)| name.trim() == r.setup_name).map(|(h, _)| h.trim().to_lowercase()))
        .ok_or("the release's checksum list doesn't mention its setup file")?;

    let dir = std::env::temp_dir().join("Ivy-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&r.setup_name);
    let resp = agent.get(&r.setup_url).call().map_err(plain)?;
    let total: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let (mut hasher, mut done, mut buf, mut last_pct) = (Sha256::new(), 0u64, vec![0u8; 1 << 16], 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|_| "the download was interrupted".to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        let pct = if total > 0 { done * 100 / total } else { 0 };
        if pct != last_pct {
            last_pct = pct;
            let _ = app.emit("ivy://update-progress", pct);
        }
    }
    drop(file);
    let got = format!("{:x}", hasher.finalize());
    if got != expected {
        let _ = std::fs::remove_file(&path);
        return Err("the downloaded setup didn't match its checksum, so it was deleted".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.2.10", "0.2.9"));
        assert!(is_newer("v0.3.0", "0.2.7"));
        assert!(!is_newer("0.2.7", "0.2.7"));
        assert!(!is_newer("0.2.6", "0.2.7"));
        assert!(!is_newer("nonsense", "0.2.7"));
    }
}
