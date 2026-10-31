//! OHOS keycode table — binding `KeyCode` to the winit key model.
//!
//! Self-authored exhaustive translation over the 330 variants of
//! `ohos_xcomponent_binding::KeyCode` (the community `winit-ohos` table was
//! unreachable — see the p4 proposal deviation 4), written directly against
//! the 0.30 target API with `platform_impl/android/keycodes.rs` as the
//! structural template for the keys both platforms share.
//!
//! The physical-key table is exhaustive with **no wildcard arm**: adding a
//! binding variant must fail compilation here, forcing a conscious table
//! review. Media / volume / brightness / system keys map to `None` — the p4
//! design pins them as the negative bucket of the spot-check matrix (they are
//! system-consumed before reaching an `XComponent`).

use crate::keyboard::{Key, KeyCode, KeyLocation, NamedKey, NativeKey};
use ohos_xcomponent_binding::KeyCode as OhosKeyCode;

/// Maps a binding `KeyCode` to the winit physical key.
///
/// Returns `None` for OHOS-specific media / volume / brightness / system
/// keys — the design pins them as the negative bucket (system-consumed
/// before reaching an `XComponent`); the caller derives
/// `PhysicalKey::Unidentified` from `None`. Deliberately deviates from the
/// android table (which maps e.g. `VolumeUp` to `AudioVolumeUp`) for that
/// bucket only. Exhaustive without a wildcard arm by design.
pub(crate) fn to_physical_key(code: OhosKeyCode) -> Option<KeyCode> {
    match code {
        // Letters.
        OhosKeyCode::A => Some(KeyCode::KeyA),
        OhosKeyCode::B => Some(KeyCode::KeyB),
        OhosKeyCode::C => Some(KeyCode::KeyC),
        OhosKeyCode::D => Some(KeyCode::KeyD),
        OhosKeyCode::E => Some(KeyCode::KeyE),
        OhosKeyCode::F => Some(KeyCode::KeyF),
        OhosKeyCode::G => Some(KeyCode::KeyG),
        OhosKeyCode::H => Some(KeyCode::KeyH),
        OhosKeyCode::I => Some(KeyCode::KeyI),
        OhosKeyCode::J => Some(KeyCode::KeyJ),
        OhosKeyCode::K => Some(KeyCode::KeyK),
        OhosKeyCode::L => Some(KeyCode::KeyL),
        OhosKeyCode::M => Some(KeyCode::KeyM),
        OhosKeyCode::N => Some(KeyCode::KeyN),
        OhosKeyCode::O => Some(KeyCode::KeyO),
        OhosKeyCode::P => Some(KeyCode::KeyP),
        OhosKeyCode::Q => Some(KeyCode::KeyQ),
        OhosKeyCode::R => Some(KeyCode::KeyR),
        OhosKeyCode::S => Some(KeyCode::KeyS),
        OhosKeyCode::T => Some(KeyCode::KeyT),
        OhosKeyCode::U => Some(KeyCode::KeyU),
        OhosKeyCode::V => Some(KeyCode::KeyV),
        OhosKeyCode::W => Some(KeyCode::KeyW),
        OhosKeyCode::X => Some(KeyCode::KeyX),
        OhosKeyCode::Y => Some(KeyCode::KeyY),
        OhosKeyCode::Z => Some(KeyCode::KeyZ),

        // Top-row digits.
        OhosKeyCode::Key0 => Some(KeyCode::Digit0),
        OhosKeyCode::Key1 => Some(KeyCode::Digit1),
        OhosKeyCode::Key2 => Some(KeyCode::Digit2),
        OhosKeyCode::Key3 => Some(KeyCode::Digit3),
        OhosKeyCode::Key4 => Some(KeyCode::Digit4),
        OhosKeyCode::Key5 => Some(KeyCode::Digit5),
        OhosKeyCode::Key6 => Some(KeyCode::Digit6),
        OhosKeyCode::Key7 => Some(KeyCode::Digit7),
        OhosKeyCode::Key8 => Some(KeyCode::Digit8),
        OhosKeyCode::Key9 => Some(KeyCode::Digit9),

        // Function row.
        OhosKeyCode::F1 => Some(KeyCode::F1),
        OhosKeyCode::F2 => Some(KeyCode::F2),
        OhosKeyCode::F3 => Some(KeyCode::F3),
        OhosKeyCode::F4 => Some(KeyCode::F4),
        OhosKeyCode::F5 => Some(KeyCode::F5),
        OhosKeyCode::F6 => Some(KeyCode::F6),
        OhosKeyCode::F7 => Some(KeyCode::F7),
        OhosKeyCode::F8 => Some(KeyCode::F8),
        OhosKeyCode::F9 => Some(KeyCode::F9),
        OhosKeyCode::F10 => Some(KeyCode::F10),
        OhosKeyCode::F11 => Some(KeyCode::F11),
        OhosKeyCode::F12 => Some(KeyCode::F12),
        OhosKeyCode::F13 => Some(KeyCode::F13),
        OhosKeyCode::F14 => Some(KeyCode::F14),
        OhosKeyCode::F15 => Some(KeyCode::F15),
        OhosKeyCode::F16 => Some(KeyCode::F16),
        OhosKeyCode::F17 => Some(KeyCode::F17),
        OhosKeyCode::F18 => Some(KeyCode::F18),
        OhosKeyCode::F19 => Some(KeyCode::F19),
        OhosKeyCode::F20 => Some(KeyCode::F20),
        OhosKeyCode::F21 => Some(KeyCode::F21),
        OhosKeyCode::F22 => Some(KeyCode::F22),
        OhosKeyCode::F23 => Some(KeyCode::F23),
        OhosKeyCode::F24 => Some(KeyCode::F24),

        // Numpad cluster.
        OhosKeyCode::Numpad0 => Some(KeyCode::Numpad0),
        OhosKeyCode::Numpad1 => Some(KeyCode::Numpad1),
        OhosKeyCode::Numpad2 => Some(KeyCode::Numpad2),
        OhosKeyCode::Numpad3 => Some(KeyCode::Numpad3),
        OhosKeyCode::Numpad4 => Some(KeyCode::Numpad4),
        OhosKeyCode::Numpad5 => Some(KeyCode::Numpad5),
        OhosKeyCode::Numpad6 => Some(KeyCode::Numpad6),
        OhosKeyCode::Numpad7 => Some(KeyCode::Numpad7),
        OhosKeyCode::Numpad8 => Some(KeyCode::Numpad8),
        OhosKeyCode::Numpad9 => Some(KeyCode::Numpad9),
        OhosKeyCode::NumpadDivide => Some(KeyCode::NumpadDivide),
        OhosKeyCode::NumpadMultiply => Some(KeyCode::NumpadMultiply),
        OhosKeyCode::NumpadSubtract => Some(KeyCode::NumpadSubtract),
        OhosKeyCode::NumpadAdd => Some(KeyCode::NumpadAdd),
        OhosKeyCode::NumpadDot => Some(KeyCode::NumpadDecimal),
        OhosKeyCode::NumpadComma => Some(KeyCode::NumpadComma),
        OhosKeyCode::NumpadEnter => Some(KeyCode::NumpadEnter),
        OhosKeyCode::NumpadEquals => Some(KeyCode::NumpadEqual),
        OhosKeyCode::NumpadLeftParen => Some(KeyCode::NumpadParenLeft),
        OhosKeyCode::NumpadRightParen => Some(KeyCode::NumpadParenRight),
        OhosKeyCode::NumLock => Some(KeyCode::NumLock),

        // D-pad / navigation.
        OhosKeyCode::DpadUp => Some(KeyCode::ArrowUp),
        OhosKeyCode::DpadDown => Some(KeyCode::ArrowDown),
        OhosKeyCode::DpadLeft => Some(KeyCode::ArrowLeft),
        OhosKeyCode::DpadRight => Some(KeyCode::ArrowRight),
        OhosKeyCode::MoveHome => Some(KeyCode::Home),
        OhosKeyCode::MoveEnd => Some(KeyCode::End),
        OhosKeyCode::PageUp => Some(KeyCode::PageUp),
        OhosKeyCode::PageDown => Some(KeyCode::PageDown),
        OhosKeyCode::Insert => Some(KeyCode::Insert),
        OhosKeyCode::ForwardDel => Some(KeyCode::Delete),

        // Editing cluster.
        OhosKeyCode::Enter => Some(KeyCode::Enter),
        OhosKeyCode::Tab => Some(KeyCode::Tab),
        OhosKeyCode::Space => Some(KeyCode::Space),
        OhosKeyCode::Escape => Some(KeyCode::Escape),
        OhosKeyCode::Del => Some(KeyCode::Backspace),

        // Punctuation.
        OhosKeyCode::Comma => Some(KeyCode::Comma),
        OhosKeyCode::Period => Some(KeyCode::Period),
        OhosKeyCode::Minus => Some(KeyCode::Minus),
        // winit does not separate '+' from '=' — same physical key.
        OhosKeyCode::Equals => Some(KeyCode::Equal),
        OhosKeyCode::Plus => Some(KeyCode::Equal),
        OhosKeyCode::Grave => Some(KeyCode::Backquote),
        OhosKeyCode::Slash => Some(KeyCode::Slash),
        OhosKeyCode::Backslash => Some(KeyCode::Backslash),
        OhosKeyCode::Semicolon => Some(KeyCode::Semicolon),
        OhosKeyCode::Apostrophe => Some(KeyCode::Quote),
        OhosKeyCode::LeftBracket => Some(KeyCode::BracketLeft),
        OhosKeyCode::RightBracket => Some(KeyCode::BracketRight),

        // Modifiers.
        OhosKeyCode::AltLeft => Some(KeyCode::AltLeft),
        OhosKeyCode::AltRight => Some(KeyCode::AltRight),
        OhosKeyCode::ShiftLeft => Some(KeyCode::ShiftLeft),
        OhosKeyCode::ShiftRight => Some(KeyCode::ShiftRight),
        OhosKeyCode::CtrlLeft => Some(KeyCode::ControlLeft),
        OhosKeyCode::CtrlRight => Some(KeyCode::ControlRight),
        OhosKeyCode::MetaLeft => Some(KeyCode::SuperLeft),
        OhosKeyCode::MetaRight => Some(KeyCode::SuperRight),
        OhosKeyCode::CapsLock => Some(KeyCode::CapsLock),
        OhosKeyCode::ScrollLock => Some(KeyCode::ScrollLock),

        // Control-key duplicates.
        OhosKeyCode::SysRq => Some(KeyCode::PrintScreen),
        // Pause/Break are usually the same physical key.
        OhosKeyCode::Break => Some(KeyCode::Pause),

        // Clipboard.
        OhosKeyCode::Copy => Some(KeyCode::Copy),
        OhosKeyCode::Paste => Some(KeyCode::Paste),
        OhosKeyCode::Cut => Some(KeyCode::Cut),

        // Layout-specific keys.
        OhosKeyCode::Yen => Some(KeyCode::IntlYen),
        OhosKeyCode::Ro => Some(KeyCode::IntlRo),
        // The extra key next to left Shift on ISO layouts.
        OhosKeyCode::Key102nd => Some(KeyCode::IntlBackslash),

        // Japanese keys.
        OhosKeyCode::Katakana => Some(KeyCode::Katakana),
        OhosKeyCode::Hiragana => Some(KeyCode::Hiragana),
        OhosKeyCode::KatakanaHiragana => Some(KeyCode::KanaMode),
        OhosKeyCode::Henkan => Some(KeyCode::Convert),
        OhosKeyCode::Muhenkan => Some(KeyCode::NonConvert),

        // OHOS-specific: media keys (system-consumed).
        OhosKeyCode::MediaPlayPause
        | OhosKeyCode::MediaStop
        | OhosKeyCode::MediaNext
        | OhosKeyCode::MediaPrevious
        | OhosKeyCode::MediaRewind
        | OhosKeyCode::MediaFastForward
        | OhosKeyCode::MediaPlay
        | OhosKeyCode::MediaPause
        | OhosKeyCode::MediaClose
        | OhosKeyCode::MediaEject
        | OhosKeyCode::MediaRecord
        | OhosKeyCode::PlayPause
        | OhosKeyCode::Previous
        | OhosKeyCode::Next
        | OhosKeyCode::StopCD
        | OhosKeyCode::Play
        | OhosKeyCode::BassBoost
        | OhosKeyCode::MediaRepeat
        | OhosKeyCode::EjectCloseCD
        | OhosKeyCode::VideoNext
        | OhosKeyCode::VideoPrev => None,

        // OHOS-specific: volume keys.
        OhosKeyCode::VolumeUp
        | OhosKeyCode::VolumeDown
        | OhosKeyCode::VolumeMute
        | OhosKeyCode::Mute => None,

        // OHOS-specific: brightness keys.
        OhosKeyCode::BrightnessUp
        | OhosKeyCode::BrightnessDown
        | OhosKeyCode::BrightnessCycle
        | OhosKeyCode::BrightnessZero
        | OhosKeyCode::BrightnessToggle
        | OhosKeyCode::BrightnessMin
        | OhosKeyCode::BrightnessMax
        | OhosKeyCode::DisplayOff => None,

        // OHOS-specific: system keys.
        OhosKeyCode::Power
        | OhosKeyCode::Sleep
        | OhosKeyCode::Wakeup
        | OhosKeyCode::Suspend
        | OhosKeyCode::Home
        | OhosKeyCode::Back
        | OhosKeyCode::Menu
        | OhosKeyCode::Camera
        | OhosKeyCode::ScreenLock
        | OhosKeyCode::DirectionRotateDisplay
        | OhosKeyCode::CycleWindows
        | OhosKeyCode::Computer
        | OhosKeyCode::Dashboard
        | OhosKeyCode::TaskManager
        | OhosKeyCode::ControlPanel
        | OhosKeyCode::AppSelect
        | OhosKeyCode::ScreenSaver
        | OhosKeyCode::Assistant
        | OhosKeyCode::VirtualMultitask => None,

        // OHOS-specific: connectivity / hardware toggles.
        OhosKeyCode::Battery
        | OhosKeyCode::Bluetooth
        | OhosKeyCode::Wlan
        | OhosKeyCode::Uwb
        | OhosKeyCode::WwanWimax
        | OhosKeyCode::RfKill
        | OhosKeyCode::KeyboardIllumToggle
        | OhosKeyCode::KeyboardIllumDown
        | OhosKeyCode::KeyboardIllumUp
        | OhosKeyCode::SwitchVideoMode
        | OhosKeyCode::AltErase
        | OhosKeyCode::Iso => None,

        // OHOS-specific: application-launcher keys.
        OhosKeyCode::Explorer
        | OhosKeyCode::Envelope
        | OhosKeyCode::Forward
        | OhosKeyCode::ForwardMail
        | OhosKeyCode::Calc
        | OhosKeyCode::File
        | OhosKeyCode::Bookmarks
        | OhosKeyCode::Config
        | OhosKeyCode::Refresh
        | OhosKeyCode::Exit
        | OhosKeyCode::Edit
        | OhosKeyCode::New
        | OhosKeyCode::Redo
        | OhosKeyCode::Close
        | OhosKeyCode::Print
        | OhosKeyCode::Chat
        | OhosKeyCode::Finance
        | OhosKeyCode::Cancel
        | OhosKeyCode::Send
        | OhosKeyCode::Reply
        | OhosKeyCode::Save
        | OhosKeyCode::Documents
        | OhosKeyCode::WordProcessor
        | OhosKeyCode::Editor
        | OhosKeyCode::Spreadsheet
        | OhosKeyCode::GraphicsEditor
        | OhosKeyCode::Presentation
        | OhosKeyCode::Database
        | OhosKeyCode::News
        | OhosKeyCode::Voicemail
        | OhosKeyCode::AddressBook
        | OhosKeyCode::Messenger
        | OhosKeyCode::SpellCheck
        | OhosKeyCode::Coffee
        | OhosKeyCode::Images
        | OhosKeyCode::ButtonConfig
        | OhosKeyCode::Journal
        | OhosKeyCode::KbdLayoutNext
        | OhosKeyCode::MsDos
        | OhosKeyCode::Goto
        | OhosKeyCode::Info
        | OhosKeyCode::Program => None,

        // OHOS-specific: editing extras without an android-table counterpart.
        OhosKeyCode::Undo
        | OhosKeyCode::Open
        | OhosKeyCode::Find
        | OhosKeyCode::Help
        | OhosKeyCode::Again
        | OhosKeyCode::Props
        | OhosKeyCode::ScrollUp
        | OhosKeyCode::ScrollDown
        | OhosKeyCode::KbdInputAssistPrev
        | OhosKeyCode::KbdInputAssistNext
        | OhosKeyCode::KbdInputAssistPrevGroup
        | OhosKeyCode::KbdInputAssistNextGroup
        | OhosKeyCode::KbdInputAssistAccept
        | OhosKeyCode::KbdInputAssistCancel => None,

        // OHOS-specific: TV / remote keys.
        OhosKeyCode::PVR
        | OhosKeyCode::Subtitle
        | OhosKeyCode::FullScreen
        | OhosKeyCode::Keyboard
        | OhosKeyCode::AspectRatio
        | OhosKeyCode::PC
        | OhosKeyCode::TV
        | OhosKeyCode::TV2
        | OhosKeyCode::VCR
        | OhosKeyCode::VCR2
        | OhosKeyCode::Sat
        | OhosKeyCode::CD
        | OhosKeyCode::Tape
        | OhosKeyCode::Tuner
        | OhosKeyCode::Player
        | OhosKeyCode::DVD
        | OhosKeyCode::Audio
        | OhosKeyCode::Video
        | OhosKeyCode::Memo
        | OhosKeyCode::Calendar
        | OhosKeyCode::Red
        | OhosKeyCode::Green
        | OhosKeyCode::Yellow
        | OhosKeyCode::Blue
        | OhosKeyCode::ChannelUp
        | OhosKeyCode::ChannelDown
        | OhosKeyCode::Channel
        | OhosKeyCode::Last
        | OhosKeyCode::Restart
        | OhosKeyCode::Slow
        | OhosKeyCode::Shuffle
        | OhosKeyCode::Videophone
        | OhosKeyCode::Games
        | OhosKeyCode::ZoomIn
        | OhosKeyCode::ZoomOut
        | OhosKeyCode::ZoomReset
        | OhosKeyCode::Stop
        | OhosKeyCode::Question
        | OhosKeyCode::Connect
        | OhosKeyCode::Sport
        | OhosKeyCode::Shop
        | OhosKeyCode::Hp
        | OhosKeyCode::Sound
        | OhosKeyCode::BtnMisc
        | OhosKeyCode::Setup
        | OhosKeyCode::Front
        | OhosKeyCode::SendFile
        | OhosKeyCode::DeleteFile
        | OhosKeyCode::Xfer
        | OhosKeyCode::Prog1
        | OhosKeyCode::Prog2
        | OhosKeyCode::Prog3
        | OhosKeyCode::Prog4
        | OhosKeyCode::Scale
        | OhosKeyCode::Macro
        | OhosKeyCode::Move
        | OhosKeyCode::LineFeed => None,

        // OHOS-specific: Korean keys (no winit KeyCode counterpart pinned).
        OhosKeyCode::Hanguel | OhosKeyCode::Hanja => None,

        // OHOS-specific: keypad / placeholder remnants.
        OhosKeyCode::Unknown
        | OhosKeyCode::Fn
        | OhosKeyCode::Star
        | OhosKeyCode::Pound
        | OhosKeyCode::At
        | OhosKeyCode::Sym
        | OhosKeyCode::DpadCenter
        | OhosKeyCode::Function
        | OhosKeyCode::ZenkakuHankaku
        | OhosKeyCode::NumpadPlusMinus
        | OhosKeyCode::Btn0
        | OhosKeyCode::Btn1
        | OhosKeyCode::Btn2
        | OhosKeyCode::Btn3
        | OhosKeyCode::Btn4
        | OhosKeyCode::Btn5
        | OhosKeyCode::Btn6
        | OhosKeyCode::Btn7
        | OhosKeyCode::Btn8
        | OhosKeyCode::Btn9 => None,
    }
}

/// Maps a binding `KeyCode` to the winit logical key.
///
/// Printable keys map to a lowercase `Key::Character` (egui derives the case
/// from modifiers); named keys map to `Key::Named`; the rest — the
/// OHOS-specific media / volume / brightness / system bucket — map to
/// `Key::Unidentified`. The trailing wildcard is android-template-shaped and
/// safe: an unknown binding variant logically *is* unidentified.
pub(crate) fn to_logical_key(code: OhosKeyCode) -> Key {
    match code {
        // Printable → Character (lowercase).
        OhosKeyCode::A => Key::Character("a".into()),
        OhosKeyCode::B => Key::Character("b".into()),
        OhosKeyCode::C => Key::Character("c".into()),
        OhosKeyCode::D => Key::Character("d".into()),
        OhosKeyCode::E => Key::Character("e".into()),
        OhosKeyCode::F => Key::Character("f".into()),
        OhosKeyCode::G => Key::Character("g".into()),
        OhosKeyCode::H => Key::Character("h".into()),
        OhosKeyCode::I => Key::Character("i".into()),
        OhosKeyCode::J => Key::Character("j".into()),
        OhosKeyCode::K => Key::Character("k".into()),
        OhosKeyCode::L => Key::Character("l".into()),
        OhosKeyCode::M => Key::Character("m".into()),
        OhosKeyCode::N => Key::Character("n".into()),
        OhosKeyCode::O => Key::Character("o".into()),
        OhosKeyCode::P => Key::Character("p".into()),
        OhosKeyCode::Q => Key::Character("q".into()),
        OhosKeyCode::R => Key::Character("r".into()),
        OhosKeyCode::S => Key::Character("s".into()),
        OhosKeyCode::T => Key::Character("t".into()),
        OhosKeyCode::U => Key::Character("u".into()),
        OhosKeyCode::V => Key::Character("v".into()),
        OhosKeyCode::W => Key::Character("w".into()),
        OhosKeyCode::X => Key::Character("x".into()),
        OhosKeyCode::Y => Key::Character("y".into()),
        OhosKeyCode::Z => Key::Character("z".into()),

        OhosKeyCode::Key0 => Key::Character("0".into()),
        OhosKeyCode::Key1 => Key::Character("1".into()),
        OhosKeyCode::Key2 => Key::Character("2".into()),
        OhosKeyCode::Key3 => Key::Character("3".into()),
        OhosKeyCode::Key4 => Key::Character("4".into()),
        OhosKeyCode::Key5 => Key::Character("5".into()),
        OhosKeyCode::Key6 => Key::Character("6".into()),
        OhosKeyCode::Key7 => Key::Character("7".into()),
        OhosKeyCode::Key8 => Key::Character("8".into()),
        OhosKeyCode::Key9 => Key::Character("9".into()),

        OhosKeyCode::Comma => Key::Character(",".into()),
        OhosKeyCode::Period => Key::Character(".".into()),
        OhosKeyCode::Grave => Key::Character("`".into()),
        OhosKeyCode::Minus => Key::Character("-".into()),
        OhosKeyCode::Equals => Key::Character("=".into()),
        OhosKeyCode::LeftBracket => Key::Character("[".into()),
        OhosKeyCode::RightBracket => Key::Character("]".into()),
        OhosKeyCode::Backslash => Key::Character("\\".into()),
        OhosKeyCode::Semicolon => Key::Character(";".into()),
        OhosKeyCode::Apostrophe => Key::Character("'".into()),
        OhosKeyCode::Slash => Key::Character("/".into()),
        OhosKeyCode::At => Key::Character("@".into()),
        OhosKeyCode::Plus => Key::Character("+".into()),
        OhosKeyCode::Star => Key::Character("*".into()),
        OhosKeyCode::Pound => Key::Character("#".into()),

        OhosKeyCode::Numpad0 => Key::Character("0".into()),
        OhosKeyCode::Numpad1 => Key::Character("1".into()),
        OhosKeyCode::Numpad2 => Key::Character("2".into()),
        OhosKeyCode::Numpad3 => Key::Character("3".into()),
        OhosKeyCode::Numpad4 => Key::Character("4".into()),
        OhosKeyCode::Numpad5 => Key::Character("5".into()),
        OhosKeyCode::Numpad6 => Key::Character("6".into()),
        OhosKeyCode::Numpad7 => Key::Character("7".into()),
        OhosKeyCode::Numpad8 => Key::Character("8".into()),
        OhosKeyCode::Numpad9 => Key::Character("9".into()),
        OhosKeyCode::NumpadDivide => Key::Character("/".into()),
        OhosKeyCode::NumpadMultiply => Key::Character("*".into()),
        OhosKeyCode::NumpadSubtract => Key::Character("-".into()),
        OhosKeyCode::NumpadAdd => Key::Character("+".into()),
        OhosKeyCode::NumpadDot => Key::Character(".".into()),
        OhosKeyCode::NumpadComma => Key::Character(",".into()),
        OhosKeyCode::NumpadEquals => Key::Character("=".into()),
        OhosKeyCode::NumpadLeftParen => Key::Character("(".into()),
        OhosKeyCode::NumpadRightParen => Key::Character(")".into()),

        // Named.
        OhosKeyCode::Enter | OhosKeyCode::NumpadEnter | OhosKeyCode::DpadCenter => {
            Key::Named(NamedKey::Enter)
        },
        OhosKeyCode::Tab => Key::Named(NamedKey::Tab),
        OhosKeyCode::Space => Key::Named(NamedKey::Space),
        OhosKeyCode::Escape => Key::Named(NamedKey::Escape),
        OhosKeyCode::Del => Key::Named(NamedKey::Backspace),
        OhosKeyCode::ForwardDel => Key::Named(NamedKey::Delete),
        OhosKeyCode::DpadUp => Key::Named(NamedKey::ArrowUp),
        OhosKeyCode::DpadDown => Key::Named(NamedKey::ArrowDown),
        OhosKeyCode::DpadLeft => Key::Named(NamedKey::ArrowLeft),
        OhosKeyCode::DpadRight => Key::Named(NamedKey::ArrowRight),
        OhosKeyCode::PageUp => Key::Named(NamedKey::PageUp),
        OhosKeyCode::PageDown => Key::Named(NamedKey::PageDown),
        OhosKeyCode::MoveHome => Key::Named(NamedKey::Home),
        OhosKeyCode::MoveEnd => Key::Named(NamedKey::End),
        OhosKeyCode::Insert => Key::Named(NamedKey::Insert),
        OhosKeyCode::CapsLock => Key::Named(NamedKey::CapsLock),
        OhosKeyCode::ScrollLock => Key::Named(NamedKey::ScrollLock),
        OhosKeyCode::NumLock => Key::Named(NamedKey::NumLock),
        OhosKeyCode::SysRq => Key::Named(NamedKey::PrintScreen),
        OhosKeyCode::Break => Key::Named(NamedKey::Pause),
        OhosKeyCode::AltLeft | OhosKeyCode::AltRight => Key::Named(NamedKey::Alt),
        OhosKeyCode::ShiftLeft | OhosKeyCode::ShiftRight => Key::Named(NamedKey::Shift),
        OhosKeyCode::CtrlLeft | OhosKeyCode::CtrlRight => Key::Named(NamedKey::Control),
        OhosKeyCode::MetaLeft | OhosKeyCode::MetaRight => Key::Named(NamedKey::Super),
        OhosKeyCode::Function => Key::Named(NamedKey::Fn),
        OhosKeyCode::ZenkakuHankaku => Key::Named(NamedKey::ZenkakuHankaku),
        OhosKeyCode::Muhenkan => Key::Named(NamedKey::NonConvert),
        OhosKeyCode::Henkan => Key::Named(NamedKey::Convert),
        OhosKeyCode::KatakanaHiragana => Key::Named(NamedKey::HiraganaKatakana),
        OhosKeyCode::Katakana => Key::Named(NamedKey::Katakana),
        OhosKeyCode::Hiragana => Key::Named(NamedKey::Hiragana),
        OhosKeyCode::Copy => Key::Named(NamedKey::Copy),
        OhosKeyCode::Paste => Key::Named(NamedKey::Paste),
        OhosKeyCode::Cut => Key::Named(NamedKey::Cut),
        OhosKeyCode::Help => Key::Named(NamedKey::Help),

        // The OHOS-specific media / volume / brightness / system bucket and
        // everything without a pinned mapping.
        _ => Key::Unidentified(NativeKey::Unidentified),
    }
}

/// Maps a binding `KeyCode` to the winit key location.
///
/// All four modifier families have Left/Right splits in the binding; the
/// numpad cluster (digits, operators, `NumLock`) reports `Numpad`; anything
/// else reports `Standard`. The trailing wildcard is android-template-shaped
/// and safe: an unknown binding variant has no side/numpad information.
pub(crate) fn to_location(code: OhosKeyCode) -> KeyLocation {
    match code {
        OhosKeyCode::AltLeft
        | OhosKeyCode::ShiftLeft
        | OhosKeyCode::CtrlLeft
        | OhosKeyCode::MetaLeft => KeyLocation::Left,
        OhosKeyCode::AltRight
        | OhosKeyCode::ShiftRight
        | OhosKeyCode::CtrlRight
        | OhosKeyCode::MetaRight => KeyLocation::Right,
        OhosKeyCode::NumLock
        | OhosKeyCode::Numpad0
        | OhosKeyCode::Numpad1
        | OhosKeyCode::Numpad2
        | OhosKeyCode::Numpad3
        | OhosKeyCode::Numpad4
        | OhosKeyCode::Numpad5
        | OhosKeyCode::Numpad6
        | OhosKeyCode::Numpad7
        | OhosKeyCode::Numpad8
        | OhosKeyCode::Numpad9
        | OhosKeyCode::NumpadDivide
        | OhosKeyCode::NumpadMultiply
        | OhosKeyCode::NumpadSubtract
        | OhosKeyCode::NumpadAdd
        | OhosKeyCode::NumpadDot
        | OhosKeyCode::NumpadComma
        | OhosKeyCode::NumpadEnter
        | OhosKeyCode::NumpadEquals
        | OhosKeyCode::NumpadLeftParen
        | OhosKeyCode::NumpadRightParen
        | OhosKeyCode::NumpadPlusMinus => KeyLocation::Numpad,
        _ => KeyLocation::Standard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UT for `to_physical_key` — spot matrix over the 330-variant table
    /// (positive rows + the OHOS-specific `None` bucket).
    /// # Brief
    /// 1. Standard keys (letters, digits, F-row, navigation, punctuation, modifiers, numpad) map to
    ///    `Some` of the winit `KeyCode` they share with the android template.
    /// 2. OHOS-specific media / volume / brightness / system keys map to `None`.
    #[test]
    fn keycode_table_spot_matrix() {
        // 1. Positive rows.
        assert_eq!(to_physical_key(OhosKeyCode::A), Some(KeyCode::KeyA));
        assert_eq!(to_physical_key(OhosKeyCode::Z), Some(KeyCode::KeyZ));
        assert_eq!(to_physical_key(OhosKeyCode::Key0), Some(KeyCode::Digit0));
        assert_eq!(to_physical_key(OhosKeyCode::Key9), Some(KeyCode::Digit9));
        assert_eq!(to_physical_key(OhosKeyCode::Enter), Some(KeyCode::Enter));
        assert_eq!(to_physical_key(OhosKeyCode::Space), Some(KeyCode::Space));
        assert_eq!(to_physical_key(OhosKeyCode::Tab), Some(KeyCode::Tab));
        assert_eq!(to_physical_key(OhosKeyCode::Del), Some(KeyCode::Backspace));
        assert_eq!(to_physical_key(OhosKeyCode::ForwardDel), Some(KeyCode::Delete));
        assert_eq!(to_physical_key(OhosKeyCode::DpadUp), Some(KeyCode::ArrowUp));
        assert_eq!(to_physical_key(OhosKeyCode::DpadLeft), Some(KeyCode::ArrowLeft));
        assert_eq!(to_physical_key(OhosKeyCode::ShiftLeft), Some(KeyCode::ShiftLeft));
        assert_eq!(to_physical_key(OhosKeyCode::ShiftRight), Some(KeyCode::ShiftRight));
        assert_eq!(to_physical_key(OhosKeyCode::CtrlLeft), Some(KeyCode::ControlLeft));
        assert_eq!(to_physical_key(OhosKeyCode::AltRight), Some(KeyCode::AltRight));
        assert_eq!(to_physical_key(OhosKeyCode::MetaLeft), Some(KeyCode::SuperLeft));
        assert_eq!(to_physical_key(OhosKeyCode::F1), Some(KeyCode::F1));
        assert_eq!(to_physical_key(OhosKeyCode::F13), Some(KeyCode::F13));
        assert_eq!(to_physical_key(OhosKeyCode::Numpad7), Some(KeyCode::Numpad7));
        assert_eq!(to_physical_key(OhosKeyCode::NumpadDot), Some(KeyCode::NumpadDecimal));
        assert_eq!(to_physical_key(OhosKeyCode::NumpadEquals), Some(KeyCode::NumpadEqual));
        assert_eq!(to_physical_key(OhosKeyCode::Equals), Some(KeyCode::Equal));
        assert_eq!(to_physical_key(OhosKeyCode::Grave), Some(KeyCode::Backquote));

        // 2. OHOS-specific `None` bucket.
        assert_eq!(to_physical_key(OhosKeyCode::VolumeUp), None);
        assert_eq!(to_physical_key(OhosKeyCode::VolumeDown), None);
        assert_eq!(to_physical_key(OhosKeyCode::VolumeMute), None);
        assert_eq!(to_physical_key(OhosKeyCode::Mute), None);
        assert_eq!(to_physical_key(OhosKeyCode::BrightnessUp), None);
        assert_eq!(to_physical_key(OhosKeyCode::Power), None);
        assert_eq!(to_physical_key(OhosKeyCode::Home), None);
        assert_eq!(to_physical_key(OhosKeyCode::Back), None);
    }

    /// UT for `to_logical_key` — printable / named / unidentified tri-state,
    /// with `text` derived from the winit `Key::to_text` contract.
    /// # Brief
    /// 1. Printable keys map to lowercase `Character` and carry that text.
    /// 2. Named keys map to `Named`; Enter carries the control-character text `"\r"` per the winit
    ///    `Key::to_text` contract (apply-time adjudication — the design row originally said `None`,
    ///    which contradicted its own formula).
    /// 3. OHOS-specific keys map to `Unidentified` with no text.
    #[test]
    fn key_logical_and_text() {
        // 1. Printable.
        assert_eq!(to_logical_key(OhosKeyCode::A), "a");
        assert_eq!(to_logical_key(OhosKeyCode::A).to_text(), Some("a"));
        assert_eq!(to_logical_key(OhosKeyCode::Key5), "5");
        assert_eq!(to_logical_key(OhosKeyCode::Minus), "-");
        assert_eq!(to_logical_key(OhosKeyCode::NumpadAdd), "+");

        // 2. Named.
        assert_eq!(to_logical_key(OhosKeyCode::Enter), NamedKey::Enter);
        assert_eq!(to_logical_key(OhosKeyCode::Enter).to_text(), Some("\r"));
        assert_eq!(to_logical_key(OhosKeyCode::Space), NamedKey::Space);
        assert_eq!(to_logical_key(OhosKeyCode::Space).to_text(), Some(" "));
        assert_eq!(to_logical_key(OhosKeyCode::DpadUp), NamedKey::ArrowUp);
        assert_eq!(to_logical_key(OhosKeyCode::AltLeft), NamedKey::Alt);

        // 3. Unidentified.
        assert!(matches!(to_logical_key(OhosKeyCode::VolumeUp), Key::Unidentified(_)));
        assert_eq!(to_logical_key(OhosKeyCode::VolumeUp).to_text(), None);
        assert_eq!(to_logical_key(OhosKeyCode::Mute).to_text(), None);
    }

    /// UT for `to_location` — Left / Right / Numpad / Standard.
    /// # Brief
    /// 1. Split modifier variants report their side (the binding has Left/Right splits for all four
    ///    modifier families).
    /// 2. Numpad cluster keys report `Numpad`.
    /// 3. Everything else reports `Standard`.
    #[test]
    fn key_location_mapping() {
        // 1. Modifier sides.
        assert_eq!(to_location(OhosKeyCode::ShiftLeft), KeyLocation::Left);
        assert_eq!(to_location(OhosKeyCode::ShiftRight), KeyLocation::Right);
        assert_eq!(to_location(OhosKeyCode::CtrlLeft), KeyLocation::Left);
        assert_eq!(to_location(OhosKeyCode::CtrlRight), KeyLocation::Right);
        assert_eq!(to_location(OhosKeyCode::AltLeft), KeyLocation::Left);
        assert_eq!(to_location(OhosKeyCode::AltRight), KeyLocation::Right);
        assert_eq!(to_location(OhosKeyCode::MetaLeft), KeyLocation::Left);
        assert_eq!(to_location(OhosKeyCode::MetaRight), KeyLocation::Right);

        // 2. Numpad cluster.
        assert_eq!(to_location(OhosKeyCode::Numpad7), KeyLocation::Numpad);
        assert_eq!(to_location(OhosKeyCode::NumpadEnter), KeyLocation::Numpad);
        assert_eq!(to_location(OhosKeyCode::NumpadDot), KeyLocation::Numpad);
        assert_eq!(to_location(OhosKeyCode::NumLock), KeyLocation::Numpad);

        // 3. Standard.
        assert_eq!(to_location(OhosKeyCode::A), KeyLocation::Standard);
        assert_eq!(to_location(OhosKeyCode::Space), KeyLocation::Standard);
        assert_eq!(to_location(OhosKeyCode::Enter), KeyLocation::Standard);
        assert_eq!(to_location(OhosKeyCode::VolumeUp), KeyLocation::Standard);
    }
}
