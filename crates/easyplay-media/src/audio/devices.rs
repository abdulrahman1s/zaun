//! The microphones and speakers, asked of PulseAudio's introspection API on a short-lived
//! context (ADR-0035). Blocks for a moment: run it on the job pool.
use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use easyplay_core::model::{AudioDevice, AudioDevices};
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};

/// How long the sound server may take to answer.
const ANSWER_FOR: Duration = Duration::from_secs(3);

/// A device as the server reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Raw {
    pub name: Option<String>,
    pub description: Option<String>,
    /// A source that records an output (`monitor_of_sink`): not a microphone.
    pub monitor: bool,
}

/// The devices worth offering: named, not monitors, once each; the description falls back
/// to the name.
pub(crate) fn offered(raw: Vec<Raw>) -> Vec<AudioDevice> {
    let mut out: Vec<AudioDevice> = Vec::new();
    for r in raw {
        let Some(name) = r.name.filter(|n| !n.is_empty()) else {
            continue;
        };
        if r.monitor || out.iter().any(|d| d.name == name) {
            continue;
        }
        let description = r
            .description
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| name.clone());
        out.push(AudioDevice { name, description });
    }
    out
}

fn err(what: &str) -> io::Error {
    io::Error::other(format!("the sound server {what}"))
}

/// Ask the server (as `PULSE_SERVER` or the session's default names it).
pub(crate) fn list() -> io::Result<AudioDevices> {
    let mut ml = Mainloop::new().ok_or_else(|| err("main loop could not be made"))?;
    let mut ctx = Context::new(&ml, "EasyPlay").ok_or_else(|| err("context could not be made"))?;
    ctx.connect(None, FlagSet::NOAUTOSPAWN, None)
        .map_err(|e| err(&format!("could not be reached: {e}")))?;
    let deadline = crate::clock::now() + ANSWER_FOR;
    let step = |ml: &mut Mainloop| -> io::Result<()> {
        if crate::clock::now() > deadline {
            return Err(err("did not answer in time"));
        }
        match ml.iterate(false) {
            IterateResult::Success(0) => std::thread::sleep(Duration::from_millis(5)),
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) | IterateResult::Err(_) => return Err(err("main loop failed")),
        }
        Ok(())
    };
    loop {
        match ctx.get_state() {
            State::Ready => break,
            State::Failed | State::Terminated => return Err(err("refused the connection")),
            _ => step(&mut ml)?,
        }
    }
    let inputs: Rc<RefCell<Vec<Raw>>> = Rc::default();
    let outputs: Rc<RefCell<Vec<Raw>>> = Rc::default();
    let done = Rc::new(RefCell::new(0u8));
    let intro = ctx.introspect();
    {
        let (list, done) = (Rc::clone(&inputs), Rc::clone(&done));
        intro.get_source_info_list(move |r| match r {
            ListResult::Item(i) => list.borrow_mut().push(Raw {
                name: i.name.as_deref().map(str::to_owned),
                description: i.description.as_deref().map(str::to_owned),
                monitor: i.monitor_of_sink.is_some(),
            }),
            ListResult::End | ListResult::Error => *done.borrow_mut() += 1,
        });
    }
    {
        let (list, done) = (Rc::clone(&outputs), Rc::clone(&done));
        intro.get_sink_info_list(move |r| match r {
            ListResult::Item(i) => list.borrow_mut().push(Raw {
                name: i.name.as_deref().map(str::to_owned),
                description: i.description.as_deref().map(str::to_owned),
                monitor: false,
            }),
            ListResult::End | ListResult::Error => *done.borrow_mut() += 1,
        });
    }
    while *done.borrow() < 2 {
        step(&mut ml)?;
    }
    ctx.disconnect();
    Ok(AudioDevices {
        inputs: offered(inputs.take()),
        outputs: offered(outputs.take()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(name: &str, description: &str, monitor: bool) -> Raw {
        Raw {
            name: Some(name.into()),
            description: Some(description.into()),
            monitor,
        }
    }

    #[test]
    fn monitors_unnamed_and_repeated_devices_are_left_out_and_descriptions_fall_back_to_names() {
        let list = offered(vec![
            raw("alsa_input.usb-headset", "USB headset Mono", false),
            raw("alsa_output.pci.monitor", "Monitor of Built-in Audio", true),
            Raw::default(),
            raw("alsa_input.usb-headset", "again", false),
            raw("bluez_input.ab", "  ", false),
        ]);
        let names: Vec<(&str, &str)> = list
            .iter()
            .map(|d| (d.name.as_str(), d.description.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("alsa_input.usb-headset", "USB headset Mono"),
                ("bluez_input.ab", "bluez_input.ab"),
            ]
        );
    }
}
