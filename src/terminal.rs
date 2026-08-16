//! a terminal for the lua layer: alacritty's brain, wisp's body.
//!
//! `alacritty_terminal` provides the parts that take a decade to get
//! right -- the vt state machine (`Term`), the escape parser (vte) and
//! the pty spawn -- and this module deliberately refuses the part that
//! doesn't fit: its threaded event loop. the pty master is switched to
//! non-blocking instead and `update()` drains it on the main thread,
//! polled from a lua coroutine like every other background task.
//!
//! colors resolve in two layers: the terminal's own runtime table (osc 4
//! and friends) wins, then the palette set from lua (the theme owns the
//! 16 base colors). cells come out as per-line runs, so a full screen is
//! a few hundred values across the boundary, not tens of thousands.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::rc::Rc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{self, Term, TermDamage, TermMode};
use alacritty_terminal::tty::{self, ChildEvent, EventedPty, Options, Shell};
use alacritty_terminal::vte::ansi::{self, CursorShape, NamedColor, Rgb};

use crate::renderer::Color;

/// most bytes fed to the vt engine per `update()` call; a chattier child
/// keeps the rest in the kernel's pty buffer (natural backpressure) so
/// one update can never stall a frame indefinitely
const READ_CAP: usize = 1024 * 1024;

/// cap on input queued toward the child while its pty is full; writes
/// past it are refused, mirroring the spirit of process.rs's cap
const WRITE_CAP: usize = 4 * 1024 * 1024;

const SCROLLBACK: usize = 10_000;

/// run flags crossing to lua, a stable little language of our own
pub const FLAG_BOLD: u8 = 1;
pub const FLAG_ITALIC: u8 = 2;
pub const FLAG_UNDERLINE: u8 = 4;
pub const FLAG_STRIKEOUT: u8 = 8;

/// a horizontal stretch of cells sharing one style. `width` counts
/// terminal columns, not chars: a wide cjk glyph is one char, two
/// columns, and the view realigns to the grid after every run so pixel
/// drift can never accumulate
#[derive(Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub flags: u8,
    pub width: usize,
}

/// terminal-initiated happenings the view may care about
#[derive(Debug, PartialEq, Eq)]
pub enum TermEvent {
    Title(String),
    ResetTitle,
    Bell,
    /// osc 52: the application asked to place text on the clipboard;
    /// whether to honor it is the lua side's decision
    Clipboard(String),
}

/// what changed since the last `take_damage()` call, in display rows
#[derive(Debug, PartialEq, Eq)]
pub enum Damage {
    Full,
    /// damaged rows, top to bottom; empty means nothing changed
    Rows(Vec<usize>),
}

/// `Term` reports through this; events queue up single-threaded and
/// `update()`/`feed()` drain them right after every advance
#[derive(Clone, Default)]
struct Listener(Rc<RefCell<VecDeque<Event>>>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push_back(event);
    }
}

pub struct TerminalOptions {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

pub struct Terminal {
    term: Term<Listener>,
    parser: ansi::Processor,
    listener: Listener,
    pty: Option<tty::Pty>,
    /// bytes owed to the child: lua input plus the terminal's own query
    /// responses, flushed as the pty accepts them
    pending_write: VecDeque<u8>,
    events: Vec<TermEvent>,
    palette: [Rgb; term::color::COUNT],
    exit_code: Option<i32>,
    /// the child hung up (eof on the master); exit_code follows shortly
    eof: bool,
    cell_size: (u16, u16),
}

fn window_size(cols: u16, rows: u16, cell: (u16, u16)) -> WindowSize {
    WindowSize {
        num_cols: cols,
        num_lines: rows,
        cell_width: cell.0,
        cell_height: cell.1,
    }
}

impl Terminal {
    /// spawns `opts.argv` (never a shell *around* it; an empty argv
    /// means the user's own login shell) on a fresh pty, `cols` x `rows`
    /// cells of `cell_size` pixels. TERM defaults to xterm-256color --
    /// universally installed, unlike alacritty's own terminfo -- but
    /// opts.env can override it
    pub fn open(
        cols: u16,
        rows: u16,
        cell_size: (u16, u16),
        opts: TerminalOptions,
    ) -> Result<Terminal, String> {
        let (cols, rows) = (cols.max(2), rows.max(2));
        let mut env: std::collections::HashMap<String, String> =
            [("TERM".to_owned(), "xterm-256color".to_owned())].into();
        for (k, v) in opts.env {
            env.insert(k, v);
        }
        let config = Options {
            shell: (!opts.argv.is_empty())
                .then(|| Shell::new(opts.argv[0].clone(), opts.argv[1..].to_vec())),
            working_directory: opts.cwd.map(Into::into),
            drain_on_exit: false,
            env,
        };
        let pty = tty::new(&config, window_size(cols, rows, cell_size), 0)
            .map_err(|err| format!("failed to open a terminal: {err}"))?;
        // the whole design rests on this: reads and writes on the master
        // return WouldBlock instead of stalling the editor. read-modify-
        // write, so whatever flags the pty opened with survive
        let fd = std::os::fd::AsRawFd::as_raw_fd(pty.file());
        let flags = nix::fcntl::fcntl(fd, nix::fcntl::FcntlArg::F_GETFL)
            .map_err(|err| format!("failed to open a terminal: {err}"))?;
        nix::fcntl::fcntl(
            fd,
            nix::fcntl::FcntlArg::F_SETFL(
                nix::fcntl::OFlag::from_bits_retain(flags) | nix::fcntl::OFlag::O_NONBLOCK,
            ),
        )
        .map_err(|err| format!("failed to open a terminal: {err}"))?;
        let mut terminal = Terminal::detached(cols, rows);
        terminal.cell_size = cell_size;
        terminal.pty = Some(pty);
        Ok(terminal)
    }

    /// a terminal with no process behind it; update() still drains
    /// queued query responses, so the vt engine is fully exercisable
    /// without ever touching a pty
    fn detached(cols: u16, rows: u16) -> Terminal {
        let listener = Listener::default();
        let config = term::Config {
            scrolling_history: SCROLLBACK,
            ..Default::default()
        };
        let size = term::test::TermSize::new(cols as usize, rows as usize);
        let term = Term::new(config, &size, listener.clone());
        Terminal {
            term,
            parser: ansi::Processor::new(),
            listener,
            pty: None,
            pending_write: VecDeque::new(),
            events: Vec::new(),
            palette: default_palette(),
            exit_code: None,
            eof: false,
            cell_size: (0, 0),
        }
    }

    /// feeds raw child output into the vt engine and services whatever
    /// the engine asked for in response (query replies, events)
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.drain_events();
    }

    /// one poll: drain child output into the engine (up to READ_CAP),
    /// flush pending input, notice an exit. returns true if the grid
    /// may have changed. never blocks
    pub fn update(&mut self) -> bool {
        let mut changed = false;
        self.flush_pending();
        // taken out for the duration: feed() needs all of self, and it
        // never touches the pty
        if let Some(mut pty) = self.pty.take() {
            changed |= self.drain_reads(pty.file());
            if self.exit_code.is_none()
                && let Some(ChildEvent::Exited(status)) = pty.next_child_event()
            {
                use std::os::unix::process::ExitStatusExt;
                // python's convention, like process.rs: signal n is -n
                self.exit_code = Some(match status {
                    Some(s) => s.signal().map(|sig| -sig).or(s.code()).unwrap_or(-1),
                    None => -1,
                });
                changed = true;
            }
            self.pty = Some(pty);
        }
        // responses generated while feeding (da, dsr, ...) go out now
        self.flush_pending();
        changed
    }

    /// drains up to READ_CAP bytes of child output into the engine;
    /// returns true if anything arrived. a signal can interrupt even a
    /// non-blocking read: that is a retry, never an eof
    fn drain_reads(&mut self, mut file: impl Read) -> bool {
        let mut changed = false;
        let mut total = 0;
        let mut chunk = [0u8; 65536];
        loop {
            match file.read(&mut chunk) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(n) => {
                    self.feed(&chunk[..n]);
                    changed = true;
                    total += n;
                    if total >= READ_CAP {
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.eof = true;
                    break;
                }
            }
        }
        changed
    }

    /// queues bytes for the child (keyboard input, paste). returns false
    /// if the write buffer is full and the tail was refused
    pub fn write(&mut self, bytes: &[u8]) -> bool {
        let room = WRITE_CAP.saturating_sub(self.pending_write.len());
        let take = bytes.len().min(room);
        self.pending_write.extend(&bytes[..take]);
        self.flush_pending();
        take == bytes.len()
    }

    fn flush_pending(&mut self) {
        if let Some(pty) = &self.pty {
            Terminal::flush_into(&mut self.pending_write, pty.file());
        }
    }

    /// flushes as much of `pending` as the fd accepts right now. a
    /// signal-interrupted write is a retry; only a dead pty forfeits
    /// the queue -- that input can never be delivered
    fn flush_into(pending: &mut VecDeque<u8>, mut file: impl Write) {
        while !pending.is_empty() {
            let (head, _) = pending.as_slices();
            match file.write(head) {
                Ok(0) => break,
                Ok(n) => {
                    pending.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    pending.clear();
                    break;
                }
            }
        }
    }

    /// answers the engine's requests and files the rest for lua
    fn drain_events(&mut self) {
        while let Some(event) = { self.listener.0.borrow_mut().pop_front() } {
            match event {
                Event::PtyWrite(text) => {
                    self.pending_write.extend(text.as_bytes());
                }
                Event::ColorRequest(index, format) => {
                    let rgb = self.term.colors()[index].unwrap_or(self.palette[index]);
                    self.pending_write.extend(format(rgb).as_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let size = window_size(
                        self.term.columns() as u16,
                        self.term.screen_lines() as u16,
                        self.cell_size,
                    );
                    self.pending_write.extend(format(size).as_bytes());
                }
                // the editor's clipboard is not readable by terminal
                // apps, only writable: answer loads with nothing
                Event::ClipboardLoad(_, format) => {
                    self.pending_write.extend(format("").as_bytes());
                }
                Event::ClipboardStore(_, data) => {
                    self.events.push(TermEvent::Clipboard(data));
                }
                Event::Title(title) => self.events.push(TermEvent::Title(title)),
                Event::ResetTitle => self.events.push(TermEvent::ResetTitle),
                Event::Bell => self.events.push(TermEvent::Bell),
                Event::Wakeup
                | Event::MouseCursorDirty
                | Event::CursorBlinkingChange
                | Event::Exit
                | Event::ChildExit(_) => {}
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    /// the 16 base colors plus default fg/bg, from the lua theme; the
    /// 6x6x6 cube and the gray ramp are fixed by the protocol
    pub fn set_palette(&mut self, base: &[Color; 16], fg: Color, bg: Color) {
        let rgb = |c: Color| Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        };
        for (i, c) in base.iter().enumerate() {
            self.palette[i] = rgb(*c);
        }
        self.palette[NamedColor::Foreground as usize] = rgb(fg);
        self.palette[NamedColor::Background as usize] = rgb(bg);
        self.palette[NamedColor::Cursor as usize] = rgb(fg);
        self.palette[NamedColor::BrightForeground as usize] = rgb(fg);
        self.palette[NamedColor::DimForeground as usize] = dim(rgb(fg));
        for i in 0..8 {
            self.palette[NamedColor::DimBlack as usize + i] = dim(self.palette[i]);
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_size: (u16, u16)) {
        let (cols, rows) = (cols.max(2), rows.max(2));
        self.cell_size = cell_size;
        let size = window_size(cols, rows, cell_size);
        if let Some(pty) = &mut self.pty {
            use alacritty_terminal::event::OnResize;
            pty.on_resize(size);
        }
        self.term
            .resize(term::test::TermSize::new(cols as usize, rows as usize));
    }

    pub fn columns(&self) -> usize {
        self.term.columns()
    }

    pub fn screen_lines(&self) -> usize {
        self.term.screen_lines()
    }

    /// pans the view into scrollback; positive is toward history
    pub fn scroll_display(&mut self, delta: i32) {
        self.term
            .scroll_display(alacritty_terminal::grid::Scroll::Delta(delta));
    }

    pub fn reset_scroll(&mut self) {
        self.term
            .scroll_display(alacritty_terminal::grid::Scroll::Bottom);
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn history_lines(&self) -> usize {
        self.term.history_size()
    }

    /// one display row as style runs, scrollback pan included; a row
    /// outside the screen is empty, never a panic (lua asks freely)
    pub fn line(&self, display_row: usize) -> Vec<Run> {
        if display_row >= self.term.screen_lines() {
            return Vec::new();
        }
        let grid = self.term.grid();
        let line = Line(display_row as i32) - grid.display_offset() as i32;
        let row = &grid[line];
        let mut runs: Vec<Run> = Vec::new();
        for col in 0..grid.columns() {
            let cell = &row[Column(col)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let width = if cell.flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            let (fg, bg, flags) = self.style_of(cell);
            match runs.last_mut() {
                Some(run) if run.fg == fg && run.bg == bg && run.flags == flags => {
                    run.text.push(cell.c);
                    run.width += width;
                    if let Some(extra) = cell.zerowidth() {
                        run.text.extend(extra);
                    }
                }
                _ => {
                    let mut text = String::new();
                    text.push(cell.c);
                    if let Some(extra) = cell.zerowidth() {
                        text.extend(extra);
                    }
                    runs.push(Run {
                        text,
                        fg,
                        bg,
                        flags,
                        width,
                    });
                }
            }
        }
        runs
    }

    fn style_of(&self, cell: &term::cell::Cell) -> (Color, Color, u8) {
        let mut fg = self.resolve(cell.fg);
        let mut bg = self.resolve(cell.bg);
        if cell.flags.contains(Flags::DIM) {
            fg = {
                let d = dim(Rgb {
                    r: fg.r,
                    g: fg.g,
                    b: fg.b,
                });
                Color {
                    r: d.r,
                    g: d.g,
                    b: d.b,
                    a: 255,
                }
            };
        }
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.contains(Flags::HIDDEN) {
            fg = bg;
        }
        let mut flags = 0;
        if cell.flags.intersects(Flags::BOLD) {
            flags |= FLAG_BOLD;
        }
        if cell.flags.intersects(Flags::ITALIC) {
            flags |= FLAG_ITALIC;
        }
        if cell.flags.intersects(Flags::ALL_UNDERLINES) {
            flags |= FLAG_UNDERLINE;
        }
        if cell.flags.intersects(Flags::STRIKEOUT) {
            flags |= FLAG_STRIKEOUT;
        }
        (fg, bg, flags)
    }

    fn resolve(&self, color: ansi::Color) -> Color {
        let rgb = match color {
            ansi::Color::Spec(rgb) => rgb,
            ansi::Color::Named(name) => self.lookup(name as usize),
            ansi::Color::Indexed(index) => self.lookup(index as usize),
        };
        Color {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
            a: 255,
        }
    }

    /// runtime table first (osc 4 and friends), then the theme palette
    fn lookup(&self, index: usize) -> Rgb {
        self.term.colors()[index].unwrap_or(self.palette[index])
    }

    /// cursor position in display coordinates, and whether to draw it;
    /// panned into scrollback, the cursor is off-screen and hidden
    pub fn cursor(&self) -> (usize, i32, bool) {
        let point = self.term.grid().cursor.point;
        let offset = self.term.grid().display_offset();
        let visible = offset == 0
            && self.term.cursor_style().shape != CursorShape::Hidden
            && self.term.mode().contains(TermMode::SHOW_CURSOR);
        (point.column.0, point.line.0, visible)
    }

    /// display rows touched since the last call; resets the tracking.
    /// the cursor's row always counts as touched -- alacritty semantics,
    /// the cursor repaints every frame
    pub fn take_damage(&mut self) -> Damage {
        let damage = match self.term.damage() {
            TermDamage::Full => Damage::Full,
            TermDamage::Partial(lines) => Damage::Rows(lines.map(|b| b.line).collect()),
        };
        self.term.reset_damage();
        damage
    }

    /// mode bits the view's key translation must respect
    pub fn app_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn mouse_mode(&self) -> bool {
        self.term.mode().intersects(TermMode::MOUSE_MODE)
    }

    /// which mouse reporting the app asked for: none, "click" (press
    /// and release), "drag" (plus motion while a button is held), or
    /// "motion" (every movement)
    pub fn mouse_protocol(&self) -> Option<&'static str> {
        let mode = self.term.mode();
        if mode.contains(TermMode::MOUSE_MOTION) {
            Some("motion")
        } else if mode.contains(TermMode::MOUSE_DRAG) {
            Some("drag")
        } else if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            Some("click")
        } else {
            None
        }
    }

    /// how mouse reports go on the wire: "sgr" (modern, unlimited
    /// coordinates), "utf8", or "normal" (legacy bytes, 223 columns)
    pub fn mouse_encoding(&self) -> &'static str {
        let mode = self.term.mode();
        if mode.contains(TermMode::SGR_MOUSE) {
            "sgr"
        } else if mode.contains(TermMode::UTF8_MOUSE) {
            "utf8"
        } else {
            "normal"
        }
    }

    pub fn alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// private mode 1007: a wheel over the alt screen should arrive as
    /// arrow keys (how `less` and `vim` scroll without mouse support)
    pub fn alternate_scroll(&self) -> bool {
        self.term.mode().contains(TermMode::ALTERNATE_SCROLL)
    }

    /// whether this display row flows into the next with no newline;
    /// selection copy must not invent line breaks the text never had
    pub fn line_wrapped(&self, display_row: usize) -> bool {
        if display_row >= self.term.screen_lines() {
            return false;
        }
        let grid = self.term.grid();
        let line = Line(display_row as i32) - grid.display_offset() as i32;
        grid[line][Column(grid.columns() - 1)]
            .flags
            .contains(Flags::WRAPLINE)
    }

    pub fn running(&self) -> bool {
        self.exit_code.is_none() && !self.eof
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub fn pid(&self) -> Option<u32> {
        self.pty.as_ref().map(|pty| pty.child().id())
    }
}

/// `Pty::drop` sends sighup and then waits without a timeout, so a
/// child that traps sighup would hang the editor on tab close. hup
/// first here too (the polite close every emulator sends), a short
/// grace for shells to quit on their own, then sigkill -- which cannot
/// be ignored -- so the wait below is always prompt
impl Drop for Terminal {
    fn drop(&mut self) {
        use nix::sys::signal::{Signal, kill};
        use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
        let Some(pty) = &self.pty else { return };
        if self.exit_code.is_some() {
            return; // already reaped by the exit poll
        }
        let pid = nix::unistd::Pid::from_raw(pty.child().id() as i32);
        let _ = kill(pid, Signal::SIGHUP);
        for _ in 0..20 {
            match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                // exited (reaped here; the pty's own wait becomes a
                // no-op) or already gone
                _ => return,
            }
        }
        let _ = kill(pid, Signal::SIGKILL);
    }
}

fn dim(c: Rgb) -> Rgb {
    Rgb {
        r: (c.r as u32 * 2 / 3) as u8,
        g: (c.g as u32 * 2 / 3) as u8,
        b: (c.b as u32 * 2 / 3) as u8,
    }
}

/// xterm's stock colors; lua replaces the parts a theme owns the moment
/// the view exists, but a bare terminal must still resolve every index
fn default_palette() -> [Rgb; term::color::COUNT] {
    let mut p = [Rgb { r: 0, g: 0, b: 0 }; term::color::COUNT];
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    for (i, &(r, g, b)) in BASE.iter().enumerate() {
        p[i] = Rgb { r, g, b };
    }
    // the 6x6x6 color cube
    for i in 0..216 {
        let c = |v: usize| if v == 0 { 0 } else { (v * 40 + 55) as u8 };
        p[16 + i] = Rgb {
            r: c(i / 36),
            g: c(i / 6 % 6),
            b: c(i % 6),
        };
    }
    // the gray ramp
    for i in 0..24 {
        let g = (8 + i * 10) as u8;
        p[232 + i] = Rgb { r: g, g, b: g };
    }
    let fg = Rgb {
        r: 229,
        g: 229,
        b: 229,
    };
    p[NamedColor::Foreground as usize] = fg;
    p[NamedColor::Background as usize] = Rgb { r: 0, g: 0, b: 0 };
    p[NamedColor::Cursor as usize] = fg;
    p[NamedColor::BrightForeground as usize] = fg;
    p[NamedColor::DimForeground as usize] = dim(fg);
    for i in 0..8 {
        p[NamedColor::DimBlack as usize + i] = dim(p[i]);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(t: &Terminal, row: usize) -> String {
        let mut s: String = t.line(row).iter().map(|r| r.text.as_str()).collect();
        s.truncate(s.trim_end().len());
        s
    }

    fn screen_text(t: &Terminal) -> String {
        (0..t.screen_lines())
            .map(|r| text_of(t, r))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// real time, pty tests only; the engine tests need no clock at all
    fn poll_until(t: &mut Terminal, ms: u64, mut done: impl FnMut(&mut Terminal) -> bool) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < ms as u128 {
            t.update();
            if done(t) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    fn sh(script: &str) -> TerminalOptions {
        TerminalOptions {
            argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
            cwd: None,
            env: Vec::new(),
        }
    }

    // --- the vt engine, no pty anywhere -------------------------------

    #[test]
    fn plain_text_lands_in_the_grid() {
        let mut t = Terminal::detached(20, 4);
        t.feed(b"hello");
        assert_eq!(text_of(&t, 0), "hello");
        assert_eq!(t.cursor(), (5, 0, true));
    }

    #[test]
    fn sgr_colors_split_runs() {
        let mut t = Terminal::detached(20, 4);
        t.feed(b"\x1b[31mred\x1b[42mgrn");
        let runs = t.line(0);
        assert_eq!(runs[0].text, "red");
        assert_eq!((runs[0].fg.r, runs[0].fg.g, runs[0].fg.b), (205, 0, 0));
        assert_eq!((runs[0].bg.r, runs[0].bg.g, runs[0].bg.b), (0, 0, 0));
        assert_eq!(runs[1].text, "grn");
        assert_eq!((runs[1].bg.r, runs[1].bg.g, runs[1].bg.b), (0, 205, 0));
    }

    #[test]
    fn long_lines_wrap() {
        let mut t = Terminal::detached(10, 4);
        t.feed(b"aaaaaaaaaabbbbb");
        assert_eq!(text_of(&t, 0), "aaaaaaaaaa");
        assert_eq!(text_of(&t, 1), "bbbbb");
    }

    #[test]
    fn the_alternate_screen_restores_on_leave() {
        let mut t = Terminal::detached(20, 4);
        t.feed(b"hi");
        t.feed(b"\x1b[?1049h\x1b[2J\x1b[Halt");
        assert_eq!(text_of(&t, 0), "alt");
        t.feed(b"\x1b[?1049l");
        assert_eq!(text_of(&t, 0), "hi");
    }

    #[test]
    fn scrollback_pans_and_snaps_back() {
        let mut t = Terminal::detached(20, 4);
        for i in 1..=9 {
            t.feed(format!("l{i}\r\n").as_bytes());
        }
        // 9 lines plus the cursor line on a 4-row screen: l6 is on top
        assert_eq!(text_of(&t, 0), "l7");
        t.scroll_display(3);
        assert_eq!(t.display_offset(), 3);
        assert_eq!(text_of(&t, 0), "l4");
        // the cursor is off-screen while panned
        assert!(!t.cursor().2);
        t.reset_scroll();
        assert_eq!(t.display_offset(), 0);
        assert_eq!(text_of(&t, 0), "l7");
        assert!(t.history_lines() >= 3);
    }

    #[test]
    fn wide_chars_occupy_two_cells_but_appear_once() {
        let mut t = Terminal::detached(10, 2);
        t.feed("日".as_bytes());
        assert_eq!(t.cursor().0, 2, "a wide char advances two columns");
        let text: String = t.line(0).iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text.matches('日').count(), 1);
    }

    #[test]
    fn titles_and_bells_reach_the_event_queue() {
        let mut t = Terminal::detached(20, 4);
        t.feed(b"\x1b]0;wisp\x07\x07");
        assert_eq!(
            t.take_events(),
            vec![TermEvent::Title("wisp".into()), TermEvent::Bell]
        );
        assert!(t.take_events().is_empty(), "events are drained by taking");
    }

    #[test]
    fn da_queries_are_answered_without_a_pty() {
        let mut t = Terminal::detached(20, 4);
        t.feed(b"\x1b[c");
        let response: Vec<u8> = t.pending_write.iter().copied().collect();
        assert!(
            response.starts_with(b"\x1b[?") && response.ends_with(b"c"),
            "unexpected da response {response:?}"
        );
    }

    #[test]
    fn damage_reports_touched_rows_then_resets() {
        let mut t = Terminal::detached(20, 4);
        t.take_damage(); // a fresh terminal starts fully damaged
        t.feed(b"\r\nx");
        match t.take_damage() {
            Damage::Full => {}
            Damage::Rows(rows) => assert!(rows.contains(&1), "row 1 missing from {rows:?}"),
        }
        // the tracking resets, except the cursor row: alacritty keeps
        // the cursor's line damaged so it repaints every frame
        assert_eq!(t.take_damage(), Damage::Rows(vec![1]));
    }

    #[test]
    fn resize_reshapes_the_grid() {
        let mut t = Terminal::detached(10, 4);
        t.feed(b"hello");
        t.resize(5, 2, (0, 0));
        assert_eq!((t.columns(), t.screen_lines()), (5, 2));
        t.resize(40, 10, (0, 0));
        assert_eq!((t.columns(), t.screen_lines()), (40, 10));
    }

    #[test]
    fn osc4_overrides_win_over_the_palette() {
        let mut t = Terminal::detached(20, 4);
        t.feed(b"\x1b]4;1;rgb:00/00/ff\x07\x1b[31mx");
        let run = &t.line(0)[0];
        assert_eq!((run.fg.r, run.fg.g, run.fg.b), (0, 0, 255));
    }

    #[test]
    fn the_palette_from_lua_recolors_named_colors() {
        let mut t = Terminal::detached(20, 4);
        let mut base = [Color::default(); 16];
        base[1] = Color {
            r: 1,
            g: 2,
            b: 3,
            a: 255,
        };
        t.set_palette(
            &base,
            Color {
                r: 9,
                g: 9,
                b: 9,
                a: 255,
            },
            Color {
                r: 7,
                g: 7,
                b: 7,
                a: 255,
            },
        );
        t.feed(b"\x1b[31mx y");
        let runs = t.line(0);
        assert_eq!((runs[0].fg.r, runs[0].fg.g, runs[0].fg.b), (1, 2, 3));
        assert_eq!((runs[0].bg.r, runs[0].bg.g, runs[0].bg.b), (7, 7, 7));
    }

    #[test]
    fn inverse_swaps_colors_and_styles_become_flags() {
        let mut t = Terminal::detached(20, 4);
        t.feed(b"\x1b[1;3;4;7mZ");
        let run = &t.line(0)[0];
        assert_eq!(
            run.flags,
            FLAG_BOLD | FLAG_ITALIC | FLAG_UNDERLINE,
            "wrong flags {:#b}",
            run.flags
        );
        // inverse: the glyph paints in the background color
        assert_eq!((run.fg.r, run.fg.g, run.fg.b), (0, 0, 0));
        assert_eq!((run.bg.r, run.bg.g, run.bg.b), (229, 229, 229));
    }

    #[test]
    fn mouse_modes_surface_protocol_and_encoding() {
        let mut t = Terminal::detached(20, 4);
        assert_eq!(t.mouse_protocol(), None);
        t.feed(b"\x1b[?1000h");
        assert_eq!(t.mouse_protocol(), Some("click"));
        assert_eq!(t.mouse_encoding(), "normal");
        t.feed(b"\x1b[?1002h");
        assert_eq!(t.mouse_protocol(), Some("drag"));
        t.feed(b"\x1b[?1003h\x1b[?1006h");
        assert_eq!(t.mouse_protocol(), Some("motion"));
        assert_eq!(t.mouse_encoding(), "sgr");
        t.feed(b"\x1b[?1006l\x1b[?1005h");
        assert_eq!(t.mouse_encoding(), "utf8");
        t.feed(b"\x1b[?1003l\x1b[?1002l\x1b[?1000l");
        assert_eq!(t.mouse_protocol(), None);
        assert!(!t.mouse_mode());
    }

    #[test]
    fn wrapped_rows_and_the_alt_screen_are_visible() {
        let mut t = Terminal::detached(10, 4);
        t.feed(b"aaaaaaaaaabbb");
        assert!(t.line_wrapped(0), "row 0 flowed into row 1");
        assert!(!t.line_wrapped(1));
        assert!(!t.line_wrapped(99), "off-screen is false, never a panic");
        assert!(!t.alt_screen());
        t.feed(b"\x1b[?1049h");
        assert!(t.alt_screen());
        t.feed(b"\x1b[?1049l");
        assert!(!t.alt_screen());
    }

    #[test]
    fn alternate_scroll_follows_the_private_mode() {
        let mut t = Terminal::detached(10, 4);
        t.feed(b"\x1b[?1007h");
        assert!(t.alternate_scroll());
        t.feed(b"\x1b[?1007l");
        assert!(!t.alternate_scroll());
    }

    #[test]
    fn a_signal_interrupted_read_is_a_retry_not_an_eof() {
        // a reader that gets interrupted once, delivers, then dries up;
        // the old code declared eof on the interrupt and the terminal
        // died under a stray signal
        struct Reader(u32);
        impl std::io::Read for Reader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0 += 1;
                match self.0 {
                    1 => Err(ErrorKind::Interrupted.into()),
                    2 => {
                        buf[..2].copy_from_slice(b"hi");
                        Ok(2)
                    }
                    _ => Err(ErrorKind::WouldBlock.into()),
                }
            }
        }
        let mut t = Terminal::detached(20, 4);
        assert!(t.drain_reads(Reader(0)));
        assert!(!t.eof, "an interrupted read must never read as eof");
        assert_eq!(text_of(&t, 0), "hi");
    }

    #[test]
    fn a_signal_interrupted_write_keeps_the_queue() {
        // interrupted once, then the pty is merely full; the old code
        // lumped the interrupt in with "pty gone" and dropped all input
        struct Writer(u32);
        impl std::io::Write for Writer {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                self.0 += 1;
                match self.0 {
                    1 => Err(ErrorKind::Interrupted.into()),
                    _ => Err(ErrorKind::WouldBlock.into()),
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut pending: VecDeque<u8> = b"abc".iter().copied().collect();
        Terminal::flush_into(&mut pending, Writer(0));
        assert_eq!(pending.len(), 3, "queued input must survive a signal");
    }

    // --- the pty, real processes --------------------------------------

    #[test]
    fn a_command_round_trips_through_the_pty() {
        let mut t = Terminal::open(80, 24, (8, 16), sh("printf 'wisp-ok'")).unwrap();
        assert!(poll_until(&mut t, 5000, |t| !t.running()));
        assert!(screen_text(&t).contains("wisp-ok"));
        assert_eq!(t.exit_code(), Some(0));
    }

    #[test]
    fn the_child_sees_the_window_size_and_term() {
        let mut t = Terminal::open(20, 5, (8, 16), sh("stty size; printf %s \"$TERM\"")).unwrap();
        assert!(poll_until(&mut t, 5000, |t| !t.running()));
        let screen = screen_text(&t);
        assert!(screen.contains("5 20"), "stty saw: {screen}");
        assert!(screen.contains("xterm-256color"), "TERM was: {screen}");
    }

    #[test]
    fn input_reaches_the_child() {
        let mut t =
            Terminal::open(80, 24, (8, 16), sh("read line; printf 'got-%s' \"$line\"")).unwrap();
        assert!(t.write(b"abc\n"));
        assert!(
            poll_until(&mut t, 5000, |t| screen_text(t).contains("got-abc")),
            "screen: {}",
            screen_text(&t)
        );
    }

    #[test]
    fn exit_codes_come_back() {
        let mut t = Terminal::open(80, 24, (8, 16), sh("exit 7")).unwrap();
        assert!(poll_until(&mut t, 5000, |t| !t.running()));
        assert_eq!(t.exit_code(), Some(7));
    }

    #[test]
    fn resize_reaches_the_running_child() {
        let mut t = Terminal::open(80, 24, (8, 16), sh("sleep 0.2; stty size")).unwrap();
        t.resize(33, 11, (8, 16));
        assert!(poll_until(&mut t, 5000, |t| !t.running()));
        let screen = screen_text(&t);
        assert!(screen.contains("11 33"), "stty saw: {screen}");
    }

    #[test]
    fn a_dropped_terminal_reaps_its_child() {
        let t = Terminal::open(80, 24, (8, 16), sh("sleep 100")).unwrap();
        // give the shell a moment to exec sleep, then find the live pid
        let pid = t.pid().unwrap() as i32;
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(t);
        // drop sent sighup and waited: the direct child must be gone
        let gone = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err();
        assert!(gone, "child {pid} survived the drop");
    }

    #[test]
    fn dropping_a_terminal_never_hangs_on_a_hup_proof_child() {
        // a child that traps sighup used to hang `Pty::drop`'s wait --
        // and the editor with it -- forever. run the drop in its own
        // thread so a regression fails loudly instead of freezing
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut t =
                Terminal::open(80, 24, (8, 16), sh("trap '' HUP; echo ready; sleep 30")).unwrap();
            // the trap must be installed before the drop sends sighup
            assert!(poll_until(&mut t, 5000, |t| {
                screen_text(t).contains("ready")
            }));
            drop(t);
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "drop hung on a sighup-ignoring child"
        );
    }

    #[test]
    fn terminal_and_spawn_children_reap_independently() {
        // alacritty's tty registers a sigchld handler; process.rs polls
        // try_wait. neither may eat the other's exit
        let mut t = Terminal::open(80, 24, (8, 16), sh("exit 3")).unwrap();
        let p = crate::process::spawn(
            &["/bin/sh".into(), "-c".into(), "exit 5".into()],
            Default::default(),
        )
        .unwrap();
        assert!(poll_until(&mut t, 5000, |t| !t.running()));
        assert_eq!(t.exit_code(), Some(3));
        let start = std::time::Instant::now();
        while p.returncode().is_none() && start.elapsed().as_secs() < 5 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(p.returncode(), Some(5));
    }
}
