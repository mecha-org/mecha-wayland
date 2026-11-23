//! Match rules: what the daemon is asked to route to us, and the same
//! rule checked locally so each signal is tagged with the rules it
//! satisfies.

use std::collections::HashMap;

use smallvec::SmallVec;

use crate::bus::BusId;
use crate::message::{Message, MessageKind};
use crate::names::{BusName, InterfaceName, MemberName, ObjectPath};
use crate::value::PropMap;
use crate::wire::DecodeError;

/// Names one `add_match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MatchId {
    pub bus: BusId,
    pub(crate) n: u64,
}

/// A match rule. Every key set must match; unset keys match anything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchRule {
    kind: Option<MessageKind>,
    sender: Option<BusName>,
    interface: Option<InterfaceName>,
    member: Option<MemberName>,
    path: Option<ObjectPath>,
    path_namespace: Option<ObjectPath>,
    destination: Option<BusName>,
    /// `argN` keys, sorted by `N`: argument `N` must be this string.
    args: Vec<(u8, String)>,
    /// Argument 0 is this bus name or one below it, at a dot.
    arg0_namespace: Option<String>,
}

impl MatchRule {
    /// Signals only: the start of almost every rule.
    pub fn signal() -> Self {
        MatchRule {
            kind: Some(MessageKind::Signal),
            ..MatchRule::default()
        }
    }

    /// A unique name, or a well-known one whose owner is then followed.
    pub fn sender(mut self, sender: impl Into<BusName>) -> Self {
        self.sender = Some(sender.into());
        self
    }

    pub fn interface(mut self, interface: impl Into<InterfaceName>) -> Self {
        self.interface = Some(interface.into());
        self
    }

    pub fn member(mut self, member: impl Into<MemberName>) -> Self {
        self.member = Some(member.into());
        self
    }

    pub fn path(mut self, path: impl Into<ObjectPath>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The path itself or anything below it.
    pub fn path_namespace(mut self, path: impl Into<ObjectPath>) -> Self {
        self.path_namespace = Some(path.into());
        self
    }

    pub fn destination(mut self, destination: impl Into<BusName>) -> Self {
        self.destination = Some(destination.into());
        self
    }

    /// Argument `n` (below 64) must be the string `value`.
    pub fn arg(mut self, n: u8, value: impl Into<String>) -> Self {
        assert!(n < 64, "dbus: match rules have arg0 to arg63");
        self.args.retain(|(m, _)| *m != n);
        self.args.push((n, value.into()));
        self.args.sort_by_key(|(m, _)| *m);
        self
    }

    /// Argument 0 is the bus name `namespace` or one below it, e.g.
    /// `org.mpris.MediaPlayer2` matches `org.mpris.MediaPlayer2.vlc`.
    pub fn arg0_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.arg0_namespace = Some(namespace.into());
        self
    }

    /// `PropertiesChanged` for one interface of one object.
    pub fn properties_changed(
        sender: impl Into<BusName>,
        path: impl Into<ObjectPath>,
        interface: impl Into<InterfaceName>,
    ) -> Self {
        let interface = interface.into();
        MatchRule::signal()
            .sender(sender)
            .path(path)
            .interface("org.freedesktop.DBus.Properties")
            .member("PropertiesChanged")
            .arg(0, interface.as_str())
    }

    /// The rule as the daemon's `AddMatch` takes it.
    pub fn to_rule_string(&self) -> String {
        let mut parts = Vec::new();
        let mut push = |k: &str, v: &str| {
            parts.push(format!("{k}='{}'", v.replace('\'', "'\\''")));
        };
        if let Some(k) = self.kind {
            push(
                "type",
                match k {
                    MessageKind::MethodCall => "method_call",
                    MessageKind::MethodReturn => "method_return",
                    MessageKind::Error => "error",
                    MessageKind::Signal => "signal",
                },
            );
        }
        if let Some(s) = &self.sender {
            push("sender", s.as_str());
        }
        if let Some(i) = &self.interface {
            push("interface", i.as_str());
        }
        if let Some(m) = &self.member {
            push("member", m.as_str());
        }
        if let Some(p) = &self.path {
            push("path", p.as_str());
        }
        if let Some(p) = &self.path_namespace {
            push("path_namespace", p.as_str());
        }
        if let Some(d) = &self.destination {
            push("destination", d.as_str());
        }
        for (n, v) in &self.args {
            push(&format!("arg{n}"), v);
        }
        if let Some(ns) = &self.arg0_namespace {
            push("arg0namespace", ns);
        }
        parts.join(",")
    }

    pub(crate) fn sender_name(&self) -> Option<&BusName> {
        self.sender.as_ref()
    }

    /// Whether `msg` satisfies the rule. `owner` gives a well-known
    /// name's current unique owner; an unresolved name matches nothing.
    pub(crate) fn matches<'o>(
        &self,
        msg: &Message,
        owner: impl Fn(&BusName) -> Option<&'o BusName>,
    ) -> bool {
        if self.kind.is_some_and(|k| k != msg.kind()) {
            return false;
        }
        if let Some(want) = &self.sender {
            let Some(from) = msg.sender() else {
                return false;
            };
            let ok = from == want || (!want.is_unique() && owner(want) == Some(from));
            if !ok {
                return false;
            }
        }
        if self
            .interface
            .as_ref()
            .is_some_and(|i| msg.interface() != Some(i))
        {
            return false;
        }
        if self
            .member
            .as_ref()
            .is_some_and(|m| msg.member() != Some(m))
        {
            return false;
        }
        if self.path.as_ref().is_some_and(|p| msg.path() != Some(p)) {
            return false;
        }
        if let Some(ns) = &self.path_namespace {
            let Some(p) = msg.path() else {
                return false;
            };
            let ns = ns.as_str();
            let inside = ns == "/"
                || p.as_str() == ns
                || p.as_str()
                    .strip_prefix(ns)
                    .is_some_and(|rest| rest.starts_with('/'));
            if !inside {
                return false;
            }
        }
        if self
            .destination
            .as_ref()
            .is_some_and(|d| msg.destination() != Some(d))
        {
            return false;
        }
        if let Some(ns) = &self.arg0_namespace {
            let Some(arg) = msg.string_arg(0) else {
                return false;
            };
            let inside = arg == *ns
                || arg
                    .strip_prefix(ns.as_str())
                    .is_some_and(|rest| rest.starts_with('.'));
            if !inside {
                return false;
            }
        }
        self.args
            .iter()
            .all(|(n, v)| msg.string_arg(*n as usize).as_deref() == Some(v.as_str()))
    }
}

/// `org.freedesktop.DBus.Properties.PropertiesChanged`.
pub struct PropertiesChanged;

impl PropertiesChanged {
    /// The interface, the changed values, and the invalidated names.
    pub fn parse(msg: &Message) -> Result<(InterfaceName, PropMap, Vec<String>), DecodeError> {
        msg.body()
    }
}

/// One rule as registered: the rule and every `MatchId` sharing it.
pub(crate) struct Entry {
    pub(crate) rule: MatchRule,
    pub(crate) ids: Vec<u64>,
}

/// A followed well-known name.
pub(crate) struct Owner {
    pub(crate) refs: usize,
    pub(crate) unique: Option<BusName>,
}

/// Every rule on one connection, keyed by its rule string, and the
/// owners of the well-known names those rules name.
#[derive(Default)]
pub(crate) struct Matches {
    pub(crate) next: u64,
    pub(crate) ids: HashMap<u64, String>,
    pub(crate) rules: HashMap<String, Entry>,
    pub(crate) owners: HashMap<BusName, Owner>,
}

impl Matches {
    /// Every `MatchId` whose rule `msg` satisfies, in the order added.
    pub(crate) fn matching(&self, msg: &Message, bus: BusId) -> SmallVec<[MatchId; 2]> {
        let owner = |n: &BusName| self.owners.get(n).and_then(|o| o.unique.as_ref());
        let mut ids: SmallVec<[MatchId; 2]> = self
            .rules
            .values()
            .filter(|e| e.rule.matches(msg, owner))
            .flat_map(|e| e.ids.iter().map(move |&n| MatchId { bus, n }))
            .collect();
        ids.sort_by_key(|m| m.n);
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(path: &str, iface: &str, member: &str, sender: &str) -> Message {
        Message::signal(
            ObjectPath::new(path).unwrap(),
            InterfaceName::new(iface).unwrap(),
            MemberName::new(member).unwrap(),
        )
        .with_sender(BusName::new(sender).unwrap())
    }

    #[test]
    fn rule_strings_quote_and_escape() {
        let r = MatchRule::signal()
            .sender("org.bluez")
            .interface("org.freedesktop.DBus.ObjectManager")
            .member("InterfacesAdded")
            .path_namespace("/org/bluez")
            .arg(0, "it's");
        assert_eq!(
            r.to_rule_string(),
            "type='signal',sender='org.bluez',interface='org.freedesktop.DBus.ObjectManager',\
             member='InterfacesAdded',path_namespace='/org/bluez',arg0='it'\\''s'"
        );
    }

    #[test]
    fn matching() {
        let none = |_: &BusName| None;
        let m = sig("/org/bluez/hci0", "a.b", "C", ":1.5").with_body(&("x", 3u32));
        assert!(MatchRule::signal().matches(&m, none));
        assert!(
            MatchRule::signal()
                .path_namespace("/org/bluez")
                .matches(&m, none)
        );
        assert!(MatchRule::signal().path_namespace("/").matches(&m, none));
        assert!(
            !MatchRule::signal()
                .path_namespace("/org/blu")
                .matches(&m, none)
        );
        assert!(
            MatchRule::signal()
                .path("/org/bluez/hci0")
                .matches(&m, none)
        );
        assert!(MatchRule::signal().arg(0, "x").matches(&m, none));
        assert!(!MatchRule::signal().arg(0, "y").matches(&m, none));
        assert!(
            !MatchRule::signal().arg(1, "3").matches(&m, none),
            "argN is for strings"
        );
        assert!(MatchRule::signal().sender(":1.5").matches(&m, none));
        assert!(
            !MatchRule::signal().sender("org.bluez").matches(&m, none),
            "an unresolved name matches nothing"
        );
        let owner = BusName::from(":1.5");
        assert!(
            MatchRule::signal()
                .sender("org.bluez")
                .matches(&m, |_| Some(&owner))
        );
        assert!(!MatchRule::signal().destination(":1.1").matches(&m, none));
    }

    #[test]
    fn arg0_namespace() {
        let none = |_: &BusName| None;
        let rule = MatchRule::signal()
            .member("NameOwnerChanged")
            .arg0_namespace("org.mpris.MediaPlayer2");
        assert_eq!(
            rule.to_rule_string(),
            "type='signal',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'"
        );
        let owner_changed = |name: &str| {
            sig(
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "NameOwnerChanged",
                "org.freedesktop.DBus",
            )
            .with_body(&(name, "", ":1.49"))
        };
        assert!(rule.matches(&owner_changed("org.mpris.MediaPlayer2"), none));
        assert!(rule.matches(&owner_changed("org.mpris.MediaPlayer2.vlc"), none));
        assert!(rule.matches(
            &owner_changed("org.mpris.MediaPlayer2.firefox.instance_1_49"),
            none
        ));
        assert!(
            !rule.matches(&owner_changed("org.mpris.MediaPlayer2Foo"), none),
            "only at a dot"
        );
        assert!(!rule.matches(&owner_changed("org.bluez"), none));
        let numeric = sig("/a", "a.b", "NameOwnerChanged", ":1.5").with_body(&(3u32,));
        assert!(!rule.matches(&numeric, none), "arg0 must be a string");
    }
}
