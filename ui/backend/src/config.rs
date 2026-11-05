use std::{
    fs, io,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::Mutex,
};

use rand::Rng;
use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 7373;

#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub repo: PathBuf,
    /// Executable that runs this project's sessions instead of `AppState::claude` (decision 39).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    token: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default)]
    projects: Vec<Project>,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn new_token() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub struct Store {
    path: PathBuf,
    config: Mutex<Config>,
}

impl Store {
    pub fn default_path() -> PathBuf {
        if let Some(path) = std::env::var_os("WORKITEM_UI_CONFIG") {
            return path.into();
        }
        let home = std::env::var_os("HOME").expect("HOME is not set");
        Path::new(&home).join(".config/workitem-ui/config.toml")
    }

    /// Loads the config, creating it with a fresh token on first run.
    pub fn open(path: PathBuf) -> io::Result<Store> {
        let config = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let config = Config { token: new_token(), port: DEFAULT_PORT, projects: vec![] };
                save(&path, &config)?;
                config
            }
            Err(e) => return Err(e),
        };
        Ok(Store { path, config: Mutex::new(config) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A file next to the config file, for state the backend keeps.
    pub fn sibling(&self, file: &str) -> PathBuf {
        self.path.with_file_name(file)
    }

    pub fn token(&self) -> String {
        self.config.lock().unwrap().token.clone()
    }

    pub fn port(&self) -> u16 {
        self.config.lock().unwrap().port
    }

    pub fn projects(&self) -> Vec<Project> {
        self.config.lock().unwrap().projects.clone()
    }

    pub fn project(&self, name: &str) -> Option<Project> {
        self.projects().into_iter().find(|p| p.name == name)
    }

    pub fn rotate_token(&self) -> io::Result<String> {
        let mut config = self.config.lock().unwrap();
        let mut next = config.clone();
        next.token = new_token();
        save(&self.path, &next)?;
        *config = next;
        Ok(config.token.clone())
    }

    pub fn add_project(&self, name: &str, repo: &str) -> Result<Project, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err("name must be non-empty text".into());
        }
        let repo = Path::new(repo.trim());
        if !repo.is_absolute() || !repo.is_dir() {
            return Err(format!("{} is not an existing absolute directory", repo.display()));
        }
        let project = Project {
            name: name.to_string(),
            repo: repo.canonicalize().map_err(|e| e.to_string())?,
            agent: None,
        };
        let mut config = self.config.lock().unwrap();
        if config.projects.iter().any(|p| p.name == project.name) {
            return Err(format!("project '{name}' already exists"));
        }
        let mut next = config.clone();
        next.projects.push(project.clone());
        save(&self.path, &next).map_err(|e| e.to_string())?;
        *config = next;
        Ok(project)
    }

    /// Sets or, with an empty `agent`, clears the project's agent. `None` when there is no such project.
    pub fn set_agent(&self, name: &str, agent: &str) -> Result<Option<Project>, String> {
        let agent = agent.trim();
        if !agent.is_empty() && resolve_executable(agent).is_none() {
            return Err(format!("'{agent}' is not one executable on PATH or an absolute path to one"));
        }
        let mut config = self.config.lock().unwrap();
        let mut next = config.clone();
        let Some(project) = next.projects.iter_mut().find(|p| p.name == name) else {
            return Ok(None);
        };
        project.agent = (!agent.is_empty()).then(|| agent.to_string());
        let project = project.clone();
        save(&self.path, &next).map_err(|e| e.to_string())?;
        *config = next;
        Ok(Some(project))
    }

    /// Only drops the project from the config; the repo is untouched.
    pub fn remove_project(&self, name: &str) -> Result<bool, String> {
        let mut config = self.config.lock().unwrap();
        let mut next = config.clone();
        next.projects.retain(|p| p.name != name);
        if next.projects.len() == config.projects.len() {
            return Ok(false);
        }
        save(&self.path, &next).map_err(|e| e.to_string())?;
        *config = next;
        Ok(true)
    }
}

/// Absolute path of executable `name`: an absolute path as is, a bare name looked up on `PATH`.
/// `None` for anything else (relative paths, whitespace) or when it is not an executable file.
pub fn resolve_executable(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let executable = |path: &Path| fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return None;
    }
    if name.contains('/') {
        let path = PathBuf::from(name);
        return (path.is_absolute() && executable(&path)).then_some(path);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|path| path.is_absolute() && executable(path))
}

/// Writes atomically with mode 0600; the file holds the token.
fn save(path: &Path, config: &Config) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("toml.tmp");
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    file.write_all(text.as_bytes())?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_token_file_with_private_mode_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.toml");
        let store = Store::open(path.clone()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
        assert_eq!(store.token().len(), 64);
        assert_eq!(Store::open(path).unwrap().token(), store.token());
    }

    #[test]
    fn projects_persist_and_reject_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = Store::open(path.clone()).unwrap();
        let repo = dir.path().to_str().unwrap();
        store.add_project("app", repo).unwrap();
        assert!(store.add_project("app", repo).is_err());
        assert!(store.add_project("", repo).is_err());
        assert!(store.add_project("x", "relative/dir").is_err());
        assert!(store.add_project("y", "/no/such/dir").is_err());
        assert_eq!(Store::open(path.clone()).unwrap().projects().len(), 1);
        assert!(store.remove_project("app").unwrap());
        assert!(!store.remove_project("app").unwrap());
        assert!(Store::open(path).unwrap().projects().is_empty());
    }

    #[test]
    fn agent_persists_clears_and_rejects_non_executables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = Store::open(path.clone()).unwrap();
        store.add_project("app", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(store.set_agent("app", " /bin/sh ").unwrap().unwrap().agent.as_deref(), Some("/bin/sh"));
        assert_eq!(Store::open(path.clone()).unwrap().project("app").unwrap().agent.as_deref(), Some("/bin/sh"));
        assert!(store.set_agent("app", "sh").is_ok(), "a bare name is looked up on PATH");
        for bad in ["sh -c", "bin/sh", "/no/such/agent", "/etc/hosts"] {
            assert!(store.set_agent("app", bad).is_err(), "{bad}");
        }
        assert!(store.set_agent("nope", "sh").unwrap().is_none());
        assert_eq!(store.set_agent("app", "").unwrap().unwrap().agent, None);
        assert!(!fs::read_to_string(path).unwrap().contains("agent"));
    }

    #[test]
    fn rotate_changes_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("c.toml")).unwrap();
        let before = store.token();
        assert_ne!(store.rotate_token().unwrap(), before);
    }
}
