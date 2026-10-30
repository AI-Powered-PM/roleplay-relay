//! Runtime data folder (%APPDATA%\WowChatHelper) and first-run copy of bundled defaults.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().data_dir().map_err(|e| e.to_string())?;
    Ok(base.join("WowChatHelper"))
}

/// Create the folder layout and copy any bundled default that is not there yet.
/// Existing files are never overwritten.
pub fn ensure_layout(app: &AppHandle, data: &Path) -> Result<(), String> {
    for sub in ["prompts", "characters", "logs"] {
        fs::create_dir_all(data.join(sub)).map_err(|e| e.to_string())?;
    }
    if let Some(src) = bundled_resources(app) {
        copy_missing(&src, data).map_err(|e| e.to_string())?;
    } else {
        tracing::warn!("bundled resources folder not found; defaults not copied");
    }
    Ok(())
}

fn bundled_resources(app: &AppHandle) -> Option<PathBuf> {
    let from_bundle = app.path().resource_dir().ok().map(|p| p.join("resources"));
    if let Some(p) = from_bundle.filter(|p| p.is_dir()) {
        return Some(p);
    }
    // `tauri dev` runs from target/debug; fall back to the repo copy.
    #[cfg(debug_assertions)]
    {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("resources");
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

/// Recursively copy files from `src` into `dst`, skipping any file that already exists.
pub fn copy_missing(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_missing(&entry.path(), &target)?;
        } else if !target.exists() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("wch-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn copies_missing_and_keeps_user_edits() {
        let src = temp("src");
        let dst = temp("dst");
        fs::create_dir_all(src.join("prompts")).unwrap();
        fs::write(src.join("prompts/a.md"), "default").unwrap();
        fs::write(src.join("prompts/b.md"), "default").unwrap();
        fs::create_dir_all(dst.join("prompts")).unwrap();
        fs::write(dst.join("prompts/a.md"), "edited").unwrap();

        copy_missing(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(dst.join("prompts/a.md")).unwrap(), "edited");
        assert_eq!(fs::read_to_string(dst.join("prompts/b.md")).unwrap(), "default");
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }
}
