//! Shell variables: the environment, `PATH`-like command search and prompt data.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Maximum number of variables.
pub const MAX_VARS: usize = 1024;
/// Maximum bytes in one value (a loop that doubles a string stops here).
pub const MAX_VALUE: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Var {
    value: String,
    exported: bool,
}

/// Variables of one shell. The working directory itself lives in the
/// [`crate::apps::shell::ShellFs`]; `PWD`/`OLDPWD` here only mirror it.
#[derive(Clone, Debug)]
pub struct Env {
    vars: BTreeMap<String, Var>,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

/// True for `[A-Za-z_][A-Za-z0-9_]*`.
pub fn valid_name(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    it.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Env {
    /// An environment with `PATH`, `HOME`, `USER`, `PWD` and `PS1` defaults.
    pub fn new() -> Self {
        let mut e = Self {
            vars: BTreeMap::new(),
        };
        e.set_exported("PATH", "/bin:/usr/bin");
        e.set_exported("HOME", "/");
        e.set_exported("USER", "user");
        e.set_exported("PWD", "/");
        e.set("PS1", "\\u@\\h:\\w\\$ ");
        e
    }

    /// An empty environment.
    pub fn empty() -> Self {
        Self {
            vars: BTreeMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(|v| v.value.as_str())
    }

    pub fn is_set(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    /// Set a variable. Returns false for an invalid name, a value over
    /// [`MAX_VALUE`] or when [`MAX_VARS`] is reached. Keeps the export flag.
    pub fn set(&mut self, name: &str, value: &str) -> bool {
        if !valid_name(name) || value.len() > MAX_VALUE {
            return false;
        }
        match self.vars.get_mut(name) {
            Some(v) => {
                v.value.clear();
                v.value.push_str(value);
                true
            }
            None => {
                if self.vars.len() >= MAX_VARS {
                    return false;
                }
                self.vars.insert(
                    name.to_string(),
                    Var {
                        value: value.to_string(),
                        exported: false,
                    },
                );
                true
            }
        }
    }

    /// Set and mark exported.
    pub fn set_exported(&mut self, name: &str, value: &str) -> bool {
        let ok = self.set(name, value);
        if ok {
            self.export(name);
        }
        ok
    }

    pub fn unset(&mut self, name: &str) -> bool {
        self.vars.remove(name).is_some()
    }

    /// Mark an existing variable exported (creating it empty if missing).
    pub fn export(&mut self, name: &str) -> bool {
        if !self.vars.contains_key(name) && !self.set(name, "") {
            return false;
        }
        if let Some(v) = self.vars.get_mut(name) {
            v.exported = true;
        }
        true
    }

    pub fn is_exported(&self, name: &str) -> bool {
        self.vars.get(name).is_some_and(|v| v.exported)
    }

    /// All variables in name order as `(name, value, exported)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, bool)> {
        self.vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.value.as_str(), v.exported))
    }

    pub fn len(&self) -> usize {
        self.vars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// The directories of `PATH` (empty entries skipped).
    pub fn path_dirs(&self) -> Vec<String> {
        self.get("PATH")
            .unwrap_or("")
            .split(':')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests;
