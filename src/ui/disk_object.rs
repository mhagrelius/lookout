//! One drive, as a `GObject`, so a `GtkColumnView` can hold a list of them.
//!
//! `GtkColumnView` needs a `GListModel`, and a `GListModel` needs `GObject`
//! items — so a plain [`Disk`] cannot go in one. This is the thin wrapper that
//! makes it possible, and it is the pattern every other table page reuses.

use gtk::glib;
use gtk::subclass::prelude::*;

use lookout_core::model::{Disk, Health};

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct DiskObject {
        pub disk: RefCell<Disk>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DiskObject {
        const NAME: &'static str = "LookoutDiskObject";
        type Type = super::DiskObject;
    }

    impl ObjectImpl for DiskObject {}
}

glib::wrapper! {
    pub struct DiskObject(ObjectSubclass<imp::DiskObject>);
}

impl DiskObject {
    pub fn new(disk: Disk) -> Self {
        let object: Self = glib::Object::new();
        object.imp().disk.replace(disk);
        object
    }

    pub fn disk(&self) -> Disk {
        self.imp().disk.borrow().clone()
    }

    /// The bay label, `Drive 1`.
    pub fn bay(&self) -> String {
        self.imp().disk.borrow().name.clone()
    }

    /// Model over serial, as two lines.
    pub fn identity(&self) -> String {
        let disk = self.imp().disk.borrow();
        match (&disk.model, &disk.serial) {
            (Some(model), Some(serial)) => format!("{model}\n{serial}"),
            (Some(model), None) => model.clone(),
            _ => disk.id.clone(),
        }
    }

    pub fn allocation(&self) -> String {
        let disk = self.imp().disk.borrow();
        disk.used_by.clone().unwrap_or_else(|| "Unused".into())
    }

    pub fn temperature(&self) -> String {
        self.imp()
            .disk
            .borrow()
            .temperature_c
            .map(|t| format!("{t} °C"))
            .unwrap_or_else(|| "—".into())
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.imp().disk.borrow().size_bytes
    }

    pub fn is_hot(&self) -> bool {
        self.imp().disk.borrow().is_hot()
    }

    /// The drive's S.M.A.R.T. verdict.
    ///
    /// Named for the field it returns rather than `health`, which would read
    /// as the overall drive status — a different field that can disagree.
    pub fn smart_health(&self) -> Health {
        self.imp().disk.borrow().smart_health
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk() -> Disk {
        Disk {
            id: "sda".into(),
            name: "Drive 1".into(),
            model: Some("ST10000VN0008-2PJ103".into()),
            serial: Some("ZHZ0ABCD".into()),
            temperature_c: Some(47),
            size_bytes: 10_000_831_348_736,
            used_by: Some("reuse_1".into()),
            smart_health: Health::Normal,
            ..Disk::default()
        }
    }

    #[test]
    fn a_wrapped_disk_reads_back_unchanged() {
        // Needs no display: this is a plain GObject, not a widget.
        let object = DiskObject::new(disk());
        assert_eq!(object.bay(), "Drive 1");
        assert_eq!(object.capacity_bytes(), 10_000_831_348_736);
        assert_eq!(object.temperature(), "47 °C");
        assert!(object.is_hot());
        assert_eq!(object.disk().id, "sda");
    }

    #[test]
    fn identity_falls_back_when_dsm_omits_the_serial() {
        let mut d = disk();
        d.serial = None;
        assert_eq!(DiskObject::new(d).identity(), "ST10000VN0008-2PJ103");

        let bare = Disk {
            id: "sdz".into(),
            ..Disk::default()
        };
        assert_eq!(DiskObject::new(bare).identity(), "sdz");
    }

    #[test]
    fn an_unallocated_drive_says_so_rather_than_being_blank() {
        let mut d = disk();
        d.used_by = None;
        assert_eq!(DiskObject::new(d).allocation(), "Unused");
    }

    #[test]
    fn a_drive_with_no_temperature_sensor_shows_a_dash_not_zero() {
        let mut d = disk();
        d.temperature_c = None;
        let object = DiskObject::new(d);
        assert_eq!(object.temperature(), "—");
        assert!(!object.is_hot());
    }
}
