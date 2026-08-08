//! Pools & drives — the first drill-in page.
//!
//! Establishes the pattern the remaining detail pages follow: an
//! `AdwNavigationPage` holding a stats strip and one or more `GtkColumnView`
//! tables over a `gio::ListStore` of `GObject` row items.
//!
//! Grouped by pool, the way the Overview is and the way DSM's own model is: a
//! volume sits on a pool and a pool is made of drives. The flat "every volume,
//! then every drive" this page used to show made the Allocation column carry
//! the whole relationship, which stopped being legible past one pool.

use std::cell::RefCell;

use adw::prelude::*;
use gtk::{gio, pango};

use lookout_core::model::{Disk, Pool, Storage};

use crate::ui::disk_object::DiskObject;
use crate::ui::widgets::{
    boxed_list, format_bytes, health_class, health_word, page_body, pill, section_header,
    volume_row, StatTile,
};

pub struct StoragePage {
    pub page: adw::NavigationPage,
    raw_tile: StatTile,
    usable_tile: StatTile,
    used_tile: StatTile,
    drives_tile: StatTile,
    /// The per-pool cards live here, one per pool.
    pool_box: gtk::Box,
    pools: RefCell<Vec<PoolSection>>,
    /// The unallocated-drives section, hidden when every drive is in a pool —
    /// which is the normal state of a DiskStation, so it is usually not shown.
    loose: gtk::Box,
    loose_drives: gio::ListStore,
}

impl StoragePage {
    pub fn new() -> Self {
        let (scroller, content) = page_body();

        // --- stats strip --------------------------------------------------
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        strip.set_homogeneous(true);
        let raw_tile = StatTile::new("RAW CAPACITY");
        let usable_tile = StatTile::new("USABLE");
        let used_tile = StatTile::new("USED");
        let drives_tile = StatTile::new("DRIVES");
        for tile in [&raw_tile, &usable_tile, &used_tile, &drives_tile] {
            strip.append(&tile.widget);
        }
        content.append(&strip);

        // --- one card per pool ----------------------------------------------
        content.append(&section_header(
            "Storage pools",
            "SYNO.Storage.CGI.Storage",
            None,
        ));
        let pool_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.append(&pool_box);

        // --- drives in no pool ----------------------------------------------
        let loose = gtk::Box::new(gtk::Orientation::Vertical, 12);
        loose.append(&section_header(
            "Unassigned drives",
            "SYNO.Storage.CGI.Storage",
            None,
        ));
        let (loose_table, loose_drives) = drive_table();
        loose.append(&loose_table);
        loose.set_visible(false);
        content.append(&loose);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&scroller));

        let page = adw::NavigationPage::new(&toolbar, "Pools & drives");
        page.set_tag(Some("storage"));

        StoragePage {
            page,
            raw_tile,
            usable_tile,
            used_tile,
            drives_tile,
            pool_box,
            pools: RefCell::new(Vec::new()),
            loose,
            loose_drives,
        }
    }

    pub fn update(&self, storage: &Storage) {
        let raw: u64 = storage.disks.iter().map(|d| d.size_bytes).sum();
        let usable: u64 = storage.volumes.iter().map(|v| v.total_bytes).sum();
        let used: u64 = storage.volumes.iter().map(|v| v.used_bytes).sum();
        let fraction = if usable == 0 {
            0.0
        } else {
            used as f64 / usable as f64
        };

        self.raw_tile.set(
            &format_bytes(raw),
            &count(storage.disks.len(), "drive"),
            false,
        );
        self.usable_tile.set(
            &format_bytes(usable),
            &count(storage.volumes.len(), "volume"),
            false,
        );
        self.used_tile.set(
            &format!("{:.0}%", fraction * 100.0),
            &format!("{} free", format_bytes(usable.saturating_sub(used))),
            fraction >= 0.80,
        );

        let hot = storage.disks.iter().filter(|d| d.is_hot()).count();
        self.drives_tile.set(
            &storage.disks.len().to_string(),
            &if hot > 0 {
                format!("{hot} running warm")
            } else {
                "all nominal".into()
            },
            hot > 0,
        );

        self.sync_pools(storage);

        let loose: Vec<&Disk> = storage.unassigned_disks();
        self.loose.set_visible(!loose.is_empty());
        fill(&self.loose_drives, &loose);
    }

    /// Match the cards on screen to the pools DSM reports.
    ///
    /// Rebuilt only when the set of pools changes, which on a NAS is close to
    /// never: a `GtkColumnView` thrown away and rebuilt every five seconds
    /// would lose the column widths the user dragged and their scroll
    /// position, and pools do not come and go the way rows in them do.
    fn sync_pools(&self, storage: &Storage) {
        let mut sections = self.pools.borrow_mut();

        if sections
            .iter()
            .map(|section| &section.id)
            .ne(storage.pools.iter().map(|pool| &pool.id))
        {
            while let Some(child) = self.pool_box.first_child() {
                self.pool_box.remove(&child);
            }
            *sections = storage
                .pools
                .iter()
                .map(|pool| {
                    let section = PoolSection::new(&pool.id);
                    self.pool_box.append(&section.widget);
                    section
                })
                .collect();
        }

        for (section, pool) in sections.iter().zip(&storage.pools) {
            section.update(storage, pool);
        }
    }
}

impl Default for StoragePage {
    fn default() -> Self {
        StoragePage::new()
    }
}

/// One pool: its own line, the volumes on it, and the drives under it.
struct PoolSection {
    id: String,
    widget: gtk::Frame,
    detail: gtk::Label,
    status: gtk::Box,
    volumes: gtk::ListBox,
    table: gtk::ScrolledWindow,
    drives: gio::ListStore,
}

impl PoolSection {
    fn new(id: &str) -> Self {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.add_css_class("pool-card");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let title = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let name = gtk::Label::new(Some(&format!("Storage Pool {id}")));
        name.add_css_class("heading");
        name.set_xalign(0.0);
        let detail = gtk::Label::new(None);
        detail.add_css_class("caption");
        detail.add_css_class("dim-label");
        detail.set_xalign(0.0);
        title.append(&name);
        title.append(&detail);
        title.set_hexpand(true);
        header.append(&title);
        // The pill is replaced rather than relabelled: its colour is a style
        // class, and adding a class on every poll accumulates them.
        let status = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status.set_valign(gtk::Align::Center);
        header.append(&status);
        body.append(&header);

        let children = gtk::Box::new(gtk::Orientation::Vertical, 12);
        children.add_css_class("pool-children");
        let volumes = boxed_list();
        children.append(&volumes);
        let (table, drives) = drive_table();
        children.append(&table);
        body.append(&children);

        let widget = gtk::Frame::new(None);
        widget.add_css_class("card");
        widget.set_child(Some(&body));

        PoolSection {
            id: id.to_string(),
            widget,
            detail,
            status,
            volumes,
            table,
            drives,
        }
    }

    fn update(&self, storage: &Storage, pool: &Pool) {
        let disks = storage.disks_in(pool);

        self.detail.set_text(&format!(
            "{} · {} · {}",
            pool.raid_type.as_deref().unwrap_or("unknown layout"),
            format_bytes(pool.total_bytes),
            count(disks.len(), "drive"),
        ));

        while let Some(child) = self.status.first_child() {
            self.status.remove(&child);
        }
        self.status
            .append(&pill(health_word(pool.health), health_class(pool.health)));

        let volumes = storage.volumes_in(pool);
        while let Some(child) = self.volumes.first_child() {
            self.volumes.remove(&child);
        }
        for volume in &volumes {
            self.volumes.append(&volume_row(volume));
        }
        // An empty boxed list still draws its border, so a pool with no
        // volumes on it would show an empty card edge.
        self.volumes.set_visible(!volumes.is_empty());

        self.table.set_visible(!disks.is_empty());
        fill(&self.drives, &disks);
    }
}

/// The per-drive table, with its store.
///
/// No Allocation column: which pool a drive belongs to is now the section it
/// is in, and repeating `reuse_1` down a card headed "Storage Pool reuse_1"
/// says nothing. The unassigned table drops it for the same reason.
fn drive_table() -> (gtk::ScrolledWindow, gio::ListStore) {
    let drives = gio::ListStore::new::<DiskObject>();
    let selection = gtk::NoSelection::new(Some(drives.clone()));
    let table = gtk::ColumnView::new(Some(selection));
    table.add_css_class("card");
    table.set_show_row_separators(true);

    column(&table, "Bay", 76, |object| {
        let label = mono(&object.bay());
        label.remove_css_class("monospace");
        label
    });
    // The one column allowed to grow, so the table fills the card instead of
    // stopping short of it and leaving a gutter the width of the column the
    // pool grouping replaced.
    column(&table, "Model / serial", 260, |object| {
        let label = mono(&object.identity());
        label.set_ellipsize(pango::EllipsizeMode::Middle);
        label
    })
    .set_expand(true);
    column(&table, "Capacity", 100, |object| {
        mono(&format_bytes(object.capacity_bytes()))
    });
    column(&table, "Temp", 84, |object| {
        let label = mono(&object.temperature());
        // The handoff colours this at 45 °C, which is a display threshold
        // rather than a manufacturer limit.
        if object.is_hot() {
            label.add_css_class("warning");
        }
        label
    });

    // The S.M.A.R.T. column is a pill rather than text, so it builds
    // differently from the others.
    let smart = gtk::SignalListItemFactory::new();
    smart.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        holder.set_margin_top(6);
        holder.set_margin_bottom(6);
        item.set_child(Some(&holder));
    });
    smart.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let Some(holder) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(object) = item.item().and_downcast::<DiskObject>() else {
            return;
        };
        while let Some(child) = holder.first_child() {
            holder.remove(&child);
        }
        let health = object.smart_health();
        holder.append(&pill(health_word(health), health_class(health)));
    });
    let smart_column = gtk::ColumnViewColumn::new(Some("S.M.A.R.T."), Some(smart));
    smart_column.set_fixed_width(108);
    table.append_column(&smart_column);

    let scroller = gtk::ScrolledWindow::new();
    // Wide tables scroll sideways rather than clipping when the window is
    // narrowed, which is the design's rule for every table page.
    scroller.set_hscrollbar_policy(gtk::PolicyType::Automatic);
    scroller.set_vscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_propagate_natural_height(true);
    scroller.set_child(Some(&table));

    (scroller, drives)
}

/// Replace a store's contents rather than the store itself, which keeps the
/// column widths and any scroll position the user had.
fn fill(store: &gio::ListStore, disks: &[&Disk]) {
    store.remove_all();
    for disk in disks {
        store.append(&DiskObject::new((*disk).clone()));
    }
}

/// A text column bound from a `DiskObject`.
fn column<F>(table: &gtk::ColumnView, title: &str, width: i32, bind: F) -> gtk::ColumnViewColumn
where
    F: Fn(&DiskObject) -> gtk::Label + 'static,
{
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        item.set_child(Some(&holder));
    });
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let Some(holder) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(object) = item.item().and_downcast::<DiskObject>() else {
            return;
        };
        // Rebuilt on bind because a recycled row would otherwise keep the
        // previous drive's `.warning` class on a cool drive.
        while let Some(child) = holder.first_child() {
            holder.remove(&child);
        }
        holder.append(&bind(&object));
    });

    let column = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    column.set_fixed_width(width);
    column.set_resizable(true);
    table.append_column(&column);
    column
}

/// "1 volume", "3 volumes". A single-volume DiskStation is the common case, so
/// "1 volumes" would be on screen for most people who ever open this page.
fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

fn mono(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("monospace");
    label.add_css_class("caption");
    label.set_xalign(0.0);
    label.set_margin_top(6);
    label.set_margin_bottom(6);
    label.set_margin_start(4);
    label.set_margin_end(4);
    label
}
