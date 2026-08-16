use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Translate a key press into the bytes a terminal would send to the shell.
///
/// Returns `None` for keys the console does not forward, which the caller
/// handles itself.
pub(crate) fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    let base: Vec<u8> = match key.code {
        KeyCode::Char(character) => {
            if control {
                // Ctrl+A..Ctrl+Z and the handful of control codes above them.
                let byte = match character.to_ascii_lowercase() {
                    letter @ 'a'..='z' => letter as u8 - b'a' + 1,
                    ' ' | '@' => 0,
                    '[' => 0x1b,
                    '\\' => 0x1c,
                    ']' => 0x1d,
                    '^' => 0x1e,
                    '_' | '?' => 0x1f,
                    _ => return None,
                };
                vec![byte]
            } else {
                let mut buffer = [0_u8; 4];
                character.encode_utf8(&mut buffer).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::F(number) => function_key(number)?,
        _ => return None,
    };

    // Alt is sent as an ESC prefix, the convention every common shell expects.
    if alt {
        let mut bytes = Vec::with_capacity(base.len() + 1);
        bytes.push(0x1b);
        bytes.extend_from_slice(&base);
        return Some(bytes);
    }
    Some(base)
}

fn function_key(number: u8) -> Option<Vec<u8>> {
    let sequence: &[u8] = match number {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    };
    Some(sequence.to_vec())
}
