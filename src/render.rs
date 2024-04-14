use std::collections::BTreeMap;
use std::sync::{mpsc, Arc, Mutex};
use thread_priority::{ThreadBuilderExt, ThreadPriority};

use crate::instrument::{Instrument, InstrumentEvent, Note, NoteEvent};
use crate::JamEvent;

pub type FrameInstant = u64;

const MAX_BUFFER_SPECULATE_SIZE: usize = 1024;

pub struct RenderQueue {
    pub buffer: dasp::ring_buffer::Bounded<Box<[f32]>>,
    pub tail_frame: u64,
    pub last_consumed_size: u64,
}

impl RenderQueue {
    pub fn new() -> Self {
        RenderQueue {
            buffer: dasp::ring_buffer::Bounded::from_raw_parts(
                0,
                0,
                Box::from([0f32; MAX_BUFFER_SPECULATE_SIZE]),
            ),
            last_consumed_size: 0,
            tail_frame: 0,
        }
    }

    fn plus_sample_time(&self, samples_elapsed: u64) -> FrameInstant {
        self.tail_frame + samples_elapsed
    }

    /// The current timestamp at the head of the buffer, i.e. the insertion point
    pub fn head_time(&self) -> FrameInstant {
        self.plus_sample_time(self.buffer.len() as u64)
    }

    /// The current timestamp at the head of the buffer, i.e. the extraction point
    pub fn tail_time(&self) -> FrameInstant {
        self.plus_sample_time(0)
    }
}

pub fn setup_rendering(
    buf: Arc<Mutex<RenderQueue>>,
    mut instruments: Vec<Box<dyn Instrument>>,
) -> mpsc::Sender<Option<JamEvent>> {
    let (send, recv) = mpsc::channel();

    std::thread::Builder::new()
        .name("rendering".to_string())
        .spawn_with_priority(ThreadPriority::Max, move |result| {
            if let Err(e) = result {
                eprintln!("Warning: Could not set thread priority: {e}")
            }
            let mut voices = BTreeMap::<(u32, u32), (FrameInstant, Box<dyn Note>)>::new();
            loop {
                for event in recv.try_iter() {
                    let Some(event) = event else { return };
                    let now = {
                        let mut buf = buf.lock().unwrap();
                        assert!(buf.buffer.drain().all(|_| true));
                        assert_eq!(buf.buffer.len(), 0);
                        buf.head_time()
                    };
                    match event {
                        JamEvent::InstrumentEvent {
                            instrument: iid,
                            event,
                        } => {
                            let Some(instrument) = instruments.get_mut(iid as usize) else {
                                eprintln!("Warning: event on nonexistent instrument");
                                continue;
                            };
                            match event {
                                InstrumentEvent::SetParam { param } => {
                                    instrument.set_param(param);
                                }
                                InstrumentEvent::NoteEvent { voice, event } => {
                                    match event {
                                        NoteEvent::Hit {} => {
                                            let note = instrument.note(voice);
                                            if let Some((_, mut oldnote)) =
                                                voices.insert((iid, voice), (now, note))
                                            {
                                                // idk if necessary lol
                                                oldnote.mute();
                                            }
                                        }
                                        NoteEvent::SetParam { param } => {
                                            let Some((_, note)) = voices.get_mut(&(iid, voice))
                                            else {
                                                eprintln!("Warning: event on nonexistent note");
                                                continue;
                                            };
                                            note.set_param(param);
                                        }
                                        NoteEvent::Mute {} => {
                                            let Some((_, note)) = voices.get_mut(&(iid, voice))
                                            else {
                                                eprintln!("Warning: event on nonexistent note");
                                                continue;
                                            };
                                            note.mute();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let mut buf = buf.lock().unwrap();
                let now = buf.head_time();
                let retired = buf.tail_time();
                if buf.buffer.len() == buf.buffer.max_len() {
                    continue;
                }

                let mut result = 0f32;
                voices.retain(|(_, _), (ts, note)| {
                    if note.finished(retired - *ts) {
                        return false;
                    }
                    result += note.render(now - *ts);
                    true
                });
                buf.buffer.push(result);
            }
        })
        .unwrap();

    send
}
