use vizia::prelude::*;
use std::sync::mpsc;
use std::collections::HashSet;
use crate::JamEvent;
use crate::instrument::{InstrumentEvent, InstrumentParam, NoteEvent, NoteParam};

#[derive(Lens)]
pub struct VizData {
    pressed: HashSet<Code>,
    event_submission: mpsc::Sender<Option<JamEvent>>,
}

fn code_to_pitch_and_voice(code: Code) -> Option<(f32, u32)> {
    let note = match code {
        Code::KeyA => 0,
        Code::KeyS => 2,
        Code::KeyD => 4,
        Code::KeyF => 5,
        Code::KeyJ => 7,
        Code::KeyK => 9,
        Code::KeyL => 11,
        Code::Semicolon => 12,
        _ => return None,
    };
    Some((440.0 * f32::powf(2.0, note as f32 / 12.0), note))
}

impl Model for VizData {
    fn event(&mut self, _cx: &mut EventContext, event: &mut Event) {
        event.map(|window_event, _| match window_event {
            WindowEvent::KeyDown(code, _) => {
                let code = *code;
                if !self.pressed.insert(code) {
                    return;
                }
                let Some((pitch, voice)) = code_to_pitch_and_voice(code) else {
                    return;
                };
                self.event_submission
                    .send(Some(JamEvent::InstrumentEvent {
                        instrument: 0,
                        event: InstrumentEvent::SetParam {
                            param: InstrumentParam::NextNote(NoteParam::Pitch(pitch)),
                        },
                    }))
                    .unwrap();
                self.event_submission
                    .send(Some(JamEvent::InstrumentEvent {
                        instrument: 0,
                        event: InstrumentEvent::NoteEvent {
                            voice,
                            event: NoteEvent::Hit {},
                        },
                    }))
                    .unwrap();
            }
            WindowEvent::KeyUp(code, _) => {
                let code = *code;
                if !self.pressed.remove(&code) {
                    return;
                }
                let Some((_, voice)) = code_to_pitch_and_voice(code) else {
                    return;
                };
                self.event_submission
                    .send(Some(JamEvent::InstrumentEvent {
                        instrument: 0,
                        event: InstrumentEvent::NoteEvent {
                            voice,
                            event: NoteEvent::Mute {},
                        },
                    }))
                    .unwrap();
            }
            _ => {}
        });
    }
}

impl VizData {
    fn new(event_submission: mpsc::Sender<Option<JamEvent>>) -> Self {
        Self {
            pressed: HashSet::new(),
            event_submission,
        }
    }
}

pub fn setup_input(event_submission: mpsc::Sender<Option<JamEvent>>) -> Application {
    Application::new(|cx| {
        VizData::new(event_submission).build(cx);
        HStack::new(cx, |_| {})
            .size(Pixels(50.0))
            .lock_focus_to_within();
    })
}
