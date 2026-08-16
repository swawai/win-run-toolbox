use std::collections::BTreeMap;

use crate::subject_kind::{
    SubjectFacetArgument, SubjectFacetArgumentBinding, SubjectFacetBinding, SubjectFacetResolver,
    SubjectFacetTemplate, SubjectKind,
};

use super::{
    CommandNode, CommandSource,
    module_contract::{
        ModuleFacet, ModuleFacetArgument, ModuleFacetBinding, ModuleFacetResolver,
        ModuleSubjectKind,
    },
};

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

pub(super) fn resolve_subject_kinds(commands: &mut [CommandNode]) {
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
    let mut owners = BTreeMap::<String, Vec<usize>>::new();
    for (index, command) in commands.iter().enumerate() {
        if let Some(module) = &command.module {
            for subject_kind in &module.subject_kinds {
                owners
                    .entry(subject_kind.kind.clone())
                    .or_default()
                    .push(index);
            }
        }
    }

    for (kind, indexes) in owners.iter().filter(|(_, indexes)| indexes.len() > 1) {
        for index in indexes {
            append_diagnostic(
                &mut commands[*index],
                format!("subject kind '{kind}' is declared by more than one command module"),
            );
        }
    }

    for index in 0..commands.len() {
        let declarations = commands[index]
            .module
            .as_ref()
            .map(|module| module.subject_kinds.clone())
            .unwrap_or_default();
        for declaration in declarations {
            if owners
                .get(&declaration.kind)
                .is_some_and(|indexes| indexes.len() > 1)
            {
                continue;
            }
            match resolve_subject_kind(declaration, &capabilities) {
                Ok(subject_kind) => commands[index].subject_kinds.push(subject_kind),
                Err(diagnostic) => append_diagnostic(&mut commands[index], diagnostic),
            }
        }
    }
}

fn resolve_subject_kind(
    declaration: ModuleSubjectKind,
    capabilities: &BTreeMap<String, ResolverCapability>,
) -> Result<SubjectKind, String> {
    let facets = declaration
        .facets
        .into_iter()
        .map(|facet| resolve_subject_facet(facet, capabilities))
        .collect::<Result<Vec<_>, _>>()?;
    let subject_kind = SubjectKind {
        kind: declaration.kind,
        facets,
    };
    subject_kind.validate()?;
    Ok(subject_kind)
}

fn resolve_subject_facet(
    declaration: ModuleFacet,
    capabilities: &BTreeMap<String, ResolverCapability>,
) -> Result<SubjectFacetTemplate, String> {
    let Some(ModuleFacetResolver::Command {
        address,
        arguments,
        accepts_tail,
        confirmation,
        returns,
    }) = declaration.resolver
    else {
        return Err(format!(
            "subject facet '{}' must declare a command resolver",
            declaration.id
        ));
    };
    let Some(capability) = capabilities.get(&address) else {
        return Err(format!(
            "subject facet '{}' references missing command '{}'",
            declaration.id, address
        ));
    };
    if !capability.web_runnable() {
        return Err(format!(
            "subject facet '{}' command '{}' is not an exact runnable Kernel or Action command",
            declaration.id, address
        ));
    }
    let arguments = arguments
        .into_iter()
        .map(|argument| match argument {
            ModuleFacetArgument::Literal(value) => SubjectFacetArgument::Literal(value),
            ModuleFacetArgument::Binding(binding) => match binding.bind {
                ModuleFacetBinding::SubjectId => {
                    SubjectFacetArgument::Binding(SubjectFacetArgumentBinding {
                        bind: SubjectFacetBinding::SubjectId,
                    })
                }
                ModuleFacetBinding::CommandAddress => unreachable!(
                    "module validation rejects commandAddress in Subject facet templates"
                ),
            },
        })
        .collect();
    Ok(SubjectFacetTemplate {
        id: declaration.id,
        kind: declaration.kind,
        renderer: declaration.renderer,
        icon: declaration.icon,
        label: declaration.label,
        summary: declaration.summary,
        resolver: SubjectFacetResolver::Command {
            address,
            arguments,
            accepts_tail,
            confirmation,
            returns,
        },
    })
}

fn append_diagnostic(command: &mut CommandNode, diagnostic: String) {
    command.diagnostic = Some(match command.diagnostic.take() {
        Some(existing) => format!("{existing}; {diagnostic}"),
        None => diagnostic,
    });
}
