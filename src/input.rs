use vizia::prelude::*;
use std::sync::mpsc;
use std::collections::HashSet;
use crate::JamEvent;
use crate::config::JamConfigLua;
use crate::instrument::{InstrumentEvent, InstrumentParam, NoteEvent, NoteParam};

pub use vizia::prelude::Code as KeyCode;
pub use vizia::prelude::Modifiers as KeyModifiers;

#[derive(Lens)]
pub struct VizData {
    pressed: HashSet<KeyCode>,
    event_submission: mpsc::Sender<Option<JamEvent>>,
    config: JamConfigLua,
}

fn code_to_pitch_and_voice(code: KeyCode) -> Option<(f32, u32)> {
    let note = match code {
        KeyCode::KeyA => 0,
        KeyCode::KeyS => 2,
        KeyCode::KeyD => 4,
        KeyCode::KeyF => 5,
        KeyCode::KeyJ => 7,
        KeyCode::KeyK => 9,
        KeyCode::KeyL => 11,
        KeyCode::Semicolon => 12,
        _ => return None,
    };
    Some((440.0 * f32::powf(2.0, note as f32 / 12.0), note))
}

impl Model for VizData {
    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|window_event, _| match window_event {
            WindowEvent::KeyDown(code, _) => {
                let code = *code;
                if !self.pressed.insert(code) {
                    return;
                }
                self.config.on_keypress(code, *cx.modifiers()).expect("lua error!");
            }
            WindowEvent::KeyUp(code, _) => {
                let code = *code;
                if !self.pressed.remove(&code) {
                    return;
                }
                self.config.on_keyup(code, *cx.modifiers()).expect("lua error!");
            }
            _ => {}
        });
    }
}

impl VizData {
    fn new(event_submission: mpsc::Sender<Option<JamEvent>>, config: JamConfigLua) -> Self {
        Self {
            pressed: HashSet::new(),
            event_submission,
            config,
        }
    }
}

pub fn setup_input(event_submission: mpsc::Sender<Option<JamEvent>>, config: JamConfigLua) -> Application {
    Application::new(|cx| {
        VizData::new(event_submission, config).build(cx);
        HStack::new(cx, |_| {})
            .size(Pixels(50.0))
            .lock_focus_to_within();
    })
}
