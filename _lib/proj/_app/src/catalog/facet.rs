use std::collections::{BTreeMap, BTreeSet};

use crate::{
    facet::{Facet, FacetResolver},
    profile::EntryLanguage,
    subject::SubjectRef,
    subject_kind::SubjectKindRef,
};

use super::{
    CommandNode, CommandSource,
    module_contract::{ModuleFacet, ModuleFacetArgument, ModuleFacetBinding, ModuleFacetResolver},
};

mod defaults;

use defaults::{children_facet, default_facets};

const HELP_ADDRESS: &str = ".help";
const CHECK_ADDRESS: &str = ".check";
const RUNS_ADDRESS: &str = ".runs";
const RUN_KIND: &str = "run";

#[derive(Clone, Copy)]
struct ResolverCapability {
    source: CommandSource,
    runnable: bool,
    canonical: bool,
}

impl ResolverCapability {
    fn web_runnable(self) -> bool {
        self.runnable && self.canonical && self.source != CommandSource::Control
    }
}

pub(super) fn resolve_command_facets(commands: &mut [CommandNode], language: EntryLanguage) {
    let parents = commands
        .iter()
        .filter_map(|command| command.parent.clone())
        .collect::<BTreeSet<_>>();
    let capabilities = commands
        .iter()
        .map(|command| {
            (
                command.address.clone(),
                ResolverCapability {
                    source: command.source,
                    runnable: command.runnable,
                    canonical: command.alias_of.is_none(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let help_available = capabilities
        .get(HELP_ADDRESS)
        .is_some_and(|capability| capability.web_runnable());
    let check_available = capabilities
        .get(CHECK_ADDRESS)
        .is_some_and(|capability| capability.web_runnable());
    let runs_available = capabilities
        .get(RUNS_ADDRESS)
        .is_some_and(|capability| capability.web_runnable());
    let subject_kind_providers = commands
        .iter()
        .flat_map(|command| {
            command.subject_kinds.iter().map(|subject_kind| {
                (
                    subject_kind.kind.clone(),
                    SubjectRef::Command {
                        source: command.source,
                        address: command.address.clone(),
                    },
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let run_subject_kind = subject_kind_providers
        .get(RUN_KIND)
        .filter(|provider| {
            matches!(
                provider,
                SubjectRef::Command {
                    source: CommandSource::Kernel,
                    address,
                } if address == RUNS_ADDRESS
            )
        })
        .cloned()
        .map(|provider| SubjectKindRef {
            kind: RUN_KIND.to_owned(),
            provider,
        });
    for command in commands {
        let children = parents
            .contains(&command.address)
            .then(|| children_facet(language));
        let defaults = default_facets(
            command,
            language,
            help_available,
            check_available,
            runs_available,
            run_subject_kind.as_ref(),
        );
        let core_ids = children
            .iter()
            .chain(defaults.iter())
            .map(|facet| facet.id.clone())
            .collect::<BTreeSet<_>>();
        let declarations = command
            .module
            .as_ref()
            .map(|module| module.facets.clone())
            .unwrap_or_default();
        let declaration_order = declarations
            .iter()
            .map(|facet| facet.id.clone())
            .collect::<Vec<_>>();
        let declared_ids = declaration_order.iter().cloned().collect::<BTreeSet<_>>();
        let mut declared = BTreeMap::new();
        for declaration in declarations {
            let id = declaration.id.clone();
            match resolve_declared_facet(
                &command.address,
                declaration,
                &capabilities,
                &subject_kind_providers,
            ) {
                Ok(facet) => {
                    declared.insert(id, facet);
                }
                Err(diagnostic) => append_diagnostic(command, diagnostic),
            }
        }

        let mut facets = Vec::new();
        if let Some(children) = children {
            append_core_facet(&mut facets, children, &declared_ids, &mut declared);
        }
        for id in declaration_order
            .iter()
            .filter(|id| !core_ids.contains(*id))
        {
            if let Some(facet) = declared.remove(id) {
                facets.push(facet);
            }
        }
        for facet in defaults {
            append_core_facet(&mut facets, facet, &declared_ids, &mut declared);
        }
        command.facets = facets;
    }
}

fn append_core_facet(
    facets: &mut Vec<Facet>,
    core: Facet,
    declared_ids: &BTreeSet<String>,
    declared: &mut BTreeMap<String, Facet>,
) {
    if let Some(replacement) = declared.remove(&core.id) {
        facets.push(replacement);
    } else if !declared_ids.contains(&core.id) {
        facets.push(core);
    }
}

fn resolve_declared_facet(
    owner: &str,
    declaration: ModuleFacet,
    capabilities: &BTreeMap<String, ResolverCapability>,
    subject_kind_providers: &BTreeMap<String, SubjectRef>,
) -> Result<Facet, String> {
    if let Some(subject_kind) = &declaration.subject_kind {
        if subject_kind_providers.get(&subject_kind.kind) != Some(&subject_kind.provider) {
            return Err(format!(
                "facet '{}' references an unavailable Subject kind provider",
                declaration.id
            ));
        }
    }
    let resolver = match declaration.resolver {
        None => None,
        Some(ModuleFacetResolver::Command {
            address,
            arguments,
            accepts_tail,
            confirmation,
            returns,
        }) => {
            let Some(capability) = capabilities.get(&address) else {
                return Err(format!(
                    "facet '{}' references missing command '{}'",
                    declaration.id, address
                ));
            };
            if !capability.web_runnable() {
                return Err(format!(
                    "facet '{}' command '{}' is not an exact runnable Kernel or Action command",
                    declaration.id, address
                ));
            }
            let arguments = arguments
                .into_iter()
                .map(|argument| match argument {
                    ModuleFacetArgument::Literal(value) => value,
                    ModuleFacetArgument::Binding(binding) => match binding.bind {
                        ModuleFacetBinding::CommandAddress => owner.to_owned(),
                        ModuleFacetBinding::SubjectId => {
                            unreachable!("module validation rejects subject.id in Command facets")
                        }
                    },
                })
                .collect();
            Some(FacetResolver::Command {
                address,
                arguments,
                accepts_tail,
                confirmation,
                returns,
            })
        }
    };
    Ok(Facet {
        id: declaration.id,
        kind: declaration.kind,
        renderer: declaration.renderer,
        icon: declaration.icon,
        label: declaration.label,
        summary: declaration.summary,
        subject_kind: declaration.subject_kind,
        resolver,
    })
}

fn append_diagnostic(command: &mut CommandNode, diagnostic: String) {
    command.diagnostic = Some(match command.diagnostic.take() {
        Some(existing) => format!("{existing}; {diagnostic}"),
        None => diagnostic,
    });
}
