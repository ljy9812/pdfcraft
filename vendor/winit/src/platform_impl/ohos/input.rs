//! OHOS input event translation.
//!
//! Translates `XComponent` binding input data (touch / mouse / axis / key)
//! into winit `WindowEvent`s. The mapping contracts live in
//! `ohos-winit-p4` design.md D2-D6; the android backend
//! (`platform_impl/android/mod.rs`) is the reference for the primary-point
//! versus all-points touch semantics.

use super::keycodes::{to_location, to_logical_key, to_physical_key};
use super::{DeviceId, KeyEventExtra};
use crate::dpi::PhysicalPosition;
use crate::event::{
    ElementState, Force, KeyEvent, Modifiers, MouseButton as WinitMouseButton, MouseScrollDelta,
    Touch, TouchPhase, WindowEvent,
};
use crate::keyboard::{KeyCode as WinitKeyCode, ModifiersState, NativeKeyCode, PhysicalKey};
use ohos_xcomponent_binding::{
    Action, KeyEventData, MouseButton as OhosMouseButton, TouchEvent, TouchEventData,
};
use openharmony_ability::{AxisEventData, InputEvent, MouseAction, MouseEventData};
use smol_str::SmolStr;
use tracing::trace;

/// The fork's dummy device id shared by all synthesized input events.
fn dummy_device_id() -> crate::event::DeviceId {
    crate::event::DeviceId(DeviceId::dummy())
}

/// Translates a binding touch event into winit `Touch` window events.
///
/// Follows the android backend's primary-point / all-points semantics:
/// `Down`/`Up` emit exactly one `Touch` for the event-level primary point,
/// `Move`/`Cancel` map every `touch_point` (with the event-level phase, which
/// is how android applies its action to all pointers), and `Unknown` emits
/// nothing. See `ohos-winit-p4` design.md D2 for the field mapping contract.
pub(crate) fn translate_touch(data: &TouchEventData) -> Vec<WindowEvent> {
    match data.event_type {
        TouchEvent::Down => vec![primary_touch(TouchPhase::Started, data)],
        TouchEvent::Up => vec![primary_touch(TouchPhase::Ended, data)],
        TouchEvent::Move => all_points_touch(TouchPhase::Moved, data),
        TouchEvent::Cancel => all_points_touch(TouchPhase::Cancelled, data),
        TouchEvent::Unknown => Vec::new(),
    }
}

/// Builds the single primary-point `Touch` for `Down`/`Up` events.
fn primary_touch(phase: TouchPhase, data: &TouchEventData) -> WindowEvent {
    touch_from_parts(phase, data.x, data.y, data.id, data.force)
}

/// Builds one `Touch` per touch point for `Move`/`Cancel` events.
fn all_points_touch(phase: TouchPhase, data: &TouchEventData) -> Vec<WindowEvent> {
    data.touch_points
        .iter()
        .map(|point| touch_from_parts(phase, point.x, point.y, point.id, point.force))
        .collect()
}

/// Assembles a winit `Touch` from translated binding fields.
fn touch_from_parts(phase: TouchPhase, x: f32, y: f32, id: i32, force: f32) -> WindowEvent {
    WindowEvent::Touch(Touch {
        device_id: dummy_device_id(),
        phase,
        location: PhysicalPosition::new(x as f64, y as f64),
        // Real touch ids are non-negative; a pathological negative id would
        // silently wrap to a huge u64 under `as`, so degrade to 0 instead
        // (audit F2: no silent wrap-around on external input).
        id: u64::try_from(id).unwrap_or(0),
        force: Some(Force::Normalized(force.clamp(0.0, 1.0) as f64)),
    })
}

/// Translates an ability mouse event into winit mouse window events.
///
/// See `ohos-winit-p4` design.md D3 for the action x button matrix.
/// `None` is the only empty action arm (listed explicitly, no wildcard);
/// `Press`/`Release` with `NoneButton` also produce nothing because there is
/// no button to report.
pub(crate) fn translate_mouse(data: &MouseEventData) -> Vec<WindowEvent> {
    match data.action {
        MouseAction::Move => vec![WindowEvent::CursorMoved {
            device_id: dummy_device_id(),
            position: PhysicalPosition::new(data.x as f64, data.y as f64),
        }],
        MouseAction::Press => mouse_input(ElementState::Pressed, data),
        MouseAction::Release => mouse_input(ElementState::Released, data),
        // Hover events carry no position data, so the coordinates are unused.
        MouseAction::HoverEnter => {
            vec![WindowEvent::CursorEntered { device_id: dummy_device_id() }]
        },
        MouseAction::HoverLeave => vec![WindowEvent::CursorLeft { device_id: dummy_device_id() }],
        MouseAction::None => Vec::new(),
    }
}

/// Builds the `MouseInput` for a Press/Release action, if a button is known.
fn mouse_input(state: ElementState, data: &MouseEventData) -> Vec<WindowEvent> {
    let Some(button) = to_mouse_button(data.button) else {
        trace!(
            ?state,
            ?data.button,
            "ohos: mouse press/release without a button; dropped"
        );
        return Vec::new();
    };
    vec![WindowEvent::MouseInput { device_id: dummy_device_id(), state, button }]
}

/// Maps the binding mouse buttons onto the winit ones. `NoneButton` has no
/// winit counterpart and maps to `None`.
fn to_mouse_button(button: OhosMouseButton) -> Option<WinitMouseButton> {
    match button {
        OhosMouseButton::LeftButton => Some(WinitMouseButton::Left),
        OhosMouseButton::RightButton => Some(WinitMouseButton::Right),
        OhosMouseButton::MiddleButton => Some(WinitMouseButton::Middle),
        OhosMouseButton::BackButton => Some(WinitMouseButton::Back),
        OhosMouseButton::ForwardButton => Some(WinitMouseButton::Forward),
        OhosMouseButton::NoneButton => None,
    }
}

/// Translates an ability axis event into a winit wheel event.
///
/// The deltas pass through as `LineDelta` with a single `Moved` phase (no
/// Started/Ended synthesis - egui-winit only consumes delta+phase).
/// `pinch_scale` is not part of the demo interaction set and is dropped;
/// an actually active pinch is traced so the loss is observable on-device.
pub(crate) fn translate_axis(data: &AxisEventData) -> Vec<WindowEvent> {
    // 0.0 = no pinch data, 1.0 = no change; anything else is a live pinch.
    if data.pinch_scale != 0.0 && data.pinch_scale != 1.0 {
        trace!(pinch_scale = data.pinch_scale, "ohos: pinch scale is not translated; dropped");
    }
    vec![WindowEvent::MouseWheel {
        device_id: dummy_device_id(),
        delta: MouseScrollDelta::LineDelta(data.delta_x, data.delta_y),
        phase: TouchPhase::Moved,
    }]
}

/// Tracks modifier state synthesized from OHOS key events, which carry no
/// modifiers themselves (design D6, web-backend differential-push pattern).
#[derive(Debug, Default)]
pub(crate) struct ModifiersTracker {
    state: ModifiersState,
}

impl ModifiersTracker {
    /// Applies a modifier press/release; returns `true` when the state changed.
    fn update(&mut self, bit: ModifiersState, state: ElementState) -> bool {
        let before = self.state;
        match state {
            ElementState::Pressed => self.state.insert(bit),
            ElementState::Released => self.state.remove(bit),
        }
        self.state != before
    }

    /// The currently tracked modifier state.
    fn state(&self) -> ModifiersState {
        self.state
    }

    /// Clears all tracked modifiers. Called on `Focused(false)` and
    /// `Suspended` dispatch: the OS resets modifiers on focus loss, and a
    /// stale SHIFT would poison later sessions.
    pub(crate) fn reset(&mut self) {
        self.state = ModifiersState::empty();
    }
}

/// Returns the `ModifiersState` bit a winit physical key code controls, if any.
///
/// CapsLock/NumLock lock states are deliberately not tracked (OHOS key
/// events carry no lock semantics; egui derives case from Character+SHIFT).
fn modifier_bit(code: WinitKeyCode) -> Option<ModifiersState> {
    match code {
        WinitKeyCode::ShiftLeft | WinitKeyCode::ShiftRight => Some(ModifiersState::SHIFT),
        WinitKeyCode::ControlLeft | WinitKeyCode::ControlRight => Some(ModifiersState::CONTROL),
        WinitKeyCode::AltLeft | WinitKeyCode::AltRight => Some(ModifiersState::ALT),
        WinitKeyCode::SuperLeft | WinitKeyCode::SuperRight => Some(ModifiersState::SUPER),
        _ => None,
    }
}

/// Translates a binding key event into winit keyboard window events.
///
/// Assembles the `KeyEvent` per design D5 (android `mod.rs:458` template):
/// the three keycode-table faces plus `text` derived from the logical key and
/// gated on Pressed. When the key controls a modifier, a differential
/// `ModifiersChanged` is pushed *before* the `KeyboardInput` (winit
/// convention; egui-winit relies on modifiers being current when the key
/// event is consumed).
pub(crate) fn translate_key(
    data: &KeyEventData,
    modifiers: &mut ModifiersTracker,
) -> Vec<WindowEvent> {
    let state = match data.action {
        Action::Down => ElementState::Pressed,
        Action::Up => ElementState::Released,
        Action::Unknown => return Vec::new(),
    };

    let physical = to_physical_key(data.code);
    let logical = to_logical_key(data.code);

    let mut events = Vec::new();
    if let Some(bit) = physical.and_then(modifier_bit) {
        if modifiers.update(bit, state) {
            events.push(WindowEvent::ModifiersChanged(Modifiers::from(modifiers.state())));
        }
    }

    let text =
        if state == ElementState::Pressed { logical.to_text().map(SmolStr::new) } else { None };

    events.push(WindowEvent::KeyboardInput {
        device_id: dummy_device_id(),
        event: KeyEvent {
            physical_key: physical
                .map(PhysicalKey::Code)
                .unwrap_or(PhysicalKey::Unidentified(NativeKeyCode::Unidentified)),
            logical_key: logical,
            text,
            location: to_location(data.code),
            state,
            repeat: false,
            platform_specific: KeyEventExtra {},
        },
        is_synthetic: false,
    });
    events
}

/// Loop-side input translator (design D1): routes each distilled ability
/// input event to its p4 translation function, threading the modifier state
/// machine through key events. Lives on the `EventLoop` (as a `RefCell` —
/// dispatch takes `&self`) so modifiers persist across input events within a
/// focus session.
#[derive(Debug, Default)]
pub(crate) struct InputTranslator {
    modifiers: ModifiersTracker,
}

impl InputTranslator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Translates one ability input event into winit `WindowEvent`s.
    ///
    /// `ImeEvent` yields nothing — IME is out of p4 scope (the ability layer
    /// exposes no preedit); the arm is explicit so the exhaustive match fails
    /// if the binding grows a variant.
    pub(crate) fn translate(&mut self, input: &InputEvent) -> Vec<WindowEvent> {
        match input {
            InputEvent::TouchEvent(data) => translate_touch(data),
            InputEvent::MouseEvent(data) => translate_mouse(data),
            InputEvent::AxisEvent(data) => translate_axis(data),
            InputEvent::KeyEvent(data) => translate_key(data, &mut self.modifiers),
            InputEvent::ImeEvent(_) => Vec::new(),
        }
    }

    /// Clears tracked modifiers — hooked at `Focused(false)` and `Suspended`
    /// dispatch (design D6: the OS resets modifiers on focus loss).
    pub(crate) fn reset(&mut self) {
        self.modifiers.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Test-only binding imports: keeping them out of the module-level import
    // list keeps the non-test ohos build (winit as a dependency) warning-free.
    use ohos_xcomponent_binding::{EventSource, KeyCode as OhosKeyCode, TouchPointData};

    /// UT for translate_touch — touch_phase_maps_to_touchphase.
    /// # Brief
    /// 1. Build a Down/Up/Move/Cancel touch event and one with Unknown.
    /// 2. Assert the produced Touch phases are Started/Ended/Moved/Cancelled.
    /// 3. Assert the Unknown event produces no events at all.
    #[test]
    fn touch_phase_maps_to_touchphase() {
        let cases = [
            (TouchEvent::Down, TouchPhase::Started),
            (TouchEvent::Up, TouchPhase::Ended),
            (TouchEvent::Move, TouchPhase::Moved),
            (TouchEvent::Cancel, TouchPhase::Cancelled),
        ];
        for (event_type, expected_phase) in cases {
            let events = translate_touch(&touch_event(event_type, 10.0, 20.0, 0, 0.5));
            assert_eq!(events.len(), 1, "{event_type:?} must yield one Touch");
            assert_eq!(as_touch(&events, 0).phase, expected_phase);
        }

        let unknown = translate_touch(&touch_event(TouchEvent::Unknown, 10.0, 20.0, 0, 0.5));
        assert!(unknown.is_empty(), "Unknown phase must not produce events, got {unknown:?}");
    }

    /// UT for translate_touch — touch_primary_vs_all_points.
    /// # Brief
    /// 1. Down/Up events carry both event-level fields and touch_points.
    /// 2. Assert exactly one Touch built from the event-level primary point.
    /// 3. Move/Cancel with N touch_points yield N Touches, one per point.
    #[test]
    fn touch_primary_vs_all_points() {
        for event_type in [TouchEvent::Down, TouchEvent::Up] {
            let data = TouchEventData {
                id: 3,
                x: 100.0,
                y: 200.0,
                event_type,
                force: 0.5,
                // The point list must be ignored for the primary-point phases.
                touch_points: vec![
                    touch_point(event_type, 900.0, 800.0, 7, 0.5),
                    touch_point(event_type, 700.0, 600.0, 8, 0.5),
                ],
                ..TouchEventData::default()
            };
            let events = translate_touch(&data);
            assert_eq!(events.len(), 1, "{event_type:?} yields primary point only");
            let touch = as_touch(&events, 0);
            assert_eq!(touch.id, 3);
            assert_eq!(touch.location.x, 100.0);
            assert_eq!(touch.location.y, 200.0);
        }

        for event_type in [TouchEvent::Move, TouchEvent::Cancel] {
            let data = TouchEventData {
                id: 0,
                x: 100.0,
                y: 200.0,
                event_type,
                force: 0.5,
                touch_points: vec![
                    touch_point(event_type, 11.0, 12.0, 1, 0.25),
                    touch_point(event_type, 21.0, 22.0, 2, 0.5),
                    touch_point(event_type, 31.0, 32.0, 3, 0.75),
                ],
                ..TouchEventData::default()
            };
            let events = translate_touch(&data);
            assert_eq!(events.len(), 3, "{event_type:?} maps every touch point");
            for (index, expected_x) in [(0, 11.0), (1, 21.0), (2, 31.0)] {
                let touch = as_touch(&events, index);
                assert_eq!(touch.id, (index + 1) as u64);
                assert_eq!(touch.location.x, expected_x);
                assert_eq!(touch.location.y, (expected_x + 1.0));
            }
        }
    }

    /// UT for translate_touch — touch_field_mapping.
    /// # Brief
    /// 1. Assert location passes through x/y as physical pixels.
    /// 2. Assert id widens from i32 to u64.
    /// 3. Assert force maps to clamped Normalized at all three boundaries.
    #[test]
    fn touch_field_mapping() {
        let data = touch_event(TouchEvent::Down, 123.0, 456.0, 9, 0.5);
        let events = translate_touch(&data);
        let touch = as_touch(&events, 0);
        assert_eq!(touch.location, PhysicalPosition::new(123.0, 456.0));
        assert_eq!(touch.id, 9);
        assert_eq!(touch.force, Some(Force::Normalized(0.5)));

        // Clamp boundaries: above 1.0 and below 0.0 snap into [0, 1].
        let over_events = translate_touch(&touch_event(TouchEvent::Down, 1.0, 2.0, 0, 1.7));
        assert_eq!(as_touch(&over_events, 0).force, Some(Force::Normalized(1.0)));

        let under_events = translate_touch(&touch_event(TouchEvent::Down, 1.0, 2.0, 0, -0.1));
        assert_eq!(as_touch(&under_events, 0).force, Some(Force::Normalized(0.0)));
    }

    /// UT for translate_mouse — mouse_action_matrix.
    /// # Brief
    /// 1. Build a mouse event for each of the six ability MouseAction values.
    /// 2. Assert Move→CursorMoved (px passthrough), Press/Release→MouseInput,
    ///    HoverEnter/HoverLeave→CursorEntered/CursorLeft.
    /// 3. Assert None is the only empty arm (zero events).
    #[test]
    fn mouse_action_matrix() {
        let moved = translate_mouse(&mouse_event(
            MouseAction::Move,
            100.0,
            200.0,
            OhosMouseButton::LeftButton,
        ));
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0], WindowEvent::CursorMoved {
            device_id: dummy_device_id(),
            position: PhysicalPosition::new(100.0, 200.0),
        });

        let pressed = translate_mouse(&mouse_event(
            MouseAction::Press,
            1.0,
            2.0,
            OhosMouseButton::LeftButton,
        ));
        assert_eq!(pressed.len(), 1);
        assert_eq!(pressed[0], WindowEvent::MouseInput {
            device_id: dummy_device_id(),
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
        });

        let released = translate_mouse(&mouse_event(
            MouseAction::Release,
            1.0,
            2.0,
            OhosMouseButton::RightButton,
        ));
        assert_eq!(released.len(), 1);
        assert_eq!(released[0], WindowEvent::MouseInput {
            device_id: dummy_device_id(),
            state: ElementState::Released,
            button: WinitMouseButton::Right,
        });

        // Hover events come from MouseEventData::hover() in the ability layer,
        // the only constructor the fork sees for the synthesized variants.
        let entered = translate_mouse(&MouseEventData::hover(true));
        assert_eq!(entered, vec![WindowEvent::CursorEntered { device_id: dummy_device_id() }]);

        let left = translate_mouse(&MouseEventData::hover(false));
        assert_eq!(left, vec![WindowEvent::CursorLeft { device_id: dummy_device_id() }]);

        let none =
            translate_mouse(&mouse_event(MouseAction::None, 1.0, 2.0, OhosMouseButton::LeftButton));
        assert!(none.is_empty(), "MouseAction::None must not produce events");
    }

    /// UT for translate_mouse — mouse_button_matrix.
    /// # Brief
    /// 1. Build Press events for each of the six binding MouseButton values.
    /// 2. Assert the five real buttons map to winit Left/Right/Middle/Back/Forward.
    /// 3. Assert NoneButton produces no MouseInput.
    #[test]
    fn mouse_button_matrix() {
        let cases = [
            (OhosMouseButton::LeftButton, WinitMouseButton::Left),
            (OhosMouseButton::RightButton, WinitMouseButton::Right),
            (OhosMouseButton::MiddleButton, WinitMouseButton::Middle),
            (OhosMouseButton::BackButton, WinitMouseButton::Back),
            (OhosMouseButton::ForwardButton, WinitMouseButton::Forward),
        ];
        for (button, expected) in cases {
            let events = translate_mouse(&mouse_event(MouseAction::Press, 5.0, 6.0, button));
            assert_eq!(events.len(), 1, "{button:?} must yield one MouseInput");
            assert_eq!(events[0], WindowEvent::MouseInput {
                device_id: dummy_device_id(),
                state: ElementState::Pressed,
                button: expected,
            });
        }

        let no_button = translate_mouse(&mouse_event(
            MouseAction::Press,
            5.0,
            6.0,
            OhosMouseButton::NoneButton,
        ));
        assert!(no_button.is_empty(), "Press without a button must not produce a MouseInput");
    }

    /// UT for translate_axis — axis_maps_to_mouse_wheel.
    /// # Brief
    /// 1. Build an axis event with deltas and an active pinch scale.
    /// 2. Assert exactly one MouseWheel with LineDelta passthrough and phase Moved.
    /// 3. Assert the pinch scale produces no additional event.
    #[test]
    fn axis_maps_to_mouse_wheel() {
        let data = AxisEventData {
            delta_x: 1.5,
            delta_y: -3.0,
            pinch_scale: 2.0,
            ..AxisEventData::default()
        };
        let events = translate_axis(&data);
        assert_eq!(events.len(), 1, "pinch must not produce extra events");
        assert_eq!(events[0], WindowEvent::MouseWheel {
            device_id: dummy_device_id(),
            delta: MouseScrollDelta::LineDelta(1.5, -3.0),
            phase: TouchPhase::Moved,
        });
    }

    /// Builds a binding touch event for a single finger: the event-level
    /// primary point is mirrored into a one-element `touch_points` list, the
    /// shape a real single-pointer Move/Cancel event arrives in. Tests that
    /// care about the point list override it explicitly.
    fn touch_event(event_type: TouchEvent, x: f32, y: f32, id: i32, force: f32) -> TouchEventData {
        TouchEventData {
            id,
            x,
            y,
            event_type,
            force,
            touch_points: vec![touch_point(event_type, x, y, id, force)],
            ..TouchEventData::default()
        }
    }

    /// UT for translate_key — key_action_mapping.
    /// # Brief
    /// 1. Build KeyA events with Down/Up/Unknown actions.
    /// 2. Assert Down/Up produce one KeyboardInput each with Pressed/Released, is_synthetic ==
    ///    false, no repeat, and the Pressed-gated text of D5.
    /// 3. Assert Unknown produces no events.
    #[test]
    fn key_action_mapping() {
        let mut tracker = ModifiersTracker::default();

        let down = translate_key(&key_event(OhosKeyCode::A, Action::Down), &mut tracker);
        assert_eq!(down.len(), 1);
        let (event, is_synthetic) = as_keyboard_input(&down, 0);
        assert!(!is_synthetic);
        assert!(!event.repeat);
        assert_eq!(event.state, ElementState::Pressed);
        assert_eq!(
            event.physical_key,
            PhysicalKey::Code(WinitKeyCode::KeyA),
            "physical key comes from the keycode table"
        );
        assert_eq!(event.text, Some(SmolStr::new("a")));

        let up = translate_key(&key_event(OhosKeyCode::A, Action::Up), &mut tracker);
        assert_eq!(up.len(), 1);
        let (event, is_synthetic) = as_keyboard_input(&up, 0);
        assert!(!is_synthetic);
        assert_eq!(event.state, ElementState::Released);
        assert_eq!(event.text, None, "text is Pressed-gated (design D5)");

        let unknown = translate_key(&key_event(OhosKeyCode::A, Action::Unknown), &mut tracker);
        assert!(unknown.is_empty(), "Unknown action must not produce events");
    }

    /// UT for translate_key — modifiers_differential_push.
    /// # Brief
    /// 1. Feed ShiftLeft Down, then KeyA Down (Shift held), then ShiftLeft Up.
    /// 2. Assert the first yields [ModifiersChanged(SHIFT), KeyboardInput] with Changed first
    ///    (winit convention; egui-winit relies on it).
    /// 3. Assert the unchanged state pushes no ModifiersChanged, and Up clears the bit again.
    #[test]
    fn modifiers_differential_push() {
        let mut tracker = ModifiersTracker::default();

        let shift_down =
            translate_key(&key_event(OhosKeyCode::ShiftLeft, Action::Down), &mut tracker);
        assert_eq!(shift_down.len(), 2);
        assert_eq!(
            shift_down[0],
            WindowEvent::ModifiersChanged(Modifiers::from(ModifiersState::SHIFT))
        );
        let (event, _) = as_keyboard_input(&shift_down, 1);
        assert_eq!(event.state, ElementState::Pressed);

        let a_down = translate_key(&key_event(OhosKeyCode::A, Action::Down), &mut tracker);
        assert_eq!(a_down.len(), 1, "unchanged modifier state must not push ModifiersChanged");
        assert!(matches!(a_down[0], WindowEvent::KeyboardInput { .. }));

        let shift_up = translate_key(&key_event(OhosKeyCode::ShiftLeft, Action::Up), &mut tracker);
        assert_eq!(shift_up.len(), 2);
        assert_eq!(
            shift_up[0],
            WindowEvent::ModifiersChanged(Modifiers::from(ModifiersState::empty()))
        );
        let (event, _) = as_keyboard_input(&shift_up, 1);
        assert_eq!(event.state, ElementState::Released);
    }

    /// UT for ModifiersTracker — modifiers_reset_on_focus_loss.
    /// # Brief
    /// 1. Hold Shift via a Down event, then call reset() (focus loss/suspend).
    /// 2. Assert the tracked state is empty afterwards.
    /// 3. Assert a subsequent non-modifier Down produces no ModifiersChanged.
    #[test]
    fn modifiers_reset_on_focus_loss() {
        let mut tracker = ModifiersTracker::default();
        let shift_down =
            translate_key(&key_event(OhosKeyCode::ShiftLeft, Action::Down), &mut tracker);
        assert_eq!(
            shift_down[0],
            WindowEvent::ModifiersChanged(Modifiers::from(ModifiersState::SHIFT))
        );

        tracker.reset();
        assert_eq!(tracker.state(), ModifiersState::empty());

        let a_down = translate_key(&key_event(OhosKeyCode::A, Action::Down), &mut tracker);
        assert_eq!(a_down.len(), 1, "no stale ModifiersChanged after reset");
        assert!(matches!(a_down[0], WindowEvent::KeyboardInput { .. }));
    }

    /// Builds an ability mouse event with the given action, button, and
    /// window-relative position.
    fn mouse_event(action: MouseAction, x: f32, y: f32, button: OhosMouseButton) -> MouseEventData {
        MouseEventData { action, button, x, y, ..MouseEventData::default() }
    }

    /// Builds a binding touch point with per-point fields.
    fn touch_point(event_type: TouchEvent, x: f32, y: f32, id: i32, force: f32) -> TouchPointData {
        TouchPointData { id, x, y, event_type, force, ..TouchPointData::default() }
    }

    /// Builds a binding key event; `KeyEventData` has no Default impl.
    fn key_event(code: OhosKeyCode, action: Action) -> KeyEventData {
        KeyEventData { code, action, device_id: 0, source: EventSource::Keyboard, timestamp: 0 }
    }

    /// Unwraps the index-th window event as a KeyboardInput, returning the
    /// key event and the is_synthetic flag.
    fn as_keyboard_input(events: &[WindowEvent], index: usize) -> (&KeyEvent, bool) {
        match &events[index] {
            WindowEvent::KeyboardInput { event, is_synthetic, .. } => (event, *is_synthetic),
            other => panic!("expected a KeyboardInput event, got {other:?}"),
        }
    }

    /// Unwraps the index-th window event as a Touch, failing loudly otherwise.
    fn as_touch(events: &[WindowEvent], index: usize) -> &Touch {
        match &events[index] {
            WindowEvent::Touch(touch) => touch,
            other => panic!("expected a Touch event, got {other:?}"),
        }
    }
}
