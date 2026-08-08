//! Lookout's DiskStation half: the client, the records, the recorded history.
//!
//! Nothing here links a UI toolkit and nothing here draws. That is the point
//! — a shell on Windows or macOS keeps this crate whole and replaces
//! everything above it.
//!
//! The shape of a session:
//!
//! ```no_run
//! use lookout_core::dsm::{self, Client, Credentials, Host};
//! use lookout_core::model::SystemInfo;
//!
//! let mut client = Client::new(Host::new("nas.example.ts.net"));
//! let caps = dsm::discover(&client)?;            // needs no login
//! client.login(&Credentials::new("user", "pw"))?;
//!
//! if caps.has("SYNO.Core.System") {
//!     let info = SystemInfo::from_json(&client.call("SYNO.Core.System", 1, "info")?);
//!     println!("{:?}", info.model);
//! }
//! # Ok::<(), lookout_core::dsm::Error>(())
//! ```

pub mod action;
pub mod config;
pub mod dsm;
pub mod model;
pub mod poll;
pub mod trend;

pub use action::{container as container_action, ContainerAction};
pub use config::Config;
pub use poll::{Plan, Slot, Snapshot};
pub use trend::{Range, Sample, Trends};
