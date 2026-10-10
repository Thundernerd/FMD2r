//! A minimal `TStringList` with name/value access, as Synapse uses for headers and cookies.

/// Ordered lines with `Name<sep>Value` access, case-insensitive on names like FMD2's
/// `Headers` and `Cookies` lists (baseunits/httpsendthread.pas:503-508).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameValueList {
    lines: Vec<String>,
    separator: char,
}

impl NameValueList {
    pub(crate) fn new(separator: char) -> Self {
        Self {
            lines: Vec::new(),
            separator,
        }
    }

    /// The separator between a line's name and value (`NameValueSeparator`).
    pub fn separator(&self) -> char {
        self.separator
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Raw access to the lines, like assigning `TStrings.Strings[i]` or `Text`.
    pub fn lines_mut(&mut self) -> &mut Vec<String> {
        &mut self.lines
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn push(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// The name part of line `i`; empty when the line has no separator (`Names[i]`).
    pub fn name_at(&self, i: usize) -> &str {
        self.lines
            .get(i)
            .and_then(|l| l.split_once(self.separator))
            .map_or("", |(name, _)| name)
    }

    /// The value part of line `i`; the whole line when it has no separator
    /// (`ValueFromIndex[i]`).
    pub fn value_at(&self, i: usize) -> &str {
        self.lines.get(i).map_or("", |l| {
            l.split_once(self.separator)
                .map_or(l.as_str(), |(_, value)| value)
        })
    }

    /// Index of the first line whose name equals `name`, ignoring case (`IndexOfName`).
    pub fn index_of_name(&self, name: &str) -> Option<usize> {
        (0..self.lines.len()).find(|&i| {
            self.lines[i]
                .split_once(self.separator)
                .is_some_and(|(n, _)| n.eq_ignore_ascii_case(name))
        })
    }

    /// Index of the line equal to `line`, ignoring case (`IndexOf`).
    pub fn index_of(&self, line: &str) -> Option<usize> {
        self.lines.iter().position(|l| l.eq_ignore_ascii_case(line))
    }

    /// `Values[name]`.
    pub fn value(&self, name: &str) -> &str {
        self.index_of_name(name).map_or("", |i| self.value_at(i))
    }

    /// Sets `name`'s line to `name<sep>value`, appending it when missing; an empty value
    /// deletes the line (`Values[name] := value`).
    pub fn set_value(&mut self, name: &str, value: &str) {
        let index = self.index_of_name(name);
        if value.is_empty() {
            if let Some(i) = index {
                self.lines.remove(i);
            }
            return;
        }
        let line = format!("{name}{}{value}", self.separator);
        match index {
            Some(i) => self.lines[i] = line,
            None => self.lines.push(line),
        }
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.lines.len() {
            self.lines.remove(i);
        }
    }
}
