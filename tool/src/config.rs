#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn env_read_replace_write() {
        let path = std::env::temp_dir().join(format!("espctl-env-{}", std::process::id()));
        std::fs::write(&path, "# keep\nOTHER=abc=def\nESPCTL_TOKEN=old\nESPCTL_TOKEN=duplicate\nESPCTL_HOST=oldhost\n").unwrap();
        save_pair(&path, "0123456789abcdef0123456789abcdef", Some("1.2.3.4")).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("# keep\nOTHER=abc=def\n"));
        assert_eq!(content.matches("ESPCTL_TOKEN=").count(), 1);
        let values = read_env(&path).unwrap();
        assert_eq!(values["ESPCTL_HOST"], "1.2.3.4");
        assert_eq!(values["ESPCTL_TOKEN"], "0123456789abcdef0123456789abcdef");
        save_pair(&path, "new", None).unwrap();
        assert_eq!(read_env(&path).unwrap()["ESPCTL_HOST"], "1.2.3.4");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn precedence() {
        let dotenv = [
            ("ESPCTL_HOST".into(), "file".into()),
            ("ESPCTL_TOKEN".into(), "filetoken".into()),
        ]
        .into_iter()
        .collect();
        let env = |k: &str| match k {
            "ESPCTL_HOST" => Some("env".into()),
            "ESPCTL_TOKEN" => Some("envtoken".into()),
            _ => None,
        };
        assert_eq!(
            resolve(Some("flag".into()), &dotenv, env),
            Settings {
                host: "flag".into(),
                token: Some("envtoken".into())
            }
        );
        assert_eq!(resolve(None, &dotenv, env).host, "env");
        assert_eq!(
            resolve(None, &dotenv, |_| None),
            Settings {
                host: "file".into(),
                token: Some("filetoken".into())
            }
        );
        assert_eq!(
            resolve(None, &Default::default(), |_| None).host,
            "espc6.local"
        );
    }
}

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};
#[derive(Debug, PartialEq)]
pub struct Settings {
    pub host: String,
    pub token: Option<String>,
}
pub fn root_from(start: &Path) -> PathBuf {
    for dir in start.ancestors() {
        if dir.join("Cargo.toml").is_file() && dir.join("firmware/partitions.csv").is_file() {
            return dir.to_path_buf();
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}
pub fn read_env(path: &Path) -> io::Result<HashMap<String, String>> {
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    Ok(content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            Some((k.trim().to_owned(), v.trim().to_owned()))
        })
        .collect())
}
pub fn resolve(
    host: Option<String>,
    dotenv: &HashMap<String, String>,
    env: impl Fn(&str) -> Option<String>,
) -> Settings {
    Settings {
        host: host
            .or_else(|| env("ESPCTL_HOST"))
            .or_else(|| dotenv.get("ESPCTL_HOST").cloned())
            .unwrap_or_else(|| "espc6.local".into()),
        token: env("ESPCTL_TOKEN").or_else(|| dotenv.get("ESPCTL_TOKEN").cloned()),
    }
}
pub fn save_pair(path: &Path, token: &str, ip: Option<&str>) -> io::Result<()> {
    let old = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut lines: Vec<String> = old
        .lines()
        .filter(|line| {
            let key = line.split_once('=').map(|(k, _)| k.trim());
            key != Some("ESPCTL_TOKEN") && !(ip.is_some() && key == Some("ESPCTL_HOST"))
        })
        .map(str::to_owned)
        .collect();
    lines.push(format!("ESPCTL_TOKEN={token}"));
    if let Some(ip) = ip {
        lines.push(format!("ESPCTL_HOST={ip}"));
    }
    let content = lines.join("\n") + "\n";
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(content.as_bytes())
}
