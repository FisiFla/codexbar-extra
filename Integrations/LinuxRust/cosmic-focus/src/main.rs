// SPDX-License-Identifier: GPL-3.0-only
//! `codexbar-cosmic-focus list | activate <identifier>`: list or activate COSMIC toplevels.

use cosmic_protocols::{
    toplevel_info::v1::client::{zcosmic_toplevel_handle_v1, zcosmic_toplevel_info_v1},
    toplevel_management::v1::client::zcosmic_toplevel_manager_v1,
};
use wayland_client::{
    event_created_child,
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};

struct Toplevel {
    handle: ExtForeignToplevelHandleV1,
    identifier: Option<String>,
    app_id: String,
    title: String,
}

#[derive(Default)]
struct App {
    list: Option<ExtForeignToplevelListV1>,
    info: Option<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1>,
    manager: Option<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1>,
    seat: Option<wl_seat::WlSeat>,
    toplevels: Vec<Toplevel>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        app: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "ext_foreign_toplevel_list_v1" => {
                app.list = Some(registry.bind(name, 1, qh, ()));
            }
            // get_cosmic_toplevel is a v2 request.
            "zcosmic_toplevel_info_v1" if version >= 2 => {
                app.info = Some(registry.bind(name, version.min(3), qh, ()));
            }
            "zcosmic_toplevel_manager_v1" => {
                app.manager = Some(registry.bind(name, version.min(4), qh, ()));
            }
            "wl_seat" if app.seat.is_none() => {
                app.seat = Some(registry.bind(name, 1, qh, ()));
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            app.toplevels.push(Toplevel {
                handle: toplevel,
                identifier: None,
                app_id: String::new(),
                title: String::new(),
            });
        }
    }

    event_created_child!(App, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for App {
    fn event(
        app: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;
        if let Event::Closed = event {
            app.toplevels.retain(|t| &t.handle != handle);
            return;
        }
        let Some(t) = app.toplevels.iter_mut().find(|t| &t.handle == handle) else {
            return;
        };
        match event {
            Event::Title { title } => t.title = title,
            Event::AppId { app_id } => t.app_id = app_id,
            Event::Identifier { identifier } => t.identifier = Some(identifier),
            _ => {}
        }
    }
}

impl Dispatch<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        _: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    // The deprecated v1 `toplevel` event is never sent to v2+ clients.
    event_created_child!(App, zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, ()),
    ]);
}

impl Dispatch<zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
        _: zcosmic_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1,
        _: zcosmic_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn connect() -> Result<(Connection, EventQueue<App>, App), String> {
    let conn =
        Connection::connect_to_env().map_err(|e| format!("cannot connect to Wayland: {e}"))?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let mut app = App::default();
    conn.display().get_registry(&qh, ());
    // Globals, then the toplevel list, then each handle's properties.
    for _ in 0..3 {
        queue
            .roundtrip(&mut app)
            .map_err(|e| format!("Wayland roundtrip failed: {e}"))?;
    }
    if app.list.is_none() {
        return Err("compositor does not advertise ext_foreign_toplevel_list_v1".into());
    }
    Ok((conn, queue, app))
}

fn list() -> Result<(), String> {
    let (_conn, _queue, app) = connect()?;
    let rows: Vec<serde_json::Value> = app
        .toplevels
        .iter()
        .filter_map(|t| {
            let id = t.identifier.as_ref()?;
            Some(serde_json::json!({"id": id, "app_id": t.app_id, "title": t.title}))
        })
        .collect();
    println!("{}", serde_json::Value::Array(rows));
    Ok(())
}

fn activate(identifier: &str) -> Result<(), String> {
    let (_conn, mut queue, mut app) = connect()?;
    let qh = queue.handle();
    let info = app
        .info
        .clone()
        .ok_or("compositor does not advertise zcosmic_toplevel_info_v1 v2+")?;
    let manager = app
        .manager
        .clone()
        .ok_or("compositor does not advertise zcosmic_toplevel_manager_v1")?;
    let seat = app.seat.clone().ok_or("no wl_seat available")?;
    let target = app
        .toplevels
        .iter()
        .find(|t| t.identifier.as_deref() == Some(identifier))
        .ok_or_else(|| format!("no toplevel with identifier {identifier}"))?;
    let cosmic = info.get_cosmic_toplevel(&target.handle, &qh, ());
    // Let the compositor learn the cosmic handle before the request that uses it.
    queue
        .roundtrip(&mut app)
        .map_err(|e| format!("Wayland roundtrip failed: {e}"))?;
    manager.activate(&cosmic, &seat);
    queue
        .roundtrip(&mut app)
        .map_err(|e| format!("Wayland roundtrip failed: {e}"))?;
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["list"] => list(),
        ["activate", id] => activate(id),
        _ => Err("usage: codexbar-cosmic-focus list | activate <identifier>".to_string()),
    };
    if let Err(e) = result {
        eprintln!("codexbar-cosmic-focus: {e}");
        std::process::exit(2);
    }
}
