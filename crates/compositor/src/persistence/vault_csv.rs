// Import login CSV exports (Chrome / Bitwarden) into the local vault.

use super::vault::{self, VaultEntry, VaultError, VaultSession};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
}

impl std::fmt::Display for ImportStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} added, {} updated, {} skipped",
            self.added, self.updated, self.skipped
        )
    }
}

pub fn expand_user_path(path: &str) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        return PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest);
    }
    PathBuf::from(path)
}

pub fn import_path(session: &mut VaultSession, path: &str) -> Result<ImportStats, String> {
    let path = expand_user_path(path);
    if path.as_os_str().is_empty() {
        return Err("empty path".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| "CSV is not UTF-8".to_string())?;
    let entries = parse_login_csv(&text).map_err(|e| e.to_string())?;
    if entries.is_empty() {
        return Err("no login rows found (Chrome or Bitwarden CSV?)".into());
    }
    session
        .import_entries(entries)
        .map_err(|e| e.to_string())
}

/// Parse Chrome (`name,url,username,password`) or Bitwarden
/// (`login_uri,login_username,login_password`) password CSV.
pub fn parse_login_csv(text: &str) -> Result<Vec<VaultEntry>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::Headers)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| format!("CSV headers: {e}"))?
        .clone();
    let header_map: HashMap<String, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_ascii_lowercase(), i))
        .collect();

    let chrome = header_map.contains_key("url")
        && header_map.contains_key("username")
        && header_map.contains_key("password");
    let bitwarden = header_map.contains_key("login_uri")
        && header_map.contains_key("login_username")
        && header_map.contains_key("login_password");

    if !chrome && !bitwarden {
        return Err(
            "unrecognized CSV — need Chrome (url,username,password) or Bitwarden (login_uri,…)"
                .into(),
        );
    }

    let col = |name: &str| header_map.get(name).copied();
    let (url_i, user_i, pass_i, notes_i, type_i) = if chrome {
        (
            col("url").unwrap(),
            col("username").unwrap(),
            col("password").unwrap(),
            col("note").or_else(|| col("notes")),
            None,
        )
    } else {
        (
            col("login_uri").unwrap(),
            col("login_username").unwrap(),
            col("login_password").unwrap(),
            col("notes"),
            col("type"),
        )
    };

    let mut out = Vec::new();
    for (row_idx, record) in reader.records().enumerate() {
        let record = record.map_err(|e| format!("CSV row {}: {e}", row_idx + 2))?;
        if let Some(ti) = type_i {
            let ty = record.get(ti).unwrap_or("").trim();
            if !ty.is_empty() && !ty.eq_ignore_ascii_case("login") {
                continue;
            }
        }
        let url = record.get(url_i).unwrap_or("").trim();
        let username = record.get(user_i).unwrap_or("").trim();
        let password = record.get(pass_i).unwrap_or("").trim();
        if url.is_empty() || password.is_empty() {
            continue;
        }
        // Bitwarden can put multiple URIs separated by commas / newlines.
        let first_url = url
            .split(['\n', ',', ' '])
            .map(str::trim)
            .find(|s| !s.is_empty())
            .unwrap_or(url);
        let notes = notes_i
            .and_then(|i| record.get(i))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        out.push(VaultEntry {
            id: String::new(),
            origin: vault::normalize_origin(first_url),
            username: username.to_string(),
            password: password.to_string(),
            email: None,
            address_line1: None,
            city: None,
            postal_code: None,
            country: None,
            notes,
            updated_at: vault::now_unix(),
        });
    }
    Ok(out)
}

impl VaultSession {
    /// Merge entries keyed by (origin, username). Saves once at the end.
    pub fn import_entries(&mut self, entries: Vec<VaultEntry>) -> Result<ImportStats, VaultError> {
        let mut stats = ImportStats::default();
        let now = vault::now_unix();
        for mut entry in entries {
            entry.origin = vault::normalize_origin(&entry.origin);
            if entry.origin.is_empty() || entry.password.is_empty() {
                stats.skipped += 1;
                continue;
            }
            if let Some(existing) = self.data.entries.iter_mut().find(|e| {
                vault::host_key(&e.origin) == vault::host_key(&entry.origin)
                    && e.username == entry.username
            }) {
                if existing.password == entry.password
                    && existing.notes == entry.notes
                {
                    stats.skipped += 1;
                    continue;
                }
                existing.password = entry.password;
                if entry.notes.is_some() {
                    existing.notes = entry.notes;
                }
                existing.updated_at = now;
                stats.updated += 1;
            } else {
                if entry.id.is_empty() {
                    entry.id = new_import_id();
                }
                entry.updated_at = now;
                self.data.entries.push(entry);
                stats.added += 1;
            }
        }
        if stats.added > 0 || stats.updated > 0 {
            self.save()?;
        }
        Ok(stats)
    }
}

fn new_import_id() -> String {
    // Same shape as vault::new_id (private there).
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chrome_csv() {
        let csv = "name,url,username,password\n\
GitHub,https://github.com/login,octocat,s3cret\n\
Empty,https://example.com,,\n";
        let entries = parse_login_csv(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin, "https://github.com");
        assert_eq!(entries[0].username, "octocat");
        assert_eq!(entries[0].password, "s3cret");
    }

    #[test]
    fn parse_bitwarden_csv() {
        let csv = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
,,login,GH,,,0,https://github.com,octocat,pw,\n\
,,note,Memo,,,,,,,\n";
        let entries = parse_login_csv(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin, "https://github.com");
        assert_eq!(entries[0].username, "octocat");
        assert_eq!(entries[0].password, "pw");
    }

    #[test]
    fn quoted_password_with_comma() {
        let csv = "url,username,password\nhttps://a.com,u,\"a,b,c\"\n";
        let entries = parse_login_csv(csv).unwrap();
        assert_eq!(entries[0].password, "a,b,c");
    }
}
