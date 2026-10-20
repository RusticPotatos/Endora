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
