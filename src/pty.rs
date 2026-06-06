//! PTY-backed persistent bash session.
//!
//! Provides [`BashPtySession`], a reusable shell abstraction that preserves state (cwd,
//! environment, functions, aliases, shell options) across calls and supports reliable
//! command framing with exit-status capture.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::SystemTime;

use tokio::io::unix::AsyncFd;

/// Configuration for spawning a new bash PTY session.
pub struct BashPtyConfig {
    /// Initial working directory for the shell.
    pub cwd: PathBuf,
    /// Environment variables to set in the shell.
    pub env: BTreeMap<String, String>,
    /// Path to the shell binary (default: `/bin/bash`).
    pub shell: PathBuf,
    /// When set, the spawned process is `wrapper[0]` with argv
    /// `[wrapper[0..], shell, --noprofile, --norc, --noediting, -i]`.
    /// Use this to run the shell inside a sandbox or other wrapper.
    pub shell_wrapper: Option<Vec<String>>,
    /// Terminal row count.
    pub rows: u16,
    /// Terminal column count.
    pub cols: u16,
    /// Timeout for waiting on a single command to complete.
    pub command_timeout: std::time::Duration,
}

impl Default for BashPtyConfig {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            env: BTreeMap::new(),
            shell: PathBuf::from("/bin/bash"),
            shell_wrapper: None,
            rows: 24,
            cols: 80,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }
}

/// The result of running a single command in a [`BashPtySession`].
#[derive(Debug)]
pub struct BashPtyResult {
    /// Combined terminal transcript with internal markers stripped.
    pub output: String,
    /// Exit status of the command.
    pub status: ExitStatus,
}

/// A persistent, PTY-backed bash session.
///
/// Commands execute inside the same long-lived shell process so that working
/// directory, environment variables, functions, aliases, and shell options
/// persist across calls.
pub struct BashPtySession {
    config: BashPtyConfig,
    inner: Option<SessionInner>,
}

struct SessionInner {
    master_fd: AsyncFd<OwnedFd>,
    child_pid: libc::pid_t,
    prompt_prefix: String,
}

// ---------------------------------------------------------------------------
// libc helpers
// ---------------------------------------------------------------------------

/// Opens a pseudo-terminal pair via `openpty(3)` and returns `(master, slave)` fds.
fn open_pty(rows: u16, cols: u16) -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    ws.ws_row = rows;
    ws.ws_col = cols;

    let ret = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &ws as *const libc::winsize as *mut libc::winsize,
        )
    };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) })
}

/// Puts the PTY into a deterministic transport mode for programmatic input.
///
/// Bash/readline and child processes can mutate terminal modes.  Reasserting
/// raw, no-echo behavior before command injection prevents the kernel line
/// discipline from editing command bytes and prevents those bytes from being
/// reflected into the output transcript.
fn configure_command_terminal<T: AsRawFd>(fd: &T) -> std::io::Result<()> {
    let raw = fd_raw(fd);
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(raw, &mut termios) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    termios.c_iflag &= !(libc::IGNBRK
        | libc::BRKINT
        | libc::PARMRK
        | libc::ISTRIP
        | libc::INLCR
        | libc::IGNCR
        | libc::ICRNL
        | libc::IXON);
    termios.c_oflag &= !libc::OPOST;
    termios.c_cflag |= libc::CS8;
    termios.c_lflag &= !(libc::ECHO
        | libc::ECHOE
        | libc::ECHOK
        | libc::ECHONL
        | libc::ICANON
        | libc::IEXTEN
        | libc::ISIG);
    termios.c_cc[libc::VMIN] = 1;
    termios.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(raw, libc::TCSANOW, &termios) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Sets the `O_NONBLOCK` flag on a file descriptor.
fn set_nonblock(fd: RawFd) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let ret = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if ret < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const POSIX_SPAWN_SETSID_FLAG: libc::c_short = 0x0400;

#[cfg(not(target_os = "macos"))]
const POSIX_SPAWN_SETSID_FLAG: libc::c_short = libc::POSIX_SPAWN_SETSID as libc::c_short;

#[cfg(target_os = "macos")]
unsafe fn posix_spawn_addchdir_np(
    actions: *mut libc::posix_spawn_file_actions_t,
    path: *const libc::c_char,
) -> libc::c_int {
    unsafe extern "C" {
        fn posix_spawn_file_actions_addchdir_np(
            actions: *mut libc::posix_spawn_file_actions_t,
            path: *const libc::c_char,
        ) -> libc::c_int;
    }

    unsafe { posix_spawn_file_actions_addchdir_np(actions, path) }
}

#[cfg(not(target_os = "macos"))]
unsafe fn posix_spawn_addchdir_np(
    actions: *mut libc::posix_spawn_file_actions_t,
    path: *const libc::c_char,
) -> libc::c_int {
    unsafe { libc::posix_spawn_file_actions_addchdir_np(actions, path) }
}

fn posix_ok(ret: libc::c_int) -> std::io::Result<()> {
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(ret))
    }
}

fn cstring_from_path(path: &std::path::Path, label: &str) -> std::io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{label} contains an interior NUL byte: {err}"),
        )
    })
}

fn shell_environment(env: &BTreeMap<String, String>) -> std::io::Result<Vec<CString>> {
    let mut effective_env = BTreeMap::new();
    effective_env.insert(
        "HOME".to_string(),
        std::env::var("HOME").unwrap_or_else(|_| "/".into()),
    );
    effective_env.insert(
        "PATH".to_string(),
        "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
    );
    effective_env.insert("TERM".to_string(), "dumb".to_string());
    effective_env.extend(env.iter().map(|(key, value)| (key.clone(), value.clone())));

    effective_env
        .into_iter()
        .map(|(key, value)| {
            CString::new(format!("{key}={value}")).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("environment entry {key:?} contains an interior NUL byte: {err}"),
                )
            })
        })
        .collect()
}

fn pty_slave_path(slave: &OwnedFd) -> std::io::Result<CString> {
    let mut buf = vec![0 as libc::c_char; 1024];
    let ret = unsafe { libc::ttyname_r(fd_raw(slave), buf.as_mut_ptr(), buf.len() - 1) };
    if ret != 0 {
        return Err(std::io::Error::from_raw_os_error(ret));
    }
    Ok(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_owned())
}

struct SpawnFileActions {
    inner: libc::posix_spawn_file_actions_t,
}

impl SpawnFileActions {
    fn new() -> std::io::Result<Self> {
        let mut inner = unsafe { std::mem::zeroed() };
        posix_ok(unsafe { libc::posix_spawn_file_actions_init(&mut inner) })?;
        Ok(Self { inner })
    }

    fn add_chdir(&mut self, path: &CStr) -> std::io::Result<()> {
        posix_ok(unsafe { posix_spawn_addchdir_np(&mut self.inner, path.as_ptr()) })
    }

    fn add_close(&mut self, fd: RawFd) -> std::io::Result<()> {
        posix_ok(unsafe { libc::posix_spawn_file_actions_addclose(&mut self.inner, fd) })
    }

    fn add_dup2(&mut self, src: RawFd, dst: RawFd) -> std::io::Result<()> {
        posix_ok(unsafe { libc::posix_spawn_file_actions_adddup2(&mut self.inner, src, dst) })
    }

    fn add_open(
        &mut self,
        fd: RawFd,
        path: &CStr,
        flags: libc::c_int,
        mode: libc::mode_t,
    ) -> std::io::Result<()> {
        posix_ok(unsafe {
            libc::posix_spawn_file_actions_addopen(&mut self.inner, fd, path.as_ptr(), flags, mode)
        })
    }

    fn as_ptr(&self) -> *const libc::posix_spawn_file_actions_t {
        &self.inner
    }
}

impl Drop for SpawnFileActions {
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawn_file_actions_destroy(&mut self.inner);
        }
    }
}

struct SpawnAttr {
    inner: libc::posix_spawnattr_t,
}

impl SpawnAttr {
    fn new() -> std::io::Result<Self> {
        let mut inner = unsafe { std::mem::zeroed() };
        posix_ok(unsafe { libc::posix_spawnattr_init(&mut inner) })?;
        Ok(Self { inner })
    }

    fn set_flags(&mut self, flags: libc::c_short) -> std::io::Result<()> {
        posix_ok(unsafe { libc::posix_spawnattr_setflags(&mut self.inner, flags) })
    }

    fn as_ptr(&self) -> *const libc::posix_spawnattr_t {
        &self.inner
    }
}

impl Drop for SpawnAttr {
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawnattr_destroy(&mut self.inner);
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt / framing
// ---------------------------------------------------------------------------

/// Default timeout for waiting on a command's completion prompt.
const DEFAULT_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// A random-ish prompt prefix unlikely to collide with command output.
const PROMPT_PREFIX: &str = "__CLAUDIUS_PTY_PROMPT__";

/// Prompt suffix after the numeric exit status.
const PROMPT_SUFFIX: &str = "__";

/// Readonly shell variable exposing the session nonce for diagnostics.
const PROMPT_NONCE_VAR: &str = "__claudius_prompt_nonce";

const OSC_TERMINATOR: char = '\u{7}';
const OSC133_PROMPT_START: &str = "\u{1b}]133;A\u{7}";
const OSC133_PROMPT_END: &str = "\u{1b}]133;B\u{7}";
const OSC133_COMMAND_START: &str = "\u{1b}]133;C\u{7}";
const OSC133_COMMAND_DONE_PREFIX: &str = "\u{1b}]133;D;";
const MAX_SPURIOUS_READY_RETRIES: usize = 1024;

fn prompt_prefix(tag: &str) -> String {
    format!("{PROMPT_PREFIX}{tag}_STATUS_")
}

fn prompt_string(prompt_prefix: &str) -> String {
    // Framing rides on readonly prompt state instead of a shell-visible helper
    // fd or mutable helper functions.  PS0 marks command start with OSC 133;C,
    // and PS1 emits OSC 133;D with the previous status plus a nonce-tagged
    // prompt line we can parse back out of the PTY transcript.
    format!(
        "{OSC133_COMMAND_DONE_PREFIX}$?{OSC_TERMINATOR}{OSC133_PROMPT_START}{prompt_prefix}$?{PROMPT_SUFFIX}\\n{OSC133_PROMPT_END}"
    )
}

fn bash_ansi_c_quote(s: &str) -> String {
    let mut quoted = String::from("$'");
    for ch in s.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '\'' => quoted.push_str("\\'"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\u{1b}' => quoted.push_str("\\e"),
            '\u{7}' => quoted.push_str("\\a"),
            c if c.is_ascii() && !c.is_ascii_control() => quoted.push(c),
            c if c.is_ascii() => write!(quoted, "\\x{:02x}", c as u8).expect("write ascii escape"),
            c => quoted.push(c),
        }
    }
    quoted.push('\'');
    quoted
}

enum ReadUntilPrompt {
    Prompt { output: String, exit_code: i32 },
    Eof { output: String },
}

struct BashPtyLineStreamer {
    output_tx: tokio::sync::mpsc::UnboundedSender<String>,
    sent_lines: usize,
}

impl BashPtyLineStreamer {
    fn new(output_tx: tokio::sync::mpsc::UnboundedSender<String>) -> Self {
        Self {
            output_tx,
            sent_lines: 0,
        }
    }

    fn emit_available(&mut self, raw_output: &str, final_output: bool) {
        let lines = clean_output_lines_for_streaming(raw_output, final_output);
        for line in lines.iter().skip(self.sent_lines) {
            let _ = self.output_tx.send(line.clone());
        }
        self.sent_lines = lines.len();
    }
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl BashPtySession {
    /// Creates a new session from the given configuration, spawning the shell.
    pub async fn new(config: BashPtyConfig) -> std::io::Result<Self> {
        let mut session = Self {
            config,
            inner: None,
        };
        session.spawn_and_drain().await?;
        Ok(session)
    }

    /// Runs `command` in the persistent shell.
    ///
    /// If `restart` is `true`, any live session is discarded and a fresh shell is
    /// started before executing the command.  Nonzero exit codes are reported in
    /// [`BashPtyResult::status`]; `Err` is only returned for transport failures.
    pub async fn run(&mut self, command: &str, restart: bool) -> std::io::Result<BashPtyResult> {
        self.run_inner(command, restart, None).await
    }

    /// Runs `command` and sends cleaned PTY output lines as they become available.
    ///
    /// Each channel item is one line without its line terminator.  The sequence
    /// of sent lines matches `BashPtyResult::output.lines()` for the returned
    /// result, including interior blank lines and excluding trailing blank
    /// lines.  If the receiver is dropped, command execution continues and the
    /// final [`BashPtyResult`] is still returned.
    pub async fn run_with_output_channel(
        &mut self,
        command: &str,
        restart: bool,
        output_tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> std::io::Result<BashPtyResult> {
        self.run_inner(command, restart, Some(output_tx)).await
    }

    async fn run_inner(
        &mut self,
        command: &str,
        restart: bool,
        output_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> std::io::Result<BashPtyResult> {
        if restart {
            self.kill_inner();
        }

        // Lazily (re-)spawn if the shell is dead.
        if !self.is_alive_inner() {
            self.spawn_and_drain().await?;
        }

        if let Some(ref inner) = self.inner {
            configure_command_terminal(&inner.master_fd)?;
            drain_available(&inner.master_fd)?;
        }

        let inner = self.inner.as_ref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "shell session is not alive")
        })?;

        let mut input = String::with_capacity(command.len() + 8);
        input.push_str("{\n");
        if command.is_empty() {
            input.push_str(":\n");
        } else {
            input.push_str(command);
        }
        if !command.is_empty() && !command.ends_with('\n') {
            input.push('\n');
        }
        input.push_str("}\n");

        // Command text is written directly to the shell's stdin over the PTY.
        // Command completion is detected by readonly prompt state plus OSC 133
        // markers, so framing does not depend on mutable shell variables or
        // helper builtins like `printf`.
        pty_write_all(&inner.master_fd, input.as_bytes()).await?;

        match read_until_prompt(
            &inner.master_fd,
            &inner.prompt_prefix,
            self.config.command_timeout,
            output_tx,
        )
        .await?
        {
            ReadUntilPrompt::Prompt { output, exit_code } => Ok(BashPtyResult {
                output: clean_output(&output),
                status: exit_status_from_code(exit_code),
            }),
            ReadUntilPrompt::Eof { output } => {
                let status = self.take_exited_status()?;
                match status {
                    Some(status) if status.signal().is_some() => Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!(
                            "shell exited before completion marker with {}; partial transcript: {}",
                            describe_exit_status(status),
                            output.escape_debug()
                        ),
                    )),
                    Some(status) => Ok(BashPtyResult {
                        output: clean_output(&output),
                        status,
                    }),
                    None => Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!(
                            "PTY master EOF before prompt; partial transcript: {}",
                            output.escape_debug()
                        ),
                    )),
                }
            }
        }
    }

    /// Closes the session, killing the shell process if alive.
    pub async fn close(&mut self) -> std::io::Result<()> {
        self.kill_inner();
        Ok(())
    }

    /// Returns `true` if the underlying shell process is still running.
    pub fn is_alive(&mut self) -> std::io::Result<bool> {
        Ok(self.is_alive_inner())
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Spawns a new shell and waits for it to initialize.
    async fn spawn_and_drain(&mut self) -> std::io::Result<()> {
        self.spawn_shell()?;
        if let Some(ref inner) = self.inner {
            match read_until_prompt(
                &inner.master_fd,
                &inner.prompt_prefix,
                self.config.command_timeout,
                None,
            )
            .await?
            {
                ReadUntilPrompt::Prompt { .. } => {}
                ReadUntilPrompt::Eof { output } => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!(
                            "shell exited during setup; partial transcript: {}",
                            output.escape_debug()
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn spawn_shell(&mut self) -> std::io::Result<()> {
        let shell_path = self.config.shell.clone();
        let cwd = self.config.cwd.clone();
        let env = self.config.env.clone();
        let prompt_nonce = rand_nonce().to_string();
        let prompt_prefix = prompt_prefix(&prompt_nonce);
        let prompt = prompt_string(&prompt_prefix);
        let shell_c = cstring_from_path(&shell_path, "shell path")?;
        let cwd_c = cstring_from_path(&cwd, "working directory")?;
        let arg_noprofile = CString::new("--noprofile").unwrap();
        let arg_norc = CString::new("--norc").unwrap();
        let arg_noediting = CString::new("--noediting").unwrap();
        let arg_i = CString::new("-i").unwrap();
        let envp_storage = shell_environment(&env)?;

        // If a wrapper is configured, the spawned binary is wrapper[0] and
        // the full argv is
        // [wrapper[0], wrapper[1..], shell, --noprofile, --norc, --noediting, -i].
        let (spawn_path, extra_args_storage);
        if let Some(ref wrapper) = self.config.shell_wrapper {
            assert!(!wrapper.is_empty(), "shell_wrapper must not be empty");
            spawn_path = PathBuf::from(&wrapper[0]);
            extra_args_storage = wrapper[1..]
                .iter()
                .map(|arg| CString::new(arg.as_bytes()).unwrap())
                .collect::<Vec<_>>();
        } else {
            spawn_path = shell_path.clone();
            extra_args_storage = vec![];
        }
        let spawn_c = cstring_from_path(&spawn_path, "spawn path")?;

        let mut argv_storage = vec![spawn_c.clone()];
        for arg in &extra_args_storage {
            argv_storage.push(arg.clone());
        }
        if self.config.shell_wrapper.is_some() {
            argv_storage.push(shell_c.clone());
        }
        argv_storage.push(arg_noprofile.clone());
        argv_storage.push(arg_norc.clone());
        argv_storage.push(arg_noediting.clone());
        argv_storage.push(arg_i.clone());

        let mut argv: Vec<*mut libc::c_char> = argv_storage
            .iter()
            .map(|s| s.as_ptr() as *mut libc::c_char)
            .collect();
        argv.push(std::ptr::null_mut());

        let mut envp = envp_storage
            .iter()
            .map(|entry| entry.as_ptr() as *mut libc::c_char)
            .collect::<Vec<_>>();
        envp.push(std::ptr::null_mut());

        let (master, slave) = open_pty(self.config.rows, self.config.cols)?;
        let slave_path = pty_slave_path(&slave)?;

        let mut file_actions = SpawnFileActions::new()?;
        file_actions.add_chdir(&cwd_c)?;
        file_actions.add_close(fd_raw(&master))?;
        file_actions.add_close(fd_raw(&slave))?;
        file_actions.add_open(0, &slave_path, libc::O_RDWR, 0)?;
        file_actions.add_dup2(0, 1)?;
        file_actions.add_dup2(0, 2)?;

        let mut attr = SpawnAttr::new()?;
        attr.set_flags(POSIX_SPAWN_SETSID_FLAG)?;

        let mut pid = 0;
        let ret = unsafe {
            libc::posix_spawnp(
                &mut pid,
                spawn_c.as_ptr(),
                file_actions.as_ptr(),
                attr.as_ptr(),
                argv.as_mut_ptr(),
                envp.as_mut_ptr(),
            )
        };
        posix_ok(ret)?;

        drop(slave);

        let setup = format!(
            "{prompt_nonce_var}={prompt_nonce}; \
             readonly {prompt_nonce_var}; \
             PS0={ps0}; \
             PS1={ps1}; \
             PS2=''; \
             PROMPT_COMMAND=''; \
             readonly PS0; \
             readonly PS1; \
             readonly PS2; \
             readonly PROMPT_COMMAND\n",
            prompt_nonce_var = PROMPT_NONCE_VAR,
            prompt_nonce = bash_ansi_c_quote(&prompt_nonce),
            ps0 = bash_ansi_c_quote(OSC133_COMMAND_START),
            ps1 = bash_ansi_c_quote(&prompt),
        );
        let master_fd = match (|| -> std::io::Result<AsyncFd<OwnedFd>> {
            configure_command_terminal(&master)?;
            blocking_write(&master, setup.as_bytes())?;
            set_nonblock(fd_raw(&master))?;
            AsyncFd::new(master)
        })() {
            Ok(master_fd) => master_fd,
            Err(err) => {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                    libc::waitpid(pid, std::ptr::null_mut(), 0);
                }
                return Err(err);
            }
        };

        self.inner = Some(SessionInner {
            master_fd,
            child_pid: pid,
            prompt_prefix,
        });
        Ok(())
    }

    fn is_alive_inner(&mut self) -> bool {
        if let Some(ref inner) = self.inner {
            let ret =
                unsafe { libc::waitpid(inner.child_pid, std::ptr::null_mut(), libc::WNOHANG) };
            if ret == 0 {
                return true;
            }
            // Child exited or error.
            self.inner = None;
            false
        } else {
            false
        }
    }

    fn kill_inner(&mut self) {
        if let Some(inner) = self.inner.take() {
            unsafe {
                libc::kill(inner.child_pid, libc::SIGKILL);
                libc::waitpid(inner.child_pid, std::ptr::null_mut(), 0);
            }
            // master_fd is dropped here, closing the PTY.
        }
    }

    fn take_exited_status(&mut self) -> std::io::Result<Option<ExitStatus>> {
        let Some(inner) = self.inner.take() else {
            return Ok(None);
        };
        let mut raw_status = 0;
        let ret = unsafe { libc::waitpid(inner.child_pid, &mut raw_status, 0) };
        if ret == inner.child_pid {
            Ok(Some(ExitStatus::from_raw(raw_status)))
        } else if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ECHILD) {
                Ok(None)
            } else {
                Err(err)
            }
        } else {
            Err(std::io::Error::other(format!(
                "waitpid({}) returned unexpected pid {}",
                inner.child_pid, ret
            )))
        }
    }
}

impl Drop for BashPtySession {
    fn drop(&mut self) {
        self.kill_inner();
    }
}

// ---------------------------------------------------------------------------
// Async PTY I/O helpers
// ---------------------------------------------------------------------------

/// Extracts the raw fd from an OwnedFd without consuming it.
fn fd_raw<T: AsRawFd>(fd: &T) -> RawFd {
    fd.as_raw_fd()
}

fn fd_write<T: AsRawFd>(fd: &T, buf: &[u8]) -> std::io::Result<usize> {
    let n = unsafe { libc::write(fd_raw(fd), buf.as_ptr() as *const libc::c_void, buf.len()) };
    if n < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

fn fd_read<T: AsRawFd>(fd: &T, buf: &mut [u8]) -> std::io::Result<usize> {
    let n = unsafe { libc::read(fd_raw(fd), buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if n < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Write `buf` to the master fd using blocking I/O (suitable for small writes
/// before the fd is set to non-blocking).
fn blocking_write<T: AsRawFd>(fd: &T, buf: &[u8]) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        match fd_write(fd, &buf[offset..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "failed to write PTY setup data",
                ));
            }
            Ok(written) => offset += written,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Writes all of `buf` to the PTY master fd using reactor-driven readiness.
async fn pty_write_all(fd: &AsyncFd<OwnedFd>, buf: &[u8]) -> std::io::Result<()> {
    let mut offset = 0;
    let mut spurious_ready_retries = 0usize;
    while offset < buf.len() {
        let mut guard = fd.writable().await?;
        match guard.try_io(|inner| fd_write(inner.get_ref(), &buf[offset..])) {
            Ok(Ok(0)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "failed to write command input to PTY",
                ));
            }
            Ok(Ok(written)) => {
                offset += written;
                spurious_ready_retries = 0;
            }
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Ok(Err(err)) => return Err(err),
            Err(_) => {
                // `try_io` returns `Err` on a spurious readiness notification
                // after clearing the ready flag, so the next `writable().await`
                // will sleep until the fd becomes writable again.
                spurious_ready_retries += 1;
                if spurious_ready_retries >= MAX_SPURIOUS_READY_RETRIES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "PTY master reported spurious writable readiness too many times",
                    ));
                }
                continue;
            }
        }
    }
    Ok(())
}

/// Returns `true` if the error represents an EIO on the PTY master, which on Linux signals that
/// the slave side has been closed (equivalent to EOF on macOS).
fn is_pty_eof(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(libc::EIO)
}

/// Reads available bytes from the PTY master fd, waiting on readability.
/// Returns `Ok(bytes)` or an error.
async fn pty_read(fd: &AsyncFd<OwnedFd>) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; 8192];
    loop {
        let mut guard = fd.readable().await?;
        match guard.try_io(|inner| fd_read(inner.get_ref(), &mut buf)) {
            Ok(Ok(0)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "PTY master EOF: shell exited",
                ));
            }
            Ok(Ok(read)) => {
                buf.truncate(read);
                return Ok(buf);
            }
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Ok(Err(err)) if is_pty_eof(&err) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "PTY master EIO: shell exited",
                ));
            }
            Ok(Err(err)) => return Err(err),
            Err(_) => continue,
        }
    }
}

/// Drains all currently available output from the PTY (non-blocking).
fn drain_available(fd: &AsyncFd<OwnedFd>) -> std::io::Result<()> {
    let mut buf = vec![0u8; 8192];
    loop {
        match fd_read(fd.get_ref(), &mut buf) {
            Ok(0) => return Ok(()),
            Ok(_) => continue,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) if is_pty_eof(&err) => return Ok(()),
            Err(err) => return Err(err),
        }
    }
}

/// Reads from the PTY until the bash prompt sentinel appears or the shell exits.
async fn read_until_prompt(
    fd: &AsyncFd<OwnedFd>,
    prompt_prefix: &str,
    timeout: std::time::Duration,
    output_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> std::io::Result<ReadUntilPrompt> {
    let mut accumulated = Vec::new();
    let mut line_streamer = output_tx.map(BashPtyLineStreamer::new);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some((output, exit_code)) = parse_prompt_output_bytes(&accumulated, prompt_prefix) {
            if let Some(ref mut line_streamer) = line_streamer {
                line_streamer.emit_available(&output, true);
            }
            return Ok(ReadUntilPrompt::Prompt { output, exit_code });
        }

        let chunk = match tokio::time::timeout_at(deadline, pty_read(fd)).await {
            Ok(Ok(chunk)) => chunk,
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                let output = String::from_utf8_lossy(&accumulated).into_owned();
                if let Some(ref mut line_streamer) = line_streamer {
                    line_streamer.emit_available(&output, true);
                }
                return Ok(ReadUntilPrompt::Eof { output });
            }
            Ok(Err(err)) => {
                let transcript = String::from_utf8_lossy(&accumulated);
                return Err(std::io::Error::new(
                    err.kind(),
                    format!("{}; partial transcript: {}", err, transcript.escape_debug()),
                ));
            }
            Err(_) => {
                let transcript = String::from_utf8_lossy(&accumulated);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "timed out waiting for prompt; partial transcript: {}",
                        transcript.escape_debug()
                    ),
                ));
            }
        };

        accumulated.extend_from_slice(&chunk);
        if let Some((output, exit_code)) = parse_prompt_output_bytes(&accumulated, prompt_prefix) {
            if let Some(ref mut line_streamer) = line_streamer {
                line_streamer.emit_available(&output, true);
            }
            return Ok(ReadUntilPrompt::Prompt { output, exit_code });
        }
        if let Some(ref mut line_streamer) = line_streamer {
            let output = streamable_output_before_prompt(&accumulated);
            line_streamer.emit_available(&output, false);
        }
    }
}

// ---------------------------------------------------------------------------
// Output parsing
// ---------------------------------------------------------------------------

fn streamable_output_before_prompt(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let end = text.rfind(OSC133_COMMAND_DONE_PREFIX).unwrap_or(text.len());
    text[..end].to_string()
}

#[cfg(test)]
/// Splits the trailing prompt sentinel from the PTY transcript.
fn parse_prompt_output<'a>(raw: &'a str, prompt_prefix: &str) -> Option<(&'a str, i32)> {
    let idx = raw.rfind(prompt_prefix)?;
    let after_prefix = &raw[idx + prompt_prefix.len()..];
    let suffix_idx = after_prefix.find(PROMPT_SUFFIX)?;
    let code_str = &after_prefix[..suffix_idx];
    if code_str.is_empty() || !code_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let exit_code = code_str.parse::<i32>().ok()?;
    Some((&raw[..idx], exit_code))
}

fn parse_prompt_output_bytes(raw: &[u8], prompt_prefix: &str) -> Option<(String, i32)> {
    let text = String::from_utf8_lossy(raw);
    let idx = text.rfind(OSC133_COMMAND_DONE_PREFIX)?;
    let after_done = &text[idx + OSC133_COMMAND_DONE_PREFIX.len()..];
    let terminator_idx = after_done.find(OSC_TERMINATOR)?;
    let done_code_str = &after_done[..terminator_idx];
    if done_code_str.is_empty() || !done_code_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let done_exit_code = done_code_str.parse::<i32>().ok()?;

    let after_done = &after_done[terminator_idx + OSC_TERMINATOR.len_utf8()..];
    let after_prompt_start = after_done.strip_prefix(OSC133_PROMPT_START)?;
    let after_prefix = after_prompt_start.strip_prefix(prompt_prefix)?;
    let suffix_idx = after_prefix.find(PROMPT_SUFFIX)?;
    let code_str = &after_prefix[..suffix_idx];
    if code_str.is_empty() || !code_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let exit_code = code_str.parse::<i32>().ok()?;
    if exit_code != done_exit_code {
        return None;
    }

    let after_suffix = &after_prefix[suffix_idx + PROMPT_SUFFIX.len()..];
    let after_crs = after_suffix.trim_start_matches('\r');
    let after_newline = after_crs.strip_prefix('\n')?;
    after_newline.strip_prefix(OSC133_PROMPT_END)?;

    let output = &text[..idx];
    Some((output.to_string(), exit_code))
}

fn strip_internal_markers(s: &str) -> String {
    let mut stripped = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = rest.find('\u{1b}') {
        stripped.push_str(&rest[..idx]);
        rest = &rest[idx..];
        if let Some(after_marker) = strip_osc133_marker(rest) {
            rest = after_marker;
        } else if let Some(ch) = rest.chars().next() {
            stripped.push(ch);
            rest = &rest[ch.len_utf8()..];
        } else {
            break;
        }
    }
    stripped.push_str(rest);
    stripped
}

fn strip_osc133_marker(s: &str) -> Option<&str> {
    if let Some(rest) = s.strip_prefix(OSC133_PROMPT_START) {
        return Some(rest);
    }
    if let Some(rest) = s.strip_prefix(OSC133_PROMPT_END) {
        return Some(rest);
    }
    if let Some(rest) = s.strip_prefix(OSC133_COMMAND_START) {
        return Some(rest);
    }
    let rest = s.strip_prefix(OSC133_COMMAND_DONE_PREFIX)?;
    let idx = rest.find(OSC_TERMINATOR)?;
    let code_str = &rest[..idx];
    if code_str.is_empty() || !code_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(&rest[idx + OSC_TERMINATOR.len_utf8()..])
}

/// Removes terminal control noise (carriage returns, trailing whitespace).
fn clean_output(s: &str) -> String {
    clean_output_lines(s).join("\n")
}

fn clean_output_lines(s: &str) -> Vec<String> {
    let stripped = strip_internal_markers(s);
    clean_output_lines_from_stripped(&stripped)
}

fn clean_output_lines_for_streaming(s: &str, final_output: bool) -> Vec<String> {
    let stripped = strip_internal_markers(s);
    let mut lines = clean_output_lines_from_stripped(&stripped);
    if !final_output && !stripped.ends_with('\n') && !lines.is_empty() {
        lines.pop();
    }
    lines
}

fn clean_output_lines_from_stripped(stripped: &str) -> Vec<String> {
    let mut lines: Vec<&str> = stripped.lines().collect();
    // Trim trailing empty lines.
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines
        .iter()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect::<Vec<_>>()
}

fn describe_exit_status(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        format!("exit code {code}")
    } else if let Some(signal) = status.signal() {
        format!("signal {signal}")
    } else {
        format!("{status:?}")
    }
}

/// Constructs an `ExitStatus` from a raw exit code.
fn exit_status_from_code(code: i32) -> ExitStatus {
    ExitStatus::from_raw(code << 8)
}

/// Simple nonce generation from PID + time.
fn rand_nonce() -> u64 {
    let pid = std::process::id() as u64;
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    pid.wrapping_mul(6364136223846793005).wrapping_add(ts)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::os::fd::OwnedFd;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::io::FromRawFd;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    struct InvalidFd;

    impl AsRawFd for InvalidFd {
        fn as_raw_fd(&self) -> RawFd {
            -1
        }
    }

    async fn make_session() -> BashPtySession {
        make_session_with_timeout(Duration::from_secs(10)).await
    }

    async fn make_session_with_timeout(command_timeout: Duration) -> BashPtySession {
        let config = BashPtyConfig {
            cwd: std::env::current_dir().unwrap(),
            command_timeout,
            ..Default::default()
        };
        BashPtySession::new(config).await.expect("spawn session")
    }

    fn assert_bash_pty_result_eq(actual: &BashPtyResult, expected: BashPtyResult) {
        assert_eq!(
            actual.output, expected.output,
            "unexpected PTY transcript: actual={:?} expected={:?}",
            actual.output, expected.output
        );
        assert_eq!(
            actual.status, expected.status,
            "unexpected PTY exit status: actual={:?} expected={:?}",
            actual.status, expected.status
        );
    }

    async fn collect_streamed_lines(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(line) = rx.recv().await {
            lines.push(line);
        }
        lines
    }

    fn make_pipe() -> (OwnedFd, OwnedFd) {
        let mut fds = [-1; 2];
        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(ret, 0, "pipe failed: {}", std::io::Error::last_os_error());
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }

    fn assert_raw_os_error(err: &std::io::Error, errno: i32) {
        assert_eq!(
            err.raw_os_error(),
            Some(errno),
            "expected errno {errno}, got {err:?}"
        );
    }

    fn read_exact_bytes(fd: &OwnedFd, len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        let mut offset = 0;
        while offset < len {
            match fd_read(fd, &mut buf[offset..]) {
                Ok(0) => panic!("unexpected EOF after reading {offset} of {len} bytes"),
                Ok(read) => offset += read,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => panic!("read failed: {err}"),
            }
        }
        buf
    }

    fn get_fd_flags<T: AsRawFd>(fd: &T) -> i32 {
        let flags = unsafe { libc::fcntl(fd_raw(fd), libc::F_GETFL) };
        assert!(
            flags >= 0,
            "fcntl(F_GETFL) failed: {}",
            std::io::Error::last_os_error()
        );
        flags
    }

    fn get_termios<T: AsRawFd>(fd: &T) -> libc::termios {
        let mut termios = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::tcgetattr(fd_raw(fd), &mut termios) };
        assert_eq!(
            ret,
            0,
            "tcgetattr failed: {}",
            std::io::Error::last_os_error()
        );
        termios
    }

    fn get_winsize<T: AsRawFd>(fd: &T) -> libc::winsize {
        let mut ws = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::ioctl(fd_raw(fd), libc::TIOCGWINSZ, &mut ws) };
        assert_eq!(
            ret,
            0,
            "ioctl(TIOCGWINSZ) failed: {}",
            std::io::Error::last_os_error()
        );
        ws
    }

    fn env_entries(entries: Vec<CString>) -> BTreeMap<String, String> {
        entries
            .into_iter()
            .map(|entry| {
                let entry = entry
                    .into_string()
                    .expect("environment entry should be valid UTF-8");
                let (key, value) = entry
                    .split_once('=')
                    .expect("environment entry should contain '='");
                (key.to_string(), value.to_string())
            })
            .collect()
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "claudius-pty-{label}-{}-{}",
            std::process::id(),
            rand_nonce()
        ));
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    fn physical_path(path: &Path) -> PathBuf {
        fs::canonicalize(path).expect("canonicalize path")
    }

    #[test]
    fn default_config_has_expected_defaults() {
        let config = BashPtyConfig::default();
        let expected_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        assert_eq!(config.cwd, expected_cwd);
        assert_eq!(config.env, BTreeMap::new());
        assert_eq!(config.shell, PathBuf::from("/bin/bash"));
        assert_eq!(config.shell_wrapper, None);
        assert_eq!(config.rows, 24);
        assert_eq!(config.cols, 80);
        assert_eq!(config.command_timeout, DEFAULT_COMMAND_TIMEOUT);
    }

    #[test]
    fn posix_ok_returns_os_error_for_nonzero_errno() {
        let err = posix_ok(libc::ENOENT).expect_err("nonzero errno should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert_raw_os_error(&err, libc::ENOENT);
    }

    #[test]
    fn prompt_prefix_formats_tag() {
        assert_eq!(
            prompt_prefix("nonce"),
            "__CLAUDIUS_PTY_PROMPT__nonce_STATUS_"
        );
    }

    #[test]
    fn prompt_string_contains_expected_markers() {
        let prefix = prompt_prefix("nonce");
        let prompt = prompt_string(&prefix);
        assert_eq!(
            prompt,
            format!(
                "{OSC133_COMMAND_DONE_PREFIX}$?{OSC_TERMINATOR}{OSC133_PROMPT_START}{prefix}$?{PROMPT_SUFFIX}\\n{OSC133_PROMPT_END}"
            )
        );
    }

    #[test]
    fn bash_ansi_c_quote_escapes_special_characters() {
        assert_eq!(
            bash_ansi_c_quote("a\\'\n\r\u{1b}\u{7}\u{1f}z"),
            "$'a\\\\\\'\\n\\r\\e\\a\\x1fz'"
        );
    }

    #[test]
    fn bash_ansi_c_quote_preserves_plain_ascii_and_unicode() {
        assert_eq!(bash_ansi_c_quote("plain snowman ☃"), "$'plain snowman ☃'");
    }

    #[test]
    fn parse_prompt_output_ignores_trailing_text_after_marker() {
        let prefix = prompt_prefix("marker");
        let raw = format!("payload{prefix}7__prompt>");
        assert_eq!(parse_prompt_output(&raw, &prefix), Some(("payload", 7)));
    }

    #[test]
    fn parse_prompt_output_rejects_empty_status() {
        let prefix = prompt_prefix("marker");
        let raw = format!("payload{prefix}{PROMPT_SUFFIX}");
        assert_eq!(parse_prompt_output(&raw, &prefix), None);
    }

    #[test]
    fn parse_prompt_output_rejects_nondigit_status() {
        let prefix = prompt_prefix("marker");
        let raw = format!("payload{prefix}x{PROMPT_SUFFIX}");
        assert_eq!(parse_prompt_output(&raw, &prefix), None);
    }

    #[test]
    fn parse_prompt_output_requires_prompt_suffix() {
        let prefix = prompt_prefix("marker");
        let raw = format!("payload{prefix}7");
        assert_eq!(parse_prompt_output(&raw, &prefix), None);
    }

    #[test]
    fn parse_prompt_output_requires_matching_prefix() {
        let prefix = prompt_prefix("marker");
        let raw = "__CLAUDIUS_PTY_PROMPT__other_STATUS_7__";
        assert_eq!(parse_prompt_output(raw, &prefix), None);
    }

    #[test]
    fn parse_prompt_output_uses_last_marker() {
        let prefix = prompt_prefix("marker");
        let raw = format!("alpha{prefix}1__beta{prefix}2__prompt>");
        let expected = format!("alpha{prefix}1__beta");
        assert_eq!(
            parse_prompt_output(&raw, &prefix),
            Some((expected.as_str(), 2))
        );
    }

    #[test]
    fn parse_prompt_output_bytes_decodes_lossy_utf8() {
        let prefix = prompt_prefix("marker");
        let mut raw = vec![0xff, b'a'];
        raw.extend_from_slice(
            format!(
                "{OSC133_COMMAND_DONE_PREFIX}7{OSC_TERMINATOR}{OSC133_PROMPT_START}{prefix}7{PROMPT_SUFFIX}\n{OSC133_PROMPT_END}"
            )
            .as_bytes(),
        );
        assert_eq!(
            parse_prompt_output_bytes(&raw, &prefix),
            Some(("\u{fffd}a".to_string(), 7))
        );
    }

    #[test]
    fn parse_prompt_output_bytes_rejects_unframed_prompt_prefix() {
        let prefix = prompt_prefix("marker");
        let raw = format!("payload{prefix}7__");
        assert_eq!(parse_prompt_output_bytes(raw.as_bytes(), &prefix), None);
    }

    #[test]
    fn streamable_output_before_prompt_withholds_incomplete_prompt_suffix() {
        let prefix = prompt_prefix("marker");
        let raw = format!(
            "ready\ndone{OSC133_COMMAND_DONE_PREFIX}0{OSC_TERMINATOR}{OSC133_PROMPT_START}{prefix}0{PROMPT_SUFFIX}\n"
        );
        assert_eq!(
            streamable_output_before_prompt(raw.as_bytes()),
            "ready\ndone"
        );
    }

    #[test]
    fn strip_osc133_marker_strips_known_markers() {
        assert_eq!(
            strip_osc133_marker(&format!("{OSC133_PROMPT_START}rest")),
            Some("rest")
        );
        assert_eq!(
            strip_osc133_marker(&format!("{OSC133_PROMPT_END}rest")),
            Some("rest")
        );
        assert_eq!(
            strip_osc133_marker(&format!("{OSC133_COMMAND_START}rest")),
            Some("rest")
        );
        assert_eq!(
            strip_osc133_marker(&format!(
                "{OSC133_COMMAND_DONE_PREFIX}7{OSC_TERMINATOR}rest"
            )),
            Some("rest")
        );
    }

    #[test]
    fn strip_osc133_marker_rejects_malformed_done_marker() {
        assert_eq!(
            strip_osc133_marker(&format!(
                "{OSC133_COMMAND_DONE_PREFIX}x{OSC_TERMINATOR}rest"
            )),
            None
        );
        assert_eq!(
            strip_osc133_marker(&format!("{OSC133_COMMAND_DONE_PREFIX}{OSC_TERMINATOR}rest")),
            None
        );
    }

    #[test]
    fn strip_osc133_marker_requires_done_terminator() {
        assert_eq!(
            strip_osc133_marker(&format!("{OSC133_COMMAND_DONE_PREFIX}7rest")),
            None
        );
    }

    #[test]
    fn strip_internal_markers_preserves_unknown_escape_sequences_and_malformed_markers() {
        let raw =
            format!("\u{1b}[31mred\u{1b}[0m {OSC133_COMMAND_DONE_PREFIX}x{OSC_TERMINATOR}tail");
        assert_eq!(strip_internal_markers(&raw), raw);
    }

    #[test]
    fn clean_output_strips_osc133_markers() {
        let raw = format!(
            "{OSC133_COMMAND_START}payload{OSC133_COMMAND_DONE_PREFIX}7{OSC_TERMINATOR}{OSC133_PROMPT_START}prompt{OSC133_PROMPT_END}\r\n"
        );
        assert_eq!(clean_output(&raw), "payloadprompt");
    }

    #[test]
    fn clean_output_trims_trailing_blank_lines_and_carriage_returns() {
        let raw = "alpha\r\n\r\nbeta\r\n\r\n";
        assert_eq!(clean_output(raw), "alpha\n\nbeta");
    }

    #[test]
    fn clean_output_preserves_other_escape_sequences() {
        let raw = format!(
            "{OSC133_COMMAND_START}\u{1b}[31mred\u{1b}[0m{OSC133_PROMPT_START}{OSC133_PROMPT_END}\r\n"
        );
        assert_eq!(clean_output(&raw), "\u{1b}[31mred\u{1b}[0m");
    }

    #[test]
    fn clean_output_drops_all_blank_lines_when_nothing_remains() {
        assert_eq!(clean_output("\r\n \r\n\t\r\n"), "");
    }

    #[test]
    fn clean_output_lines_for_streaming_buffers_incomplete_and_trailing_blank_lines() {
        let raw = format!("{OSC133_COMMAND_START}alpha\r\n\r\nbeta");
        assert_eq!(
            clean_output_lines_for_streaming(&raw, false),
            vec!["alpha".to_string(), String::new()]
        );

        let raw = format!("{OSC133_COMMAND_START}alpha\r\n\r\n");
        assert_eq!(
            clean_output_lines_for_streaming(&raw, false),
            vec!["alpha".to_string()]
        );
        assert_eq!(
            clean_output_lines_for_streaming(&raw, true),
            vec!["alpha".to_string()]
        );
    }

    #[test]
    fn describe_exit_status_formats_exit_codes() {
        assert_eq!(
            describe_exit_status(exit_status_from_code(17)),
            "exit code 17"
        );
    }

    #[test]
    fn describe_exit_status_formats_signals() {
        assert_eq!(
            describe_exit_status(ExitStatus::from_raw(libc::SIGKILL)),
            format!("signal {}", libc::SIGKILL)
        );
    }

    #[test]
    fn exit_status_from_code_round_trips_code() {
        assert_eq!(exit_status_from_code(23).code(), Some(23));
    }

    #[test]
    fn cstring_from_path_rejects_interior_nul() {
        let path = PathBuf::from(OsString::from_vec(b"bad\0path".to_vec()));
        let err = cstring_from_path(&path, "working directory").expect_err("path with NUL");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("interior NUL byte"));
    }

    #[test]
    fn shell_environment_includes_defaults_and_overrides() {
        let mut env = BTreeMap::new();
        env.insert("PATH".to_string(), "/custom/bin".to_string());
        env.insert("FOO".to_string(), "bar".to_string());

        let entries = env_entries(shell_environment(&env).expect("shell environment"));
        let expected_home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        assert_eq!(entries.get("HOME"), Some(&expected_home));
        assert_eq!(entries.get("PATH"), Some(&"/custom/bin".to_string()));
        assert_eq!(entries.get("TERM"), Some(&"dumb".to_string()));
        assert_eq!(entries.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn shell_environment_home_override_replaces_default_exactly() {
        let env = BTreeMap::from([("HOME".to_string(), "/override".to_string())]);
        let entries = env_entries(shell_environment(&env).expect("shell environment"));
        let expected = BTreeMap::from([
            ("HOME".to_string(), "/override".to_string()),
            (
                "PATH".to_string(),
                "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
            ),
            ("TERM".to_string(), "dumb".to_string()),
        ]);
        assert_eq!(entries, expected);
    }

    #[test]
    fn shell_environment_rejects_interior_nul() {
        let mut env = BTreeMap::new();
        env.insert("BAD".to_string(), "va\0lue".to_string());
        let err = shell_environment(&env).expect_err("env with NUL");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("interior NUL byte"));
    }

    #[test]
    fn shell_environment_rejects_interior_nul_in_key() {
        let mut env = BTreeMap::new();
        env.insert("BA\0D".to_string(), "value".to_string());
        let err = shell_environment(&env).expect_err("env key with NUL");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("interior NUL byte"));
    }

    #[test]
    fn open_pty_applies_requested_window_size() {
        let (_master, slave) = open_pty(42, 99).expect("open pty");
        let ws = get_winsize(&slave);
        assert_eq!(ws.ws_row, 42);
        assert_eq!(ws.ws_col, 99);
    }

    #[test]
    fn configure_command_terminal_sets_raw_no_echo_mode() {
        let (master, _slave) = open_pty(24, 80).expect("open pty");
        let mut termios = get_termios(&master);
        termios.c_iflag |= libc::ICRNL | libc::IXON;
        termios.c_oflag |= libc::OPOST;
        termios.c_lflag |= libc::ECHO
            | libc::ECHOE
            | libc::ECHOK
            | libc::ECHONL
            | libc::ICANON
            | libc::IEXTEN
            | libc::ISIG;
        termios.c_cc[libc::VMIN] = 7;
        termios.c_cc[libc::VTIME] = 9;
        let ret = unsafe { libc::tcsetattr(fd_raw(&master), libc::TCSANOW, &termios) };
        assert_eq!(
            ret,
            0,
            "tcsetattr failed: {}",
            std::io::Error::last_os_error()
        );

        configure_command_terminal(&master).expect("configure command terminal");

        let termios = get_termios(&master);
        assert_eq!(termios.c_iflag & (libc::ICRNL | libc::IXON), 0);
        assert_eq!(termios.c_oflag & libc::OPOST, 0);
        assert_eq!(
            termios.c_lflag
                & (libc::ECHO
                    | libc::ECHOE
                    | libc::ECHOK
                    | libc::ECHONL
                    | libc::ICANON
                    | libc::IEXTEN
                    | libc::ISIG),
            0
        );
        assert_eq!(termios.c_cc[libc::VMIN], 1);
        assert_eq!(termios.c_cc[libc::VTIME], 0);
    }

    #[test]
    fn configure_command_terminal_rejects_non_tty_fd() {
        let (read_fd, _write_fd) = make_pipe();
        let err = configure_command_terminal(&read_fd).expect_err("pipes are not tty devices");
        assert_raw_os_error(&err, libc::ENOTTY);
    }

    #[test]
    fn set_nonblock_sets_o_nonblock() {
        let (read_fd, _write_fd) = make_pipe();
        assert_eq!(get_fd_flags(&read_fd) & libc::O_NONBLOCK, 0);
        set_nonblock(fd_raw(&read_fd)).expect("set nonblock");
        assert_ne!(get_fd_flags(&read_fd) & libc::O_NONBLOCK, 0);
    }

    #[test]
    fn set_nonblock_rejects_bad_fd() {
        let err = set_nonblock(-1).expect_err("invalid fd should fail");
        assert_raw_os_error(&err, libc::EBADF);
    }

    #[test]
    fn fd_read_write_round_trip_through_pipe() {
        let (read_fd, write_fd) = make_pipe();
        let payload = b"hello";
        assert_eq!(fd_write(&write_fd, payload).expect("write"), payload.len());
        assert_eq!(read_exact_bytes(&read_fd, payload.len()), payload);
    }

    #[test]
    fn fd_read_reports_bad_fd() {
        let invalid_fd = InvalidFd;
        let mut buf = [0u8; 1];
        let err = fd_read(&invalid_fd, &mut buf).expect_err("invalid fd should fail");
        assert_raw_os_error(&err, libc::EBADF);
    }

    #[test]
    fn fd_write_reports_bad_fd() {
        let invalid_fd = InvalidFd;
        let err = fd_write(&invalid_fd, b"x").expect_err("invalid fd should fail");
        assert_raw_os_error(&err, libc::EBADF);
    }

    #[test]
    fn blocking_write_writes_complete_buffer() {
        let (read_fd, write_fd) = make_pipe();
        let payload = vec![b'x'; 1024];
        blocking_write(&write_fd, &payload).expect("blocking write");
        assert_eq!(read_exact_bytes(&read_fd, payload.len()), payload);
    }

    #[tokio::test]
    async fn drain_available_clears_buffered_bytes() {
        let (read_fd, write_fd) = make_pipe();
        blocking_write(&write_fd, b"stale output").expect("write stale bytes");
        set_nonblock(fd_raw(&read_fd)).expect("set nonblock");
        let read_fd = AsyncFd::new(read_fd).expect("wrap read fd");

        drain_available(&read_fd).expect("drain available bytes");

        let mut buf = [0u8; 1];
        let err = fd_read(read_fd.get_ref(), &mut buf).expect_err("buffer should be empty");
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    }

    #[tokio::test]
    async fn drain_available_returns_ok_at_eof() {
        let (read_fd, write_fd) = make_pipe();
        set_nonblock(fd_raw(&read_fd)).expect("set nonblock");
        let read_fd = AsyncFd::new(read_fd).expect("wrap read fd");
        drop(write_fd);

        drain_available(&read_fd).expect("drain at eof");
    }

    #[tokio::test]
    async fn pty_read_returns_available_bytes() {
        let (read_fd, write_fd) = make_pipe();
        set_nonblock(fd_raw(&read_fd)).expect("set nonblock");
        let read_fd = AsyncFd::new(read_fd).expect("wrap read fd");
        blocking_write(&write_fd, b"chunk").expect("write chunk");

        let chunk = pty_read(&read_fd).await.expect("pty read");
        assert_eq!(chunk, b"chunk");
    }

    #[tokio::test]
    async fn pty_read_reports_eof_when_writer_is_closed() {
        let (read_fd, write_fd) = make_pipe();
        set_nonblock(fd_raw(&read_fd)).expect("set nonblock");
        let read_fd = AsyncFd::new(read_fd).expect("wrap read fd");
        drop(write_fd);

        let err = pty_read(&read_fd)
            .await
            .expect_err("closed writer should yield eof");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
        assert!(err.to_string().contains("PTY master EOF"));
    }

    #[tokio::test]
    async fn pty_write_all_writes_full_buffer() {
        let (read_fd, write_fd) = make_pipe();
        set_nonblock(fd_raw(&write_fd)).expect("set nonblock");
        let write_fd = AsyncFd::new(write_fd).expect("wrap write fd");

        pty_write_all(&write_fd, b"payload")
            .await
            .expect("pty write all");

        assert_eq!(read_exact_bytes(&read_fd, 7), b"payload");
    }

    #[tokio::test]
    async fn read_until_prompt_handles_chunk_boundaries() {
        let (read_fd, write_fd) = make_pipe();
        set_nonblock(fd_raw(&read_fd)).expect("set nonblock");
        let read_fd = AsyncFd::new(read_fd).expect("wrap read fd");
        let prefix = prompt_prefix("split");
        let writer_prefix = prefix.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            blocking_write(&write_fd, b"pay").expect("write first chunk");
            tokio::time::sleep(Duration::from_millis(10)).await;
            blocking_write(&write_fd, b"load").expect("write second chunk");
            tokio::time::sleep(Duration::from_millis(10)).await;
            blocking_write(
                &write_fd,
                format!(
                    "{OSC133_COMMAND_DONE_PREFIX}7{OSC_TERMINATOR}{OSC133_PROMPT_START}{writer_prefix}7{PROMPT_SUFFIX}\r\n{OSC133_PROMPT_END}"
                )
                .as_bytes(),
            )
            .expect("write prompt");
        });

        match read_until_prompt(&read_fd, &prefix, Duration::from_secs(1), None)
            .await
            .expect("read until prompt")
        {
            ReadUntilPrompt::Prompt { output, exit_code } => {
                assert_eq!(output, "payload");
                assert_eq!(exit_code, 7);
            }
            ReadUntilPrompt::Eof { output } => panic!("expected prompt, got EOF with {output:?}"),
        }
    }

    #[tokio::test]
    async fn read_until_prompt_returns_eof_with_partial_output() {
        let (read_fd, write_fd) = make_pipe();
        set_nonblock(fd_raw(&read_fd)).expect("set nonblock");
        let read_fd = AsyncFd::new(read_fd).expect("wrap read fd");
        let prefix = prompt_prefix("eof");

        blocking_write(&write_fd, b"partial").expect("write partial output");
        drop(write_fd);

        match read_until_prompt(&read_fd, &prefix, Duration::from_secs(1), None)
            .await
            .expect("read until eof")
        {
            ReadUntilPrompt::Prompt { output, exit_code } => {
                panic!("expected EOF, got prompt output={output:?} exit_code={exit_code}")
            }
            ReadUntilPrompt::Eof { output } => assert_eq!(output, "partial"),
        }
    }

    #[tokio::test]
    async fn read_until_prompt_timeout_reports_partial_transcript() {
        let (read_fd, write_fd) = make_pipe();
        set_nonblock(fd_raw(&read_fd)).expect("set nonblock");
        let read_fd = AsyncFd::new(read_fd).expect("wrap read fd");
        let prefix = prompt_prefix("timeout");

        blocking_write(&write_fd, b"before").expect("write partial output");

        let err = match read_until_prompt(&read_fd, &prefix, Duration::from_millis(50), None).await
        {
            Ok(ReadUntilPrompt::Prompt { .. }) => panic!("expected timeout, got prompt"),
            Ok(ReadUntilPrompt::Eof { .. }) => panic!("expected timeout, got EOF"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(err.to_string().contains("before"));
    }

    #[tokio::test]
    async fn persistence_cwd() {
        let mut session = make_session().await;

        let r1 = session.run("pwd", false).await.expect("pwd 1");
        let original_dir = r1.output.trim().to_string();
        println!("original dir: {original_dir:?}");

        session.run("cd /tmp", false).await.expect("cd /tmp");

        let r3 = session.run("pwd", false).await.expect("pwd 2");
        let new_dir = r3.output.trim().to_string();
        println!("new dir after cd: {new_dir:?}");

        assert!(
            new_dir.contains("/tmp") || new_dir.contains("/private/tmp"),
            "expected cwd to reflect cd; got {new_dir:?}"
        );
        assert_ne!(original_dir, new_dir, "cwd should have changed after cd");

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn persistence_env() {
        let mut session = make_session().await;
        session.run("export FOO=bar", false).await.expect("export");
        let r = session
            .run("printf '%s\\n' \"$FOO\"", false)
            .await
            .expect("echo FOO");
        let output = r.output.trim();
        println!("FOO={output:?}");
        assert_eq!(output, "bar", "environment variable should persist");

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn persistence_function() {
        let mut session = make_session().await;
        session
            .run("f() { printf hi; }", false)
            .await
            .expect("define f");
        let r = session.run("f", false).await.expect("call f");
        let output = r.output.trim();
        println!("f output={output:?}");
        assert_eq!(output, "hi", "shell function should persist");

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn persistence_alias() {
        let mut session = make_session().await;
        session
            .run("alias sayhi='printf hi'", false)
            .await
            .expect("define alias");
        let r = session.run("sayhi", false).await.expect("call alias");
        let output = r.output.trim();
        println!("alias output={output:?}");
        assert_eq!(output, "hi", "shell alias should persist");

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn restart_resets_state() {
        let mut session = make_session().await;
        let original_cwd = std::env::current_dir().unwrap();

        session
            .run("export RESETME=yes", false)
            .await
            .expect("export");
        session.run("cd /tmp", false).await.expect("cd");

        // restart=true
        let r = session
            .run("printf '%s\\n' \"RESETME=$RESETME\"; pwd", true)
            .await
            .expect("restart run");
        let output = r.output.trim();
        println!("after restart: {output:?}");
        assert!(
            output.contains("RESETME=") && !output.contains("RESETME=yes"),
            "env should be reset after restart; got {output:?}"
        );
        let lines: Vec<&str> = output.lines().collect();
        let pwd_line = lines.last().expect("should have pwd line").trim();
        let expected = original_cwd.to_string_lossy();
        assert!(
            pwd_line == expected.as_ref() || pwd_line.ends_with(expected.as_ref()),
            "cwd should be reset to initial; got {pwd_line:?}, expected to end with {expected:?}"
        );

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn multiline_command() {
        let mut session = make_session().await;
        let cmd = r#"cat <<'HEREDOC'
hello
world
HEREDOC"#;
        let r = session.run(cmd, false).await.expect("heredoc");
        let output = r.output.trim();
        println!("multiline output={output:?}");
        assert!(
            output.contains("hello") && output.contains("world"),
            "heredoc output missing; got {output:?}"
        );
        assert!(r.status.success(), "heredoc should succeed");

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn nonzero_exit() {
        let mut session = make_session().await;
        let r = session
            .run("printf nope; exit 7", false)
            .await
            .expect("nonzero exit");
        println!("nonzero output={:?} status={:?}", r.output, r.status);
        assert!(
            r.output.contains("nope"),
            "transcript should contain 'nope'; got {:?}",
            r.output
        );
        assert!(!r.status.success(), "status should be non-success");
        assert_eq!(
            r.status.code(),
            Some(7),
            "exit code should be 7; got {:?}",
            r.status.code()
        );
        assert!(
            !session.is_alive().expect("is_alive after exit"),
            "shell should die after bare exit"
        );
    }

    #[tokio::test]
    async fn nonzero_command_does_not_kill_shell() {
        let mut session = make_session().await;

        let r = session
            .run("printf warn >&2; false", false)
            .await
            .expect("nonzero command should still frame");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "warn".to_string(),
                status: exit_status_from_code(1),
            },
        );
        assert!(
            session.is_alive().expect("is_alive after false"),
            "plain nonzero commands should not kill the shell"
        );

        let r = session
            .run("printf after", false)
            .await
            .expect("after false");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "after".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn transport_survives_mutated_shell_state() {
        let mut session = make_session().await;
        session
            .run(
                "shopt -s expand_aliases\n\
                 alias printf='false'\n\
                 enable -n printf\n\
                 function eval() { return 97; }\n\
                 function base64() { return 98; }\n\
                 PATH=/nope",
                false,
            )
            .await
            .expect("mutate shell state");

        let r = session.run("pwd", false).await.expect("pwd after mutation");
        let output = r.output.trim();
        println!("pwd after mutation={output:?}");
        assert!(
            output.starts_with('/'),
            "transport should survive mutated shell state; got {output:?}"
        );
        assert!(r.status.success(), "pwd should still succeed");
    }

    #[tokio::test]
    async fn prompt_state_is_readonly_and_errors_are_visible() {
        let mut session = make_session().await;

        let r = session
            .run("PS1=oops; PROMPT_COMMAND=oops; printf tagged", false)
            .await
            .expect("readonly prompt state");
        assert!(
            r.output.contains("readonly variable"),
            "expected readonly error to be visible; got {:?}",
            r.output
        );
        assert!(
            !r.status.success(),
            "readonly prompt writes should fail the offending command"
        );

        let r = session
            .run("printf after", false)
            .await
            .expect("after readonly prompt state");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "after".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn prompt_nonce_probe_is_available() {
        let mut session = make_session().await;
        let r = session
            .run("printf '%s' \"$__claudius_prompt_nonce\"", false)
            .await
            .expect("nonce probe");
        assert!(
            !r.output.trim().is_empty(),
            "nonce probe should return the session nonce"
        );
        assert!(r.output.trim().bytes().all(|b| b.is_ascii_digit()));
    }

    #[tokio::test]
    async fn actual_prompt_prefix_in_output_is_preserved() {
        let mut session = make_session().await;
        let nonce = session
            .run("printf '%s' \"$__claudius_prompt_nonce\"", false)
            .await
            .expect("nonce probe")
            .output;
        let nonce = nonce.trim();

        let r = session
            .run(
                "printf '__CLAUDIUS_PTY_PROMPT__%s_STATUS_99__\\npayload' \"$__claudius_prompt_nonce\"",
                false,
            )
            .await
            .expect("prompt collision output");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: format!("__CLAUDIUS_PTY_PROMPT__{nonce}_STATUS_99__\npayload"),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn shell_option_can_kill_shell_and_still_return_status() {
        let mut session = make_session().await;
        session.run("set -e", false).await.expect("set -e");

        let r = session
            .run("printf before; false; printf after", false)
            .await
            .expect("errexit run");
        let output = r.output.trim();
        println!("errexit output={output:?} status={:?}", r.status);
        assert!(
            output.contains("before"),
            "partial transcript should include output before exit; got {output:?}"
        );
        assert!(
            !output.contains("after"),
            "shell should exit before trailing output; got {output:?}"
        );
        assert_eq!(
            r.status.code(),
            Some(1),
            "errexit should propagate shell exit code; got {:?}",
            r.status.code()
        );
        assert!(
            !session.is_alive().expect("is_alive after errexit"),
            "shell should be dead after errexit"
        );
    }

    #[tokio::test]
    async fn prompt_like_output_is_preserved() {
        let mut session = make_session().await;
        let r = session
            .run(
                "printf '%s\\n%s\\n' \
                 '__CLAUDIUS_PTY_PROMPT__user_STATUS_99__' \
                 'payload'",
                false,
            )
            .await
            .expect("prompt-like output");
        println!("prompt-like output={:?}", r.output);
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "__CLAUDIUS_PTY_PROMPT__user_STATUS_99__\npayload".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn sequential_calls_do_not_leak_internal_prompt_lines() {
        let mut session = make_session().await;
        session.run(":", false).await.expect("noop 1");
        session.run(":", false).await.expect("noop 2");

        let r = session.run("printf clean", false).await.expect("clean");
        println!("sequential clean output={:?}", r.output);
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "clean".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn terminal_mode_is_reasserted_between_commands() {
        let mut session = make_session().await;
        session.run("stty sane", false).await.expect("stty sane");

        let r = session.run("printf clean", false).await.expect("clean");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "clean".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn command_input_bypasses_terminal_line_editing() {
        let mut session = make_session().await;
        let r = session
            .run("cat <<'EOF'\nbefore\u{15}after\nEOF", false)
            .await
            .expect("literal line editing character");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "before\u{15}after".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn timeout_reports_partial_transcript_and_session_recovers() {
        let mut session = make_session_with_timeout(Duration::from_millis(200)).await;
        let err = session
            .run("printf before; sleep 1", false)
            .await
            .expect_err("timeout");
        let err_text = err.to_string();
        println!("timeout error={err_text:?}");
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::TimedOut,
            "sleeping command should time out"
        );
        assert!(
            err_text.contains("before"),
            "timeout should include partial transcript; got {err_text:?}"
        );

        tokio::time::sleep(Duration::from_secs(2)).await;

        let r = session
            .run("printf after", false)
            .await
            .expect("after timeout");
        println!("after-timeout output={:?}", r.output);
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "after".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn signaled_shell_death_returns_error_and_session_recovers() {
        let mut session = make_session().await;

        let err = session
            .run("printf before; kill -9 $$", false)
            .await
            .expect_err("kill -9 should abort framing");
        let err_text = err.to_string();
        println!("signaled death error={err_text:?}");
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::UnexpectedEof,
            "signaled shell death should surface as transport failure"
        );
        assert!(
            err_text.contains("signal 9"),
            "signaled death should report the signal; got {err_text:?}"
        );
        assert!(
            err_text.contains("before"),
            "signaled death should include the partial transcript; got {err_text:?}"
        );
        assert!(
            !session.is_alive().expect("is_alive after signal death"),
            "shell should be dead after SIGKILL"
        );

        let r = session
            .run("printf after", false)
            .await
            .expect("recover after SIGKILL");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "after".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }

    #[tokio::test]
    async fn shell_death_recovery() {
        let mut session = make_session().await;

        // Kill the shell.
        let exit_result = session.run("exit", false).await;
        // This may succeed or fail depending on when bash flushes the PTY.
        println!("exit result: {exit_result:?}");

        // The next call should transparently create a fresh session.
        let r = session
            .run("printf alive", false)
            .await
            .expect("post-death run");
        let output = r.output.trim();
        println!("post-death output={output:?}");
        assert!(
            output.contains("alive"),
            "should recover after shell death; got {output:?}"
        );
        assert!(r.status.success());

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn is_alive_reflects_state() {
        let mut session = make_session().await;
        assert!(
            session.is_alive().expect("is_alive"),
            "session should be alive"
        );

        session.close().await.expect("close");
        assert!(
            !session.is_alive().expect("is_alive after close"),
            "session should not be alive after close"
        );
    }

    #[tokio::test]
    async fn custom_config_cwd_is_used_on_spawn() {
        let temp_dir = unique_test_dir("cwd");
        let config = BashPtyConfig {
            cwd: temp_dir.clone(),
            command_timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let mut session = BashPtySession::new(config).await.expect("spawn session");

        let r = session.run("pwd -P", false).await.expect("pwd -P");
        assert_eq!(
            physical_path(Path::new(r.output.trim())),
            physical_path(&temp_dir)
        );

        session.close().await.expect("close");
        fs::remove_dir_all(&temp_dir).expect("remove temp dir");
    }

    #[tokio::test]
    async fn custom_config_env_is_used_on_spawn() {
        let mut env = BTreeMap::new();
        env.insert("CLAUDIUS_CUSTOM_ENV".to_string(), "configured".to_string());
        let config = BashPtyConfig {
            cwd: std::env::current_dir().expect("current dir"),
            env,
            command_timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let mut session = BashPtySession::new(config).await.expect("spawn session");

        let r = session
            .run("printf '%s' \"$CLAUDIUS_CUSTOM_ENV\"", false)
            .await
            .expect("print custom env");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "configured".to_string(),
                status: exit_status_from_code(0),
            },
        );

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn stderr_is_captured_in_combined_output() {
        let mut session = make_session().await;

        let r = session
            .run("printf stdout; printf stderr >&2", false)
            .await
            .expect("write stdout and stderr");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "stdoutstderr".to_string(),
                status: exit_status_from_code(0),
            },
        );

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn output_channel_streams_complete_lines_before_completion() {
        let mut session = make_session().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let run =
            session.run_with_output_channel("printf 'ready\\n'; sleep 0.2; printf done", false, tx);
        tokio::pin!(run);

        let first = tokio::select! {
            line = rx.recv() => line.expect("streamed line"),
            result = &mut run => panic!("run completed before streaming first line: {result:?}"),
        };
        assert_eq!(first, "ready");

        let r = run.await.expect("streaming run");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "ready\ndone".to_string(),
                status: exit_status_from_code(0),
            },
        );
        assert_eq!(rx.recv().await, Some("done".to_string()));
        assert_eq!(rx.recv().await, None);
    }

    #[tokio::test]
    async fn output_channel_lines_match_returned_result_output() {
        let mut session = make_session().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let r = session
            .run_with_output_channel("printf 'alpha\\n\\nbeta\\n\\n'; printf tail", false, tx)
            .await
            .expect("streaming output");

        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "alpha\n\nbeta\n\ntail".to_string(),
                status: exit_status_from_code(0),
            },
        );
        let streamed = collect_streamed_lines(&mut rx).await;
        let expected = r.output.lines().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(streamed, expected);
    }

    #[tokio::test]
    async fn empty_command_returns_success_with_empty_output() {
        let mut session = make_session().await;

        let r = session.run("", false).await.expect("empty command");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: String::new(),
                status: exit_status_from_code(0),
            },
        );

        session.close().await.expect("close");
    }

    #[tokio::test]
    async fn close_is_idempotent_and_run_respawns_shell() {
        let mut session = make_session().await;
        session.close().await.expect("first close");
        session.close().await.expect("second close");
        assert!(
            !session.is_alive().expect("is_alive after closes"),
            "session should stay closed"
        );

        let r = session.run("printf revived", false).await.expect("respawn");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "revived".to_string(),
                status: exit_status_from_code(0),
            },
        );

        session.close().await.expect("final close");
    }

    #[tokio::test]
    async fn invalid_shell_path_fails_new() {
        let config = BashPtyConfig {
            shell: PathBuf::from("/definitely/not/a/bash"),
            command_timeout: Duration::from_secs(10),
            ..Default::default()
        };

        let err = match BashPtySession::new(config).await {
            Ok(_) => panic!("invalid shell path should fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn shell_path_with_interior_nul_fails_new() {
        let config = BashPtyConfig {
            shell: PathBuf::from(OsString::from_vec(b"bad\0shell".to_vec())),
            command_timeout: Duration::from_secs(10),
            ..Default::default()
        };

        let err = match BashPtySession::new(config).await {
            Ok(_) => panic!("shell path with NUL should fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("interior NUL byte"));
    }

    #[tokio::test]
    async fn cwd_with_interior_nul_fails_new() {
        let config = BashPtyConfig {
            cwd: PathBuf::from(OsString::from_vec(b"bad\0cwd".to_vec())),
            command_timeout: Duration::from_secs(10),
            ..Default::default()
        };

        let err = match BashPtySession::new(config).await {
            Ok(_) => panic!("cwd with NUL should fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("interior NUL byte"));
    }

    #[tokio::test]
    async fn missing_cwd_fails_new() {
        let temp_dir = unique_test_dir("missing-cwd");
        fs::remove_dir_all(&temp_dir).expect("remove temp dir");
        let config = BashPtyConfig {
            cwd: temp_dir.clone(),
            command_timeout: Duration::from_secs(10),
            ..Default::default()
        };

        let err = match BashPtySession::new(config).await {
            Ok(_) => panic!("missing cwd should fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn shell_exiting_during_setup_fails_new() {
        let temp_dir = unique_test_dir("setup-eof");
        let shell_path = temp_dir.join("fake-shell");
        fs::write(&shell_path, "#!/bin/sh\nsleep 0.1\nexit 0\n").expect("write fake shell");
        let mut permissions = fs::metadata(&shell_path)
            .expect("stat fake shell")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&shell_path, permissions).expect("chmod fake shell");

        let config = BashPtyConfig {
            shell: shell_path.clone(),
            command_timeout: Duration::from_secs(1),
            ..Default::default()
        };

        let result = BashPtySession::new(config).await;
        fs::remove_dir_all(&temp_dir).expect("remove temp dir");

        let err = match result {
            Ok(_) => panic!("shell exiting during setup should fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
        assert!(err.to_string().contains("shell exited during setup"));
    }

    #[tokio::test]
    async fn env_with_interior_nul_fails_new() {
        let mut env = BTreeMap::new();
        env.insert("BAD".to_string(), "va\0lue".to_string());
        let config = BashPtyConfig {
            env,
            command_timeout: Duration::from_secs(10),
            ..Default::default()
        };

        let err = match BashPtySession::new(config).await {
            Ok(_) => panic!("env with NUL should fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("interior NUL byte"));
    }

    #[tokio::test]
    async fn restart_restores_configured_environment() {
        let mut env = BTreeMap::new();
        env.insert("CLAUDIUS_STICKY".to_string(), "configured".to_string());
        let config = BashPtyConfig {
            env,
            command_timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let mut session = BashPtySession::new(config).await.expect("spawn session");

        session
            .run("export CLAUDIUS_STICKY=mutated", false)
            .await
            .expect("mutate env");

        let r = session
            .run("printf '%s' \"$CLAUDIUS_STICKY\"", true)
            .await
            .expect("restart with configured env");
        assert_bash_pty_result_eq(
            &r,
            BashPtyResult {
                output: "configured".to_string(),
                status: exit_status_from_code(0),
            },
        );
    }
}
