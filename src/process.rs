//! subprocesses for the lua layer: `system.spawn(argv, opts)`.
//!
//! poll-based and never blocking, the same contract as the rest of the
//! api: reader and writer threads feed capped buffers, and the lua side
//! only ever touches the buffers. no shell is ever involved -- argv is
//! executed as given. reads return an empty chunk when nothing is
//! buffered right now and `None` at true end of stream (the `file:read`
//! convention), so consumer loops need no racy `running()` checks.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Condvar, Mutex};

/// per-stream cap: a full buffer stalls the child's pipe (natural
/// backpressure) instead of growing without bound on a chatty child
const BUFFER_CAP: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct Buffer {
    data: VecDeque<u8>,
    /// no more data will ever cross this buffer: the stream hit eof,
    /// the pipe broke, or the process object was dropped
    closed: bool,
}

#[derive(Default)]
struct Pipe {
    buf: Mutex<Buffer>,
    cond: Condvar,
}

/// pump a child output pipe into its buffer until eof
fn read_into(pipe: &Pipe, mut stream: impl Read) {
    let mut chunk = [0u8; 8192];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut buf = pipe.buf.lock().unwrap();
        while buf.data.len() >= BUFFER_CAP && !buf.closed {
            buf = pipe.cond.wait(buf).unwrap();
        }
        if buf.closed {
            return;
        }
        buf.data.extend(&chunk[..n]);
    }
    pipe.buf.lock().unwrap().closed = true;
}

/// pump the stdin buffer into the child until it is closed and drained
fn write_from(pipe: &Pipe, mut stream: impl Write) {
    loop {
        let chunk: Vec<u8> = {
            let mut buf = pipe.buf.lock().unwrap();
            while buf.data.is_empty() && !buf.closed {
                buf = pipe.cond.wait(buf).unwrap();
            }
            if buf.data.is_empty() {
                // closed and drained: dropping the stream is the eof
                return;
            }
            let n = buf.data.len().min(8192);
            buf.data.drain(..n).collect()
        };
        if stream.write_all(&chunk).is_err() || stream.flush().is_err() {
            break;
        }
        pipe.cond.notify_all();
    }
    // the pipe broke: further lua writes must fail, not queue forever
    let mut buf = pipe.buf.lock().unwrap();
    buf.closed = true;
    buf.data.clear();
}

fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    // python's convention: a process killed by signal n reports -n
    if let Some(sig) = status.signal() {
        return -sig;
    }
    status.code().unwrap_or(-1)
}

struct ChildState {
    child: Child,
    exit: Option<i32>,
}

#[derive(Default)]
pub struct SpawnOptions {
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    /// interleave stderr into the stdout stream, like `2>&1`
    pub merge_stderr: bool,
}

pub struct Process {
    child: Mutex<ChildState>,
    stdout: Arc<Pipe>,
    stderr: Arc<Pipe>,
    stdin: Arc<Pipe>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

pub fn spawn(argv: &[String], opts: SpawnOptions) -> Result<Process, String> {
    if argv.is_empty() {
        return Err("expected a command to spawn".to_owned());
    }
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(Stdio::piped());
    if let Some(cwd) = &opts.cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }

    // merging means handing the child the same pipe as both stdout and
    // stderr, so the interleaving is the child's own write order
    let mut merged = None;
    if opts.merge_stderr {
        let (rx, tx) = std::io::pipe().map_err(|e| e.to_string())?;
        let tx2 = tx.try_clone().map_err(|e| e.to_string())?;
        cmd.stdout(tx).stderr(tx2);
        merged = Some(rx);
    } else {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    }

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    // drop our copies of the child's pipe ends now, or eof never comes
    drop(cmd);

    let stdout = Arc::new(Pipe::default());
    let stderr = Arc::new(Pipe::default());
    let stdin = Arc::new(Pipe::default());
    let mut threads = Vec::new();

    let out_src: Box<dyn Read + Send> = match merged {
        Some(rx) => Box::new(rx),
        None => Box::new(child.stdout.take().expect("stdout was piped")),
    };
    let pipe = stdout.clone();
    threads.push(
        std::thread::Builder::new()
            .name("wisp-proc-stdout".to_owned())
            .spawn(move || read_into(&pipe, out_src))
            .map_err(|e| e.to_string())?,
    );

    if let Some(err_src) = child.stderr.take() {
        let pipe = stderr.clone();
        threads.push(
            std::thread::Builder::new()
                .name("wisp-proc-stderr".to_owned())
                .spawn(move || read_into(&pipe, err_src))
                .map_err(|e| e.to_string())?,
        );
    } else {
        // merged: nothing ever arrives on stderr, report eof right away
        stderr.buf.lock().unwrap().closed = true;
    }

    let stdin_dst = child.stdin.take().expect("stdin was piped");
    let pipe = stdin.clone();
    threads.push(
        std::thread::Builder::new()
            .name("wisp-proc-stdin".to_owned())
            .spawn(move || write_from(&pipe, stdin_dst))
            .map_err(|e| e.to_string())?,
    );

    Ok(Process {
        child: Mutex::new(ChildState { child, exit: None }),
        stdout,
        stderr,
        stdin,
        threads: Mutex::new(threads),
    })
}

fn take(pipe: &Pipe, max: Option<usize>) -> Option<Vec<u8>> {
    let mut buf = pipe.buf.lock().unwrap();
    if buf.data.is_empty() {
        // an open-but-empty stream is an empty chunk; eof is none
        return if buf.closed { None } else { Some(Vec::new()) };
    }
    let n = max.unwrap_or(usize::MAX).min(buf.data.len());
    let out = buf.data.drain(..n).collect();
    pipe.cond.notify_all();
    Some(out)
}

impl Process {
    pub fn read_stdout(&self, max: Option<usize>) -> Option<Vec<u8>> {
        take(&self.stdout, max)
    }

    pub fn read_stderr(&self, max: Option<usize>) -> Option<Vec<u8>> {
        take(&self.stderr, max)
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut buf = self.stdin.buf.lock().unwrap();
        if buf.closed {
            return Err("stdin is closed".to_owned());
        }
        if buf.data.len() + data.len() > BUFFER_CAP {
            return Err("stdin buffer is full".to_owned());
        }
        buf.data.extend(data);
        self.stdin.cond.notify_all();
        Ok(())
    }

    pub fn close_stdin(&self) {
        self.stdin.buf.lock().unwrap().closed = true;
        self.stdin.cond.notify_all();
    }

    /// exit code, or none while running. a signal death reports -signal
    pub fn returncode(&self) -> Option<i32> {
        let mut st = self.child.lock().unwrap();
        if st.exit.is_none()
            && let Ok(Some(status)) = st.child.try_wait()
        {
            st.exit = Some(exit_code(status));
        }
        st.exit
    }

    pub fn running(&self) -> bool {
        self.returncode().is_none()
    }

    pub fn pid(&self) -> u32 {
        self.child.lock().unwrap().child.id()
    }

    /// polite: sigterm, so the child may clean up (plain kill elsewhere).
    /// checked against the un-reaped child, so the pid cannot have been
    /// reused by an unrelated process
    pub fn terminate(&self) -> Result<(), String> {
        let st = self.child.lock().unwrap();
        if st.exit.is_some() {
            return Err("process has exited".to_owned());
        }
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        kill(Pid::from_raw(st.child.id() as i32), Signal::SIGTERM).map_err(|e| e.to_string())
    }

    pub fn kill(&self) -> Result<(), String> {
        let mut st = self.child.lock().unwrap();
        if st.exit.is_some() {
            return Err("process has exited".to_owned());
        }
        st.child.kill().map_err(|e| e.to_string())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // no zombies and no orphans: a process the editor lets go of is
        // killed and reaped, matching lua gc semantics for the userdata
        {
            let mut st = self.child.lock().unwrap();
            if st.exit.is_none() {
                let _ = st.child.kill();
                if let Ok(status) = st.child.wait() {
                    st.exit = Some(exit_code(status));
                }
            }
        }
        for s in [&self.stdout, &self.stderr, &self.stdin] {
            let mut buf = s.buf.lock().unwrap();
            buf.closed = true;
            buf.data.clear();
            s.cond.notify_all();
        }
        for t in self.threads.lock().unwrap().drain(..) {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Vec<String> {
        vec!["sh".into(), "-c".into(), script.into()]
    }

    fn wait_exit(p: &Process) -> i32 {
        for _ in 0..1000 {
            if let Some(code) = p.returncode() {
                return code;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("process did not exit in time");
    }

    /// read stdout to eof; `None` is the only stop condition the
    /// consumer loop needs, that is the point of the convention
    fn drain(p: &Process) -> String {
        let mut out = Vec::new();
        loop {
            match p.read_stdout(None) {
                None => break,
                Some(chunk) if chunk.is_empty() => {
                    std::thread::sleep(std::time::Duration::from_millis(2))
                }
                Some(mut chunk) => out.append(&mut chunk),
            }
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn echo_writes_its_argument() {
        let p = spawn(&["echo".into(), "hello".into()], SpawnOptions::default()).unwrap();
        assert_eq!(drain(&p), "hello\n");
        assert_eq!(wait_exit(&p), 0);
    }

    #[test]
    fn a_filter_round_trips() {
        let p = spawn(&["cat".into()], SpawnOptions::default()).unwrap();
        p.write(b"ping\n").unwrap();
        p.close_stdin();
        assert_eq!(drain(&p), "ping\n");
        assert_eq!(wait_exit(&p), 0);
    }

    #[test]
    fn stderr_is_separate_by_default() {
        let p = spawn(&sh("echo out; echo err >&2"), SpawnOptions::default()).unwrap();
        assert_eq!(drain(&p), "out\n");
        wait_exit(&p);
        let mut err = Vec::new();
        while let Some(mut chunk) = p.read_stderr(None) {
            if chunk.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(2));
            } else {
                err.append(&mut chunk);
            }
        }
        assert_eq!(String::from_utf8(err).unwrap(), "err\n");
    }

    #[test]
    fn merged_stderr_interleaves_into_stdout() {
        let opts = SpawnOptions {
            merge_stderr: true,
            ..Default::default()
        };
        let p = spawn(&sh("echo out; echo err >&2"), opts).unwrap();
        assert_eq!(drain(&p), "out\nerr\n");
        // the merged stream's stderr reports eof immediately
        assert_eq!(p.read_stderr(None), None);
    }

    #[test]
    fn env_merges_over_the_parent() {
        // the parent's environment survives (PATH finds sh), the extra
        // variable arrives on top
        let opts = SpawnOptions {
            env: vec![("WISP_TEST_VAR".into(), "42".into())],
            ..Default::default()
        };
        let p = spawn(&sh("echo $WISP_TEST_VAR"), opts).unwrap();
        assert_eq!(drain(&p), "42\n");
    }

    #[test]
    fn cwd_is_respected() {
        let opts = SpawnOptions {
            cwd: Some("/".into()),
            ..Default::default()
        };
        let p = spawn(&["pwd".into()], opts).unwrap();
        assert_eq!(drain(&p), "/\n");
    }

    #[test]
    fn kill_reports_the_signal() {
        let p = spawn(&["sleep".into(), "30".into()], SpawnOptions::default()).unwrap();
        assert!(p.running());
        p.kill().unwrap();
        assert_eq!(wait_exit(&p), -9);
    }

    #[test]
    fn terminate_is_polite_but_effective() {
        let p = spawn(&["sleep".into(), "30".into()], SpawnOptions::default()).unwrap();
        p.terminate().unwrap();
        assert_eq!(wait_exit(&p), -15);
        // and both are honest about a corpse
        assert!(p.kill().is_err());
        assert!(p.terminate().is_err());
    }

    #[test]
    fn a_missing_binary_reports_cleanly() {
        match spawn(&["wisp-no-such-binary".into()], SpawnOptions::default()) {
            Ok(_) => panic!("a missing binary spawned"),
            Err(err) => assert!(!err.is_empty()),
        }
    }

    #[test]
    fn writing_to_a_dead_child_fails_instead_of_queueing() {
        let p = spawn(&["cat".into()], SpawnOptions::default()).unwrap();
        p.kill().unwrap();
        wait_exit(&p);
        // the writer thread notices the broken pipe; writes must start
        // failing rather than queue into the void forever
        for _ in 0..1000 {
            if p.write(b"x").is_err() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("writes kept succeeding against a dead child");
    }

    #[test]
    fn dropping_a_process_kills_and_reaps_it() {
        let p = spawn(&["sleep".into(), "30".into()], SpawnOptions::default()).unwrap();
        let pid = p.pid();
        drop(p);
        // the child is gone: signalling it now must fail
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        assert!(kill(Pid::from_raw(pid as i32), None).is_err());
    }
}
