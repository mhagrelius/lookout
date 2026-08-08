//! Resource monitor — four full-width charts over a selectable range.
//!
//! This is the page the trend store exists for. DSM serves no history, so
//! every point drawn here was recorded by the app; the range toggle picks
//! which of the four tiers to read.

use adw::prelude::*;

use lookout_core::model::Utilization;
use lookout_core::{Range, Sample, Trends};

use crate::ui::chart::{Axis, Chart, Series};
use crate::ui::palette;
use crate::ui::widgets::{format_bytes, format_memory_kb, format_rate, page_body, StatTile};

/// One chart card: title, legend, current value, plot, and time ticks.
struct ChartCard {
    chart: Chart,
    current: gtk::Label,
    ticks: gtk::Box,
}

impl ChartCard {
    fn new(title: &str, legend: &str, axis: Axis) -> (gtk::Frame, ChartCard) {
        let boxed = gtk::Box::new(gtk::Orientation::Vertical, 8);
        boxed.set_margin_top(14);
        boxed.set_margin_bottom(14);
        boxed.set_margin_start(18);
        boxed.set_margin_end(18);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title_label = gtk::Label::new(Some(title));
        title_label.add_css_class("heading");
        title_label.set_xalign(0.0);
        header.append(&title_label);

        let legend_label = gtk::Label::new(Some(legend));
        legend_label.add_css_class("caption");
        legend_label.add_css_class("dim-label");
        legend_label.set_hexpand(true);
        legend_label.set_xalign(0.0);
        header.append(&legend_label);

        let current = gtk::Label::new(Some("—"));
        current.add_css_class("numeric");
        current.add_css_class("accent");
        header.append(&current);
        boxed.append(&header);

        let chart = Chart::new(axis, 132);
        boxed.append(&chart.area);

        // Five evenly spaced ticks ending in "now", per the design.
        let ticks = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        ticks.set_homogeneous(true);
        boxed.append(&ticks);

        let frame = gtk::Frame::new(None);
        frame.add_css_class("card");
        frame.set_child(Some(&boxed));

        (
            frame,
            ChartCard {
                chart,
                current,
                ticks,
            },
        )
    }

    fn set_ticks(&self, range: Range) {
        while let Some(child) = self.ticks.first_child() {
            self.ticks.remove(&child);
        }
        for (i, label) in tick_labels(range).iter().enumerate() {
            let tick = gtk::Label::new(Some(label));
            tick.add_css_class("caption");
            tick.add_css_class("dim-label");
            tick.add_css_class("monospace");
            // First left-aligned, last right-aligned, so the row spans the
            // plot rather than floating inside it.
            tick.set_xalign(match i {
                0 => 0.0,
                4 => 1.0,
                _ => 0.5,
            });
            self.ticks.append(&tick);
        }
    }
}

/// The five tick labels for a range, oldest first, ending in "now".
fn tick_labels(range: Range) -> [String; 5] {
    let total = range.span().num_minutes();
    // The boundaries are inclusive so the leftmost tick reads in the same
    // unit as the range's own label: the 24 h range starts at "-24h", not
    // "-1d", and the 1 h range at "-60m", not "-1h". An exclusive `<` puts
    // each range's first tick in the next unit up, which reads as a mismatch
    // against the button the user just pressed.
    let label = |minutes_ago: i64| {
        if minutes_ago == 0 {
            "now".to_string()
        } else if minutes_ago <= 60 {
            format!("-{minutes_ago}m")
        } else if minutes_ago <= 60 * 24 {
            format!("-{}h", minutes_ago / 60)
        } else {
            format!("-{}d", minutes_ago / (60 * 24))
        }
    };
    [
        label(total),
        label(total * 3 / 4),
        label(total / 2),
        label(total / 4),
        label(0),
    ]
}

pub struct ResourcePage {
    pub page: adw::NavigationPage,
    toggle: adw::ToggleGroup,
    cpu: ChartCard,
    network: ChartCard,
    disk: ChartCard,
    temperature: ChartCard,
    load_tile: StatTile,
    memory_tile: StatTile,
    transferred_tile: StatTile,
    samples_tile: StatTile,
}

impl ResourcePage {
    pub fn new() -> Self {
        let (scroller, content) = page_body();

        // --- range switcher -----------------------------------------------
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let endpoint = gtk::Label::new(Some("SYNO.Core.System.Utilization · recorded locally"));
        endpoint.add_css_class("caption");
        endpoint.add_css_class("dim-label");
        endpoint.add_css_class("monospace");
        endpoint.set_hexpand(true);
        endpoint.set_xalign(0.0);
        top.append(&endpoint);

        let toggle = adw::ToggleGroup::new();
        for range in Range::ALL {
            let item = adw::Toggle::new();
            item.set_label(Some(range.label()));
            toggle.add(item);
        }
        toggle.set_active(0);
        top.append(&toggle);
        content.append(&top);

        // --- charts --------------------------------------------------------
        let (cpu_card, cpu) =
            ChartCard::new("CPU utilization", "user + system · memory", Axis::Percent);
        let (net_card, network) = ChartCard::new("Network throughput", "rx · tx", Axis::Bytes);
        let (disk_card, disk) = ChartCard::new("Disk utilization", "busy %", Axis::Percent);
        let (temp_card, temperature) = ChartCard::new(
            "Temperature",
            "system",
            Axis::Celsius {
                min: 20.0,
                max: 70.0,
            },
        );
        for card in [&cpu_card, &net_card, &disk_card, &temp_card] {
            content.append(card);
        }

        // --- stat tiles -----------------------------------------------------
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        strip.set_homogeneous(true);
        let load_tile = StatTile::new("LOAD AVERAGE");
        let memory_tile = StatTile::new("MEMORY");
        let transferred_tile = StatTile::new("TRANSFERRED");
        let samples_tile = StatTile::new("SAMPLES");
        for tile in [&load_tile, &memory_tile, &transferred_tile, &samples_tile] {
            strip.append(&tile.widget);
        }
        content.append(&strip);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&scroller));

        let page = adw::NavigationPage::new(&toolbar, "Resource monitor");
        page.set_tag(Some("resources"));

        ResourcePage {
            page,
            toggle,
            cpu,
            network,
            disk,
            temperature,
            load_tile,
            memory_tile,
            transferred_tile,
            samples_tile,
        }
    }

    /// The range the toggle currently selects.
    pub fn range(&self) -> Range {
        Range::ALL
            .get(self.toggle.active() as usize)
            .copied()
            .unwrap_or(Range::Hour)
    }

    pub fn set_range(&self, range: Range) {
        let index = Range::ALL.iter().position(|r| *r == range).unwrap_or(0);
        self.toggle.set_active(index as u32);
    }

    /// Run `f` when the user picks a different range.
    pub fn connect_range_changed<F: Fn(Range) + 'static>(&self, f: F) {
        self.toggle.connect_active_notify(move |toggle| {
            let range = Range::ALL
                .get(toggle.active() as usize)
                .copied()
                .unwrap_or(Range::Hour);
            f(range);
        });
    }

    pub fn update(&self, trends: &Trends, utilization: Option<&Utilization>, range: Range) {
        let samples = trends.window(range, chrono::Utc::now());
        let colors = palette();

        for card in [&self.cpu, &self.network, &self.disk, &self.temperature] {
            card.set_ticks(range);
        }

        let cpu: Vec<f64> = samples.iter().map(|s| s.cpu_percent as f64).collect();
        let memory: Vec<f64> = samples.iter().map(|s| s.memory_percent as f64).collect();
        self.cpu.current.set_text(&match cpu.last() {
            Some(v) => format!("{v:.0}%"),
            None => "—".into(),
        });
        self.cpu.chart.set_series(vec![
            Series {
                values: cpu,
                color: colors.accent,
            },
            Series {
                values: memory,
                color: colors.dim,
            },
        ]);

        let rx: Vec<f64> = samples.iter().map(|s| s.network_rx as f64).collect();
        let tx: Vec<f64> = samples.iter().map(|s| s.network_tx as f64).collect();
        self.network
            .current
            .set_text(&match (rx.last(), tx.last()) {
                (Some(r), Some(t)) => {
                    format!("{} ↓ {} ↑", format_rate(*r as u64), format_rate(*t as u64))
                }
                _ => "—".into(),
            });
        self.network.chart.set_series(vec![
            Series {
                values: rx,
                color: colors.accent,
            },
            Series {
                values: tx,
                color: colors.success,
            },
        ]);

        let disk: Vec<f64> = samples.iter().map(|s| s.disk_utilization as f64).collect();
        self.disk.current.set_text(&match disk.last() {
            Some(v) => format!("{v:.0}%"),
            None => "—".into(),
        });
        self.disk.chart.set_series(vec![Series {
            values: disk,
            color: colors.accent,
        }]);

        let temps: Vec<f64> = samples
            .iter()
            .filter_map(|s| s.temperature_c)
            .map(|t| t as f64)
            .collect();
        self.temperature.current.set_text(&match temps.last() {
            Some(v) => format!("{v:.0} °C"),
            None => "—".into(),
        });
        self.temperature.chart.set_series(vec![Series {
            values: temps,
            color: colors.warning,
        }]);

        self.update_tiles(&samples, utilization, range);
    }

    fn update_tiles(&self, samples: &[Sample], utilization: Option<&Utilization>, range: Range) {
        match utilization {
            Some(u) => {
                self.load_tile.set(
                    &format!("{:.2}", u.cpu.load_1),
                    &format!("{:.2} · {:.2} (5m · 15m)", u.cpu.load_5, u.cpu.load_15),
                    false,
                );
                self.memory_tile.set(
                    &format!("{}%", u.memory.usage_percent),
                    &format!(
                        "{} cached · {} swap",
                        format_memory_kb(u.memory.cached_kb),
                        format_memory_kb(u.memory.total_swap_kb)
                    ),
                    u.memory.usage_percent >= 90,
                );
            }
            None => {
                self.load_tile.set("—", "no reading", false);
                self.memory_tile.set("—", "no reading", false);
            }
        }

        // Throughput is a rate per sample, so the total moved over the window
        // is the rate multiplied by that tier's interval — not a sum of rates,
        // which would be a number in no unit at all.
        let seconds = range.interval().num_seconds().max(1) as u64;
        let moved: u64 = samples
            .iter()
            .map(|s| (s.network_rx + s.network_tx) * seconds)
            .sum();
        self.transferred_tile
            .set(&format_bytes(moved), "over this range", false);

        // Says plainly how much history there is, which matters when the
        // answer is "the app started ten minutes ago".
        self.samples_tile.set(
            &samples.len().to_string(),
            &format!("every {}", human_interval(range)),
            samples.is_empty(),
        );
    }
}

impl Default for ResourcePage {
    fn default() -> Self {
        ResourcePage::new()
    }
}

fn human_interval(range: Range) -> String {
    let seconds = range.interval().num_seconds();
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min", seconds / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_labels_end_in_now_and_start_at_the_full_span() {
        let hour = tick_labels(Range::Hour);
        assert_eq!(hour[0], "-60m");
        assert_eq!(hour[2], "-30m");
        assert_eq!(hour[4], "now");
    }

    #[test]
    fn tick_labels_change_unit_with_the_range() {
        assert_eq!(tick_labels(Range::Day)[0], "-24h");
        assert_eq!(tick_labels(Range::Week)[0], "-7d");
        assert_eq!(tick_labels(Range::Month)[0], "-30d");
    }

    #[test]
    fn the_interval_reads_in_the_unit_that_suits_it() {
        assert_eq!(human_interval(Range::Hour), "5 s");
        assert_eq!(human_interval(Range::Day), "1 min");
        assert_eq!(human_interval(Range::Month), "30 min");
    }
}
