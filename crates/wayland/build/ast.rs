use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Protocol {
    #[serde(rename = "@name")]
    pub name: String,

    #[serde(rename = "$value", default)]
    pub items: Vec<ProtocolItem>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolItem {
    Copyright(Copyright),
    Description(Description),
    Interface(Interface),
}

#[derive(Debug, Deserialize, Clone)]
pub struct Copyright {
    #[serde(rename = "$value")]
    pub text: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Description {
    #[serde(rename = "@summary")]
    pub summary: Option<String>,
    #[serde(rename = "$value", default)]
    pub text: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Interface {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@version")]
    pub version: u32,
    #[serde(rename = "@frozen", default)]
    pub frozen: bool,

    #[serde(rename = "$value", default)]
    pub items: Vec<InterfaceItem>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum InterfaceItem {
    Description(Description),
    Request(Message),
    Event(Message),
    Enum(EnumDef),
}

#[derive(Debug, Deserialize, Clone)]
pub struct Message {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@type")]
    pub message_type: Option<MessageType>,
    #[serde(rename = "@since")]
    pub since: Option<u32>,
    #[serde(rename = "@deprecated-since")]
    pub deprecated_since: Option<u32>,

    pub description: Option<Description>,

    #[serde(rename = "arg", default)]
    pub args: Vec<Arg>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Destructor,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Arg {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@type")]
    pub arg_type: ArgType,
    #[serde(rename = "@interface")]
    pub interface: Option<String>,
    #[serde(rename = "@allow-null", default)]
    pub allow_null: bool,
    #[serde(rename = "@enum")]
    pub enum_type: Option<String>,
    #[serde(rename = "@summary")]
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArgType {
    Int,
    Uint,
    Fixed,
    String,
    Array,
    Fd,
    NewId,
    Object,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EnumDef {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@bitfield", default)]
    pub bitfield: bool,
    #[serde(rename = "@since")]
    pub since: Option<u32>,

    pub description: Option<Description>,

    #[serde(rename = "entry", default)]
    pub entries: Vec<EnumEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EnumEntry {
    #[serde(rename = "@name")]
    pub name: String,

    #[serde(rename = "@value")]
    pub value: String,

    #[serde(rename = "@summary")]
    pub summary: Option<String>,
    #[serde(rename = "@since")]
    pub since: Option<u32>,
}

impl Protocol {
    pub fn interfaces(&self) -> impl Iterator<Item = &Interface> {
        self.items.iter().filter_map(|i| match i {
            ProtocolItem::Interface(interface) => Some(interface),
            _ => None,
        })
    }

    pub fn find_enum(&self, current_iface: &str, enum_attr: &str) -> Option<&EnumDef> {
        let (iface_name, enum_name) = if enum_attr.contains('.') {
            enum_attr.split_once('.').unwrap()
        } else {
            (current_iface, enum_attr)
        };

        self.interfaces()
            .find(|i| i.name == iface_name)?
            .enums()
            .find(|e| e.name == enum_name)
    }
}

impl Interface {
    pub fn description(&self) -> Option<&Description> {
        self.items.iter().find_map(|i| match i {
            InterfaceItem::Description(d) => Some(d),
            _ => None,
        })
    }

    pub fn requests(&self) -> impl Iterator<Item = &Message> {
        self.items.iter().filter_map(|i| match i {
            InterfaceItem::Request(req) => Some(req),
            _ => None,
        })
    }

    pub fn events(&self) -> impl Iterator<Item = &Message> {
        self.items.iter().filter_map(|i| match i {
            InterfaceItem::Event(ev) => Some(ev),
            _ => None,
        })
    }

    pub fn enums(&self) -> impl Iterator<Item = &EnumDef> {
        self.items.iter().filter_map(|i| match i {
            InterfaceItem::Enum(en) => Some(en),
            _ => None,
        })
    }
}
