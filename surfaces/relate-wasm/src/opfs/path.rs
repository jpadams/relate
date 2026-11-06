//! Virtual paths inside one OPFS database. Directories are prefixes.

use std::collections::BTreeMap;

use relate::Error;

pub(crate) fn list_names(entries: &BTreeMap<String, u32>, path: &str) -> Vec<String> {
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}/")
    };
    let mut names = Vec::new();
    for key in entries.keys() {
        let rest = if prefix.is_empty() {
            key.as_str()
        } else if let Some(rest) = key.strip_prefix(&prefix) {
            rest
        } else {
            continue;
        };
        let name = rest.split('/').next().unwrap_or(rest);
        if !name.is_empty() && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    }
    names
}

pub(crate) fn is_prefix(entries: &BTreeMap<String, u32>, path: &str) -> bool {
    let prefix = format!("{path}/");
    entries.keys().any(|key| key.starts_with(&prefix))
}

pub(crate) fn check_path(path: &str) -> relate::Result<()> {
    let ok = !path.is_empty()
        && !path.contains('\0')
        && !path.starts_with('/')
        && !path.ends_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if ok {
        Ok(())
    } else {
        Err(Error::storage(format!("invalid OPFS path {path}")))
    }
}
