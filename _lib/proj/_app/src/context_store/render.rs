use super::{CommandSource, ContextRecord};

/// Renders one persisted Context as the canonical Agent-facing Markdown projection.
///
/// The record preserves command and note insertion order, so identical persisted
/// input always produces identical UTF-8 output.
pub fn render_markdown(record: &ContextRecord) -> String {
    let mut lines = vec![
        format!("# Context: {}", record.id),
        String::new(),
        format!("Subject: `::context/{}`", record.id),
        String::new(),
        "## Commands".to_owned(),
        String::new(),
    ];
    if record.commands.is_empty() {
        lines.push("_None._".to_owned());
    } else {
        lines.extend(
            record.commands.iter().map(|command| {
                format!("- `{}` ({})", command.address, source_name(command.source),)
            }),
        );
    }

    lines.extend([String::new(), "## Notes".to_owned(), String::new()]);
    if record.notes.is_empty() {
        lines.push("_None._".to_owned());
    } else {
        for (index, note) in record.notes.iter().enumerate() {
            if index > 0 {
                lines.push(String::new());
            }
            lines.push(format!("### Note {}", index + 1));
            lines.push(String::new());
            lines.push(note.clone());
        }
    }

    lines.extend([
        String::new(),
        "## Final Prompt".to_owned(),
        String::new(),
        if record.prompt.is_empty() {
            "_None._".to_owned()
        } else {
            record.prompt.clone()
        },
    ]);
    lines.join("\n")
}

fn source_name(source: CommandSource) -> &'static str {
    match source {
        CommandSource::Control => "control",
        CommandSource::Kernel => "kernel",
        CommandSource::Action => "action",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_store::{CONTEXT_SCHEMA, ContextCommand};

    #[test]
    fn markdown_is_byte_stable_and_preserves_user_order_and_multiline_text() {
        let record = ContextRecord {
            schema: CONTEXT_SCHEMA.to_owned(),
            id: "work".to_owned(),
            commands: vec![
                ContextCommand {
                    source: CommandSource::Kernel,
                    address: ".dev.status".to_owned(),
                },
                ContextCommand {
                    source: CommandSource::Action,
                    address: "build.app".to_owned(),
                },
            ],
            notes: vec!["Check first.".to_owned(), "Use release mode.".to_owned()],
            prompt: "Build\nthe app.".to_owned(),
        };

        assert_eq!(
            render_markdown(&record),
            "# Context: work\n\
             \n\
             Subject: `::context/work`\n\
             \n\
             ## Commands\n\
             \n\
             - `.dev.status` (kernel)\n\
             - `build.app` (action)\n\
             \n\
             ## Notes\n\
             \n\
             ### Note 1\n\
             \n\
             Check first.\n\
             \n\
             ### Note 2\n\
             \n\
             Use release mode.\n\
             \n\
             ## Final Prompt\n\
             \n\
             Build\n\
             the app."
        );
    }

    #[test]
    fn empty_sections_are_explicit_without_inventing_content() {
        let record = ContextRecord {
            schema: CONTEXT_SCHEMA.to_owned(),
            id: "empty".to_owned(),
            commands: Vec::new(),
            notes: Vec::new(),
            prompt: String::new(),
        };

        assert_eq!(
            render_markdown(&record),
            "# Context: empty\n\
             \n\
             Subject: `::context/empty`\n\
             \n\
             ## Commands\n\
             \n\
             _None._\n\
             \n\
             ## Notes\n\
             \n\
             _None._\n\
             \n\
             ## Final Prompt\n\
             \n\
             _None._"
        );
    }
}
