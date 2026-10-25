//! Prints the RTT output of a probe-rs session, every core's channels apart.
//!
//! ```text
//! cargo run --example rtt_tail -- <attach.json> [seconds, default 5]
//! ```
//!
//! The attach arguments need `rttEnabled` in their `coreConfigs`; see
//! `examples/tc275_rtt_cpu0_cpu1.json`. The target is left halted.

mod common;

use std::time::{Duration, Instant};

use aurix_debug_model::protocol::{
    RTT_CHANNEL_CONFIG_EVENT, RTT_DATA_EVENT, RttChannelConfig, RttData, RttWindowOpened,
};
use aurix_debug_model::rtt::RttChannels;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let attach = args
        .next()
        .ok_or("usage: rtt_tail <attach.json> [seconds]")?;
    let seconds: u64 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(5);

    let client = common::start(&attach)?;
    let mut rtt = RttChannels::default();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Some(event) = client.next_event(left) else {
            break;
        };
        match event.event.as_str() {
            RTT_CHANNEL_CONFIG_EVENT => {
                let config: RttChannelConfig = serde_json::from_value(event.body)?;
                let open = rtt.announced(config);
                let key = (open.core, open.channel_number);
                client.request::<RttWindowOpened>(open, common::TIMEOUT)?;
                println!("[{}] opened", rtt.get(key).unwrap().label());
            }
            RTT_DATA_EVENT => {
                let data: RttData = serde_json::from_value(event.body)?;
                let key = (data.core, data.channel_number);
                let new = rtt.received(data);
                let channel = rtt.get(key).unwrap();
                for line in channel.lines.iter().skip(channel.lines.len() - new) {
                    println!("[{}] {line}", channel.label());
                }
            }
            _ => {}
        }
    }
    for channel in rtt.channels() {
        println!("{}: {} lines", channel.label(), channel.lines.len());
    }
    common::stop(&client);
    Ok(())
}
