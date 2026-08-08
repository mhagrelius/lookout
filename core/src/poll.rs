//! What one refresh asks for, and what it comes back with.
//!
//! Kept here rather than in the widgets because it is not a drawing decision:
//! a second frontend needs the same set of calls and the same result shape,
//! and deriving them twice is how the two would drift.
//!
//! Building the plan is pure — it takes capabilities and returns calls — so
//! "does the Overview stop asking for containers when Container Manager is
//! absent?" is a unit test rather than an afternoon with a packet capture.

use serde_json::Value;

use crate::dsm::{Call, Capabilities, Client, Error, Result};
use crate::model::{
    Container, Cooling, LogPage, NetworkInterface, Package, Project, Session, Share, Storage,
    SystemInfo, Ups, Utilization,
};

/// Which API each slot of a plan came from, so results can be routed back
/// without depending on the order surviving a refactor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    System,
    Utilization,
    Storage,
    Containers,
    ContainerResources,
    Projects,
    Shares,
    Log,
    Packages,
    Sessions,
    Interfaces,
    Cooling,
    Ups,
}

/// A set of calls to make, and what each one is for.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub calls: Vec<Call>,
    pub slots: Vec<Slot>,
}

impl Plan {
    fn push(&mut self, slot: Slot, call: Call) {
        self.slots.push(slot);
        self.calls.push(call);
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn has(&self, slot: Slot) -> bool {
        self.slots.contains(&slot)
    }
}

/// Everything one refresh gathered.
///
/// Every field is optional and independent: a call that failed leaves its
/// field `None` and takes the rest of the page with it not at all. That is
/// the "one bad endpoint greys out one card" rule, expressed as a type.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub system: Option<SystemInfo>,
    pub utilization: Option<Utilization>,
    pub storage: Option<Storage>,
    pub containers: Option<Vec<Container>>,
    /// Container Manager Projects — the compose files, and which containers
    /// each one owns.
    pub projects: Option<Vec<Project>>,
    pub shares: Option<Vec<Share>>,
    pub log: Option<LogPage>,
    pub packages: Option<Vec<Package>>,
    pub sessions: Option<Vec<Session>>,
    pub interfaces: Option<Vec<NetworkInterface>>,
    pub cooling: Option<Cooling>,
    pub ups: Option<Ups>,
    /// What went wrong, per slot, for the cards that came back empty.
    pub failures: Vec<(Slot, String)>,
}

impl Snapshot {
    /// Whether anything at all arrived. An entirely empty snapshot means the
    /// host is unreachable rather than quiet, and the window says so.
    pub fn is_empty(&self) -> bool {
        self.system.is_none()
            && self.utilization.is_none()
            && self.storage.is_none()
            && self.containers.is_none()
            && self.shares.is_none()
            && self.log.is_none()
            && self.packages.is_none()
            && self.sessions.is_none()
            && self.interfaces.is_none()
    }
}

/// The calls the Overview needs, given what this host actually has.
///
/// `understood` versions are pinned per API rather than taken from the host's
/// maximum: a newer major version can change a reply's shape, and the readers
/// here were written against a specific one.
pub fn overview_plan(caps: &Capabilities) -> Plan {
    let mut plan = Plan::default();

    if let Some(v) = caps.version_for("SYNO.Core.System", 1) {
        plan.push(Slot::System, Call::new("SYNO.Core.System", v, "info"));
    }
    if let Some(v) = caps.version_for("SYNO.Core.System.Utilization", 1) {
        plan.push(
            Slot::Utilization,
            Call::new("SYNO.Core.System.Utilization", v, "get"),
        );
    }
    if let Some(v) = caps.version_for("SYNO.Storage.CGI.Storage", 1) {
        plan.push(
            Slot::Storage,
            Call::new("SYNO.Storage.CGI.Storage", v, "load_info"),
        );
    }
    // Absent unless Container Manager is installed, which is exactly the case
    // this gating exists for.
    if let Some(v) = caps.version_for("SYNO.Docker.Container", 1) {
        plan.push(
            Slot::Containers,
            Call::new("SYNO.Docker.Container", v, "list")
                .param("limit", "-1")
                .param("offset", "0")
                .param("type", "\"all\""),
        );
    }
    if let Some(v) = caps.version_for("SYNO.Docker.Container.Resource", 1) {
        plan.push(
            Slot::ContainerResources,
            Call::new("SYNO.Docker.Container.Resource", v, "get"),
        );
    }
    // Gated separately from the container list: a box can run containers with
    // no project at all, and older Container Manager builds predate the API.
    if let Some(v) = caps.version_for("SYNO.Docker.Project", 1) {
        plan.push(Slot::Projects, Call::new("SYNO.Docker.Project", v, "list"));
    }
    if let Some(v) = caps.version_for("SYNO.Core.Share", 1) {
        plan.push(
            Slot::Shares,
            Call::new("SYNO.Core.Share", v, "list").param(
                "additional",
                "[\"disk_size\",\"share_quota\",\"enc\",\"hidden\"]",
            ),
        );
    }
    if let Some(v) = caps.version_for("SYNO.Core.SyslogClient.Log", 1) {
        plan.push(
            Slot::Log,
            Call::new("SYNO.Core.SyslogClient.Log", v, "list")
                .param("start", "0")
                .param("limit", "50"),
        );
    }

    if let Some(v) = caps.version_for("SYNO.Core.Package", 2) {
        plan.push(
            Slot::Packages,
            // Only `["status"]`. Adding `version` — which is a top-level
            // field, not an addition — makes DSM answer with **zero
            // packages** rather than an error, so the page silently empties.
            Call::new("SYNO.Core.Package", v, "list").param("additional", "[\"status\"]"),
        );
    }
    if let Some(v) = caps.version_for("SYNO.Core.CurrentConnection", 1) {
        plan.push(
            Slot::Sessions,
            Call::new("SYNO.Core.CurrentConnection", v, "list"),
        );
    }

    if let Some(v) = caps.version_for("SYNO.Core.Network.Interface", 1) {
        plan.push(
            Slot::Interfaces,
            Call::new("SYNO.Core.Network.Interface", v, "list"),
        );
    }
    if let Some(v) = caps.version_for("SYNO.Core.Hardware.FanSpeed", 1) {
        plan.push(
            Slot::Cooling,
            Call::new("SYNO.Core.Hardware.FanSpeed", v, "get"),
        );
    }
    if let Some(v) = caps.version_for("SYNO.Core.ExternalDevice.UPS", 1) {
        plan.push(
            Slot::Ups,
            Call::new("SYNO.Core.ExternalDevice.UPS", v, "get"),
        );
    }

    plan
}

/// Turn a plan's results into a snapshot.
///
/// Pure, so the interesting cases — a container list that failed, a reply in
/// an unexpected shape — are unit tests rather than something you wait for a
/// NAS to do.
pub fn assemble(plan: &Plan, results: Vec<Result<Value>>) -> Snapshot {
    let mut snap = Snapshot::default();
    let mut container_resources: Option<Value> = None;

    for (slot, result) in plan.slots.iter().zip(results) {
        let data = match result {
            Ok(data) => data,
            Err(e) => {
                snap.failures.push((*slot, e.to_string()));
                continue;
            }
        };

        match slot {
            Slot::System => snap.system = Some(SystemInfo::from_json(&data)),
            Slot::Utilization => snap.utilization = Some(Utilization::from_json(&data)),
            Slot::Storage => snap.storage = Some(Storage::from_json(&data)),
            Slot::Containers => snap.containers = Some(Container::list_from_json(&data)),
            Slot::ContainerResources => container_resources = Some(data),
            Slot::Projects => snap.projects = Some(Project::list_from_json(&data)),
            Slot::Shares => snap.shares = Some(Share::list_from_json(&data)),
            Slot::Log => snap.log = Some(LogPage::from_json(&data)),
            Slot::Packages => snap.packages = Some(Package::list_from_json(&data)),
            Slot::Sessions => snap.sessions = Some(Session::list_from_json(&data)),
            Slot::Interfaces => snap.interfaces = Some(NetworkInterface::list_from_json(&data)),
            Slot::Cooling => snap.cooling = Some(Cooling::from_json(&data)),
            Slot::Ups => snap.ups = Some(Ups::from_json(&data)),
        }
    }

    // Resources arrive as their own call and are folded in afterwards, so the
    // order of the two inside the compound request does not matter.
    if let (Some(containers), Some(resources)) = (&mut snap.containers, &container_resources) {
        Container::apply_resources(containers, resources);
    }

    snap
}

/// How many ticks are skipped between polls when nobody is looking.
///
/// A monitor in a background window should not keep a NAS's disks spinning,
/// but it should not go stale either: at the default five-second interval this
/// is a poll every half minute, so switching back shows something recent while
/// the window catches up.
pub const UNFOCUSED_EVERY: u64 = 6;

/// Whether this tick should actually poll.
///
/// Pure, and in core rather than the widgets, because it is a policy about
/// how hard to lean on the DiskStation — the same answer a second frontend
/// wants. `tick` counts from zero and increments once per timer fire.
pub fn should_poll(focused: bool, tick: u64) -> bool {
    focused || tick % UNFOCUSED_EVERY == 0
}

/// Run one refresh.
///
/// The only function here that touches a socket, and it is a straight line:
/// plan, send, assemble.
pub fn overview(client: &Client, caps: &Capabilities) -> Result<Snapshot> {
    let plan = overview_plan(caps);
    if plan.is_empty() {
        return Err(Error::Malformed(
            "this host exposes none of the APIs the overview needs".into(),
        ));
    }
    let results = client.compound(&plan.calls)?;
    Ok(assemble(&plan, results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn caps_with(names: &[&str]) -> Capabilities {
        let map: serde_json::Map<String, Value> = names
            .iter()
            .map(|n| (n.to_string(), json!({"minVersion": 1, "maxVersion": 1})))
            .collect();
        Capabilities::parse(&Value::Object(map)).expect("caps should parse")
    }

    fn full_caps() -> Capabilities {
        caps_with(&[
            "SYNO.Core.System",
            "SYNO.Core.System.Utilization",
            "SYNO.Storage.CGI.Storage",
            "SYNO.Docker.Container",
            "SYNO.Docker.Container.Resource",
            "SYNO.Docker.Project",
            "SYNO.Core.Share",
            "SYNO.Core.SyslogClient.Log",
            "SYNO.Core.Package",
            "SYNO.Core.CurrentConnection",
            "SYNO.Core.Network.Interface",
            "SYNO.Core.Hardware.FanSpeed",
            "SYNO.Core.ExternalDevice.UPS",
        ])
    }

    #[test]
    fn a_full_host_is_asked_for_everything_in_one_request() {
        let plan = overview_plan(&full_caps());
        assert_eq!(plan.len(), 13);
        assert!(plan.has(Slot::Containers));
        assert!(plan.has(Slot::Projects));
    }

    #[test]
    fn container_manager_without_the_project_api_still_lists_containers() {
        // Projects are gated on their own API, not on the container one: a
        // box can run containers with no compose project in sight.
        let caps = caps_with(&["SYNO.Docker.Container"]);
        let plan = overview_plan(&caps);
        assert!(plan.has(Slot::Containers));
        assert!(!plan.has(Slot::Projects));
    }

    #[test]
    fn a_host_without_container_manager_is_not_asked_about_containers() {
        // The whole point of capability gating: no Container Manager means no
        // call, no card, and no error to explain to anyone.
        let caps = caps_with(&["SYNO.Core.System", "SYNO.Core.System.Utilization"]);
        let plan = overview_plan(&caps);
        assert!(!plan.has(Slot::Containers));
        assert!(!plan.has(Slot::ContainerResources));
        assert_eq!(plan.len(), 2);
    }

    #[test]
    fn a_host_exposing_nothing_useful_yields_an_empty_plan() {
        let plan = overview_plan(&caps_with(&["SYNO.API.Auth"]));
        assert!(plan.is_empty());
    }

    #[test]
    fn results_land_in_the_slot_they_were_asked_for() {
        let plan = overview_plan(&full_caps());
        let results: Vec<Result<Value>> = plan
            .slots
            .iter()
            .map(|slot| match slot {
                Slot::System => Ok(json!({"model": "DS-series"})),
                Slot::Utilization => Ok(json!({"cpu": {"user_load": 5}})),
                Slot::Storage => Ok(json!({"volumes": [{"id": "volume_1", "status": "normal"}]})),
                Slot::Containers => Ok(json!({"containers": [
                    {"id": "a", "name": "web", "image": "example/web", "status": "running"}
                ]})),
                Slot::ContainerResources => {
                    Ok(json!({"containers": [{"name": "web", "cpu": 2.5, "memory": 1024}]}))
                }
                Slot::Projects => Ok(json!({
                    "8ec29f37-76bb-49c9-bf07-9b30403fed54": {
                        "id": "8ec29f37-76bb-49c9-bf07-9b30403fed54",
                        "name": "web", "status": "running", "containerIds": ["a"]
                    }
                })),
                Slot::Shares => Ok(json!({"shares": [{"name": "docker"}]})),
                Slot::Log => Ok(json!({"items": [], "total": 0})),
                Slot::Packages => Ok(json!({"packages": []})),
                Slot::Sessions => Ok(json!({"items": []})),
                Slot::Interfaces => Ok(json!({"0": {"ifname": "eth0"}})),
                Slot::Cooling => Ok(json!({"dual_fan_speed": "coolfan"})),
                Slot::Ups => Ok(json!({"enable": false})),
            })
            .collect();

        let snap = assemble(&plan, results);
        assert_eq!(
            snap.system.expect("system").model.as_deref(),
            Some("DS-series")
        );
        assert_eq!(snap.storage.expect("storage").volumes.len(), 1);
        assert_eq!(snap.shares.expect("shares")[0].name, "docker");
        let projects = snap.projects.expect("projects");
        assert_eq!(projects[0].name, "web");
        assert_eq!(projects[0].container_ids, vec!["a"]);
        assert!(snap.failures.is_empty());
    }

    #[test]
    fn container_resources_are_folded_into_the_container_list() {
        // They arrive as two separate calls and have to be married up; this
        // is the reason `assemble` holds one aside rather than matching on
        // position.
        let plan = overview_plan(&full_caps());
        let results: Vec<Result<Value>> = plan
            .slots
            .iter()
            .map(|slot| match slot {
                Slot::Containers => Ok(json!({"containers": [
                    {"id": "a", "name": "web", "image": "example/web", "status": "running"}
                ]})),
                Slot::ContainerResources => {
                    Ok(json!({"containers": [{"name": "web", "cpu": 2.5, "memory": 1024}]}))
                }
                _ => Ok(json!({})),
            })
            .collect();

        let snap = assemble(&plan, results);
        let containers = snap.containers.expect("containers");
        assert_eq!(containers[0].cpu_percent, Some(2.5));
    }

    #[test]
    fn one_failed_call_costs_only_its_own_slot() {
        // The rule the whole compound design exists to deliver.
        let plan = overview_plan(&full_caps());
        let results: Vec<Result<Value>> = plan
            .slots
            .iter()
            .map(|slot| match slot {
                Slot::Storage => Err(Error::Dsm(crate::dsm::DsmError::new(
                    105,
                    "SYNO.Storage.CGI.Storage",
                ))),
                Slot::System => Ok(json!({"model": "DS-series"})),
                _ => Ok(json!({})),
            })
            .collect();

        let snap = assemble(&plan, results);
        assert!(snap.storage.is_none());
        assert!(snap.system.is_some(), "one failure must not take the rest");
        assert_eq!(snap.failures.len(), 1);
        assert_eq!(snap.failures[0].0, Slot::Storage);
        assert!(!snap.is_empty());
    }

    #[test]
    fn a_focused_window_polls_on_every_tick() {
        for tick in 0..20 {
            assert!(should_poll(true, tick), "tick {tick} should poll");
        }
    }

    #[test]
    fn an_unfocused_window_polls_occasionally_rather_than_never() {
        // Never would leave a stale page on switching back; every tick would
        // keep the disks awake for nobody.
        let polls = (0..60).filter(|t| should_poll(false, *t)).count();
        assert_eq!(polls, 10);
        assert!(
            should_poll(false, 0),
            "the first tick after losing focus polls"
        );
        assert!(!should_poll(false, 1));
    }

    #[test]
    fn a_snapshot_where_everything_failed_reads_as_empty() {
        // Which the window shows as "disconnected" rather than as a page of
        // blank cards.
        let plan = overview_plan(&full_caps());
        let results: Vec<Result<Value>> = plan
            .slots
            .iter()
            .map(|_| Err(Error::Transport("connection refused".into())))
            .collect();

        let snap = assemble(&plan, results);
        assert!(snap.is_empty());
        assert_eq!(snap.failures.len(), 13);
    }
}
