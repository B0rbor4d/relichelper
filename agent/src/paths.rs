//! Locates Warframe's EE.log across platforms and Steam layouts.
//!
//! Linux/Proton is the tricky case: the file lives inside the Proton prefix of
//! Steam app `230410`, and that prefix can sit in any Steam *library* — the
//! default root, a Flatpak install, or a custom library on another drive
//! (registered in `steamapps/libraryfolders.vdf`). We probe all of them.
//!
//! When auto-detection fails, the finished app exposes a manual path picker;
//! the chosen path is persisted via [`save_override`] and takes priority.

use std::path::{Path, PathBuf};

use regex::Regex;

/// Warframe's Steam application id.
pub const WARFRAME_APP_ID: &str = "230410";

/// Relative path from a Steam *library root* to the EE.log inside the Proton
/// prefix.
const PROTON_SUFFIX: &str =
    "steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log";

/// Returns the persisted manual-override path, if one was saved and still
/// exists on disk.
pub fn load_override() -> Option<PathBuf> {
    let p = std::fs::read_to_string(override_file()?).ok()?;
    let path = PathBuf::from(p.trim());
    path.is_file().then_some(path)
}

/// Persists a user-picked EE.log path so future runs use it directly.
pub fn save_override(path: &Path) -> std::io::Result<()> {
    let file = override_file().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no config dir available")
    })?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, path.to_string_lossy().as_bytes())
}

/// Resolves the EE.log path: persisted override first, then platform
/// auto-detection. Returns `None` if nothing is found — the caller should then
/// prompt the user with the manual picker.
pub fn locate() -> Option<PathBuf> {
    if let Some(p) = load_override() {
        return Some(p);
    }
    auto_detect()
}

/// Platform auto-detection, without consulting the saved override.
pub fn auto_detect() -> Option<PathBuf> {
    candidates().into_iter().find(|p| p.is_file())
}

/// All paths we would try, in priority order. Exposed so a UI can show the user
/// which locations were probed.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    // Windows: native install.
    if let Some(local) = dirs::data_local_dir() {
        out.push(local.join("Warframe").join("EE.log"));
    }

    // Linux/Proton: every Steam library root + the Proton suffix.
    for root in steam_library_roots() {
        out.push(root.join(PROTON_SUFFIX));
    }

    out
}

/// Discovers Steam library roots: the well-known Steam install dirs plus any
/// extra libraries registered in `libraryfolders.vdf`.
fn steam_library_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let home = dirs::home_dir();

    let mut known = Vec::new();
    if let Some(home) = &home {
        known.push(home.join(".steam/steam"));
        known.push(home.join(".steam/root"));
        known.push(home.join(".local/share/Steam"));
        known.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }

    for base in known {
        push_unique(&mut roots, base.clone());
        for extra in parse_library_folders(&base.join("steamapps/libraryfolders.vdf")) {
            push_unique(&mut roots, extra);
        }
    }

    roots
}

fn push_unique(roots: &mut Vec<PathBuf>, path: PathBuf) {
    if !roots.contains(&path) {
        roots.push(path);
    }
}

/// Extracts `"path"  "<dir>"` entries from a `libraryfolders.vdf` file. Returns
/// an empty vec if the file is absent or unreadable.
fn parse_library_folders(vdf: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(vdf) else {
        return Vec::new();
    };
    library_paths_from_vdf(&text)
}

/// Pure VDF extraction, split out for testing.
fn library_paths_from_vdf(text: &str) -> Vec<PathBuf> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#""path"\s*"([^"]+)""#).unwrap());
    re.captures_iter(text)
        // VDF escapes backslashes on Windows; normalise the common case.
        .map(|c| PathBuf::from(c[1].replace("\\\\", "\\")))
        .collect()
}

fn override_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("relichelper").join("ee_log_path"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_library_paths_including_custom_drive() {
        // Trimmed shape of a real libraryfolders.vdf with a custom library.
        let vdf = r#"
"libraryfolders"
{
    "0"
    {
        "path"		"/home/user/.local/share/Steam"
        "label"		""
    }
    "1"
    {
        "path"		"/mnt/Games/SteamLibrary"
        "apps"
        {
            "230410"	"123456789"
        }
    }
}
"#;
        let paths = library_paths_from_vdf(vdf);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/home/user/.local/share/Steam"),
                PathBuf::from("/mnt/Games/SteamLibrary"),
            ]
        );
    }

    #[test]
    fn candidates_include_proton_suffix() {
        // At least one candidate must end with the Proton EE.log suffix.
        assert!(candidates()
            .iter()
            .any(|p| p.to_string_lossy().ends_with("Warframe/EE.log")));
    }

    #[test]
    fn missing_vdf_yields_no_paths() {
        assert!(parse_library_folders(Path::new("/nonexistent/libraryfolders.vdf")).is_empty());
    }
}
