use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("swawkit-catalog-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&root).expect("create fixture root");
        Self { root }
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(&path).expect("create fixture directory");
        path
    }

    fn file(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture file parent"))
            .expect("create fixture file parent");
        fs::write(path, text).expect("write fixture file");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn discovers_control_kernel_and_action_hierarchies() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");

    fixture.file("home/_lib/proj/run.ps1", "");
    fixture.file(
        "home/_lib/proj/..entry/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.profile"}"#,
    );
    fixture.file(
        "home/_lib/proj/..entry/env/preferences/SWAWKIT_PROJ_DEFAULT_SHELL/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.profile.set"}"#,
    );
    fixture.file(
        "home/_lib/proj/..entry/env/_view/web.json",
        r#"{"schema":"swawkit.command-view/web/v1","childrenColumn":{"width":"normal"}}"#,
    );
    fixture.file(
        "home/_lib/proj/..entry/env/preferences/_view/web.json",
        r#"{"schema":"swawkit.command-view/web/v1","childrenColumn":{"width":"wide"}}"#,
    );
    fixture.file(
        "home/_lib/proj/..entry/claim/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.claim"}"#,
    );
    fixture.file("home/_lib/proj/.dev/run.ps1", "");
    fixture.file("home/_lib/proj/.dev/setup/run.cmd", "");
    fixture.file("home/_lib/proj/.help/run.ps1", "");
    fixture.file("home/_lib/proj/.h/run.ps1", "");
    fixture.file("home/_lib/proj/-con/run.ps1", "");
    fixture.file("home/_lib/proj/--nul/run.ps1", "");
    fixture.file(
        "home/_lib/proj/.info/_help/zh-CN.txt",
        "\n  Inspect {{COMMAND}}.  \nUse {{INVOCATION}}.",
    );
    fixture.file("home/_lib/proj/_private/run.ps1", "");
    fixture.file("home/_lib/proj/ordinary/run.ps1", "");
    fixture.file("home/_lib/proj/.Bad/run.ps1", "");
    fixture.file("home/_lib/proj/...invalid/run.core.json", "{}");

    fixture.file("project/.swaw/build/host/run.exe", "");
    fixture.file(
        "project/.swaw/build/host/_help/zh-CN.txt",
        "Build at {{ADDRESS}}\n{{INVOCATION}}",
    );
    fixture.file("project/.swaw/_private/run.ps1", "");
    fixture.file("project/.swaw/Bad/run.ps1", "");

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let addresses: Vec<(CommandSource, &str)> = snapshot
        .commands
        .iter()
        .map(|node| (node.source, node.address.as_str()))
        .collect();

    assert_eq!(
        addresses,
        [
            (CommandSource::Control, "..entry"),
            (CommandSource::Control, "..entry.claim"),
            (CommandSource::Control, "..entry.env"),
            (CommandSource::Control, "..entry.env.preferences"),
            (
                CommandSource::Control,
                "..entry.env.preferences.SWAWKIT_PROJ_DEFAULT_SHELL",
            ),
            (CommandSource::Kernel, ""),
            (CommandSource::Kernel, "--nul"),
            (CommandSource::Kernel, "-con"),
            (CommandSource::Kernel, ".dev"),
            (CommandSource::Kernel, ".dev.setup"),
            (CommandSource::Kernel, ".h"),
            (CommandSource::Kernel, ".help"),
            (CommandSource::Kernel, ".info"),
            (CommandSource::Action, "build"),
            (CommandSource::Action, "build.host"),
        ]
    );

    let entry = node(&snapshot, CommandSource::Control, "..entry");
    assert_eq!(entry.parent.as_deref(), Some(""));
    assert_eq!(entry.adapter.as_deref(), Some("core"));
    assert_eq!(entry.handler.as_deref(), Some("entry.profile"));
    let env = node(&snapshot, CommandSource::Control, "..entry.env");
    assert_eq!(env.parent.as_deref(), Some("..entry"));
    assert!(!env.runnable);
    assert_eq!(
        env.view.as_ref().map(|view| view.children_column.width),
        Some(ColumnWidth::Normal)
    );
    let preferences = node(&snapshot, CommandSource::Control, "..entry.env.preferences");
    assert_eq!(preferences.parent.as_deref(), Some("..entry.env"));
    assert_eq!(
        preferences
            .view
            .as_ref()
            .map(|view| view.children_column.width),
        Some(ColumnWidth::Wide)
    );
    let default_shell = node(
        &snapshot,
        CommandSource::Control,
        "..entry.env.preferences.SWAWKIT_PROJ_DEFAULT_SHELL",
    );
    assert_eq!(
        default_shell.parent.as_deref(),
        Some("..entry.env.preferences")
    );
    assert_eq!(default_shell.handler.as_deref(), Some("entry.profile.set"));
    let claim = node(&snapshot, CommandSource::Control, "..entry.claim");
    assert_eq!(claim.handler.as_deref(), Some("entry.claim"));

    let setup = node(&snapshot, CommandSource::Kernel, ".dev.setup");
    assert_eq!(setup.parent.as_deref(), Some(".dev"));
    assert_eq!(setup.entry.as_deref(), Some("run.cmd"));
    assert_eq!(setup.adapter.as_deref(), Some("cmd"));

    let info = node(&snapshot, CommandSource::Kernel, ".info");
    let help = info.help.as_ref().expect("info help");
    assert_eq!(help.summary, "Inspect fixture.");
    assert!(help.text.contains("Use fixture .info."));

    let help_alias = node(&snapshot, CommandSource::Kernel, ".h");
    assert_eq!(help_alias.alias_of.as_deref(), Some(".help"));

    let build = node(&snapshot, CommandSource::Action, "build");
    assert_eq!(build.parent.as_deref(), Some(""));
    assert!(!build.runnable);

    let host = node(&snapshot, CommandSource::Action, "build.host");
    assert_eq!(host.parent.as_deref(), Some("build"));
    assert_eq!(
        host.help.as_ref().map(|help| help.summary.as_str()),
        Some("Build at build.host")
    );
}

#[test]
fn entry_env_directory_modules_match_the_profile_variable_registry() {
    let kernel = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Proj kernel root");
    let snapshot = CatalogSnapshot::discover_optional_roots(kernel, None, "swawkit")
        .expect("source-tree Catalog");
    let setters = snapshot
        .commands
        .iter()
        .filter(|command| command.handler.as_deref() == Some("entry.profile.set"))
        .collect::<Vec<_>>();
    let actual = setters
        .iter()
        .map(|command| command.address.as_str())
        .collect::<Vec<_>>();
    let expected = crate::profile::EntryProfileRecord::environment_variable_commands()
        .into_iter()
        .map(|(group, name)| format!("..entry.env.{group}.{name}"))
        .collect::<Vec<_>>();

    assert_eq!(actual, expected);
    for command in setters {
        let help = command.help.as_ref().unwrap_or_else(|| {
            panic!("Profile setter {} must provide local Help", command.address)
        });
        assert!(!help.summary.trim().is_empty());
        assert!(
            help.text
                .contains(&format!("swawkit {} <value>", command.address)),
            "Profile setter {} must document its CLI value argument",
            command.address
        );
    }
}

#[test]
fn reports_an_invalid_parent_owned_web_view_without_stopping_discovery() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/run.ps1", "");
    fixture.file(
        "home/_lib/proj/.broken/_view/web.json",
        r#"{"schema":"swawkit.command-view/web/v1","childrenColumn":{"width":"480px"}}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let broken = node(&snapshot, CommandSource::Kernel, ".broken");

    assert!(broken.view.is_none());
    assert!(
        broken
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("unknown variant `480px`"))
    );
}

#[test]
fn restricts_owned_entries_to_their_catalog_sources() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file(
        "home/_lib/proj/.wrong/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.profile"}"#,
    );
    fixture.file("home/_lib/proj/..external/run.ps1", "");
    fixture.file(
        "home/_lib/proj/..unknown/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"dynamic.invoke"}"#,
    );
    fixture.file(
        "home/_lib/proj/.status/run.toolchain.json",
        r#"{"schema":"swawkit.toolchain-command/v1","handler":"dev.status"}"#,
    );
    fixture.file(
        "home/_lib/proj/.unknown-toolchain/run.toolchain.json",
        r#"{"schema":"swawkit.toolchain-command/v1","handler":"dev.install"}"#,
    );
    fixture.file(
        "project/.swaw/build/run.toolchain.json",
        r#"{"schema":"swawkit.toolchain-command/v1","handler":"dev.status"}"#,
    );
    fixture.file("home/_lib/proj/.bun/run.ts", "");

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let status = node(&snapshot, CommandSource::Kernel, ".status");
    assert!(status.runnable);
    assert_eq!(status.adapter.as_deref(), Some("toolchain"));
    assert_eq!(status.handler.as_deref(), Some("dev.status"));
    for (source, address, expected) in [
        (
            CommandSource::Kernel,
            ".wrong",
            "restricted to Control Plane",
        ),
        (
            CommandSource::Control,
            "..external",
            "must use a run.core.json",
        ),
        (
            CommandSource::Control,
            "..unknown",
            "unsupported Core command handler",
        ),
        (
            CommandSource::Kernel,
            ".unknown-toolchain",
            "unsupported Toolchain command handler",
        ),
        (
            CommandSource::Action,
            "build",
            "restricted to Kernel commands",
        ),
        (
            CommandSource::Kernel,
            ".bun",
            "restricted to project Action commands",
        ),
    ] {
        let command = node(&snapshot, source, address);
        assert!(!command.runnable);
        assert!(
            command
                .diagnostic
                .as_deref()
                .is_some_and(|diagnostic| diagnostic.contains(expected)),
            "unexpected diagnostic for {address}: {:?}",
            command.diagnostic
        );
    }
}

#[test]
fn reports_multiple_and_non_canonical_run_entries_without_stopping_discovery() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.multi/run.ps1", "");
    fixture.file("home/_lib/proj/.multi/run.cmd", "");
    fixture.file("home/_lib/proj/.case/RUN.PS1", "");
    fixture.file("home/_lib/proj/.ok/run.exe", "");

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let multiple = node(&snapshot, CommandSource::Kernel, ".multi");
    assert!(!multiple.runnable);
    assert!(
        multiple
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("multiple run entries"))
    );

    let non_canonical = node(&snapshot, CommandSource::Kernel, ".case");
    assert!(!non_canonical.runnable);
    assert!(
        non_canonical
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("non-canonical entry name"))
    );

    assert!(node(&snapshot, CommandSource::Kernel, ".ok").runnable);
}

#[test]
fn keeps_invalid_help_distinct_from_absent_help() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.invalid/_help/zh-CN.txt", "\n  \n");
    fixture.directory("home/_lib/proj/.absent");

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let invalid = node(&snapshot, CommandSource::Kernel, ".invalid");
    assert!(invalid.help.is_none());
    assert!(
        invalid
            .help_diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("help file is empty"))
    );
    assert!(
        invalid
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("help file is empty"))
    );

    let absent = node(&snapshot, CommandSource::Kernel, ".absent");
    assert!(absent.help.is_none());
    assert!(absent.help_diagnostic.is_none());
}

fn node<'a>(
    snapshot: &'a CatalogSnapshot,
    source: CommandSource,
    address: &str,
) -> &'a CommandNode {
    snapshot
        .commands
        .iter()
        .find(|node| node.source == source && node.address == address)
        .unwrap_or_else(|| panic!("missing node {source:?} {address}"))
}
