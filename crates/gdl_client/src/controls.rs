//! Player 1's keyboard bindings (NATIVE: the PS2 had a pad). Each action
//! presses one joyGetStatus entry of port 0 (research/player.md "Pad
//! buttons"); the defaults are the client's original keys, and
//! Settings::keys (action name → key names) replaces an action's keys.
//! The menus accept the bound keys of the directions, cross (accept),
//! triangle (back) and circle (centre) besides their fixed ones.
use bevy::input::keyboard::KeyCode;
use std::collections::BTreeMap;

/// One rebindable action.
pub struct Action {
    /// The settings file's name.
    pub name: &'static str,
    pub label: &'static str,
    /// The port-0 pad entry it presses.
    pub entry: usize,
    pub defaults: &'static [KeyCode],
}

/// Port 0's actions, in the overlay's order. Labels name the pad button
/// and what research has measured it doing; square and R2 are named only.
pub const ACTIONS: [Action; 18] = [
    Action { name: "up", label: "Up", entry: 2, defaults: &[KeyCode::ArrowUp, KeyCode::KeyW] },
    Action { name: "down", label: "Down", entry: 3, defaults: &[KeyCode::ArrowDown, KeyCode::KeyS] },
    Action { name: "left", label: "Left", entry: 0, defaults: &[KeyCode::ArrowLeft, KeyCode::KeyA] },
    Action { name: "right", label: "Right", entry: 1, defaults: &[KeyCode::ArrowRight, KeyCode::KeyD] },
    Action { name: "cross", label: "Cross (attack)", entry: 0xd, defaults: &[KeyCode::Space, KeyCode::KeyK] },
    Action { name: "circle", label: "Circle (magic)", entry: 0xb, defaults: &[KeyCode::KeyE, KeyCode::KeyL] },
    Action { name: "triangle", label: "Triangle (turbo)", entry: 0xc, defaults: &[KeyCode::ShiftLeft, KeyCode::KeyI] },
    Action { name: "square", label: "Square", entry: 0xa, defaults: &[KeyCode::KeyQ, KeyCode::KeyJ] },
    Action { name: "l1", label: "L1 (dash)", entry: 8, defaults: &[KeyCode::KeyZ] },
    Action { name: "l2", label: "L2 (team move)", entry: 9, defaults: &[KeyCode::KeyX] },
    Action { name: "r1", label: "R1 (strafe)", entry: 0x12, defaults: &[KeyCode::ControlLeft] },
    Action { name: "r2", label: "R2", entry: 0x13, defaults: &[KeyCode::KeyV] },
    Action { name: "start", label: "Start", entry: 0x14, defaults: &[KeyCode::Enter] },
    Action { name: "select", label: "Select", entry: 0x15, defaults: &[KeyCode::Tab] },
    // The right stick (entries 0xe–0x11, full deflection): Control Style
    // One Handed moves and Robotron aims and fires with it.
    Action { name: "rstick_up", label: "Right stick up", entry: 0x10, defaults: &[KeyCode::Numpad8] },
    Action { name: "rstick_down", label: "Right stick down", entry: 0x11, defaults: &[KeyCode::Numpad2] },
    Action { name: "rstick_left", label: "Right stick left", entry: 0xe, defaults: &[KeyCode::Numpad4] },
    Action { name: "rstick_right", label: "Right stick right", entry: 0xf, defaults: &[KeyCode::Numpad6] },
];

/// Keys that can be bound (the client's own function keys and Escape are
/// not: F1–F3, F9–F11 and Escape keep their native uses).
pub const BINDABLE: &[KeyCode] = &[
    KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyI,
    KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN, KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR,
    KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU, KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ, KeyCode::Digit0,
    KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8,
    KeyCode::Digit9, KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ArrowLeft, KeyCode::ArrowRight, KeyCode::Space, KeyCode::Enter,
    KeyCode::Tab, KeyCode::Backspace, KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::AltLeft,
    KeyCode::AltRight, KeyCode::Comma, KeyCode::Period, KeyCode::Slash, KeyCode::Semicolon, KeyCode::Quote, KeyCode::BracketLeft,
    KeyCode::BracketRight, KeyCode::Backslash, KeyCode::Minus, KeyCode::Equal, KeyCode::Backquote, KeyCode::Insert, KeyCode::Delete,
    KeyCode::Home, KeyCode::End, KeyCode::PageUp, KeyCode::PageDown, KeyCode::Numpad0, KeyCode::Numpad1, KeyCode::Numpad2, KeyCode::Numpad3,
    KeyCode::Numpad4, KeyCode::Numpad5, KeyCode::Numpad6, KeyCode::Numpad7, KeyCode::Numpad8, KeyCode::Numpad9, KeyCode::NumpadAdd,
    KeyCode::NumpadSubtract, KeyCode::NumpadMultiply, KeyCode::NumpadDivide, KeyCode::NumpadDecimal, KeyCode::NumpadEnter,
];

/// A key's settings-file name (its KeyCode name, e.g. "KeyA", "ArrowLeft").
pub fn key_name(k: KeyCode) -> String {
    format!("{k:?}")
}

/// A settings-file key name, if it names a bindable key.
pub fn parse_key(name: &str) -> Option<KeyCode> {
    BINDABLE.iter().copied().find(|&k| key_name(k) == name)
}

/// Each action's keys.
#[derive(Clone, Debug, PartialEq, bevy::prelude::Resource)]
pub struct Bindings {
    pub keys: [Vec<KeyCode>; ACTIONS.len()],
}

impl Default for Bindings {
    fn default() -> Self {
        Self { keys: std::array::from_fn(|i| ACTIONS[i].defaults.to_vec()) }
    }
}

impl Bindings {
    /// The defaults with the stored actions replaced (unknown actions and
    /// key names are ignored; an action whose names all fail keeps its
    /// defaults).
    pub fn from_settings(stored: &BTreeMap<String, Vec<String>>) -> Self {
        let mut b = Self::default();
        for (i, a) in ACTIONS.iter().enumerate() {
            if let Some(names) = stored.get(a.name) {
                let keys: Vec<KeyCode> = names.iter().filter_map(|n| parse_key(n)).collect();
                if !keys.is_empty() {
                    b.keys[i] = keys;
                }
            }
        }
        b
    }

    /// The actions that differ from their defaults, for the settings file.
    pub fn to_settings(&self) -> BTreeMap<String, Vec<String>> {
        ACTIONS
            .iter()
            .zip(&self.keys)
            .filter(|(a, k)| k.as_slice() != a.defaults)
            .map(|(a, k)| (a.name.to_string(), k.iter().map(|&k| key_name(k)).collect()))
            .collect()
    }

    /// The keys of the action named `name`.
    pub fn of(&self, name: &str) -> &[KeyCode] {
        ACTIONS.iter().position(|a| a.name == name).map_or(&[], |i| &self.keys[i])
    }

    /// Bind `key` alone to action `i`, taking it off any other action.
    pub fn bind(&mut self, i: usize, key: KeyCode) {
        for k in self.keys.iter_mut() {
            k.retain(|&x| x != key);
        }
        self.keys[i] = vec![key];
        // An action left without keys gets its defaults that are free.
        for (j, a) in ACTIONS.iter().enumerate() {
            if self.keys[j].is_empty() {
                let free: Vec<KeyCode> = a.defaults.iter().copied().filter(|d| !self.keys.iter().flatten().any(|x| x == d)).collect();
                self.keys[j] = free;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_to_an_empty_map() {
        let b = Bindings::default();
        assert!(b.to_settings().is_empty());
        assert_eq!(Bindings::from_settings(&BTreeMap::new()), b);
    }

    #[test]
    fn stored_names_replace_an_actions_keys() {
        let mut m = BTreeMap::new();
        m.insert("cross".to_string(), vec!["KeyN".to_string(), "Nonsense".to_string()]);
        m.insert("nothing".to_string(), vec!["KeyM".to_string()]);
        let b = Bindings::from_settings(&m);
        assert_eq!(b.of("cross"), &[KeyCode::KeyN]);
        assert_eq!(b.to_settings(), BTreeMap::from([("cross".to_string(), vec!["KeyN".to_string()])]));
    }

    #[test]
    fn binding_a_key_moves_it() {
        let mut b = Bindings::default();
        let cross = ACTIONS.iter().position(|a| a.name == "cross").unwrap();
        // W was Up's second key.
        b.bind(cross, KeyCode::KeyW);
        assert_eq!(b.of("cross"), &[KeyCode::KeyW]);
        assert_eq!(b.of("up"), &[KeyCode::ArrowUp]);
    }

    #[test]
    fn every_default_key_is_bindable_and_named() {
        for a in &ACTIONS {
            for &k in a.defaults {
                assert_eq!(parse_key(&key_name(k)), Some(k), "{}", a.name);
            }
        }
    }
}
