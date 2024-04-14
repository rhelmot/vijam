use vizia::prelude::*;
use std::collections::HashSet;
use crate::config::JamConfigLua;

pub use vizia::prelude::Code as KeyCode;
pub use vizia::prelude::Modifiers as KeyModifiers;

#[derive(Lens)]
pub struct VizData {
    pressed: HashSet<KeyCode>,
    config: JamConfigLua,
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
                self.config.on_keyup(code).expect("lua error!");
            }
            _ => {}
        });
    }
}

impl VizData {
    fn new(config: JamConfigLua) -> Self {
        Self {
            pressed: HashSet::new(),
            config,
        }
    }
}

pub fn setup_input(config: JamConfigLua) -> Application {
    Application::new(|cx| {
        VizData::new(config).build(cx);
        HStack::new(cx, |_| {})
            .size(Pixels(50.0))
            .lock_focus_to_within();
    })
}
