//! Which project a listener belongs to, derived from its working directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How many directories above the cwd to search for a `.git` root.
const MAX_DEPTH: usize = 8;

/// Resolves working directories to short project names, caching per directory.
pub struct ProjectCache {
    home: Option<PathBuf>,
    cache: HashMap<PathBuf, Option<String>>,
}

impl ProjectCache {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self {
            home,
            cache: HashMap::new(),
        }
    }

    /// A cache rooted at `$HOME`.
    pub fn from_env() -> Self {
        Self::new(std::env::var_os("HOME").map(PathBuf::from))
    }

    #[allow(dead_code)] // TODO(v0.2 Task 11b): check details / TUI detail pane
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// Project name for a working directory inside the home directory: the
    /// nearest ancestor holding `.git` (at most [`MAX_DEPTH`] levels up, never
    /// above home), otherwise the directory's own name. `None` for directories
    /// outside home (system daemons, `/`) and for home itself.
    pub fn resolve(&mut self, cwd: &Path) -> Option<String> {
        if let Some(hit) = self.cache.get(cwd) {
            return hit.clone();
        }
        let name = self.compute(cwd);
        self.cache.insert(cwd.to_path_buf(), name.clone());
        name
    }

    fn compute(&self, cwd: &Path) -> Option<String> {
        let home = self.home.as_deref()?;
        if cwd == home || !cwd.starts_with(home) {
            return None;
        }
        let root = cwd
            .ancestors()
            .take(MAX_DEPTH)
            .take_while(|dir| *dir != home)
            .find(|dir| dir.join(".git").exists())
            .unwrap_or(cwd);
        root.file_name().map(|n| n.to_string_lossy().into_owned())
    }
}

/// A path with the home directory shown as `~`.
#[allow(dead_code)] // TODO(v0.2 Task 11b): check details / TUI detail pane
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A throwaway "home" directory unique to this test.
    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kav-project-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn git_root_name_wins() {
        let home = temp_home("git");
        let web = home.join("code/shop/apps/web");
        fs::create_dir_all(&web).unwrap();
        fs::create_dir_all(home.join("code/shop/.git")).unwrap();
        let mut cache = ProjectCache::new(Some(home.clone()));
        assert_eq!(cache.resolve(&web).as_deref(), Some("shop"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cwd_basename_without_git() {
        let home = temp_home("plain");
        let dir = home.join("scratch/api-mock");
        fs::create_dir_all(&dir).unwrap();
        let mut cache = ProjectCache::new(Some(home.clone()));
        assert_eq!(cache.resolve(&dir).as_deref(), Some("api-mock"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn outside_home_is_none() {
        let home = temp_home("outside");
        let mut cache = ProjectCache::new(Some(home.clone()));
        assert_eq!(
            cache.resolve(Path::new("/opt/homebrew/var/postgresql@18")),
            None
        );
        assert_eq!(cache.resolve(Path::new("/")), None);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn home_itself_is_none() {
        let home = temp_home("itself");
        let mut cache = ProjectCache::new(Some(home.clone()));
        assert_eq!(cache.resolve(&home), None);
        assert_eq!(ProjectCache::new(None).resolve(&home), None);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn results_are_cached_per_directory() {
        let home = temp_home("cache");
        let dir = home.join("app");
        fs::create_dir_all(&dir).unwrap();
        let mut cache = ProjectCache::new(Some(home.clone()));
        assert_eq!(cache.resolve(&dir).as_deref(), Some("app"));
        fs::create_dir_all(home.join(".git")).unwrap();
        assert_eq!(
            cache.resolve(&dir).as_deref(),
            Some("app"),
            "cached answer reused"
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn display_path_uses_tilde() {
        let home = Path::new("/Users/madhu");
        assert_eq!(
            display_path(Path::new("/Users/madhu/code/app"), Some(home)),
            "~/code/app"
        );
        assert_eq!(display_path(home, Some(home)), "~");
        assert_eq!(display_path(Path::new("/opt/x"), Some(home)), "/opt/x");
        assert_eq!(display_path(Path::new("/opt/x"), None), "/opt/x");
    }
}
