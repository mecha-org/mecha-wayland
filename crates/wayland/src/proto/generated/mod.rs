include!(concat!(env!("OUT_DIR"), "/generated_shared.rs"));

#[cfg(feature = "client")]
include!(concat!(env!("OUT_DIR"), "/generated_client.rs"));

#[cfg(feature = "server")]
include!(concat!(env!("OUT_DIR"), "/generated_server.rs"));
