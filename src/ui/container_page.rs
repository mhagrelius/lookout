//! Container Manager — the tables, and the only mutating buttons in the app.
//!
//! Grouped by Container Manager Project, because that is how they were
//! deployed: a compose file owns a set of containers, and reading them as one
//! flat list loses which ones stop and start together. Containers started with
//! plain `docker run` belong to no project and get their own section.

use adw::prelude::*;
use gtk::{gio, pango};

use std::cell::RefCell;
use std::rc::Rc;

use lookout_core::model::{unowned_containers, Container, Project, State};
use lookout_core::ContainerAction;

use crate::ui::container_object::ContainerObject;
use crate::ui::widgets::{page_body, pill, section_header, section_header_parts, StatTile};

/// What the page asks the application to do. The page never touches a socket.
pub type ActionHandler = Rc<RefCell<Option<Box<dyn Fn(ContainerAction, String)>>>>;

pub struct ContainerPage {
    pub page: adw::NavigationPage,
    running_tile: StatTile,
    stopped_tile: StatTile,
    cpu_tile: StatTile,
    memory_tile: StatTile,
    /// One card per project, and the heading over them.
    projects_section: gtk::Box,
    project_box: gtk::Box,
    projects: RefCell<Vec<ProjectSection>>,
    /// Containers no project owns, hidden when there are none.
    loose: gtk::Box,
    loose_title: gtk::Label,
    loose_containers: gio::ListStore,
    handler: ActionHandler,
}

impl ContainerPage {
    pub fn new() -> Self {
        let (scroller, content) = page_body();

        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        strip.set_homogeneous(true);
        let running_tile = StatTile::new("RUNNING");
        let stopped_tile = StatTile::new("STOPPED");
        let cpu_tile = StatTile::new("CONTAINER CPU");
        let memory_tile = StatTile::new("CONTAINER MEMORY");
        for tile in [&running_tile, &stopped_tile, &cpu_tile, &memory_tile] {
            strip.append(&tile.widget);
        }
        content.append(&strip);

        let handler: ActionHandler = Rc::new(RefCell::new(None));

        // The whole projects half hides on a box that has none — a DSM
        // without the Project API, or Container Manager with nothing deployed
        // from a compose file. A "Projects" heading over empty space reads as
        // a failed call rather than an honest absence.
        let projects_section = gtk::Box::new(gtk::Orientation::Vertical, 16);
        projects_section.append(&section_header("Projects", "SYNO.Docker.Project", None));
        let project_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        projects_section.append(&project_box);
        content.append(&projects_section);

        let loose = gtk::Box::new(gtk::Orientation::Vertical, 12);
        // Titled by what is around it: "Not in a project" only means anything
        // when there are projects to not be in.
        let (loose_header, loose_title) =
            section_header_parts("Containers", "SYNO.Docker.Container", None);
        loose.append(&loose_header);
        let (loose_table, loose_containers) = container_table(handler.clone());
        loose.append(&loose_table);
        loose.set_visible(false);
        content.append(&loose);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&scroller));

        let page = adw::NavigationPage::new(&toolbar, "Container Manager");
        page.set_tag(Some("containers"));

        ContainerPage {
            page,
            running_tile,
            stopped_tile,
            cpu_tile,
            memory_tile,
            projects_section,
            project_box,
            projects: RefCell::new(Vec::new()),
            loose,
            loose_title,
            loose_containers,
            handler,
        }
    }

    /// Set what happens when a row's button is pressed.
    pub fn connect_action<F: Fn(ContainerAction, String) + 'static>(&self, f: F) {
        self.handler.replace(Some(Box::new(f)));
    }

    pub fn update(&self, containers: &[Container], projects: &[Project]) {
        let running = containers.iter().filter(|c| c.state.is_up()).count();
        let stopped = containers.len() - running;

        self.running_tile.set(&running.to_string(), "up now", false);
        self.stopped_tile.set(
            &stopped.to_string(),
            if stopped == 0 { "none" } else { "not running" },
            false,
        );

        let (cpu, memory) = totals(containers);
        self.cpu_tile.set(
            &match cpu {
                Some(c) => format!("{c:.1}%"),
                None => "—".into(),
            },
            "across all containers",
            cpu.is_some_and(|c| c >= 90.0),
        );
        self.memory_tile.set(
            &match memory {
                Some(m) => crate::ui::widgets::format_bytes(m),
                None => "—".into(),
            },
            "across all containers",
            false,
        );

        self.projects_section.set_visible(!projects.is_empty());
        self.sync_projects(containers, projects);

        let loose = unowned_containers(projects, containers);
        self.loose.set_visible(!loose.is_empty());
        self.loose_title.set_text(if projects.is_empty() {
            "Containers"
        } else {
            "Not in a project"
        });
        fill(&self.loose_containers, &loose);
    }

    /// Match the cards on screen to the projects DSM reports.
    ///
    /// Rebuilt only when the set of projects changes. A `GtkColumnView`
    /// replaced every five seconds would lose the column widths the user
    /// dragged, and — because the rows carry buttons — could swap a Stop
    /// button out from under a click.
    fn sync_projects(&self, containers: &[Container], projects: &[Project]) {
        let mut sections = self.projects.borrow_mut();

        if sections
            .iter()
            .map(|section| &section.id)
            .ne(projects.iter().map(|project| &project.id))
        {
            while let Some(child) = self.project_box.first_child() {
                self.project_box.remove(&child);
            }
            *sections = projects
                .iter()
                .map(|project| {
                    let section = ProjectSection::new(project, self.handler.clone());
                    self.project_box.append(&section.widget);
                    section
                })
                .collect();
        }

        for (section, project) in sections.iter().zip(projects) {
            section.update(project, containers);
        }
    }
}

impl Default for ContainerPage {
    fn default() -> Self {
        ContainerPage::new()
    }
}

/// One project: its compose file, its state, and the containers it owns.
struct ProjectSection {
    id: String,
    widget: gtk::Frame,
    detail: gtk::Label,
    status: gtk::Box,
    containers: gio::ListStore,
}

impl ProjectSection {
    fn new(project: &Project, handler: ActionHandler) -> Self {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.add_css_class("pool-card");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let title = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let name = gtk::Label::new(Some(&project.name));
        name.add_css_class("heading");
        name.set_xalign(0.0);
        let detail = gtk::Label::new(None);
        detail.add_css_class("caption");
        detail.add_css_class("dim-label");
        detail.add_css_class("monospace");
        detail.set_xalign(0.0);
        detail.set_ellipsize(pango::EllipsizeMode::Middle);
        title.append(&name);
        title.append(&detail);
        title.set_hexpand(true);
        header.append(&title);
        let status = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status.set_valign(gtk::Align::Center);
        header.append(&status);
        body.append(&header);

        let children = gtk::Box::new(gtk::Orientation::Vertical, 12);
        children.add_css_class("pool-children");
        let (table, containers) = container_table(handler);
        children.append(&table);
        body.append(&children);

        let widget = gtk::Frame::new(None);
        widget.add_css_class("card");
        widget.set_child(Some(&body));

        ProjectSection {
            id: project.id.clone(),
            widget,
            detail,
            status,
            containers,
        }
    }

    fn update(&self, project: &Project, containers: &[Container]) {
        let owned = project.containers_in(containers);

        // The compose file's location is the thing that identifies a project
        // on the box; the UUID DSM keys them by means nothing to anyone.
        self.detail.set_text(&match &project.path {
            Some(path) => format!("{path} · {}", count(owned.len(), "container")),
            None => count(owned.len(), "container"),
        });

        while let Some(child) = self.status.first_child() {
            self.status.remove(&child);
        }
        let (word, class) = state_pill(&project.status);
        self.status.append(&pill(&word, class));

        fill(&self.containers, &owned);
    }
}

/// A project's status word, coloured the way a container's state is.
///
/// The word is worth keeping rather than collapsing — a project can be
/// `PARTIAL`, which is neither running nor stopped and is exactly the state
/// worth noticing. But DSM sends it upper case (`"RUNNING"`, measured), so it
/// is lower-cased before being capitalised or the pill shouts.
fn state_pill(status: &str) -> (String, &'static str) {
    let class = match State::from_word(status) {
        State::Running => "success",
        State::Paused | State::Restarting => "warning",
        State::Exited => "dim-label",
        // `PARTIAL` and anything else DSM invents: not healthy, not dead.
        State::Unknown => "warning",
    };
    let lowered = status.to_lowercase();
    let mut chars = lowered.chars();
    let word = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Unknown".to_string(),
    };
    (word, class)
}

/// The container table, with its store. One per project, plus one for the
/// containers no project owns.
fn container_table(handler: ActionHandler) -> (gtk::ScrolledWindow, gio::ListStore) {
    let containers = gio::ListStore::new::<ContainerObject>();
    let selection = gtk::NoSelection::new(Some(containers.clone()));
    let table = gtk::ColumnView::new(Some(selection));
    table.add_css_class("card");
    table.set_show_row_separators(true);

    // These total 900, which is what a project card leaves once the page
    // margins, the card's padding and the nesting indent are taken out. The
    // old widths totalled more than that, so Actions — the reason this page
    // exists — sat clipped off the right edge of every card. Image takes the
    // slack when there is any, since it is the column that ellipsizes.
    text_column(&table, "Container", 165, true, |o| o.name());
    text_column(&table, "Image", 160, true, |o| o.image()).set_expand(true);
    text_column(&table, "CPU", 65, true, |o| o.cpu());
    text_column(&table, "Memory", 85, true, |o| o.memory());
    text_column(&table, "Uptime", 120, false, |o| o.uptime());
    state_column(&table);
    action_column(&table, handler);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Automatic);
    scroller.set_vscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_propagate_natural_height(true);
    scroller.set_child(Some(&table));

    (scroller, containers)
}

/// Replace a store's contents rather than the store itself, which keeps the
/// column widths and any scroll position the user had.
fn fill(store: &gio::ListStore, containers: &[&Container]) {
    store.remove_all();
    for container in containers {
        store.append(&ContainerObject::new((*container).clone()));
    }
}

/// "1 container", "3 containers".
fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// Aggregate CPU and memory across containers.
///
/// `None` when nothing reported a figure at all, which is different from
/// zero: `SYNO.Docker.Container.Resource` is a separate call, and before it
/// lands "0.0% / 0 B" would claim every container is idle rather than
/// admitting the numbers have not arrived.
fn totals(containers: &[Container]) -> (Option<f32>, Option<u64>) {
    let cpus: Vec<f32> = containers.iter().filter_map(|c| c.cpu_percent).collect();
    let memories: Vec<u64> = containers.iter().filter_map(|c| c.memory_bytes).collect();

    // `+ 0.0` is not redundant. The standard library sums floats from an
    // identity of `-0.0`, so an empty sum is negative zero and formats as
    // "-0.0%" — which is what the tile showed before this existed.
    let cpu = (!cpus.is_empty()).then(|| cpus.iter().sum::<f32>() + 0.0);
    let memory = (!memories.is_empty()).then(|| memories.iter().sum());

    (cpu, memory)
}

fn text_column<F>(
    table: &gtk::ColumnView,
    title: &str,
    width: i32,
    mono: bool,
    get: F,
) -> gtk::ColumnViewColumn
where
    F: Fn(&ContainerObject) -> String + 'static,
{
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let label = gtk::Label::new(None);
        label.add_css_class("caption");
        label.set_xalign(0.0);
        label.set_margin_top(6);
        label.set_margin_bottom(6);
        label.set_margin_start(4);
        label.set_ellipsize(pango::EllipsizeMode::Middle);
        item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let Some(label) = item.child().and_downcast::<gtk::Label>() else {
            return;
        };
        let Some(object) = item.item().and_downcast::<ContainerObject>() else {
            return;
        };
        label.set_text(&get(&object));
        // Set on bind rather than setup: a recycled row keeps whatever class
        // it had, and image names must stay monospace while uptimes must not.
        label.remove_css_class("monospace");
        if mono {
            label.add_css_class("monospace");
        }
    });

    let column = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    column.set_fixed_width(width);
    column.set_resizable(true);
    table.append_column(&column);
    column
}

/// The state pill, and under it whatever explains it.
///
/// The health check's verdict when the image defines one — measured at
/// `State.Health.Status`, and the thing that distinguishes a container that is
/// running from one that is running and answering. For a stopped container,
/// why it stopped instead.
fn state_column(table: &gtk::ColumnView) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let holder = gtk::Box::new(gtk::Orientation::Vertical, 2);
        holder.set_margin_top(6);
        holder.set_margin_bottom(6);
        holder.set_halign(gtk::Align::Start);
        item.set_child(Some(&holder));
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let (Some(holder), Some(object)) = (
            item.child().and_downcast::<gtk::Box>(),
            item.item().and_downcast::<ContainerObject>(),
        ) else {
            return;
        };
        while let Some(child) = holder.first_child() {
            holder.remove(&child);
        }
        let (word, class) = object.state_word();
        holder.append(&pill(word, class));

        // Health first: an unhealthy container is still "running", and that
        // is exactly the case where the pill alone misleads.
        let note = object
            .health_word()
            .map(|(word, class)| (word.to_string(), class))
            .or_else(|| object.exit_note().map(|note| (note, "dim-label")));

        if let Some((text, class)) = note {
            let label = gtk::Label::new(Some(&text));
            label.add_css_class("caption");
            label.add_css_class(class);
            label.set_xalign(0.0);
            label.set_ellipsize(pango::EllipsizeMode::End);
            holder.append(&label);
        }
    });

    let column = gtk::ColumnViewColumn::new(Some("State"), Some(factory));
    column.set_fixed_width(130);
    table.append_column(&column);
}

fn action_column(table: &gtk::ColumnView, handler: ActionHandler) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let holder = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        holder.set_margin_top(4);
        holder.set_margin_bottom(4);
        item.set_child(Some(&holder));
    });
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let (Some(holder), Some(object)) = (
            item.child().and_downcast::<gtk::Box>(),
            item.item().and_downcast::<ContainerObject>(),
        ) else {
            return;
        };
        while let Some(child) = holder.first_child() {
            holder.remove(&child);
        }

        // DSM owns a package container's lifecycle. Stopping one from here
        // leaves the package believing it is still running, so it gets a note
        // instead of buttons.
        if object.is_package() {
            let note = gtk::Label::new(Some("managed by DSM"));
            note.add_css_class("caption");
            note.add_css_class("dim-label");
            holder.append(&note);
            return;
        }

        let up = object.state().is_up();
        let primary = if up {
            ContainerAction::Stop
        } else {
            ContainerAction::Start
        };

        let button = gtk::Button::with_label(if up { "Stop" } else { "Start" });
        button.add_css_class("flat");
        button.connect_clicked({
            let handler = handler.clone();
            let name = object.name();
            move |_| {
                if let Some(f) = handler.borrow().as_ref() {
                    f(primary, name.clone());
                }
            }
        });
        holder.append(&button);

        if up {
            let restart = gtk::Button::with_label("Restart");
            restart.add_css_class("flat");
            restart.connect_clicked({
                let handler = handler.clone();
                let name = object.name();
                move |_| {
                    if let Some(f) = handler.borrow().as_ref() {
                        f(ContainerAction::Restart, name.clone());
                    }
                }
            });
            holder.append(&restart);
        }
    });

    let column = gtk::ColumnViewColumn::new(Some("Actions"), Some(factory));
    // Wide enough for Stop and Restart side by side: a running container
    // carries both, and at 150 the second one was cut in half.
    column.set_fixed_width(170);
    table.append_column(&column);
}

/// Ask before doing something that interrupts a running service.
pub fn confirm(
    parent: &impl IsA<gtk::Widget>,
    action: ContainerAction,
    name: &str,
    proceed: impl Fn() + 'static,
) {
    let dialog = adw::AlertDialog::new(
        Some(&format!("{} {name}?", verb(action))),
        Some(match action {
            ContainerAction::Stop => "Anything it is serving will be interrupted.",
            ContainerAction::Restart => "It will be unavailable while it restarts.",
            ContainerAction::Start => "",
        }),
    );
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("go", verb(action));
    dialog.set_response_appearance("go", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |_, response| {
        if response == "go" {
            proceed();
        }
    });
    dialog.present(Some(parent));
}

fn verb(action: ContainerAction) -> &'static str {
    match action {
        ContainerAction::Start => "Start",
        ContainerAction::Stop => "Stop",
        ContainerAction::Restart => "Restart",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lookout_core::model::State;

    fn container(cpu: Option<f32>, memory: Option<u64>) -> Container {
        Container {
            name: "c".into(),
            state: State::Running,
            cpu_percent: cpu,
            memory_bytes: memory,
            ..Container::default()
        }
    }

    #[test]
    fn totals_add_up_what_was_reported() {
        let (cpu, memory) = totals(&[
            container(Some(1.5), Some(1000)),
            container(Some(2.0), Some(2000)),
        ]);
        assert_eq!(cpu, Some(3.5));
        assert_eq!(memory, Some(3000));
    }

    #[test]
    fn an_empty_cpu_sum_is_not_negative_zero() {
        // The standard library's float Sum starts from -0.0, so the naive
        // version rendered "-0.0%" in the tile.
        let (cpu, _) = totals(&[container(Some(0.0), None)]);
        let value = cpu.expect("a figure was reported");
        assert!(
            !value.is_sign_negative(),
            "got {value}, which formats as -0.0"
        );
        assert_eq!(format!("{value:.1}"), "0.0");
    }

    #[test]
    fn nothing_reported_is_absent_rather_than_zero() {
        // Before the resource call lands, "0.0% / 0 B" would claim every
        // container is idle.
        let (cpu, memory) = totals(&[container(None, None), container(None, None)]);
        assert_eq!(cpu, None);
        assert_eq!(memory, None);
    }

    #[test]
    fn a_partial_report_still_totals_what_it_has() {
        let (cpu, memory) = totals(&[container(Some(4.0), None), container(None, Some(512))]);
        assert_eq!(cpu, Some(4.0));
        assert_eq!(memory, Some(512));
    }
}
