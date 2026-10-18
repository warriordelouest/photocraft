//! API keys in the OS credential store (Windows Credential Manager, macOS Keychain, Linux kernel
//! keyring), never in the configuration file.

const SERVICE: &str = "PhotoCraft Assistant";

#[cfg(not(target_arch = "wasm32"))]
fn entry(key_id: &str) -> crate::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key_id).map_err(|e| crate::Error::Secrets(e.to_string()))
}

/// The key stored for `key_id`; `Ok(None)` when there is none.
#[cfg(not(target_arch = "wasm32"))]
pub fn get(key_id: &str) -> crate::Result<Option<String>> {
    match entry(key_id)?.get_password() {
        Ok(k) if !k.trim().is_empty() => Ok(Some(k)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(crate::Error::Secrets(e.to_string())),
    }
}

/// Stores `key` for `key_id`; an empty key removes it.
#[cfg(not(target_arch = "wasm32"))]
pub fn set(key_id: &str, key: &str) -> crate::Result<()> {
    let key = key.trim();
    if key.is_empty() {
        return remove(key_id);
    }
    entry(key_id)?.set_password(key).map_err(|e| crate::Error::Secrets(e.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn remove(key_id: &str) -> crate::Result<()> {
    match entry(key_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(crate::Error::Secrets(e.to_string())),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn get(_key_id: &str) -> crate::Result<Option<String>> {
    Err(crate::Error::Unsupported("credential store"))
}

#[cfg(target_arch = "wasm32")]
pub fn set(_key_id: &str, _key: &str) -> crate::Result<()> {
    Err(crate::Error::Unsupported("credential store"))
}

#[cfg(target_arch = "wasm32")]
pub fn remove(_key_id: &str) -> crate::Result<()> {
    Err(crate::Error::Unsupported("credential store"))
}

/// A key shown in the settings: its first and last characters only.
pub fn masked(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "•".repeat(chars.len());
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_hides_the_middle_and_short_keys_entirely() {
        assert_eq!(masked("sk-1234567890abcd"), "sk-1…abcd");
        assert_eq!(masked("short"), "•••••");
        assert_eq!(masked(""), "");
    }
}
