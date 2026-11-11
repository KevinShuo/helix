//! Macro events include literal insertions, so generated code replays without
//! inference, auto-pairing, or interpretation as editor commands.

use crate::input::{parse_macro, KeyEvent};
use std::fmt::{self, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroEvent {
    Key(KeyEvent),
    Paste(String),
    /// A companion command from a binding which accepted literal AI text.
    Command(String),
}

impl fmt::Display for MacroEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(key) => {
                let key = key.to_string();
                if key.chars().count() == 1 {
                    f.write_str(&key)
                } else {
                    write!(f, "<{key}>")
                }
            }
            Self::Paste(text) | Self::Command(text) => {
                f.write_str(if matches!(self, Self::Paste(_)) {
                    "<paste:"
                } else {
                    "<command:"
                })?;
                for byte in text.bytes() {
                    write!(f, "{byte:02x}")?;
                }
                f.write_char('>')
            }
        }
    }
}

pub fn parse_macro_events(mut input: &str) -> anyhow::Result<Vec<MacroEvent>> {
    let mut events = Vec::new();
    while let Some((start, prefix)) = ["<paste:", "<command:"]
        .into_iter()
        .filter_map(|prefix| input.find(prefix).map(|start| (start, prefix)))
        .min_by_key(|(start, _)| *start)
    {
        events.extend(
            parse_macro(&input[..start])?
                .into_iter()
                .map(MacroEvent::Key),
        );
        let rest = &input[start + prefix.len()..];
        let end = rest
            .find('>')
            .ok_or_else(|| anyhow::anyhow!("Unterminated literal macro insertion"))?;
        let encoded = &rest[..end];
        anyhow::ensure!(
            encoded.len() <= 2_097_152 && encoded.len() % 2 == 0 && encoded.is_ascii(),
            "Invalid literal macro insertion"
        );
        let bytes: Result<Vec<_>, _> = (0..encoded.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16))
            .collect();
        let text = String::from_utf8(bytes?)?;
        events.push(if prefix == "<paste:" {
            MacroEvent::Paste(text)
        } else {
            MacroEvent::Command(text)
        });
        input = &rest[end + 1..];
    }
    events.extend(parse_macro(input)?.into_iter().map(MacroEvent::Key));
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_macro_data_is_not_interpreted_as_keys() {
        let text = "foo(\"值\")\n<esc>:quit!";
        let paste = MacroEvent::Paste(text.into());
        let serialized = format!("i{paste}<esc>");
        let events = parse_macro_events(&serialized).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[1], paste);
        for invalid in ["<paste:f>", "<paste:ff>", "<paste:zz>", "<paste:00"] {
            assert!(parse_macro_events(invalid).is_err());
        }
    }

    #[test]
    fn command_macro_events_preserve_sequence_order() {
        let serialized = "i<paste:43><command:6e6f726d616c5f6d6f6465>";
        let parsed = parse_macro_events(serialized).unwrap();
        assert_eq!(
            parsed.iter().map(ToString::to_string).collect::<String>(),
            serialized
        );
    }
}
