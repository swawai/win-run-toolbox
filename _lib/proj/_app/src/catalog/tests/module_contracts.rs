use super::*;

#[test]
fn module_contracts_are_strict_and_invalid_declarations_disable_the_command() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.provider/run.exe", "");
    fixture.file(
        "home/_lib/proj/.provider/_module.json",
        r#"{"schema":"swawkit.command-module/v4","provides":[{"contract":"swawkit.fixture/v1"}]}"#,
    );
    fixture.file("home/_lib/proj/.consumer/run.exe", "");
    fixture.file(
        "home/_lib/proj/.consumer/_module.json",
        r#"{"schema":"swawkit.command-module/v4","requires":[{"provider":".provider","contract":"swawkit.fixture/v1"}]}"#,
    );
    fixture.file("home/_lib/proj/.broken/run.exe", "");
    fixture.file(
        "home/_lib/proj/.broken/_module.json",
        r#"{"schema":"swawkit.command-module/v4","requires":[],"provides":[],"facets":[]}"#,
    );
    fixture.file("home/_lib/proj/.legacy/run.exe", "");
    fixture.file(
        "home/_lib/proj/.legacy/_module.json",
        r#"{"schema":"swawkit.command-module/v3","provides":[{"contract":"legacy/v1"}]}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let provider = node(&snapshot, CommandSource::Kernel, ".provider");
    assert_eq!(
        provider.module.as_ref().unwrap().provides[0].contract,
        "swawkit.fixture/v1"
    );
    let consumer = node(&snapshot, CommandSource::Kernel, ".consumer");
    assert_eq!(
        consumer.module.as_ref().unwrap().requires[0].provider,
        ".provider"
    );
    let broken = node(&snapshot, CommandSource::Kernel, ".broken");
    assert!(!broken.runnable);
    assert!(
        broken
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("requires, provides, facets, or subjectKinds"))
    );
    let legacy = node(&snapshot, CommandSource::Kernel, ".legacy");
    assert!(!legacy.runnable);
    assert!(
        legacy
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("unsupported module contract schema"))
    );
}
