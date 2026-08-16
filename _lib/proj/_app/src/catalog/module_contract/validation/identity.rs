use std::io;
use std::path::Path;

use crate::catalog::{invalid_data, module_contract::declaration::LocalizedText};

pub(super) fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || (index > 0 && (byte.is_ascii_digit() || byte == b'-'))
        })
}

pub(super) fn validate_text(value: &str, max: usize, field: &str, path: &Path) -> io::Result<()> {
    if value.is_empty() || value.trim() != value || value.chars().count() > max {
        return invalid_data(format!(
            "module {field} in '{}' must contain 1 to {max} trimmed characters",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn validate_localized_text(
    value: &LocalizedText,
    max: usize,
    field: &str,
    path: &Path,
) -> io::Result<()> {
    validate_text(&value.zh_cn, max, &format!("{field}.zh-CN"), path)?;
    validate_text(&value.en, max, &format!("{field}.en"), path)
}

pub(super) fn validate_contract(contract: &str, path: &Path) -> io::Result<()> {
    let valid = (1..=128).contains(&contract.len())
        && contract
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && contract.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'/' | b'-')
        });
    if valid {
        Ok(())
    } else {
        invalid_data(format!(
            "invalid module producer contract '{contract}' in '{}'",
            path.display()
        ))
    }
}

pub(super) fn valid_provider_address(address: &str) -> bool {
    let value = match address.strip_prefix('.') {
        Some(value) if !value.starts_with('.') => value,
        Some(_) => return false,
        None => address,
    };
    !value.is_empty() && value.split('.').all(valid_segment)
}

fn valid_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_addresses_are_intentionally_narrow() {
        for valid in [".dev.setup", ".dev.rust.setup", "proj.build.app"] {
            assert!(valid_provider_address(valid), "{valid}");
        }
        for invalid in ["", ".", "..entry", "Dev.setup", ".dev..setup"] {
            assert!(!valid_provider_address(invalid), "{invalid}");
        }
    }
}
