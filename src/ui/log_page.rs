//! Logs & security — counts, a severity filter, and the entries.
//!
//! The filter is client-side over the page already fetched, which is what the
//! design specifies: the poll asks for fifty entries, and narrowing to errors
//! should not cost a round trip.

use adw::prelude::*;
use gtk::pango;

use std::cell::RefCell;
use std::rc::Rc;

use lookout_core::model::{LogPage as Log, Severity};

use crate::ui::widgets::{action_row, page_body, pill, StatTile};

/// What the filter buttons select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Error,
    Warning,
    Info,
}

impl Filter {
    const ALL: [Filter; 4] = [Filter::All, Filter::Error, Filter::Warning, Filter::Info];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Error => "Error",
            Filter::Warning => "Warning",
            Filter::Info => "Info",
        }
    }

    /// Whether an entry passes.
    ///
    /// Note this is equality, not a floor: picking "Warning" shows warnings,
    /// not warnings-and-errors. That matches the design's four exclusive
    /// buttons — a floor would make "Info" mean "everything" and duplicate
    /// "All".
    fn admits(self, severity: Severity) -> bool {
        match self {
            Filter::All => true,
            Filter::Error => severity == Severity::Error,
            Filter::Warning => severity == Severity::Warning,
            Filter::Info => severity == Severity::Info,
        }
    }
}

pub struct LogPageView {
    pub page: adw::NavigationPage,
    total_tile: StatTile,
    error_tile: StatTile,
    warning_tile: StatTile,
    info_tile: StatTile,
    list: gtk::ListBox,
    filter: Rc<RefCell<Filter>>,
    latest: Rc<RefCell<Log>>,
}

impl LogPageView {
    pub fn new() -> Self {
        let (scroller, content) = page_body();

        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        strip.set_homogeneous(true);
        let total_tile = StatTile::new("ENTRIES");
        let error_tile = StatTile::new("ERRORS");
        let warning_tile = StatTile::new("WARNINGS");
        let info_tile = StatTile::new("INFO");
        for tile in [&total_tile, &error_tile, &warning_tile, &info_tile] {
            strip.append(&tile.widget);
        }
        content.append(&strip);

        let filter = Rc::new(RefCell::new(Filter::All));
        let latest = Rc::new(RefCell::new(Log::default()));

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        buttons.add_css_class("linked");
        controls.append(&buttons);

        let endpoint = gtk::Label::new(Some("SYNO.Core.SyslogClient.Log"));
        endpoint.add_css_class("caption");
        endpoint.add_css_class("dim-label");
        endpoint.add_css_class("monospace");
        endpoint.set_hexpand(true);
        endpoint.set_xalign(1.0);
        controls.append(&endpoint);
        content.append(&controls);

        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::None);
        content.append(&list);

        let view = LogPageView {
            page: adw::NavigationPage::new(&gtk::Box::new(gtk::Orientation::Vertical, 0), "Logs"),
            total_tile,
            error_tile,
            warning_tile,
            info_tile,
            list,
            filter,
            latest,
        };

        // The buttons need the view to exist so they can ask it to redraw,
        // so they are wired after it is built.
        let mut first: Option<gtk::ToggleButton> = None;
        for option in Filter::ALL {
            let button = gtk::ToggleButton::with_label(option.label());
            match &first {
                None => {
                    button.set_active(true);
                    first = Some(button.clone());
                }
                Some(anchor) => button.set_group(Some(anchor)),
            }
            button.connect_toggled({
                let filter = view.filter.clone();
                let list = view.list.clone();
                let latest = view.latest.clone();
                move |b| {
                    if !b.is_active() {
                        return;
                    }
                    filter.replace(option);
                    render(&list, &latest.borrow(), option);
                }
            });
            buttons.append(&button);
        }

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&scroller));
        view.page.set_child(Some(&toolbar));
        view.page.set_title("Logs & security");
        view.page.set_tag(Some("logs"));

        view
    }

    pub fn update(&self, log: &Log) {
        self.total_tile
            .set(&log.total.to_string(), "in the system log", false);
        self.error_tile.set(
            &log.error_count.to_string(),
            "on this page",
            log.error_count > 0,
        );
        self.warning_tile.set(
            &log.warning_count.to_string(),
            "on this page",
            log.warning_count > 0,
        );
        self.info_tile
            .set(&log.info_count.to_string(), "on this page", false);

        self.latest.replace(log.clone());
        render(&self.list, log, *self.filter.borrow());
    }
}

impl Default for LogPageView {
    fn default() -> Self {
        LogPageView::new()
    }
}

fn render(list: &gtk::ListBox, log: &Log, filter: Filter) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let matching: Vec<_> = log
        .entries
        .iter()
        .filter(|e| filter.admits(e.severity))
        .collect();

    if matching.is_empty() {
        // An empty result from a filter is not the same as an empty log, and
        // a blank card would not say which this is.
        let row = adw::ActionRow::new();
        row.set_title(if log.entries.is_empty() {
            "No log entries"
        } else {
            "Nothing at this severity"
        });
        row.add_css_class("dim-label");
        list.append(&row);
        return;
    }

    for entry in matching {
        // Via the shared helper, which turns Pango markup off. Log messages
        // routinely contain `&` and `<…>` — "logged in from <ip>" — and an
        // unescaped one makes the row render as nothing at all.
        // The timestamp is the trailing column, so it stays out of the
        // subtitle rather than appearing twice on the same row.
        let row = action_row(
            &entry.message,
            &format!("{} · {}", entry.category, entry.who),
        );
        row.set_title_lines(2);

        let class = match entry.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "dim-label",
        };
        let severity = pill(entry.severity.label(), class);
        severity.set_size_request(74, -1);
        row.add_prefix(&severity);

        let time = gtk::Label::new(Some(&entry.time));
        time.add_css_class("caption");
        time.add_css_class("monospace");
        time.add_css_class("dim-label");
        time.set_ellipsize(pango::EllipsizeMode::End);
        row.add_suffix(&time);

        list.append(&row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_admits_everything_and_each_other_filter_is_exclusive() {
        assert!(Filter::All.admits(Severity::Info));
        assert!(Filter::All.admits(Severity::Error));

        assert!(Filter::Error.admits(Severity::Error));
        assert!(!Filter::Error.admits(Severity::Warning));
        // Not a floor: Info means info, or it would duplicate All.
        assert!(!Filter::Info.admits(Severity::Error));
    }
}
