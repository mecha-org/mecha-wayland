use interactivity::{DragState, PointerEvent, TouchEvent};
use wayland::ext_session_lock_surface::ExtSessionLockSurfaceV1Event;
use wayland::zwlr_layer_shell::LayerSurfaceEvent;
use wayland::{WlBufferEvent, WlCallbackEvent};

use crate::{AppState, COLOR_LAYER_BG, COLOR_LOCK_BG, LockMode, UNLOCK_THRESHOLD, render};

pub fn on_layer_surface_configured(s: &mut AppState, ev: &LayerSurfaceEvent) {
    let LayerSurfaceEvent::Configured {
        id,
        serial,
        width,
        height,
    } = ev
    else {
        return;
    };

    let Some(ui) = s.layer_ui.as_mut() else {
        return;
    };

    ui.surface.alloc_buffers(
        &mut s.renderer,
        &mut s.wayland,
        *width as i32,
        *height as i32,
    );
    ui.recompute_layout();

    s.wayland.layer_surface.ack_configure(*id, *serial);

    let cmds = ui.render_commands();
    ui.surface
        .present(&mut s.renderer, &mut s.wayland, &mut s.callback_map, |r| {
            render::render_frame(r, cmds, COLOR_LAYER_BG);
        });
    ui.surface.frame_callback_pending = true;
}

pub fn on_lock_surface_configured(s: &mut AppState, ev: &ExtSessionLockSurfaceV1Event) {
    let ExtSessionLockSurfaceV1Event::Configure {
        id,
        serial,
        width,
        height,
    } = ev;

    let wl_surface_id = s
        .lock_uis
        .iter()
        .find(|(_, ui)| ui.lock_surface_id == *id)
        .map(|(&wl_id, _)| wl_id);
    let Some(wl_surface_id) = wl_surface_id else {
        return;
    };

    let ui = s.lock_uis.get_mut(&wl_surface_id).unwrap();
    let (w, h) = (*width as i32, *height as i32);

    if ui.surface.dmabuf[0].is_none() {
        ui.surface
            .alloc_buffers(&mut s.renderer, &mut s.wayland, w, h);
        ui.recompute_layout();
    }

    s.wayland.session_lock_surface.ack_configure(*id, *serial);

    // Collect hit areas eagerly from the render commands — this must happen
    // before present() so the registry is valid even if the frame is deferred.
    let cmds = ui.render_commands();
    render::collect_hit_areas(&mut s.hit_areas, &cmds);

    ui.surface
        .present(&mut s.renderer, &mut s.wayland, &mut s.callback_map, |r| {
            render::render_frame(r, cmds, COLOR_LOCK_BG);
        });
    ui.surface.frame_callback_pending = true;
}

pub fn on_frame_done(s: &mut AppState, ev: &WlCallbackEvent) {
    let WlCallbackEvent::Done { id, .. } = ev;
    let Some(wl_surface_id) = s.callback_map.remove(id) else {
        return;
    };

    if let Some(ui) = s.layer_ui.as_mut() {
        if ui.surface.wl_surface_id == wl_surface_id {
            let cmds = ui.render_commands();
            ui.surface
                .on_frame_done(&mut s.renderer, &mut s.wayland, &mut s.callback_map, |r| {
                    render::render_frame(r, cmds, COLOR_LAYER_BG)
                });
            return;
        }
    }

    if let Some(ui) = s.lock_uis.get_mut(&wl_surface_id) {
        let cmds = ui.render_commands();
        render::collect_hit_areas(&mut s.hit_areas, &cmds);
        ui.surface
            .on_frame_done(&mut s.renderer, &mut s.wayland, &mut s.callback_map, |r| {
                render::render_frame(r, cmds, COLOR_LOCK_BG)
            });
    }
}

pub fn on_buffer_release(s: &mut AppState, ev: &WlBufferEvent) {
    let WlBufferEvent::Release { id } = ev;
    let buf_id = *id;

    if let Some(ui) = s.layer_ui.as_mut() {
        if ui.surface.on_buffer_release(buf_id) {
            return;
        }
    }

    let released_wl_id = s.lock_uis.iter_mut().find_map(|(&wl_id, ui)| {
        if ui.surface.on_buffer_release(buf_id) {
            Some(wl_id)
        } else {
            None
        }
    });

    if let Some(wl_id) = released_wl_id {
        if s.lock_uis[&wl_id].surface.dirty {
            s.redraw_lock_ui(wl_id);
        }
    }
}

pub fn on_touch(s: &mut AppState, ev: &TouchEvent) {
    let TouchEvent::Drag {
        state,
        x,
        y,
        total_dy,
        ..
    } = ev
    else {
        return;
    };

    match state {
        DragState::Start => {
            s.trigger_lock();

            if s.mode == LockMode::Locked {
                let circle_id = s.lock_uis.values().next().map(|ui| ui.circle_node_id());
                s.touch_on_circle = circle_id
                    .and_then(|cid| s.hit_areas.hit_test(*x, *y).filter(|&h| h == cid))
                    .is_some();
            }
        }

        DragState::Move => {
            if !s.touch_on_circle || s.mode != LockMode::Locked {
                return;
            }

            let offset = (*total_dy as f32).min(0.0).max(-UNLOCK_THRESHOLD);

            for ui in s.lock_uis.values_mut() {
                ui.set_drag(offset, true);
            }

            if offset <= -UNLOCK_THRESHOLD {
                s.touch_on_circle = false;
                for ui in s.lock_uis.values_mut() {
                    ui.reset_drag();
                }
                s.trigger_unlock();
                return;
            }

            s.redraw_all_lock_surfaces();
        }

        DragState::End | DragState::Cancel => {
            if !s.touch_on_circle {
                return;
            }
            s.touch_on_circle = false;

            if s.mode == LockMode::Locked {
                for ui in s.lock_uis.values_mut() {
                    ui.reset_drag();
                }
                s.redraw_all_lock_surfaces();
            }
        }
    }
}

pub fn on_pointer(s: &mut AppState, ev: &PointerEvent) {
    match ev {
        PointerEvent::ButtonPress {
            button: 272, x, y, ..
        } => {
            s.trigger_lock();

            if s.mode == LockMode::Locked {
                let circle_id = s.lock_uis.values().next().map(|ui| ui.circle_node_id());
                let hit = s.hit_areas.hit_test(*x, *y);

                let on_circle = circle_id
                    .and_then(|cid| hit.filter(|&h| h == cid))
                    .is_some();

                if on_circle {
                    s.pointer_drag_start_y = Some(*y);
                }
            }
        }

        PointerEvent::Move { y, .. } => {
            if s.mode != LockMode::Locked {
                return;
            }

            let Some(start_y) = s.pointer_drag_start_y else {
                return;
            };

            let offset = ((*y - start_y) as f32).min(0.0).max(-UNLOCK_THRESHOLD);
            let reached = offset <= -UNLOCK_THRESHOLD;

            for ui in s.lock_uis.values_mut() {
                ui.set_drag(offset, true);
            }

            if reached {
                s.pointer_drag_start_y = None;
                for ui in s.lock_uis.values_mut() {
                    ui.reset_drag();
                }
                s.trigger_unlock();
            } else {
                s.redraw_all_lock_surfaces();
            }
        }

        PointerEvent::ButtonRelease { button: 272, .. } => {
            s.pointer_drag_start_y = None;
            if s.mode == LockMode::Locked {
                for ui in s.lock_uis.values_mut() {
                    ui.reset_drag();
                }
                s.redraw_all_lock_surfaces();
            }
        }

        _ => {}
    }
}
