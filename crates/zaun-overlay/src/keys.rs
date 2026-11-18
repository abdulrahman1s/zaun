//! Keys as egui knows them, from the compositor's key symbols.
use smithay_client_toolkit::seat::keyboard::Keysym;

/// The egui key for `sym` (`None` for keys egui has no name for).
pub fn key(sym: Keysym) -> Option<egui::Key> {
    use egui::Key;
    Some(match sym {
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete | Keysym::KP_Delete => Key::Delete,
        Keysym::Tab | Keysym::ISO_Left_Tab => Key::Tab,
        Keysym::Escape => Key::Escape,
        Keysym::Home | Keysym::KP_Home => Key::Home,
        Keysym::End | Keysym::KP_End => Key::End,
        Keysym::Left | Keysym::KP_Left => Key::ArrowLeft,
        Keysym::Right | Keysym::KP_Right => Key::ArrowRight,
        Keysym::Up | Keysym::KP_Up => Key::ArrowUp,
        Keysym::Down | Keysym::KP_Down => Key::ArrowDown,
        Keysym::Page_Up | Keysym::KP_Page_Up => Key::PageUp,
        Keysym::Page_Down | Keysym::KP_Page_Down => Key::PageDown,
        Keysym::Insert => Key::Insert,
        Keysym::space => Key::Space,
        other => {
            let c = other.key_char()?;
            let mut buf = [0u8; 4];
            return Key::from_name(c.to_ascii_uppercase().encode_utf8(&mut buf));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_keys_and_letters_map_to_egui_keys() {
        assert_eq!(key(Keysym::Return), Some(egui::Key::Enter));
        assert_eq!(key(Keysym::BackSpace), Some(egui::Key::Backspace));
        assert_eq!(key(Keysym::a), Some(egui::Key::A));
        assert_eq!(key(Keysym::A), Some(egui::Key::A));
        assert_eq!(key(Keysym::_7), Some(egui::Key::Num7));
        assert_eq!(key(Keysym::F13), None);
    }
}
