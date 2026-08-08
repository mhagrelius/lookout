//! Container Manager: `SYNO.Docker.Container` and `SYNO.Docker.Project`.
//!
//! Absent entirely when Container Manager is not installed, which is why
//! every caller checks [`crate::dsm::Capabilities::has`] before asking.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::{as_bool, as_i64, as_u64, as_unix_seconds};

/// A container's health-check verdict.
///
/// Absent, not "unknown", when the image defines no health check — most
/// images do not, and reporting those as unhealthy would be a lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerHealth {
    Healthy,
    Unhealthy,
    /// Inside its `start_period`, or probing and not yet passing.
    Starting,
}

impl ContainerHealth {
    fn from_word(word: &str) -> Option<ContainerHealth> {
        match word.to_ascii_lowercase().as_str() {
            "healthy" => Some(ContainerHealth::Healthy),
            "unhealthy" => Some(ContainerHealth::Unhealthy),
            "starting" => Some(ContainerHealth::Starting),
            _ => None,
        }
    }
}

/// What a container is doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Running,
    Paused,
    Restarting,
    Exited,
    #[default]
    Unknown,
}

impl State {
    pub fn from_word(word: &str) -> State {
        match word.to_ascii_lowercase().as_str() {
            "running" => State::Running,
            "paused" => State::Paused,
            "restarting" => State::Restarting,
            "exited" | "stopped" | "created" => State::Exited,
            _ => State::Unknown,
        }
    }

    /// Whether a Stop button or a Start button belongs on the row.
    pub fn is_up(&self) -> bool {
        matches!(self, State::Running | State::Restarting | State::Paused)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: State,
    /// The raw status word, for display where the exact wording matters.
    pub status: String,
    /// From the flat `up_time`, which DSM sends as **null** for every
    /// compose-managed container. [`Container::uptime_at`] is what the UI
    /// should ask; this on its own is blank for most of a real NAS.
    pub uptime: Option<Duration>,
    /// Unix seconds from `State.StartedAt`, which is populated when `up_time`
    /// is not.
    pub started_at: Option<u64>,
    /// The health check's verdict, when the image defines one.
    pub health: Option<ContainerHealth>,
    /// Why a stopped container stopped.
    pub exit_code: Option<i64>,
    /// It was killed for memory rather than by anyone.
    pub oom_killed: bool,
    /// True for containers DSM installed as part of a package, which should
    /// not be offered start/stop buttons — DSM manages their lifecycle.
    pub is_package: bool,
    /// Filled from `SYNO.Docker.Container.Resource`, which is a separate call.
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
}

impl Container {
    /// Read one element of `SYNO.Docker.Container`/`list`.
    pub fn from_json(v: &Value) -> Self {
        let status = v
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        // The capitalised sibling of the flat fields, and where everything
        // worth knowing about a running container actually lives.
        let state_object = v.get("State");
        let health_object = state_object.and_then(|s| s.get("Health"));

        Container {
            id: text(v, "id"),
            name: text(v, "name"),
            image: text(v, "image"),
            state: State::from_word(&status),
            status,
            // `up_time` here is a count of seconds, unlike the colon-separated
            // string `SYNO.Core.System` uses for the same idea — and it is
            // null on every compose container, hence `started_at` beside it.
            uptime: v.get("up_time").and_then(as_u64).map(Duration::from_secs),
            started_at: state_object
                .and_then(|s| s.get("StartedAt"))
                .and_then(as_unix_seconds),
            health: health_object
                .and_then(|h| h.get("Status"))
                .and_then(Value::as_str)
                .and_then(ContainerHealth::from_word),
            exit_code: state_object
                .and_then(|s| s.get("ExitCode"))
                .and_then(as_i64),
            oom_killed: state_object
                .and_then(|s| s.get("OOMKilled"))
                .and_then(as_bool)
                .unwrap_or(false),
            is_package: v.get("is_package").and_then(as_bool).unwrap_or(false),
            cpu_percent: None,
            memory_bytes: None,
        }
    }

    /// How long it has been up, given the time now.
    ///
    /// `up_time` when DSM sent one, otherwise the difference from
    /// `State.StartedAt`. The clock is a parameter rather than read here, so
    /// this stays pure and the caller owns the one impure step.
    pub fn uptime_at(&self, now_unix_seconds: u64) -> Option<Duration> {
        if let Some(uptime) = self.uptime {
            return Some(uptime);
        }
        // A container is not up if it is not running, whatever StartedAt says
        // — a stopped one keeps the time it last started.
        if !self.state.is_up() {
            return None;
        }
        let started = self.started_at?;
        Some(Duration::from_secs(
            now_unix_seconds.saturating_sub(started),
        ))
    }

    /// Read a whole `list` reply.
    pub fn list_from_json(data: &Value) -> Vec<Container> {
        data.get("containers")
            .and_then(Value::as_array)
            .map(|list| list.iter().map(Container::from_json).collect())
            .unwrap_or_default()
    }

    /// Fold in the separate resource reply, matching on name.
    pub fn apply_resources(containers: &mut [Container], data: &Value) {
        let Some(list) = data.get("containers").and_then(Value::as_array) else {
            return;
        };
        for entry in list {
            let name = text(entry, "name");
            let Some(target) = containers.iter_mut().find(|c| c.name == name) else {
                continue;
            };
            target.cpu_percent = entry.get("cpu").and_then(Value::as_f64).map(|f| f as f32);
            target.memory_bytes = entry.get("memory").and_then(as_u64);
        }
    }
}

/// A Container Manager Project — a compose file and the containers it owns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// A UUID. Every project verb takes this rather than the name, and it has
    /// to be sent quoted or DSM reads one that looks like `8ec29f37…` as a
    /// number in scientific notation.
    pub id: String,
    pub name: String,
    /// DSM sends this **upper case** — `"RUNNING"`, where a container's is
    /// `"running"`. Stored verbatim; anything showing it lower-cases first.
    pub status: String,
    /// Where the compose file lives. From `path` (`/volume1/docker/web`)
    /// rather than `share_path` (`/docker/web`), which is relative to the
    /// share and does not match anything the user typed.
    pub path: Option<String>,
    pub container_ids: Vec<String>,
}

impl Project {
    /// Read `SYNO.Docker.Project`/`list`, which answers with an object keyed
    /// by project id rather than an array.
    pub fn list_from_json(data: &Value) -> Vec<Project> {
        let Some(obj) = data.as_object() else {
            return Vec::new();
        };
        let mut out: Vec<Project> = obj
            .values()
            .filter(|v| v.is_object())
            .map(|v| Project {
                id: text(v, "id"),
                name: text(v, "name"),
                status: text(v, "status"),
                path: v
                    .get("path")
                    .or_else(|| v.get("share_path"))
                    .and_then(Value::as_str)
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned),
                container_ids: v
                    .get("containerIds")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        // The map gives no stable order; the UI wants one that does not
        // shuffle between polls.
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The containers this project owns.
    ///
    /// The link is the project's `containerIds`. A container carries no
    /// project of its own, so the lookup only runs this way — which is also
    /// why a container in no project is found by elimination rather than by
    /// asking it.
    pub fn containers_in<'a>(&self, containers: &'a [Container]) -> Vec<&'a Container> {
        containers
            .iter()
            .filter(|c| self.container_ids.contains(&c.id))
            .collect()
    }
}

/// The project that owns a container, if any.
pub fn owning_project<'a>(projects: &'a [Project], container: &Container) -> Option<&'a Project> {
    projects
        .iter()
        .find(|p| p.container_ids.contains(&container.id))
}

/// Containers no project claims: plain `docker run` containers, and the ones
/// DSM created for a package. They would otherwise vanish from a
/// project-grouped view.
pub fn unowned_containers<'a>(
    projects: &[Project],
    containers: &'a [Container],
) -> Vec<&'a Container> {
    containers
        .iter()
        .filter(|c| owning_project(projects, c).is_none())
        .collect()
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Trimmed from the live reply on the NAS.
    fn listing() -> Value {
        json!({"containers": [
            {"id": "abc123", "name": "brain-server",
             "image": "localhost:5050/brain-server:2026-08-04-2024",
             "status": "running", "up_time": 232744, "is_package": false},
            {"id": "def456", "name": "llama-embed",
             "image": "ghcr.io/ggml-org/llama.cpp:server",
             "status": "exited", "is_package": false}
        ]})
    }

    #[test]
    fn a_listing_reads_end_to_end() {
        let cs = Container::list_from_json(&listing());
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].name, "brain-server");
        assert_eq!(cs[0].state, State::Running);
        assert_eq!(cs[0].uptime, Some(Duration::from_secs(232_744)));
        assert_eq!(cs[1].state, State::Exited);
    }

    #[test]
    fn container_uptime_is_seconds_here_unlike_the_systems_colon_string() {
        // Same idea, two encodings, one API apart. Feeding this to the
        // system uptime parser yields None and a blank cell.
        let cs = Container::list_from_json(&listing());
        assert_eq!(cs[0].uptime.expect("uptime").as_secs(), 232_744);
    }

    #[test]
    fn state_decides_which_button_the_row_offers() {
        assert!(State::from_word("running").is_up());
        assert!(State::from_word("paused").is_up());
        assert!(!State::from_word("exited").is_up());
        assert!(!State::Unknown.is_up());
    }

    #[test]
    fn an_unrecognised_state_is_unknown_rather_than_running() {
        // Defaulting the other way would put a Stop button on a dead
        // container and, worse, a green dot.
        assert_eq!(State::from_word("half-way-up"), State::Unknown);
    }

    #[test]
    fn resources_are_folded_in_by_name() {
        let mut cs = Container::list_from_json(&listing());
        Container::apply_resources(
            &mut cs,
            &json!({"containers": [{"name": "brain-server", "cpu": 3.5, "memory": 104857600}]}),
        );
        assert_eq!(cs[0].cpu_percent, Some(3.5));
        assert_eq!(cs[0].memory_bytes, Some(104_857_600));
        // The one with no resource entry keeps its blanks rather than
        // inheriting its neighbour's numbers.
        assert_eq!(cs[1].cpu_percent, None);
    }

    #[test]
    fn a_resource_reply_for_a_container_we_do_not_have_is_ignored() {
        let mut cs = Container::list_from_json(&listing());
        Container::apply_resources(
            &mut cs,
            &json!({"containers": [{"name": "ghost", "cpu": 9.0}]}),
        );
        assert!(cs.iter().all(|c| c.cpu_percent.is_none()));
    }

    #[test]
    fn a_projects_location_is_the_full_path_not_the_share_relative_one() {
        // Measured: both are sent. `share_path` is "/docker/web", which is
        // not a path anyone can act on.
        let ps = Project::list_from_json(&json!({
            "55e222c4": {
                "id": "55e222c4", "name": "web", "status": "RUNNING",
                "path": "/volume1/docker/web", "share_path": "/docker/web",
                "containerIds": []
            }
        }));
        assert_eq!(ps[0].path.as_deref(), Some("/volume1/docker/web"));
        // Upper case, verbatim, unlike a container's status.
        assert_eq!(ps[0].status, "RUNNING");
    }

    #[test]
    fn projects_arrive_keyed_by_id_not_as_an_array() {
        let ps = Project::list_from_json(&json!({
            "bddfea05-8010-4dd9-a1c8-8d93867040b8": {
                "id": "bddfea05-8010-4dd9-a1c8-8d93867040b8",
                "name": "web", "status": "running",
                "share_path": "/volume1/docker/web",
                "containerIds": ["abc123"]
            },
            "8ec29f37-76bb-49c9-bf07-9b30403fed54": {
                "id": "8ec29f37-76bb-49c9-bf07-9b30403fed54",
                "name": "brain", "status": "stopped", "containerIds": []
            }
        }));
        assert_eq!(ps.len(), 2);
        // Sorted, so the list does not reshuffle on every poll.
        assert_eq!(ps[0].name, "brain");
        assert_eq!(ps[1].name, "web");
        assert_eq!(ps[1].container_ids, vec!["abc123"]);
    }

    /// One container as the NAS actually sends it, trimmed but not reshaped.
    fn compose_container() -> Value {
        json!({"containers": [{
            "id": "109e190453a5", "name": "planner-server",
            "image": "localhost:5050/planner-server:2026-08-05-8d9cd52",
            "status": "running", "is_package": false,
            // Null. This is the whole point.
            "up_time": null,
            "up_status": "Up 3 days (healthy)",
            "State": {
                "Status": "running", "Running": true, "Paused": false,
                "Restarting": false, "Dead": false, "OOMKilled": false,
                "ExitCode": 0, "Pid": 24821,
                "StartedAt": "2026-08-05T13:26:32.78343004Z",
                "FinishedAt": "0001-01-01T00:00:00Z",
                "Health": {"Status": "healthy", "FailingStreak": 0}
            }
        }]})
    }

    #[test]
    fn a_compose_container_has_an_uptime_even_though_up_time_is_null() {
        // Every container Container Manager deployed sends `up_time: null`,
        // so reading only that field left the uptime blank for the whole
        // page on a real NAS.
        let cs = Container::list_from_json(&compose_container());
        assert_eq!(cs[0].uptime, None);
        assert_eq!(cs[0].started_at, Some(1_785_936_392));

        // Three days after it started.
        let now = 1_785_936_392 + 3 * 86_400;
        assert_eq!(
            cs[0].uptime_at(now),
            Some(Duration::from_secs(3 * 86_400)),
            "should fall back to State.StartedAt"
        );
    }

    #[test]
    fn a_flat_up_time_still_wins_where_dsm_sends_one() {
        // Older replies carry it, and it needs no clock to interpret.
        let cs = Container::list_from_json(&listing());
        assert_eq!(cs[0].uptime_at(0), Some(Duration::from_secs(232_744)));
    }

    #[test]
    fn a_stopped_container_reports_no_uptime_however_recently_it_ran() {
        // StartedAt keeps the last start, so subtracting it from now would
        // report a container that died an hour ago as up for three days.
        let mut data = compose_container();
        data["containers"][0]["status"] = json!("exited");
        data["containers"][0]["State"]["Running"] = json!(false);

        let cs = Container::list_from_json(&data);
        assert_eq!(cs[0].state, State::Exited);
        assert_eq!(cs[0].uptime_at(1_785_936_392 + 3 * 86_400), None);
    }

    #[test]
    fn the_health_check_verdict_is_read_from_the_state_object() {
        let cs = Container::list_from_json(&compose_container());
        assert_eq!(cs[0].health, Some(ContainerHealth::Healthy));
        assert_eq!(cs[0].exit_code, Some(0));
        assert!(!cs[0].oom_killed);
    }

    #[test]
    fn an_image_with_no_health_check_reports_none_rather_than_unhealthy() {
        // Most images define no health check. Calling those unhealthy would
        // put a red pill on a perfectly good container.
        let cs = Container::list_from_json(&listing());
        assert_eq!(cs[0].health, None);
        assert_eq!(ContainerHealth::from_word("nonsense"), None);
    }

    #[test]
    fn an_oom_kill_is_visible_because_it_is_the_reason_it_is_down() {
        let mut data = compose_container();
        data["containers"][0]["status"] = json!("exited");
        data["containers"][0]["State"]["OOMKilled"] = json!(true);
        data["containers"][0]["State"]["ExitCode"] = json!(137);

        let cs = Container::list_from_json(&data);
        assert!(cs[0].oom_killed);
        assert_eq!(cs[0].exit_code, Some(137));
    }

    #[test]
    fn a_project_gathers_the_containers_it_owns() {
        let containers = Container::list_from_json(&listing());
        let projects = Project::list_from_json(&json!({
            "bddfea05-8010-4dd9-a1c8-8d93867040b8": {
                "id": "bddfea05-8010-4dd9-a1c8-8d93867040b8",
                "name": "brain", "status": "running",
                "share_path": "/volume1/docker/brain",
                "containerIds": ["abc123"]
            }
        }));

        let owned = projects[0].containers_in(&containers);
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].name, "brain-server");
    }

    #[test]
    fn a_container_in_no_project_is_still_reachable() {
        // A `docker run` container belongs to nothing. Grouping by project
        // without this would drop it off the page entirely.
        let containers = Container::list_from_json(&listing());
        let projects = Project::list_from_json(&json!({
            "bddfea05-8010-4dd9-a1c8-8d93867040b8": {
                "id": "bddfea05-8010-4dd9-a1c8-8d93867040b8",
                "name": "brain", "status": "running", "containerIds": ["abc123"]
            }
        }));

        let loose = unowned_containers(&projects, &containers);
        assert_eq!(loose.len(), 1);
        assert_eq!(loose[0].name, "llama-embed");
    }

    #[test]
    fn a_container_can_name_the_project_that_owns_it() {
        let containers = Container::list_from_json(&listing());
        let projects = Project::list_from_json(&json!({
            "bddfea05-8010-4dd9-a1c8-8d93867040b8": {
                "id": "bddfea05-8010-4dd9-a1c8-8d93867040b8",
                "name": "brain", "status": "running", "containerIds": ["abc123"]
            }
        }));

        assert_eq!(
            owning_project(&projects, &containers[0]).map(|p| p.name.as_str()),
            Some("brain")
        );
        assert!(owning_project(&projects, &containers[1]).is_none());
    }

    #[test]
    fn with_no_projects_at_all_every_container_is_unowned() {
        // Container Manager without a single compose project is an ordinary
        // state, not an empty page.
        let containers = Container::list_from_json(&listing());
        assert_eq!(unowned_containers(&[], &containers).len(), 2);
    }

    #[test]
    fn an_empty_or_absent_reply_yields_an_empty_list() {
        assert!(Container::list_from_json(&json!({})).is_empty());
        assert!(Project::list_from_json(&json!(null)).is_empty());
    }
}
