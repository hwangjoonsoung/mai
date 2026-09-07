use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

pub fn key(k: &KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    use KeyCode::*;
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let modifier = 1 + u8::from(k.modifiers.contains(KeyModifiers::SHIFT)) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let final_char = match k.code { Up => Some('A'), Down => Some('B'), Right => Some('C'), Left => Some('D'), Home => Some('H'), End => Some('F'), _ => None };
    if let Some(c) = final_char {
        return Some(if modifier > 1 { format!("\x1b[1;{modifier}{c}") } else if application_cursor { format!("\x1bO{c}") } else { format!("\x1b[{c}") }.into_bytes());
    }
    let tilde = match k.code { Insert => Some(2), Delete => Some(3), PageUp => Some(5), PageDown => Some(6), F(n @ 5..=12) => Some([15,17,18,19,20,21,23,24][(n-5) as usize]), _ => None };
    if let Some(n) = tilde {
        return Some(if modifier > 1 { format!("\x1b[{n};{modifier}~") } else { format!("\x1b[{n}~") }.into_bytes());
    }
    if let F(n @ 1..=4) = k.code {
        let c = (b'P' + n - 1) as char;
        return Some(if modifier > 1 { format!("\x1b[1;{modifier}{c}") } else { format!("\x1bO{c}") }.into_bytes());
    }
    let mut bytes = match k.code {
        Char(c) if ctrl => vec![match c {
            'a'..='z' => c as u8 - b'a' + 1, 'A'..='Z' => c as u8 - b'A' + 1,
            ' ' | '@' | '2' => 0, '[' | '3' => 27, '\\' | '4' => 28,
            ']' | '5' => 29, '^' | '6' => 30, '_' | '/' | '7' => 31, '?' | '8' => 127,
            _ => return None,
        }],
        Char(c) => c.to_string().into_bytes(),
        Enter if k.modifiers.contains(KeyModifiers::SHIFT) => b"\x1b\r".to_vec(),
        Enter => vec![13], Tab => vec![9], BackTab => b"\x1b[Z".to_vec(),
        Backspace => vec![127], Esc => vec![27], _ => return None,
    };
    if alt { bytes.insert(0, 27); }
    Some(bytes)
}

pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    // Remove the framing delimiter from pasted data, so it cannot terminate paste early.
    let text = text.replace("\x1b[201~", "");
    if bracketed { format!("\x1b[200~{text}\x1b[201~").into_bytes() }
    else { text.replace("\r\n", "\n").replace('\n', "\r").into_bytes() }
}

pub fn mouse(screen: &vt100::Screen, m: MouseEvent) -> Option<Vec<u8>> {
    use vt100::{MouseProtocolMode as Mode, MouseProtocolEncoding as Enc};
    let mode = screen.mouse_protocol_mode();
    if mode == Mode::None { return None; }
    let button = |b| match b { MouseButton::Left => 0, MouseButton::Middle => 1, MouseButton::Right => 2 };
    let mut code = match m.kind {
        MouseEventKind::Down(b) => button(b),
        MouseEventKind::Up(b) if mode != Mode::Press => button(b),
        MouseEventKind::Drag(b) if matches!(mode, Mode::ButtonMotion | Mode::AnyMotion) => button(b) + 32,
        MouseEventKind::Moved if mode == Mode::AnyMotion => 35,
        MouseEventKind::ScrollUp => 64, MouseEventKind::ScrollDown => 65,
        MouseEventKind::ScrollLeft => 66, MouseEventKind::ScrollRight => 67,
        _ => return Some(Vec::new()),
    };
    if m.modifiers.contains(KeyModifiers::SHIFT) { code += 4; }
    if m.modifiers.contains(KeyModifiers::ALT) { code += 8; }
    if m.modifiers.contains(KeyModifiers::CONTROL) { code += 16; }
    let release = matches!(m.kind, MouseEventKind::Up(_));
    let (x, y) = (u32::from(m.column) + 1, u32::from(m.row) + 1);
    if screen.mouse_protocol_encoding() == Enc::Sgr {
        return Some(format!("\x1b[<{code};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes());
    }
    if release { code = (code & !3) | 3; }
    let mut result = b"\x1b[M".to_vec();
    for value in [code + 32, x + 32, y + 32] {
        if screen.mouse_protocol_encoding() == Enc::Utf8 {
            let c = char::from_u32(value)?;
            let mut buf = [0; 4];
            result.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        } else {
            if value > 255 { return Some(Vec::new()); }
            result.push(value as u8);
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_preserve_unicode_control_alt_and_modes() {
        let k = |c, m| KeyEvent::new(c, m);
        assert_eq!(key(&k(KeyCode::Char('한'), KeyModifiers::NONE), false).unwrap(), "한".as_bytes());
        assert_eq!(key(&k(KeyCode::Char('b'), KeyModifiers::ALT), false).unwrap(), b"\x1bb");
        assert_eq!(key(&k(KeyCode::Char(' '), KeyModifiers::CONTROL), false).unwrap(), [0]);
        assert_eq!(key(&k(KeyCode::Up, KeyModifiers::NONE), true).unwrap(), b"\x1bOA");
        assert_eq!(key(&k(KeyCode::Left, KeyModifiers::CONTROL), false).unwrap(), b"\x1b[1;5D");
        assert_eq!(key(&k(KeyCode::F(12), KeyModifiers::NONE), false).unwrap(), b"\x1b[24~");
    }
    #[test]
    fn multiline_paste_is_one_framed_payload() {
        assert_eq!(paste("한\n둘", true), "\x1b[200~한\n둘\x1b[201~".as_bytes());
        assert_eq!(paste("a\r\nb\nc", false), b"a\rb\rc");
        assert_eq!(paste("a\x1b[201~b", true), b"\x1b[200~ab\x1b[201~");
    }
    #[test]
    fn mouse_respects_tracking_and_encoding() {
        let mut p = vt100::Parser::new(10, 20, 0);
        let m = MouseEvent { kind: MouseEventKind::Drag(MouseButton::Left), column: 4, row: 2, modifiers: KeyModifiers::NONE };
        assert!(mouse(p.screen(), m).is_none());
        p.process(b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(mouse(p.screen(), m).unwrap(), b"");
        p.process(b"\x1b[?1002h");
        assert_eq!(mouse(p.screen(), m).unwrap(), b"\x1b[<32;5;3M");
        assert_eq!(mouse(p.screen(), MouseEvent {kind: MouseEventKind::Up(MouseButton::Left), ..m}).unwrap(), b"\x1b[<0;5;3m");
    }
}
