use serde::{Deserialize, Serialize};

use crate::command_event::CommandProgress;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RunJournalPhase {
    GuardGlobal,
    GuardCommand,
    Run,
    Worker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RunJournalStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunJournalEvent {
    pub sequence: u64,
    pub timestamp_unix_ms: u64,
    pub phase: RunJournalPhase,
    #[serde(flatten)]
    pub data: RunJournalEventData,
}

impl RunJournalEvent {
    pub(crate) fn retained_bytes(&self) -> usize {
        match &self.data {
            RunJournalEventData::Output { text, .. } => text.len(),
            RunJournalEventData::Progress { progress } => progress.message.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum RunJournalEventData {
    Output {
        stream: RunJournalStream,
        text: String,
    },
    Progress {
        #[serde(flatten)]
        progress: CommandProgress,
    },
}
