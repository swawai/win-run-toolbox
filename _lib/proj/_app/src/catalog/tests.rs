use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

mod facets;
mod module_contracts;
mod subject_kinds;

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
        "home/_lib/proj/..entry/language/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.profile.set"}"#,
    );
    fixture.file(
        "home/_lib/proj/..entry/_view/web.json",
        r#"{"schema":"swawkit.command-view/web/v4","childrenColumn":{"width":"wide"}}"#,
    );
    fixture.file(
        "home/_lib/proj/..entry/claim/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.claim"}"#,
    );
    fixture.file("home/_lib/proj/.dev/run.ps1", "");
    fixture.file(
        "home/_lib/proj/.dev/bun/mode/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.profile.set"}"#,
    );
    fixture.file(
        "home/_lib/proj/.dev/setup/run.toolchain.json",
        r#"{"schema":"swawkit.toolchain-command/v1","handler":"dev.setup"}"#,
    );
    fixture.file(
        "home/_lib/proj/..runtime/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"runtime.status"}"#,
    );
    fixture.file(
        "home/_lib/proj/..runtime/cleanup/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"runtime.cleanup"}"#,
    );
    fixture.file(
        "home/_lib/proj/..runtime/host/exit/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"host.exit"}"#,
    );
    fixture.file(
        "home/_lib/proj/..runtime/host/restart/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"host.restart"}"#,
    );
    fixture.file(
        "home/_lib/proj/.help/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"meta.help"}"#,
    );
    fixture.file(
        "home/_lib/proj/.logs/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"meta.logs"}"#,
    );
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
    fixture.file("project/.swaw/python/run.py", "");
    fixture.file(
        "project/.swaw/build/host/_help/zh-CN.txt",
        "Build at {{ADDRESS}}\n{{INVOCATION}}",
    );
    fixture.file("project/.swaw/_private/run.ps1", "");
    fixture.file("project/.swaw/Bad/run.ps1", "");

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    assert_eq!(snapshot.language, "zh-CN");
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
            (CommandSource::Control, "..entry.language"),
            (CommandSource::Control, "..runtime"),
            (CommandSource::Control, "..runtime.cleanup"),
            (CommandSource::Control, "..runtime.host"),
            (CommandSource::Control, "..runtime.host.exit"),
            (CommandSource::Control, "..runtime.host.restart"),
            (CommandSource::Kernel, ""),
            (CommandSource::Kernel, "--nul"),
            (CommandSource::Kernel, "-con"),
            (CommandSource::Kernel, ".dev"),
            (CommandSource::Kernel, ".dev.bun"),
            (CommandSource::Kernel, ".dev.bun.mode"),
            (CommandSource::Kernel, ".dev.setup"),
            (CommandSource::Kernel, ".h"),
            (CommandSource::Kernel, ".help"),
            (CommandSource::Kernel, ".info"),
            (CommandSource::Kernel, ".logs"),
            (CommandSource::Action, "build"),
            (CommandSource::Action, "build.host"),
            (CommandSource::Action, "python"),
        ]
    );

    let entry = node(&snapshot, CommandSource::Control, "..entry");
    assert_eq!(entry.parent.as_deref(), Some(""));
    assert_eq!(entry.adapter.as_deref(), Some("core"));
    assert_eq!(entry.handler.as_deref(), Some("entry.profile"));
    let entry = node(&snapshot, CommandSource::Control, "..entry");
    assert_eq!(
        entry
            .view
            .as_ref()
            .and_then(|view| view.children_column.as_ref())
            .map(|column| column.width),
        Some(ColumnWidth::Wide)
    );
    let language = node(&snapshot, CommandSource::Control, "..entry.language");
    assert_eq!(language.parent.as_deref(), Some("..entry"));
    assert_eq!(language.handler.as_deref(), Some("entry.profile.set"));
    let bun_mode = node(&snapshot, CommandSource::Kernel, ".dev.bun.mode");
    assert_eq!(bun_mode.parent.as_deref(), Some(".dev.bun"));
    assert_eq!(bun_mode.adapter.as_deref(), Some("core"));
    assert_eq!(bun_mode.handler.as_deref(), Some("entry.profile.set"));
    let claim = node(&snapshot, CommandSource::Control, "..entry.claim");
    assert_eq!(claim.handler.as_deref(), Some("entry.claim"));

    let setup = node(&snapshot, CommandSource::Kernel, ".dev.setup");
    assert_eq!(setup.parent.as_deref(), Some(".dev"));
    assert_eq!(setup.entry.as_deref(), Some("run.toolchain.json"));
    assert_eq!(setup.adapter.as_deref(), Some("toolchain"));
    assert_eq!(setup.handler.as_deref(), Some("dev.setup"));

    let runtime = node(&snapshot, CommandSource::Control, "..runtime");
    assert_eq!(runtime.parent.as_deref(), Some(""));
    assert_eq!(runtime.handler.as_deref(), Some("runtime.status"));
    let cleanup = node(&snapshot, CommandSource::Control, "..runtime.cleanup");
    assert_eq!(cleanup.parent.as_deref(), Some("..runtime"));
    assert_eq!(cleanup.entry.as_deref(), Some("run.core.json"));
    assert_eq!(cleanup.adapter.as_deref(), Some("core"));
    assert_eq!(cleanup.handler.as_deref(), Some("runtime.cleanup"));
    assert!(cleanup.view.is_none());
    let exit = node(&snapshot, CommandSource::Control, "..runtime.host.exit");
    assert_eq!(exit.parent.as_deref(), Some("..runtime.host"));
    assert_eq!(exit.handler.as_deref(), Some("host.exit"));
    let restart = node(&snapshot, CommandSource::Control, "..runtime.host.restart");
    assert_eq!(restart.handler.as_deref(), Some("host.restart"));

    let info = node(&snapshot, CommandSource::Kernel, ".info");
    let help = info.help.as_ref().expect("info help");
    assert_eq!(help.summary, "Inspect fixture.");
    assert!(help.text.contains("Use fixture .info."));

    let help_alias = node(&snapshot, CommandSource::Kernel, ".h");
    assert_eq!(help_alias.alias_of.as_deref(), Some(".help"));
    let meta_help = node(&snapshot, CommandSource::Kernel, ".help");
    assert_eq!(meta_help.adapter.as_deref(), Some("core"));
    assert_eq!(meta_help.handler.as_deref(), Some("meta.help"));
    let meta_logs = node(&snapshot, CommandSource::Kernel, ".logs");
    assert_eq!(meta_logs.adapter.as_deref(), Some("core"));
    assert_eq!(meta_logs.handler.as_deref(), Some("meta.logs"));
    let root = node(&snapshot, CommandSource::Kernel, "");
    assert!(root.facets.iter().all(|facet| facet.id != "run"));
    let root_help = root
        .facets
        .iter()
        .find(|facet| facet.id == "help")
        .and_then(|facet| facet.resolver.as_ref())
        .expect("root help facet");
    assert!(matches!(
        root_help,
        FacetResolver::Command { arguments, .. } if arguments.is_empty()
    ));

    let build = node(&snapshot, CommandSource::Action, "build");
    assert_eq!(build.parent.as_deref(), Some(""));
    assert!(!build.runnable);

    let host = node(&snapshot, CommandSource::Action, "build.host");
    assert_eq!(host.parent.as_deref(), Some("build"));
    assert_eq!(
        host.help.as_ref().map(|help| help.summary.as_str()),
        Some("Build at build.host")
    );
    let python = node(&snapshot, CommandSource::Action, "python");
    assert!(!python.runnable);
    assert!(
        python
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("managed Python"))
    );
}

#[test]
fn profile_setting_modules_match_the_typed_setting_registry() {
    let kernel = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Proj kernel root");
    let snapshot = CatalogSnapshot::discover_optional_roots(
        kernel,
        None,
        "swawkit",
        PwshAvailability::ProfileUnavailable,
        EntryLanguage::default(),
    )
    .expect("source-tree Catalog");
    let setters = snapshot
        .commands
        .iter()
        .filter(|command| command.handler.as_deref() == Some("entry.profile.set"))
        .collect::<Vec<_>>();
    let mut actual = setters
        .iter()
        .map(|command| command.address.as_str())
        .collect::<Vec<_>>();
    let mut expected = crate::profile::EntryProfileRecord::profile_setting_addresses();
    actual.sort_unstable();
    expected.sort_unstable();

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
        r#"{"schema":"swawkit.command-view/web/v4","childrenColumn":{"width":"480px"}}"#,
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
fn rejects_ambiguous_web_run_operations() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file(
        "home/_lib/proj/.broken/_view/web.json",
        r#"{"schema":"swawkit.command-view/web/v4","run":{"operations":[{"id":"apply","label":"Apply","arguments":[]},{"id":"apply","label":"Again","arguments":[]}]}}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let broken = node(&snapshot, CommandSource::Kernel, ".broken");
    assert!(broken.view.is_none());
    assert!(
        broken
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("must be unique"))
    );
}

#[test]
fn module_collection_facets_are_not_restricted_to_a_context_owner() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.context/list/run.cmd", "");
    fixture.file("home/_lib/proj/.other/list/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.context/_module.json",
        r##"{"schema":"swawkit.command-module/v4","facets":[{"id":"contexts","kind":"collection","renderer":"collection","icon":"#","label":{"zh-CN":"上下文","en":"Contexts"},"summary":{"zh-CN":"浏览上下文","en":"Browse contexts"},"subjectKind":{"kind":"context","provider":{"type":"command","source":"kernel","address":".context"}},"resolver":{"type":"command","address":".context.list","arguments":["--json"],"returns":"swawkit.subject-collection/v2"}}],"subjectKinds":[{"kind":"context","facets":[{"id":"overview","kind":"operation","renderer":"run","icon":"i","label":{"zh-CN":"概览","en":"Overview"},"summary":{"zh-CN":"查看上下文","en":"Inspect context"},"resolver":{"type":"command","address":".context.list","arguments":[{"bind":"subject.id"}]}}]}]}"##,
    );
    fixture.file(
        "home/_lib/proj/.other/_module.json",
        r##"{"schema":"swawkit.command-module/v4","facets":[{"id":"items","kind":"collection","renderer":"collection","icon":"#","label":{"zh-CN":"对象","en":"Items"},"summary":{"zh-CN":"浏览对象","en":"Browse items"},"subjectKind":{"kind":"item","provider":{"type":"command","source":"kernel","address":".other"}},"resolver":{"type":"command","address":".other.list","arguments":["--json"],"returns":"swawkit.subject-collection/v2"}}],"subjectKinds":[{"kind":"item","facets":[{"id":"overview","kind":"operation","renderer":"run","icon":"i","label":{"zh-CN":"概览","en":"Overview"},"summary":{"zh-CN":"查看对象","en":"Inspect item"},"resolver":{"type":"command","address":".other.list","arguments":[{"bind":"subject.id"}]}}]}]}"##,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let collection = node(&snapshot, CommandSource::Kernel, ".context")
        .facets
        .iter()
        .find(|facet| facet.id == "contexts")
        .expect("resolved Context collection");
    assert_eq!(collection.kind, FacetKind::Collection);
    let subject_kind = collection.subject_kind.as_ref().expect("Subject kind ref");
    assert_eq!(subject_kind.kind, "context");
    assert_eq!(
        subject_kind.provider,
        crate::subject::SubjectRef::Command {
            source: CommandSource::Kernel,
            address: ".context".to_owned(),
        }
    );
    let context_kind = node(&snapshot, CommandSource::Kernel, ".context")
        .subject_kinds
        .iter()
        .find(|subject_kind| subject_kind.kind == "context")
        .expect("Context Subject kind");
    let context_overview = context_kind
        .instantiate("overview", "release-check")
        .expect("instantiate Context Facet")
        .expect("Context overview");
    assert!(matches!(
        context_overview.resolver,
        Some(FacetResolver::Command { ref arguments, .. })
            if arguments == &["release-check"]
    ));
    assert_eq!(
        collection.resolver,
        Some(FacetResolver::Command {
            address: ".context.list".to_owned(),
            arguments: vec!["--json".to_owned()],
            accepts_tail: false,
            confirmation: None,
            returns: Some("swawkit.subject-collection/v2".to_owned()),
        })
    );
    let other = node(&snapshot, CommandSource::Kernel, ".other");
    assert!(other.facets.iter().any(|facet| facet.id == "items"));
    assert_eq!(other.subject_kinds[0].kind, "item");
    assert!(other.diagnostic.is_none());
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
    fixture.file(
        "home/_lib/proj/.fake-meta/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"meta.logs"}"#,
    );
    fixture.file(
        "home/_lib/proj/..fake-meta/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"meta.logs"}"#,
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
            "restricted to Entry commands",
        ),
        (
            CommandSource::Kernel,
            ".fake-meta",
            "exact built-in Kernel commands",
        ),
        (
            CommandSource::Control,
            "..fake-meta",
            "exact built-in Kernel commands",
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
fn disabled_powershell_is_a_catalog_diagnostic_not_a_hidden_fallback() {
    let fixture = Fixture::new();
    let directory = fixture.directory("home/_lib/proj/.script");
    fixture.file("home/_lib/proj/.script/run.ps1", "");
    let logs_directory = fixture.directory("home/_lib/proj/.logs");
    fixture.file(
        "home/_lib/proj/.logs/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"meta.logs"}"#,
    );
    let pending = PendingDirectory {
        path: directory,
        address: ".script".to_owned(),
        source: CommandSource::Kernel,
        is_root: false,
    };

    let disabled = scan_node(
        &pending,
        "fixture",
        PwshAvailability::Disabled,
        EntryLanguage::default(),
    );
    assert!(!disabled.runnable);
    assert!(disabled.entry.is_none());
    assert!(
        disabled
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains(".dev.pwsh.mode"))
    );

    let enabled = scan_node(
        &pending,
        "fixture",
        PwshAvailability::Enabled,
        EntryLanguage::default(),
    );
    assert!(enabled.runnable);
    assert_eq!(enabled.adapter.as_deref(), Some("pwsh"));

    let logs = scan_node(
        &PendingDirectory {
            path: logs_directory,
            address: ".logs".to_owned(),
            source: CommandSource::Kernel,
            is_root: false,
        },
        "fixture",
        PwshAvailability::Disabled,
        EntryLanguage::default(),
    );
    assert!(logs.runnable);
    assert_eq!(logs.adapter.as_deref(), Some("core"));
    assert_eq!(logs.handler.as_deref(), Some("meta.logs"));
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

#[test]
fn selects_entry_language_help_and_falls_back_only_when_translation_is_absent() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.translated/_help/zh-CN.txt", "中文摘要");
    fixture.file("home/_lib/proj/.translated/_help/en.txt", "English summary");
    fixture.file("home/_lib/proj/.fallback/_help/zh-CN.txt", "中文回退");

    let snapshot = CatalogSnapshot::discover_roots_in_language(
        &kernel,
        &actions,
        "fixture",
        EntryLanguage::En,
    )
    .expect("English catalog");

    assert_eq!(snapshot.language, "en");
    assert_eq!(
        node(&snapshot, CommandSource::Kernel, ".translated")
            .help
            .as_ref()
            .map(|help| help.summary.as_str()),
        Some("English summary")
    );
    assert_eq!(
        node(&snapshot, CommandSource::Kernel, ".fallback")
            .help
            .as_ref()
            .map(|help| help.summary.as_str()),
        Some("中文回退")
    );
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
