//! The probe-rs requests and events outside DAP, and the DAP `disassemble` request with signed
//! offsets. The requests implement [`dap_types::requests::Request`], like the DAP ones.

use dap_types::requests::Request;
use serde::{Deserialize, Serialize};

/// Arguments of the probe-rs `liveWatch` request: global variables read without halting the
/// target. `core` picks the core in a session of several cores.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWatchArguments {
    pub expressions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<u64>,
}

/// The value of one expression of a `liveWatch` request, or why it could not be read.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWatchValue {
    pub expression: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWatchResponse {
    pub values: Vec<LiveWatchValue>,
}

/// The probe-rs `liveWatch` request.
pub enum LiveWatch {}

impl Request for LiveWatch {
    const COMMAND: &'static str = "liveWatch";
    type Arguments = LiveWatchArguments;
    type Response = LiveWatchResponse;
}

/// One memory range of a `readRanges` request (`count` bytes at `address`).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRange {
    pub address: u64,
    pub count: usize,
}

/// Arguments of the probe-rs fork's `readRanges` request: several memory ranges in one
/// request, each read on its own. `core` picks the core in a session of several cores.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadRangesArguments {
    pub ranges: Vec<MemoryRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<u64>,
}

/// One range of a `readRanges` answer: its bytes (base64), or why it could not be read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeData {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadRangesResponse {
    pub ranges: Vec<RangeData>,
}

/// The probe-rs fork's `readRanges` request.
pub enum ReadRanges {}

impl Request for ReadRanges {
    const COMMAND: &'static str = "readRanges";
    type Arguments = ReadRangesArguments;
    type Response = ReadRangesResponse;
}

/// Arguments of the probe-rs `rttWindowOpened` request: the client shows the RTT channel, so
/// the adapter starts sending its data.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RttWindowOpenedArguments {
    pub channel_number: u32,
    pub window_is_open: bool,
    /// The core the channel's event named (a session of several cores).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<u64>,
}

/// The probe-rs `rttWindowOpened` request.
pub enum RttWindowOpened {}

impl Request for RttWindowOpened {
    const COMMAND: &'static str = "rttWindowOpened";
    type Arguments = RttWindowOpenedArguments;
    type Response = ();
}

/// The event that announces an RTT up channel.
pub const RTT_CHANNEL_CONFIG_EVENT: &str = "probe-rs-rtt-channel-config";
/// The event that carries data of an RTT up channel.
pub const RTT_DATA_EVENT: &str = "probe-rs-rtt-data";

/// The body of a [`RTT_CHANNEL_CONFIG_EVENT`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RttChannelConfig {
    pub channel_number: u32,
    #[serde(default)]
    pub channel_name: String,
    #[serde(default)]
    pub data_format: String,
    /// The core of the channel, in a session of several cores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<u64>,
}

/// The body of a [`RTT_DATA_EVENT`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RttData {
    pub channel_number: u32,
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<u64>,
}

/// The event that announces an RTT down channel (input to the target).
pub const RTT_PROMPT_EVENT: &str = "probe-rs-create-prompt";

/// The body of a [`RTT_PROMPT_EVENT`]: the down channel's number is the handle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RttPrompt {
    /// `rtt` for an RTT down channel.
    pub prompt_kind: String,
    #[serde(default)]
    pub prompt_name: String,
    pub prompt_handle: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<u64>,
}

/// Arguments of an `evaluate` request in the `repl` context: a probe-rs debug console command
/// (`rtt write 0 hello`), on `core` in a session of several cores.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplCommandArguments {
    pub expression: String,
    pub context: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<u64>,
}

/// A probe-rs debug console command. Its answer's `result` is empty when the command did what
/// it was asked, else the reason it did not (probe-rs answers with success either way).
pub enum ReplCommand {}

impl Request for ReplCommand {
    const COMMAND: &'static str = "evaluate";
    type Arguments = ReplCommandArguments;
    type Response = dap_types::EvaluateResponse;
}

/// Arguments of a `disassemble` request with signed offsets: DAP allows a negative instruction
/// offset (the instructions before the reference), `dap_types::DisassembleArguments` has it
/// unsigned.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisassembleSignedArguments {
    pub memory_reference: String,
    pub instruction_offset: i64,
    pub instruction_count: u64,
    pub resolve_symbols: bool,
}

/// The DAP `disassemble` request with [`DisassembleSignedArguments`].
pub enum DisassembleSigned {}

impl Request for DisassembleSigned {
    const COMMAND: &'static str = "disassemble";
    type Arguments = DisassembleSignedArguments;
    type Response = dap_types::DisassembleResponse;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requests_and_events_have_the_probe_rs_shape() {
        let arguments = RttWindowOpenedArguments {
            channel_number: 0,
            window_is_open: true,
            core: Some(1),
        };
        assert_eq!(
            serde_json::to_value(&arguments).unwrap(),
            json!({ "channelNumber": 0, "windowIsOpen": true, "core": 1 })
        );
        let one_core = LiveWatchArguments {
            expressions: vec!["g_ticks".into()],
            core: None,
        };
        assert_eq!(
            serde_json::to_value(&one_core).unwrap(),
            json!({ "expressions": ["g_ticks"] })
        );

        let config: RttChannelConfig = serde_json::from_value(json!({
            "channelName": "Terminal", "channelNumber": 0, "dataFormat": "String"
        }))
        .unwrap();
        assert_eq!(config.core, None);
        let value: LiveWatchValue = serde_json::from_value(json!({
            "expression": "g_ticks", "type": "volatile unsigned int", "value": "36215"
        }))
        .unwrap();
        assert_eq!(value.value.as_deref(), Some("36215"));
        assert_eq!(
            serde_json::to_value(DisassembleSignedArguments {
                memory_reference: "0x8021c028@0".into(),
                instruction_offset: -24,
                instruction_count: 80,
                resolve_symbols: true,
            })
            .unwrap()["instructionOffset"],
            -24
        );
    }
}
