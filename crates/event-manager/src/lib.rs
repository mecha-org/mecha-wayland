use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};

// ── ScheduleLabel ─────────────────────────────────────────────────────────────

pub trait ScheduleLabel: 'static {}

// ── Schedule ──────────────────────────────────────────────────────────────────

struct LabelSlot {
    type_id: TypeId,
    queue: VecDeque<QueuedEvent>,
}

pub struct Schedule {
    labels: Vec<LabelSlot>,
}

impl Schedule {
    pub fn new() -> Self {
        Self { labels: Vec::new() }
    }

    pub fn add_label<L: ScheduleLabel>(mut self, _label: L) -> Self {
        self.labels.push(LabelSlot {
            type_id: TypeId::of::<L>(),
            queue: VecDeque::new(),
        });
        self
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

// ── Event ────────────────────────────────────────────────────────────────────

pub trait Event: Clone + 'static {}

// ── HasSchedule ───────────────────────────────────────────────────────────────

pub trait HasSchedule: Event {
    type Label: ScheduleLabel;
}

// ── EventHandler ─────────────────────────────────────────────────────────────

pub trait EventHandler<E: Event> {
    fn handle(&mut self, event: E, ctx: &mut EventManagerContext);
}

// ── EventManagerContext ──────────────────────────────────────────────────────

pub struct EventManagerContext {
    schedule: Rc<RefCell<Schedule>>,
}

impl EventManagerContext {
    pub fn send<E: Event + HasSchedule>(&self, event: E) {
        self.enqueue_on(TypeId::of::<E::Label>(), event);
    }

    pub fn send_on<E: Event, L: ScheduleLabel>(&self, _label: L, event: E) {
        self.enqueue_on(TypeId::of::<L>(), event);
    }

    fn enqueue_on<E: Event>(&self, label_type_id: TypeId, event: E) {
        let mut schedule = self.schedule.borrow_mut();
        let slot = schedule
            .labels
            .iter_mut()
            .find(|s| s.type_id == label_type_id)
            .unwrap_or_else(|| panic!("send: label not registered in schedule"));
        slot.queue.push_back(QueuedEvent::new(event));
    }
}

// ── QueuedEvent ──────────────────────────────────────────────────────────────

type Registry = HashMap<TypeId, Box<dyn ErasedHandlers>>;

struct QueuedEvent(Box<dyn FnOnce(&mut Registry, &mut EventManagerContext)>);

impl QueuedEvent {
    fn new<E: Event>(event: E) -> Self {
        QueuedEvent(Box::new(move |registry, ctx| {
            let Some(handlers) = registry.get_mut(&TypeId::of::<E>()) else {
                return;
            };
            handlers
                .as_any_mut()
                .downcast_mut::<EventHandlers<E>>()
                .unwrap()
                .dispatch(event, ctx);
        }))
    }
}

// ── ErasedHandlers ───────────────────────────────────────────────────────────

trait ErasedHandlers {
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ── EventHandlers<E> ─────────────────────────────────────────────────────────

struct EventHandlers<E: Event> {
    handlers: Vec<Box<dyn EventHandler<E>>>,
}

impl<E: Event> EventHandlers<E> {
    fn new() -> Self {
        Self { handlers: Vec::new() }
    }

    fn dispatch(&mut self, event: E, ctx: &mut EventManagerContext) {
        for handler in &mut self.handlers {
            handler.handle(event.clone(), ctx);
        }
    }
}

impl<E: Event> ErasedHandlers for EventHandlers<E> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── RegisteredComponent<T> ───────────────────────────────────────────────────

pub struct RegisteredComponent<T> {
    inner: Rc<RefCell<T>>,
    registry: Weak<RefCell<Registry>>,
}

impl<T> Clone for RegisteredComponent<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            registry: Weak::clone(&self.registry),
        }
    }
}

impl<T: EventHandler<E>, E: Event> EventHandler<E> for RegisteredComponent<T> {
    fn handle(&mut self, event: E, ctx: &mut EventManagerContext) {
        self.inner.borrow_mut().handle(event, ctx);
    }
}

impl<T: 'static> RegisteredComponent<T> {
    pub fn subscribe<E: Event>(&self)
    where
        T: EventHandler<E>,
    {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let mut registry = registry.borrow_mut();
        let handlers = registry
            .entry(TypeId::of::<E>())
            .or_insert_with(|| Box::new(EventHandlers::<E>::new()));
        handlers
            .as_any_mut()
            .downcast_mut::<EventHandlers<E>>()
            .unwrap()
            .handlers
            .push(Box::new(self.clone()));
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, T> {
        self.inner.borrow_mut()
    }
}

// ── EventManager ─────────────────────────────────────────────────────────────

pub struct EventManager {
    registry: Rc<RefCell<Registry>>,
    schedule: Rc<RefCell<Schedule>>,
}

impl EventManager {
    pub fn new(schedule: Schedule) -> Self {
        Self {
            registry: Rc::new(RefCell::new(HashMap::new())),
            schedule: Rc::new(RefCell::new(schedule)),
        }
    }

    pub fn register<T: 'static>(&mut self, component: T) -> RegisteredComponent<T> {
        RegisteredComponent {
            inner: Rc::new(RefCell::new(component)),
            registry: Rc::downgrade(&self.registry),
        }
    }

    pub fn context(&self) -> EventManagerContext {
        EventManagerContext {
            schedule: Rc::clone(&self.schedule),
        }
    }

    pub fn tick(&mut self) {
        let label_count = self.schedule.borrow().labels.len();
        for i in 0..label_count {
            loop {
                let event = self.schedule.borrow_mut().labels[i].queue.pop_front();
                let Some(QueuedEvent(dispatch)) = event else { break };
                let mut ctx = self.context();
                dispatch(&mut self.registry.borrow_mut(), &mut ctx);
            }
        }
    }
}
