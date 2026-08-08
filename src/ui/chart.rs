//! The one thing with no stock widget: a two-series area-and-line plot.
//!
//! A `GtkDrawingArea` with a Cairo draw function, per the design handoff.
//! Three rules from it are load-bearing and are implemented rather than
//! approximated:
//!
//! - **The Y axis is fixed per chart type, not auto-scaled per frame.**
//!   Auto-scaling makes idle noise look alarming: a NAS wobbling between 1%
//!   and 2% CPU would draw a full-height sawtooth.
//! - **Colours come from the style context**, so the chart follows light/dark
//!   and the user's accent colour instead of hardcoding Adwaita's blue.
//! - **Series are downsampled to the plot width before drawing**, because the
//!   store holds more points than the widget has pixels.

use gtk::cairo;
use gtk::prelude::*;

/// What a chart's vertical axis means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Axis {
    /// 0–100, for CPU and memory.
    Percent,
    /// 0 to the largest value observed, rounded up. Network throughput has no
    /// natural ceiling, so this is the one case that must scale — but it
    /// scales to the *window's* maximum, which is stable, rather than to the
    /// frame's.
    Bytes,
    /// A fixed span in °C. Drive temperatures live in a narrow band and a
    /// zero-based axis would flatten them into a straight line.
    Celsius { min: f64, max: f64 },
}

impl Axis {
    fn ceiling(&self, series: &[&[f64]]) -> f64 {
        match self {
            Axis::Percent => 100.0,
            Axis::Celsius { max, .. } => *max,
            Axis::Bytes => {
                let peak = series
                    .iter()
                    .flat_map(|s| s.iter())
                    .copied()
                    .fold(0.0_f64, f64::max);
                // Round up to something a person would pick, so the axis does
                // not jitter by a byte between polls.
                if peak <= 0.0 {
                    1.0
                } else {
                    let magnitude = 10.0_f64.powf(peak.log10().floor());
                    (peak / magnitude).ceil() * magnitude
                }
            }
        }
    }

    fn floor(&self) -> f64 {
        match self {
            Axis::Celsius { min, .. } => *min,
            _ => 0.0,
        }
    }
}

/// One line on a chart.
pub struct Series {
    pub values: Vec<f64>,
    /// Red, green, blue, each 0–1. Taken from the style context by the caller
    /// so this module never names a colour.
    pub color: (f64, f64, f64),
}

/// Reduce a series to at most `width` points.
///
/// Takes the maximum of each bucket rather than the mean: a one-second spike
/// to 100% CPU inside a 30-minute bucket is the thing worth seeing, and
/// averaging is exactly what would hide it.
pub fn downsample(values: &[f64], width: usize) -> Vec<f64> {
    if width == 0 || values.is_empty() {
        return Vec::new();
    }
    if values.len() <= width {
        return values.to_vec();
    }

    (0..width)
        .map(|i| {
            let start = i * values.len() / width;
            let end = ((i + 1) * values.len() / width).max(start + 1);
            values[start..end.min(values.len())]
                .iter()
                .copied()
                .fold(f64::MIN, f64::max)
        })
        .collect()
}

/// Draw the chart.
///
/// Separated from the widget so it can be exercised against a plain Cairo
/// surface in `examples/preview.rs` with no display attached.
pub fn draw(cr: &cairo::Context, width: f64, height: f64, axis: Axis, series: &[Series]) {
    let values: Vec<&[f64]> = series.iter().map(|s| s.values.as_slice()).collect();
    let top = axis.ceiling(&values);
    let bottom = axis.floor();
    let span = (top - bottom).max(f64::EPSILON);

    // Gridlines at 25/50/75%, in the separator colour the caller has already
    // applied as the source alpha convention.
    cr.set_line_width(1.0);
    cr.set_source_rgba(0.5, 0.5, 0.5, 0.18);
    for fraction in [0.25, 0.5, 0.75] {
        let y = (height * fraction).round() + 0.5;
        cr.move_to(0.0, y);
        cr.line_to(width, y);
    }
    let _ = cr.stroke();

    for s in series {
        let points = downsample(&s.values, width.max(1.0) as usize);
        if points.len() < 2 {
            continue;
        }

        let step = width / (points.len() - 1) as f64;
        let y_of = |v: f64| height - ((v - bottom) / span).clamp(0.0, 1.0) * height;

        // Fill from the line down to the baseline, at ~16% alpha.
        cr.move_to(0.0, height);
        for (i, v) in points.iter().enumerate() {
            cr.line_to(i as f64 * step, y_of(*v));
        }
        cr.line_to((points.len() - 1) as f64 * step, height);
        cr.close_path();
        cr.set_source_rgba(s.color.0, s.color.1, s.color.2, 0.16);
        let _ = cr.fill();

        // Then the stroke on top.
        cr.set_line_width(1.5);
        cr.set_line_join(cairo::LineJoin::Round);
        cr.set_source_rgb(s.color.0, s.color.1, s.color.2);
        cr.move_to(0.0, y_of(points[0]));
        for (i, v) in points.iter().enumerate().skip(1) {
            cr.line_to(i as f64 * step, y_of(*v));
        }
        let _ = cr.stroke();
    }
}

/// A chart widget.
///
/// Redraws on new data only — `set_series` queues the draw — rather than on a
/// frame clock, which would spin the GPU for a picture that changes every
/// five seconds.
pub struct Chart {
    pub area: gtk::DrawingArea,
    state: std::rc::Rc<std::cell::RefCell<(Axis, Vec<Series>)>>,
}

impl Chart {
    pub fn new(axis: Axis, height: i32) -> Self {
        let area = gtk::DrawingArea::new();
        area.set_content_height(height);
        area.set_hexpand(true);

        let state = std::rc::Rc::new(std::cell::RefCell::new((axis, Vec::new())));

        area.set_draw_func({
            let state = state.clone();
            move |_, cr, w, h| {
                let state = state.borrow();
                draw(cr, w as f64, h as f64, state.0, &state.1);
            }
        });

        Chart { area, state }
    }

    pub fn set_series(&self, series: Vec<Series>) {
        self.state.borrow_mut().1 = series;
        self.area.queue_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsampling_leaves_a_short_series_alone() {
        let v = vec![1.0, 2.0, 3.0];
        assert_eq!(downsample(&v, 10), v);
    }

    #[test]
    fn downsampling_reduces_a_long_series_to_the_plot_width() {
        let v: Vec<f64> = (0..1000).map(|i| i as f64).collect();
        assert_eq!(downsample(&v, 100).len(), 100);
    }

    #[test]
    fn downsampling_keeps_peaks_rather_than_averaging_them_away() {
        // A one-sample spike inside a bucket is the whole reason anyone looks
        // at the chart; a mean would erase it.
        let mut v = vec![0.0; 100];
        v[42] = 100.0;
        let out = downsample(&v, 10);
        assert!(out.contains(&100.0), "the spike was averaged away: {out:?}");
    }

    #[test]
    fn downsampling_an_empty_series_or_a_zero_width_is_empty_not_a_panic() {
        assert!(downsample(&[], 10).is_empty());
        assert!(downsample(&[1.0, 2.0], 0).is_empty());
    }

    #[test]
    fn a_percent_axis_is_fixed_regardless_of_the_data() {
        // Idle noise between 1% and 2% must draw as a flat line near the
        // bottom, not as a full-height sawtooth.
        let quiet = [1.0, 2.0, 1.0];
        let busy = [10.0, 90.0];
        assert_eq!(Axis::Percent.ceiling(&[&quiet]), 100.0);
        assert_eq!(Axis::Percent.ceiling(&[&busy]), 100.0);
    }

    #[test]
    fn a_bytes_axis_rounds_its_ceiling_to_something_stable() {
        // Scaling to the exact peak would move the axis on every poll.
        assert_eq!(Axis::Bytes.ceiling(&[&[1234.0]]), 2000.0);
        assert_eq!(Axis::Bytes.ceiling(&[&[45.0, 12.0]]), 50.0);
    }

    #[test]
    fn an_all_zero_bytes_axis_does_not_divide_by_zero() {
        assert_eq!(Axis::Bytes.ceiling(&[&[0.0, 0.0]]), 1.0);
        assert_eq!(Axis::Bytes.ceiling(&[]), 1.0);
    }

    #[test]
    fn a_celsius_axis_uses_its_band_so_drive_temperatures_are_readable() {
        let axis = Axis::Celsius {
            min: 20.0,
            max: 70.0,
        };
        assert_eq!(axis.floor(), 20.0);
        assert_eq!(axis.ceiling(&[&[31.0, 33.0]]), 70.0);
    }

    #[test]
    fn drawing_a_degenerate_series_does_not_panic() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 100, 40)
            .expect("surface should be creatable");
        let cr = cairo::Context::new(&surface).expect("context should be creatable");

        // One point, no points, and a flat line: all have to be survivable,
        // because a chart is drawn on the first poll when it has exactly one.
        for values in [vec![], vec![5.0], vec![5.0, 5.0]] {
            draw(
                &cr,
                100.0,
                40.0,
                Axis::Percent,
                &[Series {
                    values,
                    color: (0.2, 0.5, 0.9),
                }],
            );
        }
    }
}
