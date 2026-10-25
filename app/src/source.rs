//! Source files shown in the source view, read from the paths the adapter gives in stack
//! frames (the paths the program was built with).

use std::collections::HashMap;
use std::sync::Arc;

use gpui::SharedString;

/// Tabs are shown as spaces up to the next multiple of this.
const TAB_WIDTH: usize = 4;

pub struct SourceFile {
    pub path: String,
    pub lines: Vec<SharedString>,
}

impl SourceFile {
    fn parse(path: &str, text: &str) -> Self {
        Self {
            path: path.to_string(),
            lines: text.lines().map(|line| expand_tabs(line).into()).collect(),
        }
    }
}

fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        if c == '\t' {
            let spaces = TAB_WIDTH - out.chars().count() % TAB_WIDTH;
            out.extend(std::iter::repeat_n(' ', spaces));
        } else {
            out.push(c);
        }
    }
    out
}

/// The files read so far, and the ones that could not be read (asked once).
#[derive(Default)]
pub struct SourceFiles {
    files: HashMap<String, Result<Arc<SourceFile>, String>>,
}

impl SourceFiles {
    pub fn get(&mut self, path: &str) -> Result<Arc<SourceFile>, String> {
        self.files
            .entry(path.to_string())
            .or_insert_with(|| {
                std::fs::read(path)
                    .map(|bytes| {
                        Arc::new(SourceFile::parse(path, &String::from_utf8_lossy(&bytes)))
                    })
                    .map_err(|error| format!("{path}: {error}"))
            })
            .clone()
    }

    /// Forgets the files read, so that they are read again (they were edited).
    pub fn reload(&mut self) {
        self.files.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_lose_their_line_ends_and_tabs() {
        let file = SourceFile::parse("demo.c", "int a;\r\n\tx = 1;\r\nab\tc\n");
        let lines = file.lines.iter().map(|l| l.as_ref()).collect::<Vec<&str>>();
        assert_eq!(lines, ["int a;", "    x = 1;", "ab  c"]);
    }
}
