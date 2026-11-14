//! `SimpleChart`: bar and line charts drawn with the egui painter.

use redhive_contracts::viewpack::{ChartKind, ValueFormat};

pub struct Series<'a> {
    pub labels: &'a [String],
    pub values: &'a [f64],
}

pub fn chart(ui: &mut egui::Ui, kind: ChartKind, series: Series<'_>, format: ValueFormat, german: bool, height: f32) {
    let width = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    let n = series.values.len();
    if n == 0 {
        return;
    }
    let max = series.values.iter().cloned().fold(0.0f64, f64::max).max(1e-9);
    let left = 70.0;
    let bottom = 34.0;
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(left, 8.0), rect.max - egui::vec2(8.0, bottom));
    let axis = egui::Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color);
    painter.line_segment([plot.left_bottom(), plot.right_bottom()], axis);
    painter.line_segment([plot.left_top(), plot.left_bottom()], axis);
    let font = egui::FontId::proportional(11.0);
    for k in 0..=4 {
        let v = max * k as f64 / 4.0;
        let y = plot.bottom() - plot.height() * k as f32 / 4.0;
        painter.line_segment([egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)], egui::Stroke::new(0.5_f32, axis.color.gamma_multiply(0.5)));
        let text = super::format::format_value(&serde_json::json!(v.round()), if format == ValueFormat::Money { ValueFormat::Number } else { format }, None, german);
        painter.text(egui::pos2(plot.left() - 6.0, y), egui::Align2::RIGHT_CENTER, text, font.clone(), visuals.weak_text_color());
    }
    let slot = plot.width() / n as f32;
    let accent = visuals.selection.bg_fill;
    let hover = resp.hover_pos();
    let mut tooltip = None;
    let mut points = Vec::with_capacity(n);
    for (i, v) in series.values.iter().enumerate() {
        let x0 = plot.left() + slot * i as f32;
        let h = (plot.height() as f64 * (*v / max)) as f32;
        let top = plot.bottom() - h;
        let center_x = x0 + slot / 2.0;
        match kind {
            ChartKind::Bar => {
                let bar = egui::Rect::from_min_max(egui::pos2(x0 + slot * 0.18, top), egui::pos2(x0 + slot * 0.82, plot.bottom()));
                let hovered = hover.is_some_and(|p| p.x >= x0 && p.x < x0 + slot);
                painter.rect_filled(bar, 2.0, if hovered { accent.gamma_multiply(1.3) } else { accent });
                if hovered {
                    tooltip = Some(i);
                }
            }
            ChartKind::Line => {
                points.push(egui::pos2(center_x, top));
                if hover.is_some_and(|p| (p.x - center_x).abs() < slot / 2.0) {
                    tooltip = Some(i);
                }
            }
        }
        // Label every k-th category so they do not overlap.
        let every = ((n as f32 * 60.0 / plot.width()).ceil() as usize).max(1);
        if i % every == 0 {
            let label = series.labels.get(i).cloned().unwrap_or_default();
            let short: String = label.chars().take(14).collect();
            painter.text(egui::pos2(center_x, plot.bottom() + 4.0), egui::Align2::CENTER_TOP, short, font.clone(), visuals.text_color());
        }
    }
    if kind == ChartKind::Line && points.len() > 1 {
        painter.add(egui::Shape::line(points.clone(), egui::Stroke::new(2.0_f32, accent)));
        for p in &points {
            painter.circle_filled(*p, 3.0, accent);
        }
    }
    if let Some(i) = tooltip {
        let label = series.labels.get(i).cloned().unwrap_or_default();
        let value = super::format::format_value(&serde_json::json!(series.values[i]), format, None, german);
        resp.on_hover_text(format!("{label}: {value}"));
    }
}
