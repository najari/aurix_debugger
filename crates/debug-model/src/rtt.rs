//! The lines of the target's RTT up channels. probe-rs announces each channel
//! ([`RttChannelConfig`]) and sends its data ([`RttData`]) only once the client answered with
//! `rttWindowOpened`. In a session of several cores each core has its own channels, numbered from
//! 0, and the events name the core.
//!
//! The down channels (input to the target) are announced as prompts ([`RttPrompt`]); a line
//! goes to one with the debug console command `rtt write <channel> <text>` (probe-rs adds the
//! line break). An up channel's tab sends to the down channel of its core with the same number,
//! else to that core's first one.

use std::collections::VecDeque;

use crate::protocol::{
    ReplCommandArguments, RttChannelConfig, RttData, RttPrompt, RttWindowOpenedArguments,
};

/// What a line sent to the target looks like among the lines received.
pub const SENT_MARK: &str = "» ";

/// A down channel: its core (in a session of several cores), number and name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownChannel {
    pub core: Option<u64>,
    pub number: u32,
    pub name: String,
}

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
    down: Vec<DownChannel>,
}

impl RttChannels {
    /// A down channel the adapter announced (prompts of other kinds are not RTT).
    pub fn prompted(&mut self, prompt: RttPrompt) {
        if prompt.prompt_kind != "rtt" {
            return;
        }
        let channel = DownChannel {
            core: prompt.core,
            number: prompt.prompt_handle,
            name: prompt.prompt_name,
        };
        match self
            .down
            .iter_mut()
            .find(|d| d.core == channel.core && d.number == channel.number)
        {
            Some(known) => *known = channel,
            None => self.down.push(channel),
        }
    }

    /// The down channel the tab of up channel `key` sends to.
    pub fn down_for(&self, key: ChannelKey) -> Option<&DownChannel> {
        let of_core = || self.down.iter().filter(|d| d.core == key.0);
        of_core()
            .find(|d| d.number == key.1)
            .or_else(|| of_core().next())
    }

    /// The command that sends `text` (one line) from the tab of up channel `key`, and notes it
    /// among the tab's lines; none when the core has no down channel or the text is empty.
    pub fn send(&mut self, key: ChannelKey, text: &str) -> Option<ReplCommandArguments> {
        let text = text.lines().next().unwrap_or_default().trim_end();
        if text.is_empty() {
            return None;
        }
        let down = self.down_for(key)?.clone();
        if let Some(channel) = self.channels.iter_mut().find(|c| c.key() == key) {
            channel.lines.push_back(format!("{SENT_MARK}{text}"));
            while channel.lines.len() > MAX_LINES {
                channel.lines.pop_front();
                channel.dropped += 1;
            }
        }
        Some(ReplCommandArguments {
            expression: format!("rtt write {} {text}", down.number),
            context: "repl".into(),
            core: down.core,
        })
    }

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

    fn prompt(core: Option<u64>, number: u32, kind: &str) -> RttPrompt {
        RttPrompt {
            prompt_kind: kind.into(),
            prompt_name: "Input".into(),
            prompt_handle: number,
            core,
        }
    }

    #[test]
    fn a_line_goes_to_the_down_channel_of_the_tabs_core() {
        let mut rtt = RttChannels::default();
        rtt.announced(config(Some(0)));
        rtt.announced(config(Some(1)));
        // Not RTT: ignored.
        rtt.prompted(prompt(Some(0), 0, "other"));
        assert!(rtt.send((Some(0), 0), "hello").is_none());

        rtt.prompted(prompt(Some(1), 0, "rtt"));
        // Core 0 has no down channel; core 1 has.
        assert!(rtt.down_for((Some(0), 0)).is_none());
        assert!(rtt.send((Some(0), 0), "hello").is_none());
        let command = rtt.send((Some(1), 0), "hello world\nsecond").unwrap();
        assert_eq!(command.expression, "rtt write 0 hello world");
        assert_eq!((command.context.as_str(), command.core), ("repl", Some(1)));
        let lines = &rtt.get((Some(1), 0)).unwrap().lines;
        assert_eq!(lines.back().unwrap(), "» hello world");
        assert!(rtt.send((Some(1), 0), "  ").is_none());

        // Another number: the core's first down channel.
        rtt.prompted(prompt(Some(1), 2, "rtt"));
        assert_eq!(rtt.down_for((Some(1), 5)).unwrap().number, 0);
        assert_eq!(rtt.down_for((Some(1), 2)).unwrap().number, 2);
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
