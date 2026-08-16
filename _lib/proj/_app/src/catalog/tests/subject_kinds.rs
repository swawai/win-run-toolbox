use super::*;

#[test]
fn subject_kind_identity_is_globally_unique_without_owner_fallback() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.one/list/run.cmd", "");
    fixture.file("home/_lib/proj/.two/list/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.one/_module.json",
        r##"{"schema":"swawkit.command-module/v4","facets":[{"id":"items","kind":"collection","renderer":"collection","icon":"#","label":{"zh-CN":"Items","en":"Items"},"summary":{"zh-CN":"Browse items","en":"Browse items"},"subjectKind":{"kind":"item","provider":{"type":"command","source":"kernel","address":".one"}},"resolver":{"type":"command","address":".one.list","arguments":["--json"],"returns":"swawkit.subject-collection/v2"}}],"subjectKinds":[{"kind":"item","facets":[{"id":"open","kind":"operation","renderer":"run","icon":">","label":{"zh-CN":"Open","en":"Open"},"summary":{"zh-CN":"Open item","en":"Open item"},"resolver":{"type":"command","address":".one.list","arguments":[{"bind":"subject.id"}]}}]}]}"##,
    );
    fixture.file(
        "home/_lib/proj/.two/_module.json",
        r##"{"schema":"swawkit.command-module/v4","facets":[{"id":"items","kind":"collection","renderer":"collection","icon":"#","label":{"zh-CN":"Items","en":"Items"},"summary":{"zh-CN":"Browse items","en":"Browse items"},"subjectKind":{"kind":"item","provider":{"type":"command","source":"kernel","address":".two"}},"resolver":{"type":"command","address":".two.list","arguments":["--json"],"returns":"swawkit.subject-collection/v2"}}],"subjectKinds":[{"kind":"item","facets":[{"id":"open","kind":"operation","renderer":"run","icon":">","label":{"zh-CN":"Open","en":"Open"},"summary":{"zh-CN":"Open item","en":"Open item"},"resolver":{"type":"command","address":".two.list","arguments":[{"bind":"subject.id"}]}}]}]}"##,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    for address in [".one", ".two"] {
        let command = node(&snapshot, CommandSource::Kernel, address);
        assert!(command.subject_kinds.is_empty());
        assert!(command.facets.iter().all(|facet| facet.id != "items"));
        assert!(
            command
                .diagnostic
                .as_deref()
                .is_some_and(|message| message.contains("declared by more than one command module"))
        );
    }
}

#[test]
fn a_collection_ref_must_name_the_exact_subject_kind_provider() {
    let fixture = Fixture::new();
    let kernel = fixture.directory("home/_lib/proj");
    let actions = fixture.directory("project/.swaw");
    fixture.file("home/_lib/proj/.one/list/run.cmd", "");
    fixture.file("home/_lib/proj/.two/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.one/_module.json",
        r##"{"schema":"swawkit.command-module/v4","facets":[{"id":"items","kind":"collection","renderer":"collection","icon":"#","label":{"zh-CN":"Items","en":"Items"},"summary":{"zh-CN":"Browse items","en":"Browse items"},"subjectKind":{"kind":"item","provider":{"type":"command","source":"kernel","address":".two"}},"resolver":{"type":"command","address":".one.list","arguments":["--json"],"returns":"swawkit.subject-collection/v2"}}],"subjectKinds":[{"kind":"item","facets":[{"id":"open","kind":"operation","renderer":"run","icon":">","label":{"zh-CN":"Open","en":"Open"},"summary":{"zh-CN":"Open item","en":"Open item"},"resolver":{"type":"command","address":".one.list","arguments":[{"bind":"subject.id"}]}}]}]}"##,
    );

    let snapshot = CatalogSnapshot::discover_roots(&kernel, &actions, "fixture").expect("catalog");
    let one = node(&snapshot, CommandSource::Kernel, ".one");
    assert_eq!(one.subject_kinds[0].kind, "item");
    assert!(one.facets.iter().all(|facet| facet.id != "items"));
    assert!(
        one.diagnostic
            .as_deref()
            .is_some_and(|message| message.contains("unavailable Subject kind provider"))
    );
}
