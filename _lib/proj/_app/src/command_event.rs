use serde::{Deserialize, Serialize};

pub(crate) const COMMAND_EVENT_PROTOCOL_ENV: &str = "SWAWKIT_PROJ_CORE_COMMAND_EVENT_PROTOCOL";
pub(crate) const COMMAND_EVENT_FRAME_PROTOCOL: &str = "swawkit.command-event-frame/v1";
const COMMAND_EVENT_SCHEMA: &str = "swawkit.command-event/v1";
const FRAME_PREFIX: &str = "\u{001e}swawkit-event-v1 ";
const MAX_FRAME_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CommandProgressState {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CommandProgressUnit {
    Bytes,
    Items,
    Percent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandProgress {
    pub id: String,
    pub state: CommandProgressState,
    pub current: Option<u64>,
    pub total: Option<u64>,
    pub unit: CommandProgressUnit,
    pub message: String,
}

impl CommandProgress {
    fn validate(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 128
            && self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
            && !self.message.is_empty()
            && self.message.len() <= 512
            && !self
                .message
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\t'))
            && match (self.current, self.total) {
                (Some(current), Some(total)) => total > 0 && current <= total,
                (None, Some(_)) => false,
                _ => true,
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CapturedCommandEvent {
    Output(String),
    Progress(CommandProgress),
}

#[derive(Default)]
pub(crate) struct CommandEventFrameDecoder {
    pending: String,
}

impl CommandEventFrameDecoder {
    pub(crate) fn push(&mut self, text: &str) -> Vec<CapturedCommandEvent> {
        if !text.is_empty() {
            self.pending.push_str(text);
        }
        self.drain(false)
    }

    pub(crate) fn finish(&mut self) -> Vec<CapturedCommandEvent> {
        self.drain(true)
    }

    fn drain(&mut self, eof: bool) -> Vec<CapturedCommandEvent> {
        let mut events = Vec::new();
        loop {
            let Some(prefix_index) = self.pending.find(FRAME_PREFIX) else {
                let retained = if eof {
                    0
                } else {
                    matching_prefix_suffix(&self.pending, FRAME_PREFIX)
                };
                let output_length = self.pending.len().saturating_sub(retained);
                if output_length > 0 {
                    events.push(CapturedCommandEvent::Output(
                        self.pending.drain(..output_length).collect(),
                    ));
                }
                break;
            };
            if prefix_index > 0 {
                events.push(CapturedCommandEvent::Output(
                    self.pending.drain(..prefix_index).collect(),
                ));
                continue;
            }
            let Some(newline_index) = self.pending.find('\n') else {
                if eof || self.pending.len() > MAX_FRAME_BYTES {
                    events.push(CapturedCommandEvent::Output(std::mem::take(
                        &mut self.pending,
                    )));
                }
                break;
            };
            let record: String = self.pending.drain(..=newline_index).collect();
            if record.len() > MAX_FRAME_BYTES {
                events.push(CapturedCommandEvent::Output(record));
                continue;
            }
            let payload = record
                .strip_prefix(FRAME_PREFIX)
                .expect("event record starts with its frame prefix")
                .trim_end_matches(['\r', '\n']);
            match parse_progress(payload) {
                Some(progress) => events.push(CapturedCommandEvent::Progress(progress)),
                None => events.push(CapturedCommandEvent::Output(record)),
            }
        }
        events
    }
}

fn matching_prefix_suffix(value: &str, prefix: &str) -> usize {
    let maximum = value.len().min(prefix.len().saturating_sub(1));
    (1..=maximum)
        .rev()
        .find(|length| value.ends_with(&prefix[..*length]))
        .unwrap_or(0)
}

fn parse_progress(payload: &str) -> Option<CommandProgress> {
    let frame: ProgressFrame = serde_json::from_str(payload).ok()?;
    if frame.schema != COMMAND_EVENT_SCHEMA
        || frame.kind != ProgressFrameKind::Progress
        || !frame.progress.validate()
    {
        return None;
    }
    Some(frame.progress)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgressFrame {
    schema: String,
    kind: ProgressFrameKind,
    #[serde(flatten)]
    progress: CommandProgress,
}

#[derive(PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ProgressFrameKind {
    Progress,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: &str = "\u{001e}swawkit-event-v1 {\"schema\":\"swawkit.command-event/v1\",\"kind\":\"progress\",\"id\":\"download:fixture.zip\",\"state\":\"running\",\"current\":null,\"total\":null,\"unit\":\"bytes\",\"message\":\"Downloading fixture.zip\"}\n";

    #[test]
    fn extracts_a_frame_split_across_reads_without_delaying_neighboring_output() {
        let mut decoder = CommandEventFrameDecoder::default();

        assert_eq!(
            decoder.push(&format!("before\n{}", &FRAME[..12])),
            vec![CapturedCommandEvent::Output("before\n".to_owned())]
        );
        let events = decoder.push(&FRAME[12..]);

        assert_eq!(events.len(), 1);
        let CapturedCommandEvent::Progress(progress) = &events[0] else {
            panic!("expected progress event");
        };
        assert_eq!(progress.id, "download:fixture.zip");
        assert_eq!(progress.state, CommandProgressState::Running);
        assert!(decoder.finish().is_empty());
    }

    #[test]
    fn preserves_invalid_or_incomplete_frames_as_ordinary_output() {
        let invalid = "\u{001e}swawkit-event-v1 {\"kind\":\"progress\"}\n";
        let incomplete = "\u{001e}swawkit-event-v1 {\"schema\":";
        let mut decoder = CommandEventFrameDecoder::default();

        assert_eq!(
            decoder.push(invalid),
            vec![CapturedCommandEvent::Output(invalid.to_owned())]
        );
        assert!(decoder.push(incomplete).is_empty());
        assert_eq!(
            decoder.finish(),
            vec![CapturedCommandEvent::Output(incomplete.to_owned())]
        );
    }

    #[test]
    fn bounds_an_unterminated_frame_and_falls_back_to_output() {
        let oversized = format!("{FRAME_PREFIX}{}", "x".repeat(MAX_FRAME_BYTES));
        let mut decoder = CommandEventFrameDecoder::default();

        assert_eq!(
            decoder.push(&oversized),
            vec![CapturedCommandEvent::Output(oversized)]
        );
        assert!(decoder.finish().is_empty());
    }
}
