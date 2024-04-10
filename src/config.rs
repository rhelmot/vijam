use mlua::prelude::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::read;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use crate::input::{KeyCode, KeyModifiers};
use crate::instrument::Instrument;

pub struct JamConfig {
    state_machine: Vec<Option<JamState>>,
    current_state: u32,
    timers: BTreeMap<Instant, Box<dyn FnMut()>>,
    beats: BTreeMap<u32, Box<dyn FnMut()>>,
}

pub struct JamConfigLua {
    inner: JamConfigRc,
    lua: Lua,
}

pub struct JamStateKeyAction {
    effect: KeyCallback,
    effect_up: Option<KeyCallback>,
    state: u32,
}

pub struct KeyCallback(Box<dyn Fn(&mut JamConfig, &Lua, KeyChord) -> LuaResult<()>>);

pub struct JamState {
    name: String,
    keys: HashMap<KeyChord, JamStateKeyAction>,
    default: JamStateKeyAction,
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct KeyChord(KeyCode, KeyModifiers);

const ORDERED_MODIFIERS: [KeyModifiers; 16] = [
    KeyModifiers::CTRL
        .union(KeyModifiers::SHIFT)
        .union(KeyModifiers::ALT)
        .union(KeyModifiers::SUPER),
    KeyModifiers::SHIFT
        .union(KeyModifiers::ALT)
        .union(KeyModifiers::SUPER),
    KeyModifiers::CTRL
        .union(KeyModifiers::ALT)
        .union(KeyModifiers::SUPER),
    KeyModifiers::CTRL
        .union(KeyModifiers::SHIFT)
        .union(KeyModifiers::SUPER),
    KeyModifiers::CTRL
        .union(KeyModifiers::SHIFT)
        .union(KeyModifiers::ALT),
    KeyModifiers::ALT.union(KeyModifiers::SUPER),
    KeyModifiers::SHIFT.union(KeyModifiers::SUPER),
    KeyModifiers::SHIFT.union(KeyModifiers::ALT),
    KeyModifiers::CTRL.union(KeyModifiers::SUPER),
    KeyModifiers::CTRL.union(KeyModifiers::ALT),
    KeyModifiers::CTRL.union(KeyModifiers::SHIFT),
    KeyModifiers::SUPER,
    KeyModifiers::ALT,
    KeyModifiers::SHIFT,
    KeyModifiers::CTRL,
    KeyModifiers::empty(),
];

type JamConfigRc = Rc<RefCell<JamConfig>>;

impl<'lua> IntoLua<'lua> for KeyCallback {
    fn into_lua(self, lua: &'lua Lua) -> LuaResult<LuaValue<'lua>> {
        LuaAnyUserData::wrap(self).into_lua(lua)
    }
}

impl<'lua> FromLua<'lua> for KeyCallback {
    fn from_lua(value: LuaValue<'lua>, lua: &'lua Lua) -> LuaResult<Self> {
        if let Some(func) = value.as_function() {
            // this is a leak. but does it matter???????
            let globals = lua.globals().raw_get::<_, LuaTable>("native").unwrap();
            let acct_count = globals.raw_get::<_, usize>("__acct_count").unwrap();
            let acct = globals.raw_get::<_, LuaTable>("__acct").unwrap();
            acct.raw_set(acct_count, func).unwrap();
            globals.raw_set("__acct_count", acct_count + 1).unwrap();
            return Ok(KeyCallback(Box::new(move |_, lua, key| {
                let globals = lua.globals().raw_get::<_, LuaTable>("native").unwrap();
                let acct = globals.raw_get::<_, LuaTable>("__acct").unwrap();
                let callback = acct.raw_get::<_, LuaFunction>(acct_count).unwrap();
                callback.call((key,))
            })))
        }
        let value: LuaAnyUserData<'lua> = LuaAnyUserData::from_lua(value, lua)?;
        value.take()
    }
}

impl<'lua> FromLua<'lua> for KeyChord {
    fn from_lua(value: LuaValue<'lua>, _lua: &'lua Lua) -> LuaResult<Self> {
        Ok(parse_keyspec(
            value
                .as_str()
                .ok_or_else(|| LuaError::FromLuaConversionError {
                    from: value.type_name(),
                    to: "KeyChord",
                    message: Some("Must be String".to_owned()),
                })?,
        )
        .map_err(|e| LuaError::ExternalError(Arc::new(e)))?)
    }
}

impl<'lua> IntoLua<'lua> for KeyChord {
    fn into_lua(self, lua: &'lua Lua) -> LuaResult<LuaValue<'lua>> {
        fmt_keyspec(self).into_lua(lua)
    }
}

fn make_native_func<
    'a,
    A: FromLuaMulti<'a>,
    R: IntoLuaMulti<'a>,
    F: Fn(&mut JamConfig, &'a Lua, A) -> LuaResult<R> + 'static,
>(
    lua: &'a Lua,
    name: &'static str,
    func: F,
) {
    let native = lua.globals().get::<_, LuaTable>("native").unwrap();
    native
        .set(
            name,
            lua.create_function(move |lua, a| {
                func(
                    &mut lua.app_data_ref::<JamConfigRc>().unwrap().borrow_mut(),
                    lua,
                    a,
                )
            })
            .unwrap(),
        )
        .unwrap();
}

fn make_native_value<'a>(lua: &'a Lua, name: &'static str, value: impl IntoLua<'a>) {
    let native = lua.globals().get::<_, LuaTable>("native").unwrap();
    native.set(name, value).unwrap();
}

impl JamConfig {
    pub fn new(config: PathBuf) -> anyhow::Result<(JamConfigLua, Vec<Box<dyn Instrument>>)> {
        let lua = Lua::new();
        let instruments = vec![];
        let state_machine = vec![Some(JamState {
            name: "Normal".to_owned(),
            keys: HashMap::new(),
            default: JamStateKeyAction {
                effect: KeyCallback(Box::new(|_, _, _| Ok(()))),
                effect_up: None,
                state: 0,
            },
        })];
        let timers = BTreeMap::new();
        let beats = BTreeMap::new();
        let result = Rc::new(RefCell::new(Self {
            state_machine,
            current_state: 0,
            timers,
            beats,
        }));

        lua.set_app_data(result.clone());
        make_native_func(&lua, "play", Self::native_play);
        make_native_value(&lua, "instruments", {
            let mut r = HashMap::new();
            r.insert("HoldButton", 0);
            r.insert("PressButton", 1);
            r
        });
        make_native_value(&lua, "signals", {
            let mut r = HashMap::new();
            r.insert("Sine", 0);
            r.insert("BrownNoise", 1);
            r
        });
        make_native_func(&lua, "mkInstrument", Self::native_make_instrument);
        make_native_func(&lua, "setTempo", Self::native_set_tempo);
        make_native_func(&lua, "getTempo", Self::native_get_tempo);
        make_native_func(&lua, "onBeat", Self::native_on_beat);
        make_native_func(&lua, "onTimeout", Self::native_on_timeout);
        make_native_func(&lua, "cancelTimer", Self::native_cancel_timer);
        make_native_func(&lua, "mkMode", Self::native_make_mode);
        make_native_func(&lua, "mkPlay", Self::native_make_play);
        make_native_func(&lua, "mkMute", Self::native_make_mute);
        make_native_func(&lua, "bind", Self::native_bind);
        make_native_func(&lua, "bindUp", Self::native_bind_up);
        make_native_func(&lua, "unbind", Self::native_unbind);

        lua.load(read(config)?).exec()?;

        Ok((JamConfigLua { inner: result, lua }, instruments))
    }

    pub fn keymap_action(&mut self, lua: &Lua, key: KeyChord) -> LuaResult<()> {
        let state = self
            .state_machine
            .get_mut(self.current_state as usize)
            .expect("Invalid internal state")
            .take()
            .expect("Reentrant keymap_action????");

        let mut result = None;
        for mask in ORDERED_MODIFIERS {
            if key.1.contains(mask) {
                let chord = KeyChord(key.0, mask);
                if let Some(action) = state.keys.get(&chord) {
                    self.current_state = action.state;
                    result = Some((action.effect.0)(self, lua, chord));
                    break;
                }
            }
        }

        let result = result.unwrap_or_else(|| {
            self.current_state = state.default.state;
            (state.default.effect.0)(self, lua, key)
        });
        self.state_machine.get_mut(self.current_state as usize).unwrap().replace(state);
        result
    }

    fn native_play(
        &mut self,
        lua: &Lua,
        (instrument, pitch, voice, duration): (u32, Option<f32>, Option<u32>, Option<f32>),
    ) -> LuaResult<()> {
        todo!()
    }

    fn native_make_instrument(
        &mut self,
        lua: &Lua,
        (instrument, signal): (u32, u32),
    ) -> LuaResult<u32> {
        todo!()
    }

    fn native_set_tempo(&mut self, lua: &Lua, (tempo,): (f32,)) -> LuaResult<()> {
        todo!()
    }

    fn native_get_tempo(&mut self, lua: &Lua, (): ()) -> LuaResult<f32> {
        todo!()
    }

    fn native_on_beat(
        &mut self,
        lua: &Lua,
        (division, callback): (f32, LuaFunction),
    ) -> LuaResult<u64> {
        todo!()
    }

    fn native_on_timeout(
        &mut self,
        lua: &Lua,
        (time, callback): (f32, LuaFunction),
    ) -> LuaResult<u64> {
        todo!()
    }

    fn native_cancel_timer(&mut self, lua: &Lua, (handle,): (u64,)) -> LuaResult<()> {
        todo!()
    }

    fn native_make_mode(&mut self, lua: &Lua, (name, default_target, default_action): (String, u32, KeyCallback)) -> LuaResult<u32> {
        let result = self.state_machine.len();
        self.state_machine.push(Some(JamState {
            name,
            keys: HashMap::new(),
            default: JamStateKeyAction {
                effect: default_action,
                effect_up: None,
                state: default_target,
            },
        }));
        Ok(result as u32)
    }

    fn native_make_play<'a>(
        &mut self,
        lua: &'a Lua,
        (instrument, pitch, voice, duration): (u32, Option<f32>, Option<u32>, Option<f32>),
    ) -> LuaResult<KeyCallback> {
        Ok(KeyCallback(Box::new(|cfg, lua, key| todo!())))
    }

    fn native_make_mute(
        &mut self,
        lua: &Lua,
        (instrument, voice): (u32, Option<u32>),
    ) -> LuaResult<KeyCallback> {
        Ok(KeyCallback(Box::new(|cfg, lua, key| todo!())))
    }

    fn native_bind<'a>(
        &mut self,
        _lua: &Lua,
        (mode, key, action, next): (u32, KeyChord, KeyCallback, Option<u32>),
    ) -> LuaResult<Option<KeyCallback>> {
        let next = next.unwrap_or(mode);
        let mode = self
            .state_machine
            .get_mut(mode as usize)
            .expect("Bad mode!")
            .as_mut()
            .expect("Cannot bind during callback!");
        Ok(mode
            .keys
            .insert(
                key,
                JamStateKeyAction {
                    effect: action,
                    effect_up: None,
                    state: next,
                },
            )
            .map(|t| t.effect))
    }

    fn native_bind_up<'a>(
        &mut self,
        _lua: &Lua,
        (mode, key, action): (u32, KeyChord, KeyCallback),
    ) -> LuaResult<Option<KeyCallback>> {
        let mode = self
            .state_machine
            .get_mut(mode as usize)
            .expect("Bad mode!")
            .as_mut()
            .expect("Cannot bind during callback!");
        Ok(mode.keys.get_mut(&key).expect("Can't bind_up a key with no binding").effect_up.replace(action))
    }

    fn native_unbind<'a>(
        &mut self,
        _lua: &Lua,
        (mode, key): (u32, KeyChord),
    ) -> LuaResult<Option<KeyCallback>> {
        let mode = self
            .state_machine
            .get_mut(mode as usize)
            .expect("Bad mode!")
            .as_mut()
            .expect("Cannot bind during callback!");
        Ok(mode.keys.remove(&key).map(|t| t.effect))
    }
}

#[derive(Clone, Debug)]
pub enum KeyspecParseError {
    Empty,
    BadKey(String),
    BadModifier(String),
}

impl std::error::Error for KeyspecParseError {}

impl std::fmt::Display for KeyspecParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyspecParseError::Empty => write!(f, "Empty keyspec"),
            KeyspecParseError::BadKey(k) => write!(f, "Bad key: {k}"),
            KeyspecParseError::BadModifier(m) => write!(f, "Bad modifier: {m}"),
        }
    }
}

fn parse_keyspec(text: &str) -> Result<KeyChord, KeyspecParseError> {
    if text.len() == 0 {
        return Err(KeyspecParseError::Empty);
    }
    if text.len() == 1 {
        return parse_keyspec_code(text);
    }
    let mut iter = text.split('-').rev();
    let KeyChord(code, mut mods) = parse_keyspec_code(iter.next().unwrap())?;
    for piece in iter {
        mods |= parse_keyspec_mods(piece)?;
    }
    Ok(KeyChord(code, mods))
}

fn parse_keyspec_code(text: &str) -> Result<KeyChord, KeyspecParseError> {
    use KeyCode::*;
    let (a, b) = match text {
        "<ESC>" => (Escape, KeyModifiers::empty()),
        _ => return Err(KeyspecParseError::BadKey(text.to_owned())),
    };
    Ok(KeyChord(a, b))
}

fn parse_keyspec_mods(text: &str) -> Result<KeyModifiers, KeyspecParseError> {
    Ok(match text {
        "C" => KeyModifiers::CTRL,
        "S" => KeyModifiers::SHIFT,
        "A" => KeyModifiers::ALT,
        "W" => KeyModifiers::SUPER,
        _ => return Err(KeyspecParseError::BadModifier(text.to_owned())),
    })
}

fn fmt_keyspec(keyspec: KeyChord) -> String {
    let mut pieces = vec![];
    if keyspec.1.ctrl() {
        pieces.push("C");
    }
    if keyspec.1.shift() {
        pieces.push("S");
    }
    if keyspec.1.alt() {
        pieces.push("A");
    }
    if keyspec.1.logo() {
        pieces.push("W");
    }
    use KeyCode::*;
    pieces.push(match keyspec.0 {
        Escape => "<ESC>",
        _ => "<UNK>",
    });
    pieces.join("-")
}

impl JamConfigLua {
    pub fn on_keypress(&mut self, key: KeyCode, mods: KeyModifiers) -> LuaResult<()> {
        self.inner.borrow_mut().keymap_action(&self.lua, KeyChord(key, mods))
    }

    pub fn on_keyup(&mut self, key: KeyCode, mods: KeyModifiers) -> LuaResult<()> {
        // TODO
        Ok(())
    }
}
