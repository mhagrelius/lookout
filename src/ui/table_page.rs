//! The template the handoff's seven generic detail pages share.
//!
//! A four-tile stats strip followed by one to three titled tables. Built once
//! as one widget driven by data rather than as seven near-identical files —
//! which is what the handoff describes ("seven pages share one template") and
//! what stops the fourth copy drifting from the first.
//!
//! Rows here are `AdwActionRow`s in an `AdwPreferencesGroup`, which the
//! handoff explicitly allows for key/value and narrow tables. The wide ones
//! that need real columns use `GtkColumnView` and get their own file.

use adw::prelude::*;

use crate::ui::widgets::{action_row, boxed_list, page_body, pill, StatTile};

/// One row of a table.
pub struct Row {
    pub title: String,
    pub subtitle: String,
    /// An optional trailing pill: text and Adwaita style class.
    pub badge: Option<(String, &'static str)>,
}

impl Row {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Row {
            title: title.into(),
            subtitle: subtitle.into(),
            badge: None,
        }
    }

    pub fn badge(mut self, text: impl Into<String>, class: &'static str) -> Self {
        self.badge = Some((text.into(), class));
        self
    }
}

/// One tile in the strip.
pub struct Stat {
    pub value: String,
    pub note: String,
    pub warn: bool,
}

impl Stat {
    pub fn new(value: impl Into<String>, note: impl Into<String>) -> Self {
        Stat {
            value: value.into(),
            note: note.into(),
            warn: false,
        }
    }

    pub fn warn(mut self, warn: bool) -> Self {
        self.warn = warn;
        self
    }
}

/// A table: a heading, the API it came from, and its rows.
pub struct Section {
    pub heading: &'static str,
    pub api: &'static str,
    list: gtk::ListBox,
}

pub struct TablePage {
    pub page: adw::NavigationPage,
    tiles: Vec<StatTile>,
    sections: Vec<Section>,
}

impl TablePage {
    /// Build a page with a named stat tile per caption and a table per
    /// `(heading, api)`.
    pub fn new(
        title: &str,
        tag: &str,
        tile_captions: &[&str],
        sections: &[(&'static str, &'static str)],
    ) -> Self {
        let (scroller, content) = page_body();

        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        strip.set_homogeneous(true);
        let tiles: Vec<StatTile> = tile_captions
            .iter()
            .map(|caption| {
                let tile = StatTile::new(caption);
                strip.append(&tile.widget);
                tile
            })
            .collect();
        if !tiles.is_empty() {
            content.append(&strip);
        }

        let sections: Vec<Section> = sections
            .iter()
            .map(|(heading, api)| {
                content.append(&crate::ui::widgets::section_header(heading, api, None));
                let list = boxed_list();
                content.append(&list);
                Section { heading, api, list }
            })
            .collect();

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&scroller));

        let page = adw::NavigationPage::new(&toolbar, title);
        page.set_tag(Some(tag));

        TablePage {
            page,
            tiles,
            sections,
        }
    }

    /// Set the stat strip. Extra stats past the tile count are ignored rather
    /// than panicking — a caller adding a fifth should see four, not a crash.
    pub fn set_stats(&self, stats: &[Stat]) {
        for (tile, stat) in self.tiles.iter().zip(stats) {
            tile.set(&stat.value, &stat.note, stat.warn);
        }
    }

    /// Replace one table's rows.
    pub fn set_rows(&self, index: usize, rows: Vec<Row>) {
        let Some(section) = self.sections.get(index) else {
            return;
        };
        while let Some(child) = section.list.first_child() {
            section.list.remove(&child);
        }

        if rows.is_empty() {
            let empty = action_row("Nothing to show", "");
            empty.add_css_class("dim-label");
            section.list.append(&empty);
            return;
        }

        for row in rows {
            let widget = action_row(&row.title, &row.subtitle);
            if let Some((text, class)) = &row.badge {
                widget.add_suffix(&pill(text, class));
            }
            section.list.append(&widget);
        }
    }

    /// Mark a table as unavailable, which is not the same as empty.
    pub fn set_unavailable(&self, index: usize) {
        let Some(section) = self.sections.get(index) else {
            return;
        };
        while let Some(child) = section.list.first_child() {
            section.list.remove(&child);
        }
        let row = action_row(
            section.heading,
            "Not available from this DiskStation right now",
        );
        row.add_css_class("dim-label");
        section.list.append(&row);
    }
}
