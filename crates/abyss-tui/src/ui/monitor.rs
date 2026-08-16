use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};

use crate::monitor::{Monitor, ratio};
use crate::progress::human_bytes;
use crate::ui::helpers::fit;
use crate::ui::theme::{CORE, HEADER};

/// Fullscreen system monitor: CPU per core, memory, disks, busiest processes.
pub(crate) fn render_monitor(frame: &mut Frame, area: Rect, monitor: &Monitor) {
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" System Monitor ", HEADER))
            .border_style(Style::new().fg(Color::White).bg(Color::Blue))
            .style(CORE),
        area,
    );
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.height < 4 || inner.width < 20 {
        return;
    }

    let mut row = inner.y;
    let width = inner.width;

    row = section(frame, inner.x, row, width, "CPU");
    let overall = f64::from(monitor.cpu_total) / 100.0;
    row = gauge(
        frame,
        inner.x,
        row,
        width,
        overall.clamp(0.0, 1.0),
        &format!("all cores {:.0}%", monitor.cpu_total),
        Color::Cyan,
    );
    // One bar per core, while there is room for them.
    for (index, usage) in monitor.cpu_per_core.iter().enumerate() {
        if row >= inner.bottom().saturating_sub(6) {
            break;
        }
        row = gauge(
            frame,
            inner.x,
            row,
            width,
            (f64::from(*usage) / 100.0).clamp(0.0, 1.0),
            &format!("core {index:<3} {usage:.0}%"),
            Color::Blue,
        );
    }

    if row < inner.bottom().saturating_sub(4) {
        row = section(frame, inner.x, row, width, "Memory");
        row = gauge(
            frame,
            inner.x,
            row,
            width,
            ratio(monitor.memory_used, monitor.memory_total),
            &format!(
                "RAM  {} / {}",
                human_bytes(monitor.memory_used),
                human_bytes(monitor.memory_total)
            ),
            Color::Green,
        );
        if monitor.swap_total > 0 {
            row = gauge(
                frame,
                inner.x,
                row,
                width,
                ratio(monitor.swap_used, monitor.swap_total),
                &format!(
                    "Swap {} / {}",
                    human_bytes(monitor.swap_used),
                    human_bytes(monitor.swap_total)
                ),
                Color::Magenta,
            );
        }
    }

    for disk in &monitor.disk_rows {
        if row >= inner.bottom().saturating_sub(3) {
            break;
        }
        row = gauge(
            frame,
            inner.x,
            row,
            width,
            ratio(disk.used, disk.total),
            &format!(
                "{} {} / {}",
                fit(&disk.name, 18),
                human_bytes(disk.used),
                human_bytes(disk.total)
            ),
            Color::Yellow,
        );
    }

    if row < inner.bottom().saturating_sub(1) {
        row = section(frame, inner.x, row, width, "Processes");
        frame.render_widget(
            Paragraph::new(fit(
                &format!("{:>8}  {:>6}  {:>10}  {}", "PID", "CPU%", "MEM", "NAME"),
                width as usize,
            ))
            .style(HEADER),
            Rect::new(inner.x, row, width, 1),
        );
        row += 1;
        for process in &monitor.processes {
            if row >= inner.bottom() {
                break;
            }
            let text = format!(
                "{:>8}  {:>6.1}  {:>10}  {}",
                process.pid,
                process.cpu,
                human_bytes(process.memory),
                process.name
            );
            frame.render_widget(
                Paragraph::new(fit(&text, width as usize)).style(CORE),
                Rect::new(inner.x, row, width, 1),
            );
            row += 1;
        }
    }

    frame.render_widget(
        Paragraph::new(fit(" Esc close ", area.width as usize)).style(HEADER),
        Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
    );
}

/// Draw a section heading, returning the next free row.
fn section(frame: &mut Frame, x: u16, y: u16, width: u16, title: &str) -> u16 {
    frame.render_widget(
        Paragraph::new(fit(title, width as usize)).style(HEADER),
        Rect::new(x, y, width, 1),
    );
    y + 1
}

/// Draw one labelled bar, returning the next free row.
fn gauge(
    frame: &mut Frame,
    x: u16,
    y: u16,
    width: u16,
    value: f64,
    label: &str,
    colour: Color,
) -> u16 {
    frame.render_widget(
        Gauge::default()
            .ratio(value)
            .label(Span::styled(
                label.to_owned(),
                Style::new().fg(Color::White),
            ))
            .gauge_style(Style::new().fg(colour))
            .style(CORE),
        Rect::new(x, y, width, 1),
    );
    y + 1
}
