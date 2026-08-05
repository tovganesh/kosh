//! A name for a service, so unrelated processes can find each other.
//!
//! ## The problem this solves
//!
//! Capabilities in Kosh are scoped to a specific process:
//! `create_secure_ipc_channel(child, parent)` grants each side `SendMessage` for
//! the *other specifically*, so a process may message its parent and its
//! children and nothing else. That is a good policy and it has exactly one
//! shape — a tree — which means two siblings cannot talk at all.
//!
//! Every process that matters here is a sibling. `init` spawns the block
//! driver, the filesystem and the shell; the shell needs the filesystem, and the
//! filesystem needs the block driver, and none of those pairs is a
//! parent/child. Without something to bridge them the microkernel design stops
//! at one level.
//!
//! The bridge is a name server, which is the role `init` plays in most
//! microkernels and the kernel plays here because the kernel already owns the
//! capability manager. A service registers a name; a client looks the name up
//! and, if it is there, *gets a capability for it* along with the pid. Lookup is
//! the grant.
//!
//! ## Why that is not the same as granting everything
//!
//! A registered name is a deliberate act by the service: it is saying "anyone
//! may talk to me". Nothing else becomes reachable. A process still cannot
//! message an arbitrary pid it guesses, and `hello`'s grandparent test — a send
//! to a process that exists, is not related, and has not registered a name —
//! still comes back `PermissionDenied`. What changes is that a *service* can opt
//! in, which is the distinction between a capability system and a namespace.
//!
//! The obvious next step, and the reason this lives in the kernel rather than in
//! `init`, is that it should eventually live in `init`: a userspace name server
//! holding a delegatable capability for each service is strictly better than the
//! kernel deciding. That needs `delegate_capability` reachable from ring 3,
//! which is four syscalls that still return `NotSupported`.

use spin::Mutex;

use crate::process::ProcessId;
use crate::serial_println;

const MAX_SERVICES: usize = 8;
const MAX_NAME: usize = 16;

#[derive(Clone, Copy)]
struct Service {
    name: [u8; MAX_NAME],
    len: usize,
    pid: ProcessId,
}

impl Service {
    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.len]).unwrap_or("")
    }
}

static SERVICES: Mutex<[Option<Service>; MAX_SERVICES]> = Mutex::new([None; MAX_SERVICES]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceError {
    NameTooLong,
    /// Another process already holds this name. Deliberately not "replace the
    /// old one": a second filesystem quietly taking over the name would move
    /// every subsequent `open` to a server nobody asked for.
    NameTaken,
    TableFull,
    NotFound,
}

/// Claim `name` for `pid`.
pub fn register(name: &str, pid: ProcessId) -> Result<(), ServiceError> {
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(ServiceError::NameTooLong);
    }

    let mut table = SERVICES.lock();

    if let Some(existing) = table.iter().flatten().find(|s| s.name() == name) {
        if existing.pid != pid {
            return Err(ServiceError::NameTaken);
        }
        return Ok(());
    }

    let slot = table
        .iter()
        .position(|s| s.is_none())
        .ok_or(ServiceError::TableFull)?;

    let mut buf = [0u8; MAX_NAME];
    buf[..name.len()].copy_from_slice(name.as_bytes());
    table[slot] = Some(Service {
        name: buf,
        len: name.len(),
        pid,
    });

    serial_println!("  service '{}' registered by process {}", name, pid.0);
    Ok(())
}

/// Find `name`, and give `client` and the service permission to message each
/// other.
///
/// The grant is the point. Returning a pid on its own would be a phone number
/// with no line attached: `send_message` checks the capability, so a client that
/// knew the pid and had no capability would get `PermissionDenied` and no way to
/// tell that from the service being absent.
pub fn lookup(name: &str, client: ProcessId) -> Result<ProcessId, ServiceError> {
    let pid = {
        let table = SERVICES.lock();
        table
            .iter()
            .flatten()
            .find(|s| s.name() == name)
            .map(|s| s.pid)
            .ok_or(ServiceError::NotFound)?
    };

    // Outside the lock: `create_secure_ipc_channel` takes the capability
    // manager's, and a second path that took them in the other order would be a
    // deadlock waiting for load.
    if pid != client {
        if let Err(e) = crate::ipc::security::create_secure_ipc_channel(client, pid) {
            serial_println!(
                "  could not open a channel {}<->{} for service '{}': {:?}",
                client.0,
                pid.0,
                name,
                e
            );
        }
    }

    Ok(pid)
}

/// Drop every name held by `pid`, so a crashed service does not keep its name.
pub fn unregister_pid(pid: ProcessId) {
    let mut table = SERVICES.lock();
    for slot in table.iter_mut() {
        if matches!(slot, Some(s) if s.pid == pid) {
            if let Some(s) = slot.take() {
                serial_println!("  service '{}' went away with process {}", s.name(), pid.0);
            }
        }
    }
}

/// Number of registered services.
pub fn count() -> usize {
    SERVICES.lock().iter().flatten().count()
}

/// Call `f` with each `(name, pid)` pair.
pub fn for_each(mut f: impl FnMut(&str, u32)) {
    let table = SERVICES.lock();
    for s in table.iter().flatten() {
        f(s.name(), s.pid.0);
    }
}
