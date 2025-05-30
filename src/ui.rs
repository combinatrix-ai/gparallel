use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{collections::VecDeque, io, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub id: u32,
    pub name: String,
    pub free_memory_mb: u64,
    pub total_memory_mb: u64,
}

#[derive(Debug, Clone)]
pub enum JobState {
    Queued,
    Running { gpu_id: u32 },
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: Uuid,
    pub cmd: String,
    pub state: JobState,
    pub log_lines: VecDeque<String>,
}

pub struct AppState {
    pub gpus: Vec<GpuInfo>,
    pub jobs: Vec<JobInfo>,
    pub selected_job: Option<usize>,
    pub should_quit: bool,
    pub job_scroll_offset: usize,
    pub job_panel_visible_height: usize,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            gpus: vec![],
            jobs: vec![],
            selected_job: None,
            should_quit: false,
            job_scroll_offset: 0,
            job_panel_visible_height: 10, // Default fallback
        }
    }
}

pub struct UI {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    state: Arc<RwLock<AppState>>,
}

impl UI {
    pub async fn new(state: Arc<RwLock<AppState>>) -> Result<Self> {
        // Check if we can actually enable raw mode (requires a real TTY)
        if !atty::is(atty::Stream::Stdout) {
            return Err(anyhow::anyhow!("TUI requires stdout to be a terminal"));
        }

        enable_raw_mode().map_err(|e| anyhow::anyhow!("Failed to enable raw mode: {}", e))?;

        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
            .map_err(|e| anyhow::anyhow!("Failed to setup terminal: {}", e))?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)
            .map_err(|e| anyhow::anyhow!("Failed to create terminal: {}", e))?;

        Ok(Self { terminal, state })
    }

    pub async fn run(mut self) -> Result<()> {
        loop {
            {
                let mut state = self.state.write().await;
                if state.should_quit {
                    break;
                }

                // Auto-select first job if none selected and jobs exist
                if state.selected_job.is_none() && !state.jobs.is_empty() {
                    state.selected_job = Some(0);
                }

                // Auto-exit when all jobs are done
                if !state.jobs.is_empty()
                    && state
                        .jobs
                        .iter()
                        .all(|j| matches!(j.state, JobState::Completed | JobState::Failed))
                {
                    break;
                }
            }

            self.terminal.draw(|f| {
                let state = self.state.clone();
                tokio::task::block_in_place(|| {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        let state = state.read().await;
                        Self::draw_ui_static(f, &*state);
                    });
                });
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    let mut state = self.state.write().await;
                    match key.code {
                        KeyCode::Char('q') => state.should_quit = true,
                        KeyCode::Up => {
                            if !state.jobs.is_empty() {
                                let new_selected = match state.selected_job {
                                    Some(i) => i.saturating_sub(1),
                                    None => 0,
                                };
                                state.selected_job = Some(new_selected);

                                // Adjust scroll offset if selection goes above visible area
                                if new_selected < state.job_scroll_offset {
                                    state.job_scroll_offset = new_selected;
                                }
                            }
                        }
                        KeyCode::Down => {
                            if !state.jobs.is_empty() {
                                let new_selected = match state.selected_job {
                                    Some(i) => (i + 1).min(state.jobs.len() - 1),
                                    None => 0,
                                };
                                state.selected_job = Some(new_selected);

                                let visible_height = state.job_panel_visible_height;

                                // Adjust scroll offset if selection goes below visible area
                                if new_selected >= state.job_scroll_offset + visible_height {
                                    state.job_scroll_offset =
                                        new_selected.saturating_sub(visible_height - 1);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    fn draw_ui_static(f: &mut Frame, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Percentage(58),
                Constraint::Length(1),
            ])
            .split(f.size());

        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(chunks[0]);

        Self::draw_gpu_panel(f, top_chunks[0], &state.gpus, &state.jobs);
        let job_panel_visible_height = top_chunks[1].height.saturating_sub(2) as usize;

        Self::draw_job_queue_panel(
            f,
            top_chunks[1],
            &state.jobs,
            state.selected_job,
            state.job_scroll_offset,
            job_panel_visible_height,
        );
        Self::draw_log_panel(f, chunks[1], &state.jobs, state.selected_job);
        Self::draw_help_line(f, chunks[2], state);
    }

    fn draw_gpu_panel(f: &mut Frame, area: Rect, gpus: &[GpuInfo], jobs: &[JobInfo]) {
        let gpu_items: Vec<ListItem> = gpus
            .iter()
            .enumerate()
            .map(|(i, gpu)| {
                let free_mb = gpu.free_memory_mb;
                let total_mb = gpu.total_memory_mb;
                let usage_percent = if total_mb > 0 {
                    ((total_mb - free_mb) as f32 / total_mb as f32 * 100.0) as u8
                } else {
                    0
                };

                let color = if usage_percent > 80 {
                    Color::Red
                } else if usage_percent > 50 {
                    Color::Yellow
                } else {
                    Color::Green
                };

                // Check if any job is running on this GPU
                let running_job = jobs.iter().find(
                    |job| matches!(job.state, JobState::Running { gpu_id } if gpu_id == gpu.id),
                );

                let status_indicator = if running_job.is_some() {
                    "●" // Filled circle for running
                } else {
                    "○" // Empty circle for idle
                };

                let status_color = if running_job.is_some() {
                    Color::Green
                } else {
                    Color::DarkGray
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:<2}", i), Style::default().fg(Color::Cyan)),
                    Span::raw(" "),
                    Span::styled(status_indicator, Style::default().fg(status_color)),
                    Span::raw(" "),
                    Span::styled(format!("{:<7}", gpu.name), Style::default()),
                    Span::raw(" "),
                    Span::styled(format!("{:>6} MB", free_mb), Style::default().fg(color)),
                ]))
            })
            .collect();

        let gpu_list = List::new(gpu_items)
            .block(Block::default().borders(Borders::ALL).title(" GPUs "))
            .style(Style::default().fg(Color::White));

        f.render_widget(gpu_list, area);
    }

    fn draw_job_queue_panel(
        f: &mut Frame,
        area: Rect,
        jobs: &[JobInfo],
        selected: Option<usize>,
        scroll_offset: usize,
        visible_height: usize,
    ) {
        // Get the visible slice of jobs
        let visible_jobs: Vec<(usize, &JobInfo)> = jobs
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .collect();

        let job_items: Vec<ListItem> = visible_jobs
            .iter()
            .map(|(i, job)| {
                let state_str = match &job.state {
                    JobState::Queued => "QUEUE   ".to_string(),
                    JobState::Running { gpu_id } => format!("RUN  G{} ", gpu_id),
                    JobState::Completed => "DONE    ".to_string(),
                    JobState::Failed => "FAIL    ".to_string(),
                };

                let state_color = match &job.state {
                    JobState::Queued => Color::Yellow,
                    JobState::Running { .. } => Color::Green,
                    JobState::Completed => Color::Blue,
                    JobState::Failed => Color::Red,
                };

                let id_str = job.id.to_string();
                let short_id = id_str[..8].to_string();

                let cmd_display = if job.cmd.len() > 30 {
                    format!("{}...", &job.cmd[..27])
                } else {
                    job.cmd.clone()
                };

                let style = if Some(*i) == selected {
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(short_id, style.fg(Color::Cyan)),
                    Span::styled(" ", style),
                    Span::styled(format!("{:<30}", cmd_display), style),
                    Span::styled(" ", style),
                    Span::styled(state_str, style.fg(state_color)),
                ]))
                .style(style)
            })
            .collect();

        let job_list = List::new(job_items)
            .block(Block::default().borders(Borders::ALL).title(" Job queue "))
            .style(Style::default().fg(Color::White));

        f.render_widget(job_list, area);
    }

    fn draw_log_panel(f: &mut Frame, area: Rect, jobs: &[JobInfo], selected: Option<usize>) {
        let title = if let Some(idx) = selected {
            if let Some(job) = jobs.get(idx) {
                let id_str = job.id.to_string();
                let short_id = &id_str[..8];
                format!(" Live log : job #{} (tail -f) ", short_id)
            } else {
                " Live log ".to_string()
            }
        } else {
            " Live log ".to_string()
        };

        let log_content = if let Some(idx) = selected {
            if let Some(job) = jobs.get(idx) {
                if job.log_lines.is_empty() {
                    format!("No logs yet for job {} ({})", job.id, job.cmd)
                } else {
                    job.log_lines.iter().cloned().collect::<Vec<_>>().join("\n")
                }
            } else {
                "Job not found".to_string()
            }
        } else {
            if jobs.is_empty() {
                "No jobs available".to_string()
            } else {
                "Select a job with ↑/↓ keys".to_string()
            }
        };

        let log_paragraph = Paragraph::new(log_content)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::White));

        f.render_widget(log_paragraph, area);
    }

    fn draw_help_line(f: &mut Frame, area: Rect, _state: &AppState) {
        let help_text = Line::from(vec![
            Span::styled(
                "↑/↓",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Navigate jobs  "),
            Span::styled(
                "q",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Quit (jobs continue)  "),
            Span::styled(
                "Ctrl+C",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Force quit & stop all jobs  "),
            Span::styled(
                "Auto-exit",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" when all jobs complete"),
        ]);

        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);

        f.render_widget(help_paragraph, area);
    }
}

impl Drop for UI {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}
