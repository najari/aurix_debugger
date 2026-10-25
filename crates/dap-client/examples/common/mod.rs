//! What the examples share: a session with the probe-rs DAP server.

use std::time::Duration;

use aurix_dap_client::DapClient;
use serde_json::{Value, json};

pub const TIMEOUT: Duration = Duration::from_secs(10);

/// Connects to the server (`DAP_SERVER`, default 127.0.0.1:50000), attaches with the arguments
/// in the JSON file `attach`, ends the configuration and lets every thread run.
pub fn start(attach: &str) -> Result<DapClient, Box<dyn std::error::Error>> {
    let server = std::env::var("DAP_SERVER").unwrap_or_else(|_| "127.0.0.1:50000".into());
    let arguments: Value = serde_json::from_str(&std::fs::read_to_string(attach)?)?;
    let client = DapClient::connect(server.as_str())?;
    client.request_raw(
        "initialize",
        json!({
            "clientID": "aurix-dap-client", "adapterID": "probe-rs", "linesStartAt1": true,
            "columnsStartAt1": true, "pathFormat": "path", "supportsMemoryReferences": true,
        }),
        TIMEOUT,
    )?;
    client.request_raw("attach", arguments, Duration::from_secs(60))?;
    client.request_raw("configurationDone", json!({}), TIMEOUT)?;
    for (id, _) in threads(&client)? {
        // A core that runs already answers with an error.
        client
            .request_raw("continue", json!({ "threadId": id }), TIMEOUT)
            .ok();
    }
    Ok(client)
}

/// The session's threads (id, name).
pub fn threads(client: &DapClient) -> Result<Vec<(u64, String)>, Box<dyn std::error::Error>> {
    let threads =
        client.request::<dap_types::requests::Threads>(dap_types::ThreadsArgument {}, TIMEOUT)?;
    Ok(threads
        .threads
        .into_iter()
        .filter_map(|thread| Some((u64::try_from(thread.id).ok()?, thread.name)))
        .collect())
}

/// Halts every thread and disconnects, leaving the target halted.
pub fn stop(client: &DapClient) {
    if let Ok(threads) = threads(client) {
        for (id, _) in threads {
            client
                .request_raw("pause", json!({ "threadId": id }), TIMEOUT)
                .ok();
        }
    }
    client
        .request_raw("disconnect", json!({ "terminateDebuggee": false }), TIMEOUT)
        .ok();
}
