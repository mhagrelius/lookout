//! One container, as a `GObject`, for the Container Manager table.

use gtk::glib;
use gtk::subclass::prelude::*;

use lookout_core::model::{Container, ContainerHealth, State};

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct ContainerObject {
        pub container: RefCell<Container>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ContainerObject {
        const NAME: &'static str = "LookoutContainerObject";
        type Type = super::ContainerObject;
    }

    impl ObjectImpl for ContainerObject {}
}

glib::wrapper! {
    pub struct ContainerObject(ObjectSubclass<imp::ContainerObject>);
}

impl ContainerObject {
    pub fn new(container: Container) -> Self {
        let object: Self = glib::Object::new();
        object.imp().container.replace(container);
        object
    }

    pub fn name(&self) -> String {
        self.imp().container.borrow().name.clone()
    }

    pub fn image(&self) -> String {
        self.imp().container.borrow().image.clone()
    }

    pub fn state(&self) -> State {
        self.imp().container.borrow().state
    }

    /// Whether DSM installed this as part of a package.
    ///
    /// Those get no buttons: DSM owns their lifecycle, and stopping one from
    /// here leaves the package thinking it is still running.
    pub fn is_package(&self) -> bool {
        self.imp().container.borrow().is_package
    }

    pub fn cpu(&self) -> String {
        self.imp()
            .container
            .borrow()
            .cpu_percent
            .map(|c| format!("{c:.1}%"))
            .unwrap_or_else(|| "—".into())
    }

    pub fn memory(&self) -> String {
        self.imp()
            .container
            .borrow()
            .memory_bytes
            .map(crate::ui::widgets::format_bytes)
            .unwrap_or_else(|| "—".into())
    }

    /// How long it has been up, read against the clock now.
    ///
    /// This is the one place the wall clock enters the container display: DSM
    /// sends `up_time: null` for every compose container, so the figure has to
    /// be derived from `State.StartedAt` and is only meaningful relative to
    /// now.
    pub fn uptime(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match self.imp().container.borrow().uptime_at(now) {
            Some(d) => lookout_core::model::format_uptime(d),
            None => "—".into(),
        }
    }

    /// The health check's verdict and the class to colour it, when the image
    /// defines one at all.
    pub fn health_word(&self) -> Option<(&'static str, &'static str)> {
        match self.imp().container.borrow().health? {
            ContainerHealth::Healthy => Some(("Healthy", "success")),
            ContainerHealth::Unhealthy => Some(("Unhealthy", "error")),
            ContainerHealth::Starting => Some(("Starting", "warning")),
        }
    }

    /// Why a stopped container stopped, when it says anything worth reading.
    ///
    /// Nothing for a clean exit: "exited 0" beside an Exited pill is noise.
    pub fn exit_note(&self) -> Option<String> {
        let container = self.imp().container.borrow();
        if container.state.is_up() {
            return None;
        }
        if container.oom_killed {
            return Some("out of memory".into());
        }
        match container.exit_code {
            Some(0) | None => None,
            Some(code) => Some(format!("exit {code}")),
        }
    }

    pub fn state_word(&self) -> (&'static str, &'static str) {
        match self.state() {
            State::Running => ("Running", "success"),
            State::Paused => ("Paused", "warning"),
            State::Restarting => ("Restarting", "warning"),
            State::Exited => ("Exited", "dim-label"),
            State::Unknown => ("Unknown", "dim-label"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn container() -> Container {
        Container {
            id: "abc".into(),
            name: "brain-server".into(),
            image: "localhost:5050/brain-server:2026-08-04".into(),
            state: State::Running,
            status: "running".into(),
            uptime: Some(Duration::from_secs(232_744)),
            is_package: false,
            cpu_percent: Some(3.5),
            memory_bytes: Some(104_857_600),
            ..Container::default()
        }
    }

    #[test]
    fn a_wrapped_container_reads_back_unchanged() {
        let object = ContainerObject::new(container());
        assert_eq!(object.name(), "brain-server");
        assert_eq!(object.cpu(), "3.5%");
        assert_eq!(object.memory(), "105 MB");
        assert_eq!(object.uptime(), "2 days, 16 hours");
        assert_eq!(object.state_word(), ("Running", "success"));
    }

    #[test]
    fn a_container_with_no_resource_reading_shows_dashes_not_zeroes() {
        // Zero would read as "idle", which is a different claim from "the
        // resource call has not come back".
        let mut c = container();
        c.cpu_percent = None;
        c.memory_bytes = None;
        c.uptime = None;
        let object = ContainerObject::new(c);
        assert_eq!(object.cpu(), "—");
        assert_eq!(object.memory(), "—");
        assert_eq!(object.uptime(), "—");
    }

    #[test]
    fn a_health_check_verdict_shows_only_when_the_image_defines_one() {
        assert_eq!(ContainerObject::new(container()).health_word(), None);

        let mut healthy = container();
        healthy.health = Some(ContainerHealth::Healthy);
        assert_eq!(
            ContainerObject::new(healthy).health_word(),
            Some(("Healthy", "success"))
        );

        // The case the state pill alone gets wrong: still "Running".
        let mut sick = container();
        sick.health = Some(ContainerHealth::Unhealthy);
        let object = ContainerObject::new(sick);
        assert_eq!(object.state_word(), ("Running", "success"));
        assert_eq!(object.health_word(), Some(("Unhealthy", "error")));
    }

    #[test]
    fn a_stopped_container_says_why_unless_it_exited_cleanly() {
        // A clean exit needs no note; "exit 0" beside an Exited pill is noise.
        let mut clean = container();
        clean.state = State::Exited;
        clean.exit_code = Some(0);
        assert_eq!(ContainerObject::new(clean).exit_note(), None);

        let mut crashed = container();
        crashed.state = State::Exited;
        crashed.exit_code = Some(137);
        assert_eq!(
            ContainerObject::new(crashed).exit_note().as_deref(),
            Some("exit 137")
        );

        // Out of memory is the reason, not the code it produced.
        let mut oom = container();
        oom.state = State::Exited;
        oom.exit_code = Some(137);
        oom.oom_killed = true;
        assert_eq!(
            ContainerObject::new(oom).exit_note().as_deref(),
            Some("out of memory")
        );

        // A running container explains nothing.
        assert_eq!(ContainerObject::new(container()).exit_note(), None);
    }

    #[test]
    fn a_package_container_is_flagged_so_it_gets_no_buttons() {
        let mut c = container();
        c.is_package = true;
        assert!(ContainerObject::new(c).is_package());
        assert!(!ContainerObject::new(container()).is_package());
    }
}
