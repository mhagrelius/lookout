//! The records the app works in terms of.
//!
//! Each module reads one DSM reply into a typed record. They are all
//! `from_json` rather than `Deserialize` implementations on purpose: DSM
//! sends the same value as a number in one version and a quoted string in
//! the next, omits fields without warning, and spells one flag three ways in
//! a single reply. A hand-written reader absorbs that; a derived one turns it
//! into a parse failure and a blank page.

pub mod container;
pub mod log;
pub mod network;
pub mod package;
pub mod power;
pub mod session;
pub mod share;
pub mod storage;
pub mod system;
pub mod utilization;

pub use container::{
    owning_project, unowned_containers, Container, ContainerHealth, Project, State,
};
pub use log::{LogEntry, LogPage, Severity};
pub use network::NetworkInterface;
pub use package::Package;
pub use power::{Cooling, Ups};
pub use session::Session;
pub use share::Share;
pub use storage::{Disk, Health, Pool, Storage, Volume};
pub use system::{format_uptime, SystemInfo};
pub use utilization::{Cpu, Interface, Io, Memory, Utilization};
