//! Parsing and registering user-facing hotkey strings such as `Win+Shift+G`.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
}

impl Hotkey {
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut modifiers = MOD_NOREPEAT;
        let mut vk = None;

        for token in input.split('+').map(str::trim).filter(|s| !s.is_empty()) {
            match token.to_ascii_lowercase().as_str() {
                "win" | "windows" | "meta" => modifiers |= MOD_WIN,
                "ctrl" | "control" => modifiers |= MOD_CONTROL,
                "shift" => modifiers |= MOD_SHIFT,
                "alt" => modifiers |= MOD_ALT,
                key => {
                    if vk.is_some() {
                        return Err(format!("hotkey has more than one non-modifier key: {input}"));
                    }
                    vk = Some(parse_vk(key)?);
                }
            }
        }

        let modifier_mask = MOD_WIN | MOD_CONTROL | MOD_SHIFT | MOD_ALT;
        if modifiers & modifier_mask == 0 {
            return Err(format!("hotkey must include at least one modifier: {input}"));
        }

        Ok(Self { modifiers, vk: vk.ok_or_else(|| format!("hotkey has no key: {input}"))? })
    }
}

fn parse_vk(key: &str) -> Result<u32, String> {
    if key.len() == 1 {
        let c = key.as_bytes()[0].to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Ok(c as u32);
        }
    }

    if let Some(rest) = key.strip_prefix('f')
        && let Ok(n) = rest.parse::<u32>()
            && (1..=24).contains(&n) {
                // VK_F1 = 0x70.
                return Ok(0x6F + n);
            }

    Err(format!("unsupported hotkey key: {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkey() {
        let h = Hotkey::parse("Win+Shift+G").unwrap();
        assert_ne!(h.modifiers & MOD_WIN, 0);
        assert_ne!(h.modifiers & MOD_SHIFT, 0);
        assert_eq!(h.vk, b'G' as u32);
    }

    #[test]
    fn rejects_bare_key() {
        assert!(Hotkey::parse("G").is_err());
    }
}
