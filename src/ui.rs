use crate::keybinds;
use crate::theme;
use crate::State;
use ratatui::prelude::*;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub(crate) fn ui(frame: &mut Frame, state: &State) {
    let popup = if let Some(transient) = state.pending_transient_op {
        format_transient_menu(transient)
    } else if let Some(ref cmd) = state.command {
        format_command(cmd)
    } else {
        vec![]
    };

    let popup_len = if popup.len() > 0 { popup.len() + 1 } else { 0 } as u16;
    let layout = Layout::new(
        Direction::Vertical,
        [Constraint::Min(1), Constraint::Length(popup_len)],
    )
    .split(frame.size());

    frame.render_widget(state.screen(), layout[0]);

    if !popup.is_empty() {
        frame.render_widget(command_popup(popup), layout[1]);
    }
}

fn format_command<'b>(cmd: &crate::command::IssuedCommand) -> Vec<Line<'b>> {
    Text::styled(
        format!(
            "$ {}{}",
            cmd.args,
            if cmd.finish_acked { "" } else { "..." }
        ),
        Style::new().fg(theme::CURRENT_THEME.command),
    )
    .lines
    .into_iter()
    .chain(
        Text::raw(
            String::from_utf8(cmd.output.clone()).expect("Error turning command output to String"),
        )
        .lines,
    )
    .collect::<Vec<Line>>()
}

fn format_transient_menu<'b>(transient: crate::keybinds::TransientOp) -> Vec<Line<'b>> {
    Text::styled(
        format!(
            "{:?}",
            keybinds::list_transient_binds(&transient)
                .map(|keybind| format!("{}", keybind))
                .collect::<Vec<_>>()
        ),
        Style::new(),
    )
    .lines
}

fn command_popup(output_lines: Vec<Line>) -> Paragraph {
    Paragraph::new(output_lines).block(
        Block::new()
            .borders(Borders::TOP)
            .border_style(Style::new().fg(theme::CURRENT_THEME.highlight))
            .border_type(ratatui::widgets::BorderType::Plain),
    )
}
