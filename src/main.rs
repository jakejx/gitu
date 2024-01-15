mod cli;
mod command;
mod diff;
mod git;
mod items;
mod keybinds;
mod process;
mod screen;
mod status;
mod theme;
mod ui;

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{
        self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
    ExecutableCommand,
};
use diff::Hunk;
use items::{Actionable, Item};
use keybinds::{Op, TargetOp};
use ratatui::prelude::CrosstermBackend;
use screen::Screen;
use std::{
    io::{self, stderr, Stderr},
    process::{Command, Stdio},
};

type Terminal = ratatui::Terminal<CrosstermBackend<Stderr>>;

lazy_static::lazy_static! {
    static ref USE_DELTA: bool = Command::new("delta").output().map(|out| out.status.success()).unwrap_or(false);
}

struct State {
    quit: bool,
    screens: Vec<Screen>,
    terminal: Terminal,
}

fn main() -> io::Result<()> {
    let mut state = create_initial_state(cli::Cli::parse())?;

    state.terminal.hide_cursor()?;

    enable_raw_mode()?;
    stderr().execute(EnterAlternateScreen)?;

    run_app(&mut state)?;

    stderr().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

fn create_initial_state(args: cli::Cli) -> io::Result<State> {
    let size = terminal::size()?;
    let screens = match args.command {
        Some(cli::Commands::Show { git_show_args }) => {
            vec![screen::show::create(size, git_show_args)]
        }
        Some(cli::Commands::Log { git_log_args }) => vec![screen::log::create(size, git_log_args)],
        None => vec![screen::status::create(size)],
    };

    Ok(State {
        quit: false,
        screens,
        terminal: Terminal::new(CrosstermBackend::new(stderr()))?,
    })
}

fn run_app(state: &mut State) -> Result<(), io::Error> {
    while !state.quit {
        if let Some(screen) = state.screens.last_mut() {
            state.terminal.draw(|frame| ui::ui(frame, screen))?;
            screen.handle_command_output();
        }

        handle_events(state)?;

        if let Some(screen) = state.screens.last_mut() {
            screen.clamp_cursor();
        }
    }

    Ok(())
}

fn handle_events(state: &mut State) -> io::Result<()> {
    // TODO Won't need to poll all the time if command outputs were handled async
    if !event::poll(std::time::Duration::from_millis(100))? {
        return Ok(());
    }

    let Some(screen) = state.screens.last_mut() else {
        panic!("No screen");
    };

    match event::read()? {
        Event::Resize(w, h) => screen.size = (w, h),
        Event::Key(key) => {
            if key.kind == KeyEventKind::Press {
                screen.clear_finished_command();

                handle_action(state, key)?;
            }
        }
        _ => (),
    }

    if state.quit {
        state.screens.pop();
        if let Some(screen) = state.screens.last_mut() {
            state.quit = false;
            screen.refresh_items();
        }
    }

    Ok(())
}

fn handle_action(state: &mut State, key: event::KeyEvent) -> Result<(), io::Error> {
    let Some(screen) = state.screens.last_mut() else {
        panic!("No screen");
    };

    if let Some(action) = keybinds::action_of_key_event(key) {
        match action {
            Op::Quit => state.quit = true,
            Op::Refresh => screen.refresh_items(),
            Op::ToggleSection => screen.toggle_section(),
            Op::SelectPrevious => screen.select_previous(),
            Op::SelectNext => screen.select_next(),
            Op::HalfPageUp => screen.scroll_half_page_up(),
            Op::HalfPageDown => screen.scroll_half_page_down(),
            Op::Log => goto_log_screen(&mut state.screens)?,
            Op::Fetch => {
                screen.issue_command(&[], git::fetch_all_cmd())?;
                screen.refresh_items();
            }
            Op::Commit => {
                open_subscreen(&mut state.terminal, &[], git::commit_cmd())?;
                screen.refresh_items();
            }
            Op::Push => screen.issue_command(&[], git::push_cmd())?,
            Op::Pull => screen.issue_command(&[], git::pull_cmd())?,
            Op::Target(target_action) => {
                handle_target_action(state, target_action)?;
            }
        }
    }

    Ok(())
}

fn handle_target_action(state: &mut State, target_action: TargetOp) -> Result<(), io::Error> {
    let Some(screen) = state.screens.last_mut() else {
        panic!("No screen");
    };

    if let Some(act) = screen.get_selected_item().act.as_ref() {
        match target_action {
            TargetOp::ShowOrEdit => match act {
                Actionable::Ref(r) => {
                    goto_show_screen(r.clone(), &mut state.screens)?;
                }
                Actionable::Untracked(f) => {
                    open_subscreen(&mut state.terminal, &[], editor_cmd(f, None))?;
                    screen.refresh_items();
                }
                Actionable::Delta(d) => {
                    let terminal: &mut Terminal = &mut state.terminal;
                    open_subscreen(terminal, &[], editor_cmd(&d.new_file, None))?;
                    screen.refresh_items();
                }
                Actionable::Hunk(h) => {
                    open_subscreen(&mut state.terminal, &[], editor_cmd(&h.new_file, Some(h)))?;
                    screen.refresh_items();
                }
                Actionable::DiffLine(_) => todo!(),
            },
            TargetOp::Stage => match act {
                Actionable::Ref(_) => todo!(),
                Actionable::Untracked(u) => {
                    screen.issue_command(&[], git::stage_file_cmd(u))?;
                    screen.refresh_items();
                }
                Actionable::Delta(d) => {
                    screen.issue_command(&[], git::stage_file_cmd(&d.new_file))?;
                    screen.refresh_items();
                }
                Actionable::Hunk(h) => {
                    screen.issue_command(h.format_patch().as_bytes(), git::stage_patch_cmd())?;
                    screen.refresh_items();
                }
                Actionable::DiffLine(_) => todo!(),
            },
            TargetOp::Unstage => match act {
                Actionable::Ref(_) => todo!(),
                Actionable::Untracked(_) => todo!(),
                Actionable::Delta(d) => {
                    screen.issue_command(&[], git::unstage_file_cmd(d))?;
                    screen.refresh_items();
                }
                Actionable::Hunk(h) => {
                    screen.issue_command(h.format_patch().as_bytes(), git::unstage_patch_cmd())?;
                    screen.refresh_items();
                }
                Actionable::DiffLine(_) => todo!(),
            },
        };
    }

    Ok(())
}

fn goto_show_screen(reference: String, screens: &mut Vec<Screen>) -> Result<(), io::Error> {
    screens.push(screen::show::create(terminal::size()?, vec![reference]));
    Ok(())
}

fn goto_log_screen(screens: &mut Vec<Screen>) -> Result<(), io::Error> {
    screens.drain(1..);
    screens.push(screen::log::create(terminal::size()?, vec![]));
    Ok(())
}

fn editor_cmd(delta: &str, maybe_hunk: Option<&Hunk>) -> Command {
    let editor = std::env::var("EDITOR").expect("EDITOR not set");
    let mut cmd = Command::new(editor.clone());
    let args = match maybe_hunk {
        Some(hunk) => match editor.as_str() {
            "vi" | "vim" | "nvim" | "nano" => {
                vec![format!("+{}", hunk.new_start), delta.to_string()]
            }
            _ => vec![format!("{}:{}", delta, hunk.new_start)],
        },
        None => vec![delta.to_string()],
    };

    cmd.args(args);
    cmd
}

pub(crate) fn open_subscreen(
    terminal: &mut Terminal,
    input: &[u8],
    mut cmd: Command,
) -> Result<(), io::Error> {
    cmd.stdin(Stdio::piped());
    let mut cmd = cmd.spawn()?;

    use std::io::Write;
    cmd.stdin
        .take()
        .expect("Error taking stdin")
        .write_all(input)?;

    cmd.wait()?;

    terminal.hide_cursor()?;
    terminal.clear()?;

    Ok(())
}
