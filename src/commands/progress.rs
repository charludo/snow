use crate::Result;
use kdam::{term, tqdm, BarExt, Column, RichProgress, Spinner};
use regex::Regex;
use std::io::{stderr, IsTerminal};
use std::process::ExitStatus;

#[derive(Debug)]
pub(super) struct Progress {
    tasks_done: i32,
    tasks_total: usize,

    mb_download: f32,
    mb_disk_space: f32,
    derivations: usize,

    bar: RichProgress,
}

impl Progress {
    pub(super) fn new(name: &str, initial_total: usize) -> Result<Self> {
        term::init(stderr().is_terminal());
        term::hide_cursor()?;

        Ok(Self {
            tasks_done: -1,
            tasks_total: initial_total,
            mb_download: 0.0,
            mb_disk_space: 0.0,
            derivations: 0,

            bar: RichProgress::new(
                tqdm!(
                    initial = 0,
                    total = initial_total + 1,
                    force_refresh = true,
                    dynamic_miniters = true,
                    dynamic_ncols = true
                ),
                vec![
                    Column::Spinner(Spinner::new(
                        &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
                        1.0,
                        1.0,
                    )),
                    Column::Text(format!("[bold blue]{name}")),
                    Column::Animation,
                    Column::Percentage(1),
                    Column::Text("•".to_owned()),
                    Column::CountTotal,
                    Column::Text("• [".to_owned()),
                    Column::Text("[bold blue]0".to_string()),
                    Column::Text("drv /".to_string()),
                    Column::Text("[bold green]0".to_string()),
                    Column::Text("MiB /".to_string()),
                    Column::Text("[bold]0".to_string()),
                    Column::Text("MiB ]".to_string()),
                    Column::Text("•".to_owned()),
                    Column::ElapsedTime,
                ],
            ),
        })
    }

    pub(super) fn progress(&mut self) {
        self.tasks_done += 1;
    }

    pub(super) fn refresh(&mut self) -> Result<()> {
        if self.bar.pb.counter < self.tasks_done.max(0) as usize {
            self.bar.update(1)?;
        } else {
            self.bar.update(0)?;
        }

        Ok(())
    }

    pub(super) fn cleanup(&mut self, status: ExitStatus) -> Result<()> {
        term::show_cursor()?;
        if status.success() {
            if self.tasks_total == 0 {
                self.tasks_total += 1;
            }
            self.bar
                .replace(0, Column::Text("[bold green]✔".to_owned()));
        } else {
            self.bar.replace(0, Column::Text("[bold red]✖".to_owned()));
        }
        self.bar.refresh()?;
        Ok(())
    }

    pub(super) fn add_task(&mut self) {
        self.tasks_total += 1;
        self.bar.pb.total = self.tasks_total;
    }

    pub(super) fn add_derivations(&mut self, line: &str) {
        let re = Regex::new(r"\d+").unwrap();
        let count: usize = re
            .find(line)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or_default();

        self.derivations += count;
        self.bar
            .replace(7, Column::Text(format!("[bold blue]{}", self.derivations)));
    }

    pub(super) fn add_fetched(&mut self, line: &str) {
        let re = Regex::new(r"([\d\.]+) MiB").unwrap();
        let amounts: Vec<f32> = re
            .captures_iter(line)
            .filter_map(|cap| cap.get(1))
            .filter_map(|m| m.as_str().parse().ok())
            .collect();

        self.mb_download += amounts[0];
        self.mb_disk_space += amounts[1];
        self.bar
            .replace(9, Column::Text(format!("[bold green]{}", self.mb_download)));
        self.bar
            .replace(11, Column::Text(format!("[bold]{}", self.mb_disk_space)));
    }
}
