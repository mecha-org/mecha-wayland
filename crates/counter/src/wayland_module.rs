use crate::ring::{IoEvent, SharedRingProxy};
use crate::wire::{HEADER_SIZE, MessageBuilder, MessageHeader, MessageReader};
use io_uring::{opcode, types};
use std::env;
use std::os::fd::{IntoRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub struct Wayland {
    fd: RawFd,
    ring_proxy: SharedRingProxy,

    // Buffers for io_uring and wire.rs
    read_buf: Vec<u8>,
    write_buf: Vec<u8>,
    fds_buf: Vec<RawFd>,

    // Tracking io_uring submission states
    read_token: Option<u64>,
    write_token: Option<u64>,
}

impl Wayland {
    pub fn new(ring_proxy: SharedRingProxy) -> std::io::Result<Self> {
        // Find Wayland socket
        let xdg_runtime_dir =
            env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
        let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        let path = PathBuf::from(xdg_runtime_dir).join(wayland_display);

        // Connect and extract raw FD for io_uring
        let stream = UnixStream::connect(path)?;
        stream.set_nonblocking(true)?;
        let fd = stream.into_raw_fd();

        let mut wl = Self {
            fd,
            ring_proxy,
            read_buf: vec![0; 4096], // 4KB read buffer
            write_buf: Vec::new(),
            fds_buf: Vec::new(),
            read_token: None,
            write_token: None,
        };

        // Send Handshake
        wl.init_handshake();

        // Queue the first Read operation
        wl.submit_read();

        Ok(wl)
    }

    /// Tests `MessageBuilder`: constructs `wl_display@1.get_registry(new_id: 2)`
    fn init_handshake(&mut self) {
        println!("[Wayland] Initiating handshake (wl_display.get_registry)...");

        // Wayland Display object ID is 1. get_registry opcode is 1.
        let builder = MessageBuilder::new(&mut self.write_buf, &mut self.fds_buf, 1, 1);
        // new_id for the registry object = 2
        builder.write_u32(2).build();

        // Sync (Opcode 0 on wl_display@1)
        // Assign ID 3 to the new callback object
        let builder = MessageBuilder::new(&mut self.write_buf, &mut self.fds_buf, 1, 0);
        builder.write_u32(3).build();

        self.submit_write();
    }

    pub fn handle_io(&mut self, event: &IoEvent) {
        let IoEvent::Completed { token, result } = event;

        if Some(*token) == self.write_token {
            self.write_token = None;
            if *result > 0 {
                // Drain successfully written bytes
                self.write_buf.drain(0..*result as usize);

                // If there's more to write, submit again
                if !self.write_buf.is_empty() {
                    self.submit_write();
                }
            } else {
                eprintln!("[Wayland] Write error or EOF: {}", result);
            }
        } else if Some(*token) == self.read_token {
            self.read_token = None;
            if *result > 0 {
                self.process_read_data(*result as usize);
                // Queue the next read
                self.submit_read();
            } else if *result == 0 {
                println!("[Wayland] Connection closed by server.");
            } else {
                eprintln!("[Wayland] Read error: {}", result);
            }
        }
    }

    /// Tests `MessageReader`: Parses incoming responses, specifically `wl_registry.global` events.
    fn process_read_data(&mut self, bytes_read: usize) {
        let mut offset = 0;
        let data = &self.read_buf[..bytes_read];

        while offset < bytes_read {
            let slice = &data[offset..];

            if let Some(header) = MessageHeader::parse(slice) {
                if slice.len() < header.size as usize {
                    // Buffer didn't capture full message; incomplete message handling.
                    break;
                }

                let body_len = (header.size as usize) - HEADER_SIZE;
                let body = &slice[HEADER_SIZE..HEADER_SIZE + body_len];

                let mut reader = MessageReader::new(body, &mut self.fds_buf);

                // Check if this is a `wl_registry` (ID=2) `global` event (Opcode=0)
                if header.sender_id == 2 && header.opcode == 0 {
                    // Global event signature: name (u32), interface (string), version (u32)
                    let name = reader.read_u32().unwrap_or(0);
                    let interface = reader.read_string().unwrap_or("unknown");
                    let version = reader.read_u32().unwrap_or(0);

                    println!(
                        "[Wayland Registry Global] Name: {}, Interface: '{}', Version: {}",
                        name, interface, version
                    );
                } else if header.sender_id == 3 && header.opcode == 0 {
                    // wl_callback@3.done
                    println!("\n[Wayland Sync] COMPLETE! All initial globals received.\n",)
                } else if header.sender_id == 1 && header.opcode == 1 {
                    // wl_display@1.delete_id
                    let deleted_id = reader.read_u32().unwrap_or(0);
                    println!(
                        "[Wayland Server] Acknowledged cleanup: Object ID {} can now be reused.",
                        deleted_id
                    );
                } else {
                    println!(
                        "[Wayland Raw Event] Sender: {}, Opcode: {}, Size: {}",
                        header.sender_id, header.opcode, header.size
                    );
                }

                offset += header.size as usize;
            } else {
                // Not enough bytes for a header
                break;
            }
        }
    }

    fn submit_write(&mut self) {
        if self.write_buf.is_empty() || self.write_token.is_some() {
            return;
        }

        let sqe = opcode::Write::new(
            types::Fd(self.fd),
            self.write_buf.as_ptr(),
            self.write_buf.len() as u32,
        )
        .build();

        let token = self.ring_proxy.borrow_mut().push(sqe);
        self.write_token = Some(token);
    }

    fn submit_read(&mut self) {
        if self.read_token.is_some() {
            return;
        }

        let sqe = opcode::Read::new(
            types::Fd(self.fd),
            self.read_buf.as_mut_ptr(),
            self.read_buf.len() as u32,
        )
        .build();

        let token = self.ring_proxy.borrow_mut().push(sqe);
        self.read_token = Some(token);
    }
}

#[macro_export]
macro_rules! register_wayland {
    () => {
        app::module::Module::<crate::wayland_module::Wayland>::new().on(
            |wl: &mut crate::wayland_module::Wayland, event: &crate::ring::IoEvent| {
                wl.handle_io(event);
            },
        )
    };
}
