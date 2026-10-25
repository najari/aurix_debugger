//! The attach files used before, newest first, kept in `%APPDATA%\aurix-debugger\recent.txt`
//! (one path per line), and what an attach file says (chip, probe, cores and their programs).

use std::path::{Path, PathBuf};

/// How many attach files are remembered.
const MAX_RECENT: usize = 12;

fn recent_file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME"))?;
    Some(
        PathBuf::from(base)
            .join("aurix-debugger")
            .join("recent.txt"),
    )
}

/// The attach files used before, newest first.
pub fn load() -> Vec<String> {
    recent_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Puts `attach` first in the list and saves it.
pub fn remember(recent: &mut Vec<String>, attach: &str) -> Result<(), String> {
    push_front(recent, attach);
    let Some(path) = recent_file() else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    let text = recent.iter().map(|p| format!("{p}\n")).collect::<String>();
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))
}

fn parse(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !out.iter().any(|p: &String| same_path(p, line)) {
            out.push(line.to_string());
        }
    }
    out.truncate(MAX_RECENT);
    out
}

fn push_front(recent: &mut Vec<String>, attach: &str) {
    recent.retain(|p| !same_path(p, attach));
    recent.insert(0, attach.to_string());
    recent.truncate(MAX_RECENT);
}

/// Windows paths: the case and the separators do not matter.
pub fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// What an attach file says, for the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachSummary {
    pub chip: Option<String>,
    pub probe: Option<String>,
    /// Core index, program file name, RTT enabled.
    pub cores: Vec<(u64, String, bool)>,
}

impl AttachSummary {
    pub fn read(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        Self::parse(&text)
    }

    fn parse(text: &str) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| error.to_string())?;
        let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(String::from);
        let cores = value
            .get("coreConfigs")
            .and_then(|v| v.as_array())
            .map(|cores| {
                cores
                    .iter()
                    .map(|core| {
                        let index = core.get("coreIndex").and_then(|v| v.as_u64()).unwrap_or(0);
                        let program = core
                            .get("programBinary")
                            .and_then(|v| v.as_str())
                            .map(|p| file_name(p).to_string())
                            .unwrap_or_default();
                        let rtt = core.get("rttEnabled").and_then(|v| v.as_bool()) == Some(true);
                        (index, program, rtt)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            chip: text("chip"),
            probe: text("probe"),
            cores,
        })
    }

    /// One line: `TC275 · core 0 demo_flash.elf, core 1 rtt_demo_cpu1.elf (RTT)`.
    pub fn line(&self) -> String {
        let cores = self
            .cores
            .iter()
            .map(|(index, program, rtt)| {
                format!("core {index} {program}{}", if *rtt { " (RTT)" } else { "" })
            })
            .collect::<Vec<_>>()
            .join(", ");
        match &self.chip {
            Some(chip) if cores.is_empty() => chip.clone(),
            Some(chip) => format!("{chip} · {cores}"),
            None => cores,
        }
    }
}

pub fn file_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_comes_first_once() {
        let mut recent = parse("C:\\a.json\n\nc:/A.json\nC:\\b.json\n");
        assert_eq!(recent, ["C:\\a.json", "C:\\b.json"]);
        push_front(&mut recent, "C:\\B.JSON");
        assert_eq!(recent, ["C:\\B.JSON", "C:\\a.json"]);
    }

    #[test]
    fn an_attach_file_is_summed_up() {
        let summary = AttachSummary::parse(
            r#"{ "chip": "TC275", "probe": "058b:0000", "coreConfigs": [
                { "coreIndex": 0, "programBinary": "C:\\demo\\build\\rtt_demo.elf", "rttEnabled": true },
                { "coreIndex": 1, "programBinary": "C:\\demo\\build\\rtt_demo_cpu1.elf" } ] }"#,
        )
        .unwrap();
        assert_eq!(
            summary.line(),
            "TC275 · core 0 rtt_demo.elf (RTT), core 1 rtt_demo_cpu1.elf"
        );
        assert!(AttachSummary::parse("not json").is_err());
    }
}
