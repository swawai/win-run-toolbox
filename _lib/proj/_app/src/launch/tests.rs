use super::*;
use std::collections::HashMap;
use std::os::windows::ffi::OsStringExt;

fn request(
    direct_argv: &[&str],
    launch_declarations: &[(&str, &str)],
) -> Result<LaunchRequest, LaunchError> {
    let mut declarations: HashMap<String, OsString> = HashMap::from([
        (
            LAUNCH_PROTOCOL_ENV.to_owned(),
            OsString::from(LAUNCH_PROTOCOL_VERSION),
        ),
        (
            LAUNCH_MODE_ENV.to_owned(),
            OsString::from(LaunchMode::Cli.as_env_value()),
        ),
    ]);
    declarations.extend(
        launch_declarations
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value))),
    );

    LaunchRequest::from_sources(
        direct_argv.iter().map(OsString::from),
        PathBuf::from(r"C:\work\project"),
        |name| declarations.get(name).cloned(),
    )
}

#[test]
fn native_launcher_arguments_are_preserved() {
    let result = request(
        &[".demo", "", "a&b|c", "带 空格"],
        &[(ENTRY_FILE_ENV, r"C:\swaw\项目.exe")],
    )
    .unwrap();

    assert_eq!(result.mode, LaunchMode::Cli);
    assert_eq!(
        result.argv,
        [".demo", "", "a&b|c", "带 空格"].map(OsString::from)
    );
    assert_eq!(result.invocation_dir, PathBuf::from(r"C:\work\project"));
}

#[test]
fn internal_host_mode_is_transport_metadata_not_a_user_argument() {
    let result = request(
        &["--swawkit-internal-host", ".demo"],
        &[
            (ENTRY_FILE_ENV, r"C:\swaw\project.exe"),
            (LAUNCH_MODE_ENV, "internal-host"),
        ],
    )
    .unwrap();

    assert_eq!(result.mode, LaunchMode::InternalHost);
    assert_eq!(
        result.argv,
        ["--swawkit-internal-host", ".demo"].map(OsString::from)
    );
}

#[test]
fn explicit_cli_mode_is_accepted() {
    let result = request(
        &[],
        &[
            (ENTRY_FILE_ENV, r"C:\swaw\project.exe"),
            (LAUNCH_MODE_ENV, "cli"),
        ],
    )
    .unwrap();

    assert_eq!(result.mode, LaunchMode::Cli);
}

#[test]
fn worker_mode_is_explicit_launcher_transport() {
    let result = request(
        &[".demo"],
        &[
            (ENTRY_FILE_ENV, r"C:\swaw\project.exe"),
            (LAUNCH_MODE_ENV, "worker"),
        ],
    )
    .unwrap();

    assert_eq!(result.mode, LaunchMode::Worker);
    assert_eq!(result.argv, [".demo"].map(OsString::from));
}

#[test]
fn current_native_launcher_protocol_is_required() {
    let missing = LaunchRequest::from_sources([], PathBuf::from(r"C:\work\project"), |_name| None)
        .unwrap_err();
    assert!(missing.to_string().contains("rebuild or replace"));

    let outdated = request(
        &[],
        &[
            (LAUNCH_PROTOCOL_ENV, "2"),
            (ENTRY_FILE_ENV, r"C:\swaw\project.exe"),
        ],
    )
    .unwrap_err();
    assert!(outdated.to_string().contains("expected '3'"));
}

#[test]
fn launch_mode_is_required_by_the_current_protocol() {
    let declarations = HashMap::from([
        (
            LAUNCH_PROTOCOL_ENV.to_owned(),
            OsString::from(LAUNCH_PROTOCOL_VERSION),
        ),
        (
            ENTRY_FILE_ENV.to_owned(),
            OsString::from(r"C:\swaw\project.exe"),
        ),
    ]);

    let error = LaunchRequest::from_sources([], PathBuf::from(r"C:\work\project"), |name| {
        declarations.get(name).cloned()
    })
    .unwrap_err();

    assert!(error.to_string().contains(LAUNCH_MODE_ENV));
    assert!(error.to_string().contains("rebuild or replace"));
}

#[test]
fn native_launcher_must_consume_worker_declarations() {
    let error = request(
        &[],
        &[
            (ENTRY_FILE_ENV, r"C:\swaw\project.exe"),
            (WORKER_PROTOCOL_ENV, WORKER_PROTOCOL_VERSION),
        ],
    )
    .unwrap_err();

    assert!(error.to_string().contains(WORKER_PROTOCOL_ENV));
    assert!(error.to_string().contains("did not consume"));
}

#[test]
fn unknown_launch_mode_fails_closed() {
    let error = request(
        &[],
        &[
            (ENTRY_FILE_ENV, r"C:\swaw\project.exe"),
            (LAUNCH_MODE_ENV, "daemon"),
        ],
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("expected 'cli', 'worker', or 'internal-host'")
    );
}

#[test]
fn entry_file_is_required_and_absolute() {
    let missing = request(&[], &[]).unwrap_err();
    assert!(missing.to_string().contains(ENTRY_FILE_ENV));

    let relative = request(&[], &[(ENTRY_FILE_ENV, "project.exe")]).unwrap_err();
    assert!(relative.to_string().contains("must be absolute"));
}

#[test]
fn swawkit_environment_ownership_is_exact_and_case_insensitive() {
    for owned_name in [
        "SWAWKIT_HOME",
        "swawkit_home",
        "SWAWKIT_PROJ_",
        "swawkit_proj_unknown",
        "SwAwKiT_PrOj_Core_Command_Protocol",
    ] {
        assert!(
            is_swawkit_environment_name(OsStr::new(owned_name)),
            "expected an owned environment name: {owned_name}"
        );
    }

    for foreign_name in [
        "SWAWKIT",
        "SWAWKIT_HOME_EXTRA",
        "SWAWKIT_PROJECT",
        "XSWAWKIT_PROJ_UNKNOWN",
    ] {
        assert!(
            !is_swawkit_environment_name(OsStr::new(foreign_name)),
            "expected a foreign environment name: {foreign_name}"
        );
    }

    let mut invalid_utf16_name = "SWAWKIT_PROJ_".encode_utf16().collect::<Vec<_>>();
    invalid_utf16_name.push(0xd800);
    assert!(is_swawkit_environment_name(&OsString::from_wide(
        &invalid_utf16_name
    )));
}
