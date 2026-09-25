use clap::ValueEnum;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

/// Which incoming requests are accepted without asking.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AcceptPolicy {
    /// Every sender.
    All,
    /// Senders whose fingerprint is listed in the `known` file.
    Known,
    /// No sender.
    None,
}

impl AcceptPolicy {
    pub fn auto_accepts(self, fingerprint: &str) -> bool {
        match self {
            Self::All => true,
            Self::Known => is_known(fingerprint),
            Self::None => false,
        }
    }
}

/// `$XDG_CONFIG_HOME/lsend/known`: one fingerprint per line.
fn known_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("lsend").join("known"))
}

/// Read on every request, so edits take effect without a restart.
fn is_known(fingerprint: &str) -> bool {
    let Some(path) = known_path() else {
        return false;
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => lists(&text, fingerprint),
        Err(err) if err.kind() == ErrorKind::NotFound => false,
        Err(err) => {
            eprintln!("Could not read {}: {err}", path.display());
            false
        }
    }
}

fn lists(text: &str, fingerprint: &str) -> bool {
    text.lines()
        .any(|line| line.trim().eq_ignore_ascii_case(fingerprint))
}

/// A SHA-256 certificate fingerprint: 64 hex digits.
pub fn is_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Appends `fingerprint` to the `known` file, unless it is already listed.
pub fn trust(fingerprint: &str) -> std::io::Result<()> {
    let path = known_path().ok_or_else(|| std::io::Error::other("No config directory"))?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };
    if lists(&text, fingerprint) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A hand-edited file may lack the final newline.
    let separator = match text.is_empty() || text.ends_with('\n') {
        true => "",
        false => "\n",
    };
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{separator}{fingerprint}")
}
