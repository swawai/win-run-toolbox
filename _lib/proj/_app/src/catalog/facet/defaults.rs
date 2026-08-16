use crate::{
    facet::{Facet, FacetKind, FacetRenderer, FacetResolver},
    module_check::MODULE_CHECK_PROTOCOL,
    profile::EntryLanguage,
    subject::SUBJECT_COLLECTION_PROTOCOL,
    subject_kind::SubjectKindRef,
};

use super::{CHECK_ADDRESS, CommandNode, CommandSource, HELP_ADDRESS, RUNS_ADDRESS};

pub(super) fn children_facet(language: EntryLanguage) -> Facet {
    Facet {
        id: "children".to_owned(),
        kind: FacetKind::Collection,
        renderer: FacetRenderer::Collection,
        icon: "□".to_owned(),
        label: text(language, "子命令", "Subcommands").to_owned(),
        summary: text(language, "浏览静态子命令", "Browse static subcommands").to_owned(),
        subject_kind: None,
        resolver: Some(FacetResolver::Catalog {
            relation: "children".to_owned(),
        }),
    }
}

pub(super) fn default_facets(
    command: &CommandNode,
    language: EntryLanguage,
    help_available: bool,
    check_available: bool,
    runs_available: bool,
    run_subject_kind: Option<&SubjectKindRef>,
) -> Vec<Facet> {
    let mut facets = Vec::new();
    if command.handler.as_deref() == Some("entry.profile.set") {
        facets.push(operation_facet(
            "edit",
            FacetRenderer::Edit,
            "*",
            text(language, "设置", "Setting"),
            text(
                language,
                "修改并保存配置值",
                "Edit and save a configuration value",
            ),
            command_resolver(&command.address, [], false),
        ));
    }
    if help_available {
        let arguments = if command.address.is_empty() {
            Vec::new()
        } else {
            vec![command.address.clone()]
        };
        facets.push(operation_facet(
            "help",
            FacetRenderer::Help,
            "?",
            text(language, "帮助", "Help"),
            text(language, "阅读命令说明", "Read command help"),
            FacetResolver::Command {
                address: HELP_ADDRESS.to_owned(),
                arguments,
                accepts_tail: false,
                confirmation: None,
                returns: None,
            },
        ));
    }
    if check_available
        && command.source != CommandSource::Control
        && !command.address.is_empty()
        && command.alias_of.is_none()
        && (command.entry.is_some() || command.module.is_some() || command.diagnostic.is_some())
    {
        facets.push(Facet {
            id: "check".to_owned(),
            kind: FacetKind::Projection,
            renderer: FacetRenderer::Overview,
            icon: "!".to_owned(),
            label: text(language, "检查", "Check").to_owned(),
            summary: text(
                language,
                "检查可运行状态、依赖与产物",
                "Check readiness, dependencies, and publications",
            )
            .to_owned(),
            subject_kind: None,
            resolver: Some(FacetResolver::Command {
                address: CHECK_ADDRESS.to_owned(),
                arguments: vec![command.address.clone(), "--json".to_owned()],
                accepts_tail: false,
                confirmation: None,
                returns: Some(MODULE_CHECK_PROTOCOL.to_owned()),
            }),
        });
    }
    if runs_available
        && command.source != CommandSource::Control
        && !command.address.is_empty()
        && command.runnable
        && command.alias_of.is_none()
    {
        if let Some(subject_kind) = run_subject_kind {
            facets.push(Facet {
                id: "runs".to_owned(),
                kind: FacetKind::Collection,
                renderer: FacetRenderer::Collection,
                icon: "=".to_owned(),
                label: text(language, "运行记录", "Runs").to_owned(),
                summary: text(
                    language,
                    "浏览该命令的持久运行",
                    "Browse persisted runs for this command",
                )
                .to_owned(),
                subject_kind: Some(subject_kind.clone()),
                resolver: Some(FacetResolver::Command {
                    address: RUNS_ADDRESS.to_owned(),
                    arguments: vec![
                        "--json".to_owned(),
                        command_locator(command.source, &command.address),
                    ],
                    accepts_tail: false,
                    confirmation: None,
                    returns: Some(SUBJECT_COLLECTION_PROTOCOL.to_owned()),
                }),
            });
        }
    }
    if command.source != CommandSource::Control
        && !command.address.is_empty()
        && command.runnable
        && command.alias_of.is_none()
    {
        facets.push(operation_facet(
            "run",
            FacetRenderer::Run,
            ">",
            text(language, "执行", "Run"),
            text(
                language,
                "设置参数并启动命令",
                "Set arguments and start the command",
            ),
            command_resolver(&command.address, [], true),
        ));
    }
    facets
}

fn command_locator(source: CommandSource, address: &str) -> String {
    let source = match source {
        CommandSource::Control => "control",
        CommandSource::Kernel => "kernel",
        CommandSource::Action => "action",
    };
    format!("{source}/{address}")
}

fn operation_facet(
    id: &str,
    renderer: FacetRenderer,
    icon: &str,
    label: &str,
    summary: &str,
    resolver: FacetResolver,
) -> Facet {
    Facet {
        id: id.to_owned(),
        kind: FacetKind::Operation,
        renderer,
        icon: icon.to_owned(),
        label: label.to_owned(),
        summary: summary.to_owned(),
        subject_kind: None,
        resolver: Some(resolver),
    }
}

fn command_resolver<'a>(
    address: &str,
    arguments: impl IntoIterator<Item = &'a str>,
    accepts_tail: bool,
) -> FacetResolver {
    FacetResolver::Command {
        address: address.to_owned(),
        arguments: arguments.into_iter().map(str::to_owned).collect(),
        accepts_tail,
        confirmation: None,
        returns: None,
    }
}

fn text(language: EntryLanguage, zh_cn: &'static str, en: &'static str) -> &'static str {
    match language {
        EntryLanguage::ZhCn => zh_cn,
        EntryLanguage::En => en,
    }
}
