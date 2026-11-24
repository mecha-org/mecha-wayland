//! Validated names. Each is an owned string that passed the D-Bus
//! specification's rules when it was made: a literal through `From`
//! (panicking on a typo, a bug in source), a runtime string through
//! `new` or `TryFrom<String>`.

use std::fmt;

/// A string that is not a valid name of its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameError {
    pub kind: &'static str,
    pub input: String,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a valid {}", self.input, self.kind)
    }
}

impl std::error::Error for NameError {}

macro_rules! name {
    ($(#[$doc:meta])* $name:ident, $kind:literal, $valid:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Validate a runtime string.
            pub fn new(s: &str) -> Result<Self, NameError> {
                Self::try_from(s.to_owned())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = NameError;
            fn try_from(s: String) -> Result<Self, NameError> {
                if $valid(&s) {
                    Ok($name(s))
                } else {
                    Err(NameError { kind: $kind, input: s })
                }
            }
        }

        /// A literal. Panics if it is invalid.
        impl From<&'static str> for $name {
            fn from(s: &'static str) -> Self {
                Self::new(s).unwrap_or_else(|e| panic!("dbus: {e}"))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    };
}

name!(
    /// An object path: `/org/freedesktop/NetworkManager`.
    ObjectPath,
    "object path",
    valid_path
);
name!(
    /// A unique (`:1.42`) or well-known (`org.bluez`) bus name.
    BusName,
    "bus name",
    valid_bus_name
);
name!(
    /// `org.freedesktop.NetworkManager`
    InterfaceName,
    "interface name",
    valid_interface
);
name!(
    /// A method or signal name: `GetDevices`.
    MemberName,
    "member name",
    valid_member
);
name!(
    /// `org.freedesktop.DBus.Error.AccessDenied`
    ErrorName,
    "error name",
    valid_interface
);

impl BusName {
    /// `:1.42` rather than `org.bluez`.
    pub fn is_unique(&self) -> bool {
        self.0.starts_with(':')
    }
}

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn valid_path(s: &str) -> bool {
    if s == "/" {
        return true;
    }
    let Some(rest) = s.strip_prefix('/') else {
        return false;
    };
    rest.split('/')
        .all(|seg| !seg.is_empty() && seg.bytes().all(word))
}

/// Two or more dot-separated elements of word characters plus `extra`.
fn valid_dotted(s: &str, extra: &[u8], digit_first: bool) -> bool {
    if s.is_empty() || s.len() > 255 {
        return false;
    }
    let mut parts = 0;
    for el in s.split('.') {
        parts += 1;
        let b = el.as_bytes();
        if b.is_empty() || (!digit_first && b[0].is_ascii_digit()) {
            return false;
        }
        if !b.iter().all(|&c| word(c) || extra.contains(&c)) {
            return false;
        }
    }
    parts >= 2
}

fn valid_interface(s: &str) -> bool {
    valid_dotted(s, b"", false)
}

fn valid_bus_name(s: &str) -> bool {
    match s.strip_prefix(':') {
        Some(rest) => s.len() <= 255 && valid_dotted(rest, b"-", true),
        None => valid_dotted(s, b"-", false),
    }
}

fn valid_member(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 255 && !b[0].is_ascii_digit() && b.iter().all(|&c| word(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        for ok in ["/", "/a", "/org/freedesktop/DBus", "/a_1/B2"] {
            assert!(ObjectPath::new(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a", "//", "/a/", "/a//b", "/a-b", "/a.b"] {
            assert!(ObjectPath::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn interfaces_and_errors() {
        for ok in ["a.b", "org.freedesktop.DBus", "a_1.B_2"] {
            assert!(InterfaceName::new(ok).is_ok(), "{ok}");
            assert!(ErrorName::new(ok).is_ok(), "{ok}");
        }
        let long = "a.".repeat(128) + "b";
        for bad in ["", "a", "a.", ".a", "a..b", "a.1b", "a-b.c", long.as_str()] {
            assert!(InterfaceName::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn bus_names() {
        for ok in [":1.42", ":a.b", "org.bluez", "a-b.c", ":1.2-3"] {
            assert!(BusName::new(ok).is_ok(), "{ok}");
        }
        for bad in ["", ":", ":1", "org", "1a.b", "a..b", "a.b/c"] {
            assert!(BusName::new(bad).is_err(), "{bad}");
        }
        assert!(BusName::from(":1.42").is_unique());
        assert!(!BusName::from("org.bluez").is_unique());
    }

    #[test]
    fn members() {
        for ok in ["Hello", "_x", "Get2"] {
            assert!(MemberName::new(ok).is_ok(), "{ok}");
        }
        for bad in ["", "2x", "a.b", "a-b"] {
            assert!(MemberName::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    #[should_panic(expected = "is not a valid member name")]
    fn an_invalid_literal_panics() {
        let _ = MemberName::from("not valid");
    }

    #[test]
    fn names_compare_with_str() {
        assert!(MemberName::from("Hello") == "Hello");
        assert_eq!(ObjectPath::from("/a").to_string(), "/a");
    }
}
