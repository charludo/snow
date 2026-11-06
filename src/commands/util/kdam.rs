use crate::Result;
use kdam::{BarExt, Column, RichProgress, Spinner, term, tqdm};
use regex::Regex;
use std::io::{IsTerminal, stderr};
use std::process::ExitStatus;

#[derive(Debug)]
pub(crate) struct Progress {
    tasks_done: usize,
    tasks_total: usize,

    mb_download: f32,
    mb_disk_space: f32,
    derivations: usize,

    bar: RichProgress,
}

impl Progress {
    pub(crate) fn new(name: &str, initial_total: usize) -> Result<Self> {
        term::init(stderr().is_terminal());
        term::hide_cursor()?;

        Ok(Self {
            tasks_done: 0,
            tasks_total: initial_total,
            mb_download: 0.0,
            mb_disk_space: 0.0,
            derivations: 0,

            bar: RichProgress::new(
                tqdm!(
                    initial = 0,
                    // kdam treats a total of 0 as indefinite, which renders as 100%
                    total = initial_total.max(1),
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

    pub(crate) fn progress(&mut self) {
        self.tasks_done += 1;
    }

    pub(crate) fn set_done(&mut self, tasks_done: usize) {
        self.tasks_done = tasks_done;
    }

    pub(crate) fn refresh(&mut self) -> Result<()> {
        self.bar.pb.total = self.tasks_total.max(self.tasks_done).max(1);
        self.bar.update_to(self.tasks_done)?;
        Ok(())
    }

    pub(crate) fn cleanup(&mut self, status: ExitStatus) -> Result<()> {
        term::show_cursor()?;
        if status.success() {
            // Whatever was not recognized in the output is done as well
            self.tasks_total = self.tasks_total.max(self.tasks_done).max(1);
            self.tasks_done = self.tasks_total;
            self.bar
                .replace(0, Column::Text("[bold green]✔".to_owned()));
        } else {
            self.bar.replace(0, Column::Text("[bold red]✖".to_owned()));
        }
        self.refresh()?;
        self.bar.refresh()?;
        Ok(())
    }

    pub(crate) fn add_task(&mut self) {
        self.tasks_total += 1;
    }

    /// "these 12 derivations will be built:", or "this derivation will be built:"
    pub(crate) fn add_derivations(&mut self, line: &str) {
        let re = Regex::new(r"\d+").unwrap();
        let count: usize = re
            .find(line)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);

        self.derivations += count;
        self.bar
            .replace(7, Column::Text(format!("[bold blue]{}", self.derivations)));
    }

    /// "these 3 paths will be fetched (1.5 MiB download, 5.9 MiB unpacked):", in any unit
    pub(crate) fn add_fetched(&mut self, line: &str) {
        let re = Regex::new(r"([\d\.]+) ([KMGT]?)i?B").unwrap();
        let amounts: Vec<f32> = re
            .captures_iter(line)
            .filter_map(|cap| {
                let amount: f32 = cap[1].parse().ok()?;
                let power = ["", "K", "M", "G", "T"]
                    .iter()
                    .position(|unit| *unit == &cap[2])? as i32;
                Some(amount * 1024f32.powi(power - 2))
            })
            .collect();
        let [download, disk_space] = amounts[..] else {
            return;
        };

        self.mb_download += download;
        self.mb_disk_space += disk_space;
        self.bar.replace(
            9,
            Column::Text(format!("[bold green]{:.1}", self.mb_download)),
        );
        self.bar
            .replace(11, Column::Text(format!("[bold]{:.1}", self.mb_disk_space)));
    }
}
