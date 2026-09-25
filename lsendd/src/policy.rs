use clap::ValueEnum;
use std::io::ErrorKind;
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
        Ok(text) => text
            .lines()
            .any(|line| line.trim().eq_ignore_ascii_case(fingerprint)),
        Err(err) if err.kind() == ErrorKind::NotFound => false,
        Err(err) => {
            eprintln!("Could not read {}: {err}", path.display());
            false
        }
    }
}
