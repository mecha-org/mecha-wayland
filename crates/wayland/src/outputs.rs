//! Every `wl_output` the compositor advertises, bound and followed, once
//! [`OutputsModule`] is installed.
//!
//! - Each `wl_output` global is bound at install, and as it appears later.
//!   [`OutputAdded`] follows its first `done`. (An output below version 2
//!   has no `done`, so it is never announced.)
//! - A removed global is released (version 3 and above) and forgotten;
//!   [`OutputRemoved`] follows if [`OutputAdded`] went out for it.

use app::prelude::*;

use crate::{Globals, Interface, Wayland, WlOutput, WlOutputEvent, WlRegistryEvent, bind_version};

struct Entry {
    output: WlOutput,
    global: u32,
    /// [`OutputAdded`] went out.
    announced: bool,
}

/// The outputs, in the order they were bound.
pub struct Outputs {
    entries: Vec<Entry>,
}
impl Resource for Outputs {}

impl Outputs {
    /// Every output announced with [`OutputAdded`] and not yet removed.
    pub fn iter(&self) -> impl Iterator<Item = WlOutput> + '_ {
        self.entries
            .iter()
            .filter(|e| e.announced)
            .map(|e| e.output)
    }
}

/// An output is ready: bound, and its first state settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputAdded(pub WlOutput);
impl Signal for OutputAdded {}

/// An announced output's global went away. The object is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputRemoved(pub WlOutput);
impl Signal for OutputRemoved {}

/// Inserts [`Outputs`] and binds every `wl_output` already advertised.
/// Installs after [`WaylandModule`](crate::WaylandModule).
pub struct OutputsModule;

impl Module for OutputsModule {
    fn install(self, app: &mut App) {
        if app.has_resource::<Outputs>() {
            return;
        }
        let advertised: Vec<(u32, u32)> = app
            .resource::<Globals>()
            .iter()
            .filter(|g| g.interface == WlOutput::NAME)
            .map(|g| (g.name, g.version))
            .collect();
        app.insert_resource(Outputs {
            entries: Vec::new(),
        });
        for (name, version) in advertised {
            bind(app, name, version);
        }
        app.system(on_registry).system(on_output);
    }
}

fn bind(app: &mut App, name: u32, version: u32) {
    let output = {
        let (globals, mut wl) = app.query::<(Res<Globals>, ResMut<Wayland>)>();
        let version = bind_version(WlOutput::NAME, version, WlOutput::VERSION);
        globals.registry().bind::<WlOutput>(&mut wl, name, version)
    };
    app.resource_mut::<Outputs>().entries.push(Entry {
        output,
        global: name,
        announced: false,
    });
}

fn on_registry(app: &mut App, e: &WlRegistryEvent) {
    match e {
        WlRegistryEvent::Global {
            name,
            interface,
            version,
            ..
        } if interface == WlOutput::NAME => bind(app, *name, *version),
        WlRegistryEvent::GlobalRemove { name, .. } => {
            let removed = {
                let mut outputs = app.resource_mut::<Outputs>();
                let Some(i) = outputs.entries.iter().position(|e| e.global == *name) else {
                    return;
                };
                outputs.entries.remove(i)
            };
            if app.resource::<Wayland>().version(removed.output) >= 3 {
                removed.output.release(&mut app.resource_mut::<Wayland>());
            }
            if removed.announced {
                app.signal(OutputRemoved(removed.output));
            }
        }
        _ => {}
    }
}

fn on_output(app: &mut App, e: &WlOutputEvent) {
    let WlOutputEvent::Done { output } = e else {
        return;
    };
    let first = {
        let mut outputs = app.resource_mut::<Outputs>();
        let Some(entry) = outputs.entries.iter_mut().find(|en| en.output == *output) else {
            return;
        };
        !std::mem::replace(&mut entry.announced, true)
    };
    if first {
        app.signal(OutputAdded(*output));
    }
}
