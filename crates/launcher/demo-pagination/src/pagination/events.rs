use super::state::PagerState;
use interactivity::{DragState, KeyEvent, PointerEvent, TouchEvent};

pub const KEY_RIGHT: u32 = 106;
pub const KEY_LEFT: u32 = 105;
pub const BTN_LEFT: u32 = 272;

/// Process high-level events (touch, pointer, keyboard) to update PagerState.
/// Returns `true` if state was mutated and a redraw is needed.
pub fn process_pager_events(
    state: &mut PagerState,
    pointer_ev: Option<&PointerEvent>,
    touch_ev: Option<&TouchEvent>,
    key_ev: Option<&KeyEvent>,
    page_size: f32, // Width of Pager
    pager_hit_id: u64,
    active_hit_id: Option<u64>,
) -> bool {
    let mut redraw_needed = false;

    if let Some(KeyEvent::Press { key, .. }) = key_ev {
        if *key == KEY_RIGHT {
            state.next_page();
            redraw_needed = true;
        } else if *key == KEY_LEFT {
            state.previous_page();
            redraw_needed = true;
        }
    }

    if let Some(touch) = touch_ev {
        match touch {
            TouchEvent::Drag {
                state: drag_state,
                x,
                ..
            } => match drag_state {
                DragState::Start => {
                    if active_hit_id == Some(pager_hit_id) {
                        state.handle_drag_start(*x);
                        redraw_needed = true;
                    }
                }
                DragState::Move => {
                    if state.is_dragging {
                        state.handle_drag_move(*x);
                        redraw_needed = true;
                    }
                }
                DragState::End | DragState::Cancel => {
                    if state.is_dragging {
                        state.handle_drag_end(page_size);
                        redraw_needed = true;
                    }
                }
            },
            TouchEvent::Swipe { direction, .. } => match direction {
                interactivity::SwipeDirection::Left => {
                    state.next_page();
                    redraw_needed = true;
                }
                interactivity::SwipeDirection::Right => {
                    state.previous_page();
                    redraw_needed = true;
                }
                _ => {}
            },
            _ => {}
        }
    }

    if let Some(pointer) = pointer_ev {
        match pointer {
            PointerEvent::ButtonPress { button, x, .. }
                if *button == BTN_LEFT && active_hit_id == Some(pager_hit_id) =>
            {
                state.handle_drag_start(*x);
                redraw_needed = true;
            }
            PointerEvent::Move { x, .. } if state.is_dragging => {
                state.handle_drag_move(*x);
                redraw_needed = true;
            }
            PointerEvent::ButtonRelease { button, .. }
                if *button == BTN_LEFT && state.is_dragging =>
            {
                state.handle_drag_end(page_size);
                redraw_needed = true;
            }
            _ => {}
        }
    }

    // Tick Animation
    if state.animation_offset.is_animating() {
        redraw_needed = true;
    }

    redraw_needed
}
