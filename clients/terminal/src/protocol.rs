use serde_json::Value;

pub fn shortcut(
    code: crossterm::event::KeyCode,
    command_context: bool,
) -> crossterm::event::KeyCode {
    if command_context {
        if let crossterm::event::KeyCode::Char(c) = code {
            return crossterm::event::KeyCode::Char(c.to_ascii_lowercase());
        }
    }
    code
}

pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect()
}

#[derive(Default)]
pub struct Sse {
    buffer: Vec<u8>,
    data: Vec<String>,
    frame_size: usize,
}

impl Sse {
    pub fn push(&mut self, bytes: &[u8]) -> crate::Result<Vec<Value>> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 1024 * 1024 {
            return Err("Stream frame exceeds display budget".into());
        }
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.buffer.drain(..=end).collect();
            let line = std::str::from_utf8(&line)
                .map_err(|_| "Invalid stream encoding")?
                .trim_end_matches(['\n', '\r']);
            self.frame_size += line.len();
            if self.frame_size > 1024 * 1024 {
                return Err("Stream frame exceeds display budget".into());
            }
            if line.is_empty() {
                if !self.data.is_empty() {
                    events.push(
                        serde_json::from_str(&self.data.join("\n"))
                            .map_err(|_| "Invalid stream event")?,
                    );
                    self.data.clear();
                }
                self.frame_size = 0;
                continue;
            }
            if let Some(data) = line.strip_prefix("data:") {
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).to_owned());
            }
        }
        Ok(events)
    }
}
