use super::*;

#[test]
fn resolves_declared_module_facets_to_exact_cli_commands() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.check/run.cmd", "");
    fixture.file("home/_lib/proj/.tool/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.tool/_module.json",
        r#"{"schema":"swawkit.command-module/v4","facets":[{"id":"validate","kind":"operation","renderer":"run","icon":"!","label":{"zh-CN":"Validate","en":"Validate"},"summary":{"zh-CN":"Validate this module","en":"Validate this module"},"resolver":{"type":"command","address":".check","arguments":[{"bind":"commandAddress"},"$command","--json"]}}]}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let tool = node(&snapshot, CommandSource::Kernel, ".tool");
    let validate = tool
        .facets
        .iter()
        .find(|facet| facet.id == "validate")
        .expect("declared facet");

    assert!(tool.view.is_none());
    assert_eq!(validate.kind, FacetKind::Operation);
    assert_eq!(validate.renderer, FacetRenderer::Run);
    assert_eq!(validate.label, "Validate");
    assert_eq!(
        validate.resolver,
        Some(FacetResolver::Command {
            address: ".check".to_owned(),
            arguments: vec![
                ".tool".to_owned(),
                "$command".to_owned(),
                "--json".to_owned(),
            ],
            accepts_tail: false,
            confirmation: None,
            returns: None,
        })
    );
}

#[test]
fn reports_an_invalid_runs_override_without_restoring_the_core_default() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.logs/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.logs/_module.json",
        include_str!("../../../../.logs/_module.json"),
    );
    fixture.file("home/_lib/proj/.tool/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.tool/_module.json",
        r#"{"schema":"swawkit.command-module/v4","facets":[{"id":"runs","kind":"operation","renderer":"run","icon":"!","label":{"zh-CN":"Logs","en":"Logs"},"summary":{"zh-CN":"Custom logs","en":"Custom logs"},"resolver":{"type":"command","address":".missing","arguments":[{"bind":"commandAddress"}]}}]}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let tool = node(&snapshot, CommandSource::Kernel, ".tool");
    assert!(tool.facets.iter().all(|facet| facet.id != "runs"));
    assert!(
        tool.diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("references missing command '.missing'"))
    );
}

#[test]
fn a_module_can_replace_the_default_runs_facet() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.logs/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.logs/_module.json",
        include_str!("../../../../.logs/_module.json"),
    );
    fixture.file("home/_lib/proj/.custom-logs/run.cmd", "");
    fixture.file("home/_lib/proj/.tool/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.tool/_module.json",
        r#"{"schema":"swawkit.command-module/v4","facets":[{"id":"runs","kind":"operation","renderer":"run","icon":"!","label":{"zh-CN":"Custom logs","en":"Custom logs"},"summary":{"zh-CN":"Module-owned logs","en":"Module-owned logs"},"resolver":{"type":"command","address":".custom-logs","arguments":[{"bind":"commandAddress"}]}}]}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let tool = node(&snapshot, CommandSource::Kernel, ".tool");
    let runs = tool
        .facets
        .iter()
        .filter(|facet| facet.id == "runs")
        .collect::<Vec<_>>();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].label, "Custom logs");
    assert_eq!(runs[0].renderer, FacetRenderer::Run);
    assert!(matches!(
        runs[0].resolver.as_ref(),
        Some(FacetResolver::Command { address, arguments, .. })
            if address == ".custom-logs" && arguments == &[".tool"]
    ));
}

#[test]
fn runnable_commands_receive_one_contextual_runs_collection_from_the_logs_provider() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.logs/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.logs/_module.json",
        include_str!("../../../../.logs/_module.json"),
    );
    fixture.file("home/_lib/proj/.tool/run.cmd", "");

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let tool = node(&snapshot, CommandSource::Kernel, ".tool");
    let runs = tool
        .facets
        .iter()
        .find(|facet| facet.id == "runs")
        .expect("default runs Facet");

    assert_eq!(runs.kind, FacetKind::Collection);
    assert_eq!(runs.renderer, FacetRenderer::Collection);
    let subject_kind = runs.subject_kind.as_ref().expect("Run Subject kind ref");
    assert_eq!(subject_kind.kind, "run");
    assert_eq!(
        subject_kind.provider,
        crate::subject::SubjectRef::Command {
            source: CommandSource::Kernel,
            address: ".logs".to_owned(),
        }
    );
    assert!(matches!(
        runs.resolver.as_ref(),
        Some(FacetResolver::Command { address, arguments, returns, .. })
            if address == ".logs"
                && arguments == &["--json", "kernel/.tool"]
                && returns.as_deref() == Some(crate::subject::SUBJECT_COLLECTION_PROTOCOL)
    ));
}

#[test]
fn a_declared_projection_can_resolve_a_typed_document() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.inspect/run.cmd", "");
    fixture.file("home/_lib/proj/.tool/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.tool/_module.json",
        r#"{"schema":"swawkit.command-module/v4","facets":[{"id":"status","kind":"projection","renderer":"overview","icon":"i","label":{"zh-CN":"Status","en":"Status"},"summary":{"zh-CN":"Inspect status","en":"Inspect status"},"resolver":{"type":"command","address":".inspect","arguments":[{"bind":"commandAddress"},"--json"],"returns":"fixture.status/v1"}}]}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let status = node(&snapshot, CommandSource::Kernel, ".tool")
        .facets
        .iter()
        .find(|facet| facet.id == "status")
        .expect("declared projection");
    assert_eq!(status.kind, FacetKind::Projection);
    assert_eq!(status.renderer, FacetRenderer::Overview);
    assert_eq!(
        status.resolver,
        Some(FacetResolver::Command {
            address: ".inspect".to_owned(),
            arguments: vec![".tool".to_owned(), "--json".to_owned()],
            accepts_tail: false,
            confirmation: None,
            returns: Some("fixture.status/v1".to_owned()),
        })
    );
}

#[test]
fn a_collection_must_return_the_subject_collection_protocol() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.items/list/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.items/_module.json",
        r##"{"schema":"swawkit.command-module/v4","facets":[{"id":"items","kind":"collection","renderer":"collection","icon":"#","label":{"zh-CN":"Items","en":"Items"},"summary":{"zh-CN":"Browse items","en":"Browse items"},"subjectKind":{"kind":"item","provider":{"type":"command","source":"kernel","address":".items"}},"resolver":{"type":"command","address":".items.list","arguments":["--json"],"returns":"fixture.items/v1"}}],"subjectKinds":[{"kind":"item","facets":[{"id":"overview","kind":"operation","renderer":"run","icon":"i","label":{"zh-CN":"Overview","en":"Overview"},"summary":{"zh-CN":"Inspect item","en":"Inspect item"},"resolver":{"type":"command","address":".items.list","arguments":[{"bind":"subject.id"}]}}]}]}"##,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let items = node(&snapshot, CommandSource::Kernel, ".items");
    assert!(items.module.is_none());
    assert!(
        items
            .diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("must return swawkit.subject-collection/v2"))
    );
}

#[test]
fn a_projection_cannot_return_the_subject_collection_protocol() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.inspect/run.cmd", "");
    fixture.file("home/_lib/proj/.tool/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.tool/_module.json",
        r#"{"schema":"swawkit.command-module/v4","facets":[{"id":"items","kind":"projection","renderer":"overview","icon":"i","label":{"zh-CN":"Items","en":"Items"},"summary":{"zh-CN":"Inspect items","en":"Inspect items"},"resolver":{"type":"command","address":".inspect","arguments":["--json"],"returns":"swawkit.subject-collection/v2"}}]}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let tool = node(&snapshot, CommandSource::Kernel, ".tool");
    assert!(tool.module.is_none());
    assert!(
        tool.diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("non-collection returned protocol"))
    );
}

#[test]
fn new_module_facets_preserve_declaration_order_before_core_details() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.one/run.cmd", "");
    fixture.file("home/_lib/proj/.two/run.cmd", "");
    fixture.file("home/_lib/proj/.tool/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.tool/_module.json",
        r#"{"schema":"swawkit.command-module/v4","facets":[{"id":"zeta","kind":"operation","renderer":"run","icon":"1","label":{"zh-CN":"Zeta","en":"Zeta"},"summary":{"zh-CN":"First","en":"First"},"resolver":{"type":"command","address":".one"}},{"id":"alpha","kind":"operation","renderer":"run","icon":"2","label":{"zh-CN":"Alpha","en":"Alpha"},"summary":{"zh-CN":"Second","en":"Second"},"resolver":{"type":"command","address":".two"}}]}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let ids = node(&snapshot, CommandSource::Kernel, ".tool")
        .facets
        .iter()
        .map(|facet| facet.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(&ids[..3], ["zeta", "alpha", "overview"]);
}

#[test]
fn every_resolved_catalog_facet_satisfies_the_public_wire_contract() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.help/run.cmd", "");
    fixture.file("home/_lib/proj/.logs/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.logs/_module.json",
        include_str!("../../../../.logs/_module.json"),
    );
    fixture.file("home/_lib/proj/.inspect/run.cmd", "");
    fixture.file("home/_lib/proj/.group/child/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.group/_module.json",
        r#"{"schema":"swawkit.command-module/v4","facets":[{"id":"status","kind":"projection","renderer":"overview","icon":"i","label":{"zh-CN":"Status","en":"Status"},"summary":{"zh-CN":"Inspect status","en":"Inspect status"},"resolver":{"type":"command","address":".inspect","arguments":["--json"],"returns":"fixture.status/v1"}}]}"#,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    for command in &snapshot.commands {
        for facet in &command.facets {
            facet.validate().unwrap_or_else(|error| {
                panic!(
                    "invalid resolved Facet '{}#{}': {error}",
                    command.address, facet.id
                )
            });
        }
    }
}
