//! The lines of the target's RTT up channels. probe-rs announces each channel
//! ([`RttChannelConfig`]) and sends its data ([`RttData`]) only once the client answered with
//! `rttWindowOpened`. In a session of several cores each core has its own channels, numbered from
//! 0, and the events name the core.

use std::collections::VecDeque;

use crate::protocol::{RttChannelConfig, RttData, RttWindowOpenedArguments};

/// The lines a channel keeps, at most (the oldest go first).
pub const MAX_LINES: usize = 10_000;

/// A channel: its core (in a session of several cores) and its number.
pub type ChannelKey = (Option<u64>, u32);

#[derive(Clone, Debug, Default)]
pub struct RttChannel {
    pub core: Option<u64>,
    pub number: u32,
    pub name: String,
    pub data_format: String,
    /// Complete lines.
    pub lines: VecDeque<String>,
    /// The text after the last line break.
    pub partial: String,
    /// The lines dropped from the front (to keep at most [`MAX_LINES`]), or cleared.
    pub dropped: usize,
}

impl RttChannel {
    pub fn key(&self) -> ChannelKey {
        (self.core, self.number)
    }

    /// A name to show: `core 1 · 0: Terminal`, or `0: Terminal` with one core.
    pub fn label(&self) -> String {
        let channel = if self.name.is_empty() {
            format!("{}", self.number)
        } else {
            format!("{}: {}", self.number, self.name)
        };
        match self.core {
            Some(core) => format!("core {core} · {channel}"),
            None => channel,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RttChannels {
    channels: Vec<RttChannel>,
}

impl RttChannels {
    pub fn channels(&self) -> &[RttChannel] {
        &self.channels
    }

    pub fn get(&self, key: ChannelKey) -> Option<&RttChannel> {
        self.channels.iter().find(|c| c.key() == key)
    }

    fn entry(&mut self, key: ChannelKey) -> &mut RttChannel {
        match self.channels.iter().position(|c| c.key() == key) {
            Some(ix) => &mut self.channels[ix],
            None => {
                self.channels.push(RttChannel {
                    core: key.0,
                    number: key.1,
                    ..Default::default()
                });
                self.channels.last_mut().unwrap()
            }
        }
    }

    /// A channel the adapter announced: noted, and the `rttWindowOpened` request to send.
    pub fn announced(&mut self, config: RttChannelConfig) -> RttWindowOpenedArguments {
        let channel = self.entry((config.core, config.channel_number));
        channel.name = config.channel_name;
        channel.data_format = config.data_format;
        RttWindowOpenedArguments {
            channel_number: config.channel_number,
            window_is_open: true,
            core: config.core,
        }
    }

    /// Data of a channel: split into lines. Returns how many lines it completed.
    pub fn received(&mut self, data: RttData) -> usize {
        let channel = self.entry((data.core, data.channel_number));
        let mut text = std::mem::take(&mut channel.partial);
        text.push_str(&data.data);
        let mut rest = text.as_str();
        let mut count = 0;
        while let Some(end) = rest.find('\n') {
            channel
                .lines
                .push_back(rest[..end].trim_end_matches('\r').to_string());
            rest = &rest[end + 1..];
            count += 1;
        }
        channel.partial = rest.to_string();
        while channel.lines.len() > MAX_LINES {
            channel.lines.pop_front();
            channel.dropped += 1;
        }
        count
    }

    /// Clears a channel's lines.
    pub fn clear(&mut self, key: ChannelKey) {
        if let Some(channel) = self.channels.iter_mut().find(|c| c.key() == key) {
            channel.dropped += channel.lines.len();
            channel.lines.clear();
            channel.partial.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(core: Option<u64>) -> RttChannelConfig {
        RttChannelConfig {
            channel_number: 0,
            channel_name: "Terminal".into(),
            data_format: "String".into(),
            core,
        }
    }

    fn data(core: Option<u64>, text: &str) -> RttData {
        RttData {
            channel_number: 0,
            data: text.into(),
            core,
        }
    }

    #[test]
    fn the_channels_of_two_cores_stay_apart() {
        let mut rtt = RttChannels::default();
        let open = rtt.announced(config(Some(0)));
        assert_eq!(open.core, Some(0));
        rtt.announced(config(Some(1)));
        assert_eq!(rtt.received(data(Some(0), "CPU0 line 1\nCPU0 li")), 1);
        assert_eq!(rtt.received(data(Some(1), "CPU1 line 1\r\n")), 1);
        assert_eq!(rtt.received(data(Some(0), "ne 2\n")), 1);

        let core0 = rtt.get((Some(0), 0)).unwrap();
        assert_eq!(core0.lines, ["CPU0 line 1", "CPU0 line 2"]);
        assert_eq!(core0.label(), "core 0 · 0: Terminal");
        assert_eq!(rtt.get((Some(1), 0)).unwrap().lines, ["CPU1 line 1"]);

        rtt.clear((Some(0), 0));
        let core0 = rtt.get((Some(0), 0)).unwrap();
        assert!(core0.lines.is_empty());
        assert_eq!(core0.dropped, 2);
    }

    #[test]
    fn a_channel_keeps_the_newest_lines() {
        let mut rtt = RttChannels::default();
        rtt.announced(config(None));
        let text = (0..MAX_LINES + 5)
            .map(|i| format!("{i}\n"))
            .collect::<String>();
        rtt.received(data(None, &text));
        let channel = rtt.get((None, 0)).unwrap();
        assert_eq!(channel.lines.len(), MAX_LINES);
        assert_eq!(channel.dropped, 5);
        assert_eq!(channel.lines.front().map(String::as_str), Some("5"));
        assert_eq!(channel.label(), "0: Terminal");
    }
}
