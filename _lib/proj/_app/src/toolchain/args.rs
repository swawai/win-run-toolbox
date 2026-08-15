use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

pub(crate) enum Operation {
    Command {
        handler: String,
        arguments: Vec<OsString>,
    },
    Download {
        controlled_root: PathBuf,
        source: OsString,
        destination: PathBuf,
        progress_id: String,
    },
    ZipTest {
        archive: PathBuf,
    },
    ZipExtract {
        controlled_root: PathBuf,
        archive: PathBuf,
        destination: PathBuf,
    },
    RuntimeCleanup {
        swawkit_home: PathBuf,
        apply: bool,
        format: RuntimeCleanupFormat,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCleanupFormat {
    Text,
    Json,
}

pub(crate) fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Operation, String> {
    let values: Vec<OsString> = arguments.collect();
    let Some(operation) = values.first().and_then(|value| value.to_str()) else {
        return Err(usage());
    };
    match operation {
        "command-v1" if values.len() >= 2 => {
            let handler = unicode(&values[1], "command handler")?.to_owned();
            if handler.is_empty()
                || handler.len() > 128
                || handler.trim() != handler
                || !handler.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'.' | b'-')
                })
            {
                return Err("command-v1 handler is invalid".to_owned());
            }
            Ok(Operation::Command {
                handler,
                arguments: values.into_iter().skip(2).collect(),
            })
        }
        "download-v1" if values.len() == 5 => {
            let progress_id = unicode(&values[4], "progress ID")?.to_owned();
            if progress_id.is_empty()
                || progress_id.len() > 128
                || !progress_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
            {
                return Err("download-v1 progress ID is invalid".to_owned());
            }
            Ok(Operation::Download {
                controlled_root: PathBuf::from(&values[1]),
                source: values[2].clone(),
                destination: PathBuf::from(&values[3]),
                progress_id,
            })
        }
        "zip-test-v1" if values.len() == 2 => Ok(Operation::ZipTest {
            archive: PathBuf::from(&values[1]),
        }),
        "zip-extract-v1" if values.len() == 4 => Ok(Operation::ZipExtract {
            controlled_root: PathBuf::from(&values[1]),
            archive: PathBuf::from(&values[2]),
            destination: PathBuf::from(&values[3]),
        }),
        "runtime-cleanup-v1" if values.len() == 4 => {
            let apply = match unicode(&values[2], "Runtime cleanup action")? {
                "preview" => false,
                "apply" => true,
                _ => return Err("Runtime cleanup action must be preview or apply".to_owned()),
            };
            let format = match unicode(&values[3], "Runtime cleanup format")? {
                "text" => RuntimeCleanupFormat::Text,
                "json" => RuntimeCleanupFormat::Json,
                _ => return Err("Runtime cleanup format must be text or json".to_owned()),
            };
            Ok(Operation::RuntimeCleanup {
                swawkit_home: PathBuf::from(&values[1]),
                apply,
                format,
            })
        }
        _ => Err(usage()),
    }
}

fn unicode<'a>(value: &'a OsStr, label: &str) -> Result<&'a str, String> {
    value
        .to_str()
        .ok_or_else(|| format!("{label} must be valid Unicode"))
}

fn usage() -> String {
    "expected one exact Toolchain V1 operation:\n  command-v1 <handler> [arguments...]\n  download-v1 <controlled-root> <source> \
     <destination> <progress-id>\n  zip-test-v1 <archive>\n  zip-extract-v1 \
     <controlled-root> <archive> <destination>\n  runtime-cleanup-v1 \
     <swawkit-home> <preview|apply> <text|json>"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_only_exact_versioned_operations() {
        let operation = parse(
            [
                "download-v1",
                "C:\\root",
                "https://example.test/a.zip",
                "C:\\root\\a.zip",
                "download:a",
            ]
            .into_iter()
            .map(OsString::from),
        );
        assert!(matches!(operation, Ok(Operation::Download { .. })));
        assert!(matches!(
            parse(["command-v1", "dev.status"].into_iter().map(OsString::from)),
            Ok(Operation::Command { .. })
        ));
        assert!(parse(["command-v1", "../status"].into_iter().map(OsString::from)).is_err());
        assert!(parse([OsString::from("download")].into_iter()).is_err());
        assert!(matches!(
            parse(
                ["runtime-cleanup-v1", "C:\\home", "preview", "json"]
                    .into_iter()
                    .map(OsString::from)
            ),
            Ok(Operation::RuntimeCleanup {
                apply: false,
                format: RuntimeCleanupFormat::Json,
                ..
            })
        ));
        assert!(
            parse(
                ["runtime-cleanup-v1", "C:\\home", "delete", "json"]
                    .into_iter()
                    .map(OsString::from)
            )
            .is_err()
        );
        assert!(
            parse(
                ["download-v1", "r", "s", "d", "bad id"]
                    .into_iter()
                    .map(OsString::from)
            )
            .is_err()
        );
    }
}
