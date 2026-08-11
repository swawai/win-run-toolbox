use std::env;

use serde_json::json;

const PROTOCOL_ENV: &str = "SWAWKIT_PROJ_CORE_COMMAND_EVENT_PROTOCOL";
const FRAME_PROTOCOL: &str = "swawkit.command-event-frame/v1";
const FRAME_PREFIX: &str = "\u{001e}swawkit-event-v1 ";

pub(crate) fn progress(
    id: &str,
    state: &str,
    current: Option<u64>,
    total: Option<u64>,
    message: &str,
) {
    if env::var(PROTOCOL_ENV).as_deref() != Ok(FRAME_PROTOCOL) {
        return;
    }
    let value = json!({
        "schema": "swawkit.command-event/v1",
        "kind": "progress",
        "id": id,
        "state": state,
        "current": current,
        "total": total,
        "unit": "bytes",
        "message": message,
    });
    eprintln!("{FRAME_PREFIX}{value}");
}
