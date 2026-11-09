use super::askpass::presence_pending;
use super::terminal::{guard_terminal, set_drawn, show_cursor};
use crate::verbose;
use console::{Term, measure_text_width, truncate_str};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use log::Level;
use regex::Regex;
use std::collections::VecDeque;
use std::io::{IsTerminal, stderr};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const OUTPUT_LINES: usize = 4;
const MESSAGE_LINES: usize = 12;
const DIM: anstyle::Style = anstyle::AnsiColor::BrightBlack.on_default();
const SPINNER: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ";
const SPINNER_STYLE: anstyle::Style = anstyle::AnsiColor::Cyan.on_default().bold();
const GOOD: anstyle::Style = anstyle::AnsiColor::Green.on_default().bold();
const WARN: anstyle::Style = anstyle::AnsiColor::Yellow.on_default().bold();
const BAD: anstyle::Style = anstyle::AnsiColor::Red.on_default().bold();
const NAME: anstyle::Style = anstyle::AnsiColor::Blue.on_default().bold();
const COMMAND: anstyle::Style = anstyle::AnsiColor::Cyan.on_default();
const PRESENCE_CHECK: &str = "Waiting for YubiKey presence check to complete";
const STATUS_MIN: Duration = Duration::from_millis(2500);
const STATUS_ERASE: Duration = Duration::from_millis(350);
const STATUS_TYPE: Duration = Duration::from_millis(550);
const STATUS_CURSOR: char = '█';

/// Progress of one command, rendered as one bar
#[derive(Clone)]
pub(crate) struct Progress {
    tasks_done: usize,
    tasks_total: usize,

    mb_download: f32,
    mb_disk_space: f32,
    derivations: usize,

    started: Instant,
    finished: Option<(bool, Instant)>,
}

impl Progress {
    fn new(tasks_total: usize) -> Self {
        Self {
            tasks_done: 0,
            tasks_total,
            mb_download: 0.0,
            mb_disk_space: 0.0,
            derivations: 0,

            started: Instant::now(),
            finished: None,
        }
    }

    pub(crate) fn progress(&mut self) {
        self.tasks_done += 1;
    }

    pub(crate) fn set_done(&mut self, tasks_done: usize) {
        self.tasks_done = tasks_done;
    }

    pub(crate) fn finish(&mut self, success: bool) {
        if success {
            // Whatever was not recognized in the output is done as well
            self.tasks_total = self.total();
            self.tasks_done = self.tasks_total;
        }
        self.finished = Some((success, Instant::now()));
    }

    fn outcome(&self) -> Option<bool> {
        self.finished.map(|(success, _)| success)
    }

    pub(crate) fn add_task(&mut self) {
        self.tasks_total += 1;
    }

    pub(crate) fn set_total(&mut self, total: usize) {
        self.tasks_total = total;
    }

    /// "these 12 derivations will be built:", or "this derivation will be built:"
    pub(crate) fn add_derivations(&mut self, line: &str) {
        static COUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
        let count: usize = COUNT
            .find(line)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        self.derivations += count;
    }

    /// "these 3 paths will be fetched (1.5 MiB download, 5.9 MiB unpacked):"
    pub(crate) fn add_fetched(&mut self, line: &str) {
        static SIZE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"([\d\.]+) ([KMGT]?)i?B").unwrap());
        let amounts: Vec<f32> = SIZE
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
    }

    fn total(&self) -> usize {
        self.tasks_total.max(self.tasks_done).max(1)
    }

    fn elapsed(&self) -> Duration {
        let end = self.finished.map_or_else(Instant::now, |(_, end)| end);
        end.duration_since(self.started)
    }

    fn columns(&self) -> [String; 5] {
        [
            self.tasks_done.to_string(),
            self.total().to_string(),
            self.derivations.to_string(),
            format!("{:.1}", self.mb_download),
            format!("{:.1}", self.mb_disk_space),
        ]
    }

    fn sizes(&self, all: bool, widths: &Widths) -> String {
        let [.., w0, w1, w2] = widths.columns;
        if w0 == 0 {
            return String::new();
        }
        let [.., derivations, download, disk] = self.columns();
        let bold = anstyle::Style::new().bold();
        let sizes = format!(
            "[ {NAME}{derivations:>w0$}{NAME:#} drv / {GOOD}{download:>w1$}{GOOD:#} MiB / \
             {bold}{disk:>w2$}{bold:#} MiB ] • "
        );
        match all {
            true => sizes,
            false => " ".repeat(measure_text_width(&sizes)),
        }
    }

    fn sum<'a>(progresses: impl IntoIterator<Item = Option<&'a Progress>>) -> Option<Progress> {
        let progresses = Vec::from_iter(progresses);
        let started = Vec::from_iter(progresses.iter().flatten());
        let finished = (progresses.iter())
            .map(|progress| (*progress)?.finished)
            .collect::<Option<Vec<_>>>();
        Some(Progress {
            tasks_done: started.iter().map(|p| p.tasks_done).sum(),
            tasks_total: started
                .iter()
                .map(|p| p.tasks_total.max(p.tasks_done))
                .sum(),
            mb_download: started.iter().map(|p| p.mb_download).sum(),
            mb_disk_space: started.iter().map(|p| p.mb_disk_space).sum(),
            derivations: started.iter().map(|p| p.derivations).sum(),
            started: started.iter().map(|p| p.started).min()?,
            finished: finished.and_then(|finished| {
                let success = finished.iter().all(|(success, _)| *success);
                Some((success, finished.iter().map(|(_, end)| *end).max()?))
            }),
        })
    }
}

#[derive(Default)]
struct Widths {
    name: usize,
    columns: [usize; 5],
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Phase {
    Bar,
    Building,
    Deploying,
}

struct Row {
    name: String,
    /// Whether the row is counted in the status line
    counted: bool,
    /// Indented below the row before it, as part of its group
    child: bool,
    /// Whether the bar sums up how the rows below it are building, rather than being its own
    sums: bool,
    phase: Phase,
    progress: Option<Progress>,
    build: Option<Progress>,
    note: Option<String>,
    finished: Option<bool>,
    line: ProgressBar,
    shown: bool,
    template: String,
}

impl Row {
    fn started(&self) -> bool {
        self.progress.is_some() || self.build.is_some()
    }

    fn running(&self) -> bool {
        self.finished.is_none() && (self.started() || self.note.is_some())
    }

    fn has_bar(&self) -> bool {
        self.phase != Phase::Building
    }

    fn update(
        &mut self,
        tree: &str,
        widths: &Widths,
        progress: Option<Progress>,
        outcome: Option<bool>,
    ) {
        let started = self.started() || (self.sums && progress.is_some());
        let spinning = outcome.is_none() && (started || self.running());
        let symbol = match outcome {
            Some(true) => format!("{GOOD}✔{GOOD:#}"),
            Some(false) => format!("{BAD}✖{BAD:#}"),
            None if started => "{spinner}".to_string(),
            None if spinning => "{spinner:.black.bright}".to_string(),
            None => " ".to_string(),
        };
        let name = match outcome.is_some() || spinning {
            true => "blue.bold",
            false => "black.bright",
        };
        let width = widths.name.saturating_sub(tree.chars().count());
        let mut template = format!("{tree}{symbol} {{prefix:<{width}.{name}}} ");

        let message = match progress.filter(|_| self.has_bar()) {
            None => {
                template.push_str("{msg:.black.bright}");
                self.note.clone().unwrap_or_else(|| "queued".to_string())
            }
            Some(progress) => {
                let bar = match progress.outcome() {
                    None => "cyan/black.bright",
                    Some(true) => "green/black.bright",
                    Some(false) => "red/red",
                };
                let [done, total, ..] = widths.columns;
                template.push_str(&format!(
                    "{{wide_bar:.{bar}}} {{percent:>3}}% • {{pos:>{done}}}/{{len:<{total}}} • \
                     {{msg}}{{elapsed_precise}}"
                ));
                self.line.set_length(progress.total() as u64);
                self.line.set_position(progress.tasks_done as u64);
                self.line.set_elapsed(progress.elapsed());
                progress.sizes(self.phase == Phase::Bar, widths)
            }
        };
        if template != self.template {
            self.line.set_style(style(&template));
            self.template = template;
        }
        self.line.set_message(message);
        if spinning {
            self.line.tick();
        }
    }
}

fn divided(sections: Vec<Vec<String>>, width: usize) -> Vec<String> {
    let divider = format!("{DIM}{}{DIM:#}", "─".repeat(width));
    let mut lines = Vec::new();
    for section in sections.into_iter().filter(|section| !section.is_empty()) {
        if lines.is_empty() {
            lines.push(divider.clone());
        }
        lines.extend(section);
        lines.push(divider.clone());
    }
    lines
}

fn style(template: &str) -> ProgressStyle {
    ProgressStyle::with_template(template)
        .expect("the templates put together here are valid")
        .progress_chars("━╸━")
        .tick_chars(SPINNER)
}

fn tail_style(status: bool) -> ProgressStyle {
    style(match status {
        true => "{msg}{spinner:.cyan.bold} {prefix}",
        false => "{msg}",
    })
}

fn line(style: ProgressStyle) -> ProgressBar {
    ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::hidden()).with_style(style)
}

fn terminal_size() -> (usize, usize) {
    let (height, width) = Term::stderr().size();
    (height.into(), width.into())
}

fn show(multi: &MultiProgress, line: &ProgressBar, was: &mut bool, shown: bool, position: usize) {
    match (*was, shown) {
        (false, true) => drop(multi.insert(position, line.clone())),
        (true, false) => multi.remove(line),
        _ => {}
    }
    *was = shown;
}

pub(crate) fn prefixed(prefix: &str, line: &str) -> String {
    match stderr().is_terminal() {
        true => format!("{DIM}[{prefix}]{DIM:#} {line}"),
        false => format!("[{prefix}] {line}"),
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Tone {
    Info,
    Good,
    Warn,
    Bad,
}

impl Tone {
    fn mark(self) -> String {
        let (style, symbol) = match self {
            Tone::Info => (SPINNER_STYLE, "❄"),
            Tone::Good => (GOOD, "✔"),
            Tone::Warn => (WARN, "!"),
            Tone::Bad => (BAD, "✖"),
        };
        format!("{style}{symbol}{style:#}")
    }

    fn level(self) -> Level {
        match self {
            Tone::Info | Tone::Good => Level::Info,
            Tone::Warn => Level::Warn,
            Tone::Bad => Level::Error,
        }
    }
}

pub(crate) fn styled() -> bool {
    stderr().is_terminal() && !verbose()
}

/// Print `message`, followed by dimmed `details`, in the style of the status line, or log it
pub(crate) fn say(tone: Tone, message: &str, details: &[&str]) {
    if !styled() {
        let details: String = details
            .iter()
            .map(|detail| format!(" · {detail}"))
            .collect();
        log::log!(tone.level(), "{message}{details}");
        return;
    }
    let details: String = details
        .iter()
        .map(|detail| format!(" {DIM}· {detail}{DIM:#}"))
        .collect();
    eprintln!("{} {message}{details}", tone.mark());
}

fn single_line(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

fn retry_line(args: &[String]) -> String {
    let command = args.join(" ");
    let mark = Tone::Info.mark();
    format!("{mark} {DIM}retry:{DIM:#} {COMMAND}{command}{COMMAND:#}")
}

/// Prints something once a password prompt is over
type Held = Box<dyn FnOnce() + Send>;

struct Message {
    head: String,
    text: String,
}

impl Message {
    fn render(&self, drawing: bool) -> String {
        match drawing {
            true => self.head.clone() + &single_line(&self.text),
            false => self.head.clone() + &self.text,
        }
    }
}

pub(crate) struct Screen {
    rows: Vec<Row>,
    /// The latest lines of output, prefixed with their command
    output: VecDeque<String>,
    messages: Vec<Message>,
    /// The command retrying what failed, below the warnings and errors
    retry: Option<Vec<String>>,
    status: Option<StatusLine>,
    /// Pending password prompts, during which nothing may be drawn or printed
    prompts: usize,
    /// Draws the lines in place, hidden while nothing may be drawn
    multi: MultiProgress,
    /// Draws the lines below the rows, and the status line, once there are any
    tail: ProgressBar,
    /// Whether the style of `tail` has the status line
    tail_status: bool,
    tail_shown: bool,
    live: bool,
    logging: bool,
    held: Vec<Held>,
}

impl Screen {
    pub(crate) fn new(status: bool) -> Self {
        let live = stderr().is_terminal();
        if live {
            guard_terminal();
        }
        Self {
            rows: Vec::new(),
            output: VecDeque::with_capacity(OUTPUT_LINES),
            messages: Vec::new(),
            retry: None,
            status: status.then(|| StatusLine::new(Instant::now())),
            prompts: 0,
            multi: MultiProgress::with_draw_target(ProgressDrawTarget::hidden()),
            tail: line(tail_style(status)),
            tail_status: status,
            tail_shown: false,
            live,
            logging: verbose(),
            held: Vec::new(),
        }
    }

    pub(crate) fn add_row(&mut self, name: &str, counted: bool, child: bool) -> usize {
        self.rows.push(Row {
            name: name.to_string(),
            counted,
            child,
            sums: false,
            phase: Phase::Bar,
            progress: None,
            build: None,
            note: None,
            finished: None,
            line: line(ProgressStyle::default_bar()).with_prefix(name.to_string()),
            shown: false,
            template: String::new(),
        });
        self.rows.len() - 1
    }

    pub(crate) fn add_group(&mut self, name: &str, sums: bool) -> usize {
        let row = self.add_row(name, false, false);
        self.rows[row].sums = sums;
        row
    }

    pub(crate) fn set_job(&mut self, row: usize, phase: Phase, note: &str) {
        self.rows[row].phase = phase;
        self.set_note(row, Some(note));
    }

    pub(crate) fn phase(&self, row: usize) -> Phase {
        self.rows[row].phase
    }

    pub(crate) fn progress(&mut self, row: usize) -> &mut Progress {
        let row = &mut self.rows[row];
        let progress = match row.phase {
            Phase::Building => &mut row.build,
            Phase::Bar | Phase::Deploying => &mut row.progress,
        };
        progress.get_or_insert_with(|| Progress::new(0))
    }

    pub(crate) fn start_deploy(&mut self, row: usize, paths: usize) {
        let row = &mut self.rows[row];
        if let Some(build) = &mut row.build {
            build.finish(true);
        }
        row.phase = Phase::Deploying;
        row.progress = Some(Progress::new(paths));
    }

    pub(crate) fn finish_row(&mut self, row: usize, success: bool) {
        self.progress(row).finish(success);
        self.finished(row, success, if success { "done" } else { "failed" });
    }

    pub(crate) fn set_note(&mut self, row: usize, note: Option<&str>) {
        self.rows[row].note = note.map(str::to_string);
    }

    pub(crate) fn complete_row(&mut self, row: usize, success: bool, note: &str) {
        self.set_note(row, Some(note));
        self.finished(row, success, note);
    }

    fn finished(&mut self, row: usize, success: bool, outcome: &str) {
        self.rows[row].finished = Some(success);
        if !self.live && self.rows[row].counted {
            let message = format!("{}: {outcome}", self.rows[row].name);
            self.log(message);
        }
    }

    pub(crate) fn add_output(&mut self, prefix: Option<&str>, text: &str) {
        if self.logging {
            let line = prefix.map_or(text.to_string(), |prefix| prefixed(prefix, text));
            self.emit(move || eprintln!("{line}"));
            return;
        }
        if self.output.len() == OUTPUT_LINES {
            self.output.pop_front();
        }
        let line = prefix.map_or(text.to_string(), |prefix| format!("[{prefix}] {text}"));
        self.output.push_back(single_line(&line));
    }

    fn log(&mut self, message: String) {
        self.emit(move || say(Tone::Info, &message, &[]));
    }

    pub(crate) fn message(&mut self, tone: Tone, prefix: Option<&str>, text: &str) {
        let message = Message {
            head: format!(
                "{} {DIM}{}{DIM:#}",
                tone.mark(),
                prefix.map_or(String::new(), |prefix| format!("[{prefix}] "))
            ),
            text: text.to_string(),
        };
        if (self.messages.iter()).any(|m| m.head == message.head && m.text == message.text) {
            return;
        }
        if !self.drawing() {
            let plain = prefix.map_or(text.to_string(), |prefix| format!("{prefix}: {text}"));
            self.emit(move || say(tone, &plain, &[]));
        }
        self.messages.push(message);
    }

    pub(crate) fn set_retry(&mut self, args: Vec<String>) {
        if !self.drawing() {
            let args = args.clone();
            self.emit(move || match styled() {
                true => eprintln!("{}", retry_line(&args)),
                false => log::info!("To retry the failed hosts: {}", args.join(" ")),
            });
        }
        self.retry = Some(args);
    }

    pub(crate) fn emit(&mut self, print: impl FnOnce() + Send + 'static) {
        match self.prompts {
            0 => self.multi.suspend(print),
            _ => self.held.push(Box::new(print)),
        }
    }

    pub(crate) fn set_status(&mut self, message: String) {
        if let Some(status) = &mut self.status {
            status.set(message.clone(), Instant::now());
        }
        if !self.drawing() {
            self.log(message);
        }
    }

    pub(crate) fn pause(&mut self) {
        self.prompts += 1;
        self.hide();
        if self.drawing() {
            show_cursor(true);
        }
    }

    pub(crate) fn resume(&mut self) {
        self.prompts = self.prompts.saturating_sub(1);
        if self.prompts == 0 {
            self.held.drain(..).for_each(|print| print());
        }
    }

    fn drawing(&self) -> bool {
        self.live && !self.logging
    }

    pub(crate) fn draw(&mut self) {
        if !self.drawing() || self.prompts > 0 {
            return;
        }
        show_cursor(false);

        let (height, width) = terminal_size();
        let output = self
            .output
            .iter()
            .map(|line| format!("  {DIM}{line}{DIM:#}"));
        let below = divided(vec![output.collect(), self.messages_section(true)], width);
        let below =
            Vec::from_iter((below.iter()).map(|line| truncate_str(line, width, "").into_owned()));

        let presence = presence_pending();
        let fixed = below.len() + usize::from(self.status.is_some() || presence);
        let fits = self.rows.len() + fixed < height;
        let mut room = height.saturating_sub(fixed + 1);
        let shown = Vec::from_iter((0..self.rows.len()).map(|index| {
            let shown = fits || (room > 0 && self.running(index));
            room -= usize::from(shown && !fits);
            shown
        }));

        let counts = self.counts();
        let status = match presence {
            true => Some(format!("{COMMAND}{PRESENCE_CHECK}{COMMAND:#}")),
            false => (self.status.as_mut()).map(|status| status.render(Instant::now(), &counts)),
        };
        let status =
            status.map(|status| truncate_str(&status, width.saturating_sub(2), "").into_owned());
        self.render(shown, below, status);
    }

    pub(crate) fn finish(&mut self) {
        self.hide();
        self.held.drain(..).for_each(|print| print());
        self.prompts = 0;
        self.status = None;
        if !self.live {
            return;
        }
        let messages = match self.logging {
            true => Vec::new(),
            false => self.messages_section(false),
        };
        let below = divided(vec![messages], terminal_size().1);
        if self.rows.is_empty() && below.is_empty() {
            show_cursor(true);
            return;
        }
        self.render(vec![true; self.rows.len()], below, None);
        self.multi.set_draw_target(ProgressDrawTarget::hidden());
        eprint!("\r\n");
        set_drawn(false);
        show_cursor(true);
    }

    fn render(&mut self, shown: Vec<bool>, below: Vec<String>, status: Option<String>) {
        if self.multi.is_hidden() {
            self.multi
                .set_draw_target(ProgressDrawTarget::stderr_with_hz(1));
            set_drawn(true);
        }
        let mut position = 0;
        for (row, shown) in self.rows.iter_mut().zip(shown) {
            show(&self.multi, &row.line, &mut row.shown, shown, position);
            position += usize::from(shown);
        }
        self.update_rows();

        let tail = !below.is_empty() || status.is_some();
        if status.is_some() != self.tail_status {
            self.tail_status = status.is_some();
            self.tail.set_style(tail_style(self.tail_status));
        }
        show(
            &self.multi,
            &self.tail,
            &mut self.tail_shown,
            tail,
            position,
        );
        let mut text = below.join("\n");
        if let Some(status) = status {
            if !text.is_empty() {
                text.push('\n');
            }
            self.tail.set_prefix(status);
            self.tail.tick();
        }
        self.tail.set_message(text);

        let rows = self
            .rows
            .iter()
            .filter(|row| row.shown)
            .map(|row| &row.line);
        if let Some(line) = rows.chain(self.tail_shown.then_some(&self.tail)).next() {
            line.force_draw();
        }
    }

    fn messages_section(&self, drawing: bool) -> Vec<String> {
        let skipped = match drawing {
            true => self.messages.len().saturating_sub(MESSAGE_LINES),
            false => 0,
        };
        let skipped_line = format!("  {DIM}... and {skipped} more above{DIM:#}");
        let messages = self.messages[skipped..].iter().map(|m| m.render(drawing));
        let retry = self.retry.as_deref().map(retry_line);
        Vec::from_iter(
            (skipped > 0)
                .then_some(skipped_line)
                .into_iter()
                .chain(messages)
                .chain(retry),
        )
    }

    fn running(&self, index: usize) -> bool {
        let (progress, _) = self.state(index);
        self.rows[index].running() || progress.is_some_and(|sum| sum.outcome().is_none())
    }

    fn update_rows(&mut self) {
        let shown = Vec::from_iter(
            (0..self.rows.len())
                .filter(|&index| self.rows[index].shown)
                .map(|index| (index, self.tree(index), self.state(index))),
        );
        let mut widths = Widths::default();
        for (index, tree, (progress, _)) in &shown {
            let row = &self.rows[*index];
            let name = tree.chars().count() + row.name.chars().count();
            widths.name = widths.name.max(name);
            let Some(progress) = progress.as_ref().filter(|_| row.has_bar()) else {
                continue;
            };
            let columns = if row.phase == Phase::Bar { 5 } else { 2 };
            for (width, column) in widths
                .columns
                .iter_mut()
                .zip(progress.columns())
                .take(columns)
            {
                *width = (*width).max(column.len());
            }
        }
        for (index, tree, (progress, outcome)) in shown {
            self.rows[index].update(tree, &widths, progress, outcome);
        }
    }

    fn tree(&self, index: usize) -> &'static str {
        match (self.rows[index].child, self.rows.get(index + 1)) {
            (false, _) => "",
            (true, Some(next)) if next.child => "  ├ ",
            (true, _) => "  └ ",
        }
    }

    fn state(&self, index: usize) -> (Option<Progress>, Option<bool>) {
        let row = &self.rows[index];
        if !row.sums {
            return (row.progress.clone(), row.finished);
        }
        let children = self.rows[index + 1..].iter().take_while(|row| row.child);
        let sum = Progress::sum(children.map(|row| row.build.as_ref()));
        let outcome = sum.as_ref().and_then(Progress::outcome);
        (sum, outcome)
    }

    /// How many of the counted rows are in which state
    fn counts(&self) -> String {
        let mut counts = [0; 4];
        for row in self.rows.iter().filter(|row| row.counted) {
            counts[match row.finished {
                None if row.running() => 0,
                None => 1,
                Some(true) => 2,
                Some(false) => 3,
            }] += 1;
        }
        let parts = (counts.iter().zip(["running", "queued", "done", "failed"]))
            .filter(|(count, _)| **count > 0)
            .map(|(count, state)| format!("{count} {state}"));
        Vec::from_iter(parts).join(" · ")
    }

    fn hide(&mut self) {
        let _ = self.multi.clear();
        self.multi.set_draw_target(ProgressDrawTarget::hidden());
        set_drawn(false);
    }
}

struct StatusLine {
    message: String,
    since: Instant,
    previous: Option<String>,
    pending: Option<String>,
}

impl StatusLine {
    fn new(now: Instant) -> Self {
        Self {
            message: String::new(),
            since: now,
            previous: None,
            pending: None,
        }
    }

    fn set(&mut self, message: String, now: Instant) {
        if self.message.is_empty() {
            self.message = message;
            self.since = now;
        } else {
            self.pending = (message != self.message).then_some(message);
        }
        self.advance(now);
    }

    fn advance(&mut self, now: Instant) {
        let shown = now.duration_since(self.since);
        if shown >= STATUS_ERASE + STATUS_TYPE {
            self.previous = None;
        }
        if shown >= STATUS_MIN
            && let Some(next) = self.pending.take()
        {
            self.previous = Some(std::mem::replace(&mut self.message, next));
            self.since = now;
        }
    }

    fn text(&self, now: Instant) -> String {
        let Some(previous) = &self.previous else {
            return self.message.clone();
        };
        let elapsed = now.duration_since(self.since).as_secs_f32();
        let erase = STATUS_ERASE.as_secs_f32();
        let (text, shown) = match elapsed < erase {
            true => (previous, 1.0 - elapsed / erase),
            false => (&self.message, (elapsed - erase) / STATUS_TYPE.as_secs_f32()),
        };
        let visible = (text.chars().count() as f32 * shown.min(1.0)).ceil() as usize;
        text.chars().take(visible).chain([STATUS_CURSOR]).collect()
    }

    fn render(&mut self, now: Instant, counts: &str) -> String {
        self.advance(now);
        let message = self.text(now);
        match (counts, message.is_empty()) {
            ("", _) => message,
            (counts, true) => format!("{DIM}{counts}{DIM:#}"),
            (counts, false) => format!("{message} {DIM}· {counts}{DIM:#}"),
        }
    }
}

#[test]
fn test_progress_parsing() {
    let mut progress = Progress::new(0);
    progress.add_derivations("these 12 derivations will be built:");
    progress.add_derivations("this derivation will be built:");
    assert_eq!(progress.derivations, 13);
    progress.add_fetched("these 3 paths will be fetched (1.5 MiB download, 2.0 GiB unpacked):");
    progress.add_fetched("this path will be fetched (512.0 KiB download, 1 MiB unpacked):");
    assert_eq!(progress.mb_download, 2.0);
    assert_eq!(progress.mb_disk_space, 2049.0);
}

#[test]
fn test_status_line() {
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let mut status = StatusLine::new(start);

    // The first message is shown right away
    status.set("Evaluating 7 systems".to_string(), start);
    assert_eq!(status.text(start), "Evaluating 7 systems");

    // Later ones wait until the current one was shown long enough, the latest replacing the others
    status.set("Building 7 systems here".to_string(), at(100));
    status.set("Deploying 7 hosts".to_string(), at(200));
    status.advance(at(2000));
    assert_eq!(status.text(at(2000)), "Evaluating 7 systems");

    // Then a cursor erases the current one from right to left, and types the next one
    status.advance(at(2500));
    assert_eq!(status.text(at(2500)), "Evaluating 7 systems█");
    assert_eq!(status.text(at(2675)), "Evaluating█");
    assert_eq!(status.text(at(2850)), "█");
    assert_eq!(status.text(at(3125)), "Deploying█");
    status.advance(at(3400));
    assert_eq!(status.previous, None);
    assert_eq!(status.text(at(3400)), "Deploying 7 hosts");

    // Setting the current message again drops the pending one
    status.set("Activating gsv last".to_string(), at(3500));
    status.set("Deploying 7 hosts".to_string(), at(3600));
    status.advance(at(9000));
    assert_eq!(status.text(at(9000)), "Deploying 7 hosts");
}
