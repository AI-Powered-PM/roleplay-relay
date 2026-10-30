//! API key storage in Windows Credential Manager. The key never leaves this module
//! except as a `SecretString`; nothing here logs it or puts it in an error message.

use keyring::Entry;
use secrecy::{ExposeSecret, SecretString};

const SERVICE: &str = "WowChatHelper";
const USER: &str = "anthropic-api-key";
const MAX_KEY_LEN: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("That does not look like an API key")]
    Invalid,
    #[error("Could not access Windows Credential Manager")]
    Store,
}

fn entry() -> Result<Entry, SecretError> {
    Entry::new(SERVICE, USER).map_err(|_| SecretError::Store)
}

/// Reject empty, oversized, or whitespace/control-containing input (a pasted key is one token).
fn validate(raw: &str) -> Result<SecretString, SecretError> {
    let key = raw.trim();
    if key.is_empty()
        || key.len() > MAX_KEY_LEN
        || key.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(SecretError::Invalid);
    }
    Ok(SecretString::from(key.to_owned()))
}

pub fn has_api_key() -> Result<bool, SecretError> {
    match entry()?.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(_) => Err(SecretError::Store),
    }
}

/// For the Rust API client only. Never exposed as a command.
pub fn get_api_key() -> Result<Option<SecretString>, SecretError> {
    match entry()?.get_password() {
        Ok(k) => Ok(Some(SecretString::from(k))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(SecretError::Store),
    }
}

pub fn set_api_key(raw: String) -> Result<(), SecretError> {
    let key = validate(&raw)?;
    entry()?
        .set_password(key.expose_secret())
        .map_err(|_| SecretError::Store)
}

pub fn clear_api_key() -> Result<(), SecretError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(SecretError::Store),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_whitespace() {
        assert!(validate("").is_err());
        assert!(validate("   ").is_err());
        assert!(validate("sk-ant abc").is_err());
        assert!(validate("sk-ant-\nabc").is_err());
    }

    #[test]
    fn rejects_oversized() {
        assert!(validate(&"a".repeat(MAX_KEY_LEN + 1)).is_err());
    }

    #[test]
    fn trims_and_accepts_single_token() {
        let k = validate("  sk-ant-abc123  ").unwrap();
        assert_eq!(k.expose_secret(), "sk-ant-abc123");
    }

    #[test]
    fn error_messages_do_not_echo_input() {
        let msg = validate("bad key value").unwrap_err().to_string();
        assert!(!msg.contains("bad"));
    }
}
