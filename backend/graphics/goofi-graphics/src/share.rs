//! Pictures in device memory the child of a node the tick calls maps too: a Vulkan buffer whose
//! memory goes to the child as a file descriptor, where CUDA maps it. On a platform or a device
//! without that, no door opens and a node's pictures cross as pixels.

#[cfg(target_os = "linux")]
pub use linux::{device_uuid, Door, Link};
#[cfg(not(target_os = "linux"))]
pub use other::{device_uuid, Door, Link};

/// The child's end learns the door and the device from these: `goofi.gpu` reads them.
pub const DOOR: &str = "GOOFI_TEXTURES";
pub const DEVICE: &str = "GOOFI_TEXTURES_DEVICE";

#[cfg(target_os = "linux")]
mod linux {
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
    use std::sync::atomic::{AtomicU64, Ordering};

    use ash::vk;
    use goofi_core::texture::Shared;
    use wgpu::hal::api::Vulkan;

    use crate::gpu::{padded_row, Gpu};

    const OPAQUE_FD: vk::ExternalMemoryHandleTypeFlags = vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD;
    /// The kinds of message the child takes: a buffer for an input, the one for its output, and
    /// one it drops.
    const INPUT: u64 = 1;
    const TARGET: u64 = 2;
    const FORGET: u64 = 3;

    /// The device as CUDA names it, where it can hand a buffer's memory to another process.
    pub fn device_uuid(device: &wgpu::Device) -> Option<[u8; 16]> {
        let hal = unsafe { device.as_hal::<Vulkan>() }?;
        let instance = hal.shared_instance();
        if !hal.enabled_device_extensions().contains(&ash::khr::external_memory_fd::NAME) || instance.instance_api_version() < vk::API_VERSION_1_1 {
            return None;
        }
        let mut id = vk::PhysicalDeviceIDProperties::default();
        let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
        unsafe { instance.raw_instance().get_physical_device_properties2(hal.raw_physical_device(), &mut props) };
        Some(id.device_uuid)
    }

    /// Where a node's child joins the engine: a socket of the abstract namespace, which needs no
    /// path and goes with the process.
    pub struct Door {
        listener: UnixListener,
        name: String,
    }

    impl Door {
        pub fn open() -> Option<Door> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let name = format!("goofi-textures-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
            let listener = UnixListener::bind_addr(&SocketAddr::from_abstract_name(&name).ok()?).ok()?;
            listener.set_nonblocking(true).ok()?;
            Some(Door { listener, name })
        }

        pub fn name(&self) -> &str {
            &self.name
        }
    }

    struct Buffer {
        buffer: wgpu::Buffer,
        shared: Shared,
    }

    /// The render thread's end of one child: the buffers the two share.
    pub struct Link {
        stream: UnixStream,
        next: u64,
        inputs: Vec<Option<Buffer>>,
        target: Option<Buffer>,
    }

    impl Link {
        /// The child, once it has joined: it does so only where its CUDA device is this device.
        /// Only a process of this user is taken: the name of the socket is no secret.
        pub fn accept(door: &Door) -> Option<Link> {
            let (stream, _) = door.listener.accept().ok()?;
            let mut peer = libc::ucred { pid: 0, uid: libc::uid_t::MAX, gid: 0 };
            let mut len = size_of::<libc::ucred>() as libc::socklen_t;
            let read = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&raw mut peer).cast(), &mut len) };
            if read != 0 || peer.uid != unsafe { libc::geteuid() } {
                return None;
            }
            stream.set_nonblocking(true).ok()?;
            Some(Link { stream, next: 1, inputs: Vec::new(), target: None })
        }

        /// The buffer input `slot`'s producer is copied into, made again when its size moves.
        pub fn input(&mut self, gpu: &Gpu, slot: usize, size: (u32, u32)) -> Result<(wgpu::Buffer, Shared), String> {
            if self.inputs.len() <= slot {
                self.inputs.resize_with(slot + 1, || None);
            }
            if !self.inputs[slot].as_ref().is_some_and(|held| (held.shared.width, held.shared.height) == size) {
                let made = self.make(gpu, INPUT, size)?;
                if let Some(old) = self.inputs[slot].replace(made) {
                    self.forget(old)?;
                }
            }
            let held = self.inputs[slot].as_ref().expect("made above");
            Ok((held.buffer.clone(), held.shared))
        }

        /// The buffer the child named: the target it wrote its picture into, or an input it
        /// passed on.
        pub fn buffer(&self, id: u64) -> Option<(wgpu::Buffer, Shared)> {
            let held = self.target.iter().chain(self.inputs.iter().flatten()).find(|held| held.shared.id == id)?;
            Some((held.buffer.clone(), held.shared))
        }

        /// Take what the child asked for: a target of the size of the pictures it makes.
        pub fn asked(&mut self, gpu: &Gpu) -> Result<(), String> {
            let mut asks = [0u8; 8];
            let mut last = None;
            loop {
                match self.stream.read(&mut asks) {
                    Ok(8) => last = Some((u32::from_le_bytes([asks[0], asks[1], asks[2], asks[3]]), u32::from_le_bytes([asks[4], asks[5], asks[6], asks[7]]))),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    _ => return Err("the node's child left the texture socket".into()),
                }
            }
            let Some(size) = last.filter(|size| !self.target.as_ref().is_some_and(|held| (held.shared.width, held.shared.height) == *size)) else {
                return Ok(());
            };
            if size.0 == 0 || size.1 == 0 || size.0 > goofi_core::texture::MAX_SIZE || size.1 > goofi_core::texture::MAX_SIZE {
                return Err(format!("the node asked for a picture of {}x{}", size.0, size.1));
            }
            let made = self.make(gpu, TARGET, size)?;
            if let Some(old) = self.target.replace(made) {
                self.forget(old)?;
            }
            Ok(())
        }

        fn forget(&mut self, old: Buffer) -> Result<(), String> {
            send(&self.stream, &message(FORGET, &old.shared, 0), None)
        }

        /// A buffer of `size` whose memory the child has from now on.
        fn make(&mut self, gpu: &Gpu, kind: u64, size: (u32, u32)) -> Result<Buffer, String> {
            let shared = Shared { id: self.next, width: size.0, height: size.1, pitch: padded_row(size.0, 8) };
            self.next += 1;
            let (buffer, allocation, fd) = export(&gpu.device, u64::from(shared.pitch) * u64::from(shared.height))?;
            send(&self.stream, &message(kind, &shared, allocation), Some(&fd))?;
            Ok(Buffer { buffer, shared })
        }
    }

    fn message(kind: u64, shared: &Shared, allocation: u64) -> [u8; 40] {
        let mut out = [0u8; 40];
        out[0..8].copy_from_slice(&kind.to_le_bytes());
        out[8..16].copy_from_slice(&shared.id.to_le_bytes());
        out[16..24].copy_from_slice(&allocation.to_le_bytes());
        out[24..28].copy_from_slice(&shared.width.to_le_bytes());
        out[28..32].copy_from_slice(&shared.height.to_le_bytes());
        out[32..36].copy_from_slice(&shared.pitch.to_le_bytes());
        out
    }

    /// One message, with the descriptor it hands over. A child that does not take its messages
    /// fills the socket, and that ends the link: this thread never waits on it.
    fn send(stream: &UnixStream, bytes: &[u8], fd: Option<&OwnedFd>) -> Result<(), String> {
        let mut iov = libc::iovec { iov_base: bytes.as_ptr() as *mut _, iov_len: bytes.len() };
        let space = unsafe { libc::CMSG_SPACE(size_of::<libc::c_int>() as u32) } as usize;
        let mut control = vec![0u8; space];
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        if let Some(fd) = fd {
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = space as _;
            unsafe {
                let head = libc::CMSG_FIRSTHDR(&msg);
                (*head).cmsg_level = libc::SOL_SOCKET;
                (*head).cmsg_type = libc::SCM_RIGHTS;
                (*head).cmsg_len = libc::CMSG_LEN(size_of::<libc::c_int>() as u32) as _;
                std::ptr::write_unaligned(libc::CMSG_DATA(head).cast::<libc::c_int>(), fd.as_raw_fd());
            }
        }
        let sent = unsafe { libc::sendmsg(stream.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) };
        if sent == bytes.len() as isize {
            Ok(())
        } else {
            Err(format!("the texture socket to the node's child: {}", std::io::Error::last_os_error()))
        }
    }

    /// A device-local buffer of `bytes` that wgpu copies to and from, the size of its
    /// allocation, and its memory as the descriptor another process imports.
    fn export(device: &wgpu::Device, bytes: u64) -> Result<(wgpu::Buffer, u64, OwnedFd), String> {
        let hal = unsafe { device.as_hal::<Vulkan>() }.ok_or("the device is not a Vulkan device")?;
        let (raw, instance) = (hal.raw_device(), hal.shared_instance().raw_instance());
        let refused = |what: &str, e: vk::Result| format!("a shared buffer's {what}: {e}");
        let mut external = vk::ExternalMemoryBufferCreateInfo::default().handle_types(OPAQUE_FD);
        let usage = vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST;
        let buffer = unsafe { raw.create_buffer(&vk::BufferCreateInfo::default().size(bytes).usage(usage).push_next(&mut external), None) }
            .map_err(|e| refused("creation", e))?;
        let need = unsafe { raw.get_buffer_memory_requirements(buffer) };
        let kinds = unsafe { instance.get_physical_device_memory_properties(hal.raw_physical_device()) };
        let local = (0..kinds.memory_type_count).find(|i| {
            need.memory_type_bits & (1 << i) != 0 && kinds.memory_types[*i as usize].property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        });
        let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(OPAQUE_FD);
        let memory = local.ok_or_else(|| "a shared buffer: the device has no local memory for it".to_string()).and_then(|kind| {
            let info = vk::MemoryAllocateInfo::default().allocation_size(need.size).memory_type_index(kind).push_next(&mut export);
            unsafe { raw.allocate_memory(&info, None) }.map_err(|e| refused("memory", e))
        });
        let memory = match memory {
            Ok(memory) => memory,
            Err(why) => {
                unsafe { raw.destroy_buffer(buffer, None) };
                return Err(why);
            }
        };
        let fd = unsafe { raw.bind_buffer_memory(buffer, memory, 0) }.map_err(|e| refused("binding", e)).and_then(|()| {
            let info = vk::MemoryGetFdInfoKHR::default().memory(memory).handle_type(OPAQUE_FD);
            unsafe { ash::khr::external_memory_fd::Device::new(instance, raw).get_memory_fd(&info) }.map_err(|e| refused("descriptor", e))
        });
        let fd = match fd {
            Ok(fd) => unsafe { OwnedFd::from_raw_fd(fd) },
            Err(why) => {
                unsafe {
                    raw.destroy_buffer(buffer, None);
                    raw.free_memory(memory, None);
                }
                return Err(why);
            }
        };
        // From here wgpu owns both: it destroys the buffer and frees the memory with its last use.
        let held = unsafe { wgpu::hal::vulkan::Buffer::from_raw_managed(buffer, memory, 0, need.size) };
        let desc = wgpu::BufferDescriptor {
            label: Some("shared picture"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        };
        Ok((unsafe { device.create_buffer_from_hal::<Vulkan>(held, &desc) }, need.size, fd))
    }
}

#[cfg(not(target_os = "linux"))]
mod other {
    use goofi_core::texture::Shared;

    use crate::gpu::Gpu;

    pub fn device_uuid(_: &wgpu::Device) -> Option<[u8; 16]> {
        None
    }

    /// Never made here: no door opens.
    pub enum Door {}

    impl Door {
        pub fn open() -> Option<Door> {
            None
        }
        pub fn name(&self) -> &str {
            match *self {}
        }
    }

    /// Never made here: there is no door to accept at.
    pub enum Link {}

    impl Link {
        pub fn accept(door: &Door) -> Option<Link> {
            match *door {}
        }
        pub fn input(&mut self, _: &Gpu, _: usize, _: (u32, u32)) -> Result<(wgpu::Buffer, Shared), String> {
            match *self {}
        }
        pub fn buffer(&self, _: u64) -> Option<(wgpu::Buffer, Shared)> {
            match *self {}
        }
        pub fn asked(&mut self, _: &Gpu) -> Result<(), String> {
            match *self {}
        }
    }
}
