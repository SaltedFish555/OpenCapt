use anyhow::{Context, Result, anyhow, bail};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};

pub struct RegisteredHotkey {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
}

impl RegisteredHotkey {
    pub fn register(spec: &str) -> Result<Self> {
        let manager = GlobalHotKeyManager::new().context("failed to create hotkey manager")?;
        let hotkey = parse_hotkey(spec)?;
        manager
            .register(hotkey)
            .with_context(|| format!("failed to register hotkey {spec}"))?;
        Ok(Self { manager, hotkey })
    }

    pub fn matches_event(&self, event: &GlobalHotKeyEvent) -> bool {
        event.id() == self.hotkey.id() && event.state() == HotKeyState::Pressed
    }

    pub fn hotkey(&self) -> &HotKey {
        &self.hotkey
    }
}

impl Drop for RegisteredHotkey {
    fn drop(&mut self) {
        let _ = self.manager.unregister(self.hotkey);
    }
}

pub fn parse_hotkey(spec: &str) -> Result<HotKey> {
    let mut modifiers = Modifiers::empty();
    let mut key = None;

    for raw_part in spec.split('+') {
        let part = raw_part.trim();
        if part.is_empty() {
            continue;
        }

        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" => modifiers |= Modifiers::ALT,
            "win" | "super" | "meta" | "cmd" | "command" => modifiers |= Modifiers::SUPER,
            _ => {
                if key.is_some() {
                    bail!("hotkey may only contain one non-modifier key: {spec}");
                }
                key = Some(parse_key(part)?);
            }
        }
    }

    let key = key.ok_or_else(|| anyhow!("hotkey is missing a non-modifier key: {spec}"))?;
    Ok(HotKey::new(Some(modifiers), key))
}

fn parse_key(part: &str) -> Result<Code> {
    let upper = part.trim().to_ascii_uppercase();
    let code = match upper.as_str() {
        "A" => Code::KeyA,
        "B" => Code::KeyB,
        "C" => Code::KeyC,
        "D" => Code::KeyD,
        "E" => Code::KeyE,
        "F" => Code::KeyF,
        "G" => Code::KeyG,
        "H" => Code::KeyH,
        "I" => Code::KeyI,
        "J" => Code::KeyJ,
        "K" => Code::KeyK,
        "L" => Code::KeyL,
        "M" => Code::KeyM,
        "N" => Code::KeyN,
        "O" => Code::KeyO,
        "P" => Code::KeyP,
        "Q" => Code::KeyQ,
        "R" => Code::KeyR,
        "S" => Code::KeyS,
        "T" => Code::KeyT,
        "U" => Code::KeyU,
        "V" => Code::KeyV,
        "W" => Code::KeyW,
        "X" => Code::KeyX,
        "Y" => Code::KeyY,
        "Z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "SPACE" => Code::Space,
        "ESC" | "ESCAPE" => Code::Escape,
        "ENTER" => Code::Enter,
        "TAB" => Code::Tab,
        "BACKSPACE" => Code::Backspace,
        "DELETE" => Code::Delete,
        "INSERT" => Code::Insert,
        "HOME" => Code::Home,
        "END" => Code::End,
        "PAGEUP" => Code::PageUp,
        "PAGEDOWN" => Code::PageDown,
        "UP" => Code::ArrowUp,
        "DOWN" => Code::ArrowDown,
        "LEFT" => Code::ArrowLeft,
        "RIGHT" => Code::ArrowRight,
        "PRINTSCREEN" => Code::PrintScreen,
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        "F13" => Code::F13,
        "F14" => Code::F14,
        "F15" => Code::F15,
        "F16" => Code::F16,
        "F17" => Code::F17,
        "F18" => Code::F18,
        "F19" => Code::F19,
        "F20" => Code::F20,
        "F21" => Code::F21,
        "F22" => Code::F22,
        "F23" => Code::F23,
        "F24" => Code::F24,
        _ => bail!("unsupported hotkey key: {part}"),
    };

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkey() {
        let hotkey = parse_hotkey("Ctrl+Shift+A").expect("parse hotkey");
        assert_eq!(hotkey.key, Code::KeyA);
        assert!(hotkey.mods.contains(Modifiers::CONTROL));
        assert!(hotkey.mods.contains(Modifiers::SHIFT));
    }

    #[test]
    fn rejects_multiple_keys() {
        assert!(parse_hotkey("Ctrl+A+B").is_err());
    }
}
