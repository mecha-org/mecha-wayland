use crate::object::Object;
use crate::{WlTouch, WlTouchHandler};

pub struct Touch {
    pub inner: WlTouch,
}

impl Touch {
    pub fn new(object_id: u32) -> Self {
        Touch {
            inner: WlTouch::new(object_id),
        }
    }
}

impl Object for Touch {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl WlTouchHandler for Touch {
    fn on_down(&mut self, event: crate::WlTouchDownEvent) {
        tracing::debug!(
            "Touch down: id={}, x={}, y={}, time={}",
            event.id,
            event.x as f64 / 256.0,
            event.y as f64 / 256.0,
            event.time
        );
    }

    fn on_up(&mut self, event: crate::WlTouchUpEvent) {
        tracing::debug!("Touch up: id={}, time={}", event.id, event.time);
    }

    fn on_motion(&mut self, event: crate::WlTouchMotionEvent) {
        tracing::trace!(
            "Touch motion: id={}, x={}, y={}, time={}",
            event.id,
            event.x as f64 / 256.0,
            event.y as f64 / 256.0,
            event.time
        );
    }

    fn on_frame(&mut self) {
        tracing::debug!("Touch frame");
    }

    fn on_cancel(&mut self) {
        tracing::warn!("Touch sequence cancelled");
    }
}
