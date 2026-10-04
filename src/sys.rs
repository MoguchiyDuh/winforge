use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenUser,
};
use windows_sys::Win32::System::Console::{
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE, SetConsoleMode,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, GetCurrentProcess, OpenProcessToken,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;

use crate::schema::Os;

pub const TIMEOUT_PROBE: Duration = Duration::from_secs(30);
pub const TIMEOUT_SCRIPT: Duration = Duration::from_secs(900);
pub const TIMEOUT_WINGET_INSTALL: Duration = Duration::from_secs(1800);
pub const TIMEOUT_WINGET_SCRIPT: Duration = Duration::from_secs(900);

pub fn system32(rel: &str) -> PathBuf {
    let root = env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    return root.join("System32").join(rel);
}

pub fn powershell_exe() -> PathBuf {
    return system32(r"WindowsPowerShell\v1.0\powershell.exe");
}

pub fn cmd_exe() -> PathBuf {
    return system32("cmd.exe");
}

pub fn rand_suffix() -> String {
    return format!("{}-{}", std::process::id(), nanos());
}

pub fn enable_vt() {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0u32;
        if GetConsoleMode(handle, &mut mode) != 0 {
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

pub fn stdin_is_console() -> bool {
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0u32;
        return GetConsoleMode(handle, &mut mode) != 0;
    }
}

pub fn windows_build() -> Result<u32, String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE};
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            KEY_QUERY_VALUE,
        )
        .map_err(|err| format!("cannot open CurrentVersion key: {err}"))?;
    let raw: String = key
        .get_value("CurrentBuildNumber")
        .map_err(|err| format!("cannot read CurrentBuildNumber: {err}"))?;
    return raw
        .trim()
        .parse::<u32>()
        .map_err(|err| format!("cannot parse CurrentBuildNumber '{raw}': {err}"));
}

pub fn current_os() -> Option<Os> {
    static OS: OnceLock<Option<Os>> = OnceLock::new();
    return *OS.get_or_init(|| windows_build().ok().map(Os::from_build));
}

pub fn is_admin() -> bool {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        CloseHandle(token);
        return ok != 0 && elevation.TokenIsElevated != 0;
    }
}

static ORIGIN_SID: OnceLock<Option<String>> = OnceLock::new();

pub fn set_origin_sid(sid: Option<String>) {
    let _ = ORIGIN_SID.set(sid);
}

pub fn foreign_user_warning() -> Option<String> {
    let origin = ORIGIN_SID.get()?.as_ref()?;
    let current = current_user_sid()?;
    if origin.eq_ignore_ascii_case(&current) {
        return None;
    }
    return Some(String::from(
        "elevated as a different account than the one that started winforge; HKCU tweaks apply to the admin account, not to the signed-in user",
    ));
}

pub fn current_user_sid() -> Option<String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut needed = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        let mut buf = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            (buf.len() * 8) as u32,
            &mut needed,
        );
        CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut wide: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut wide) == 0 || wide.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *wide.add(len) != 0 {
            len += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(wide, len));
        LocalFree(wide as *mut core::ffi::c_void);
        return Some(sid);
    }
}

pub fn elevate(args: &[String]) -> Result<(), String> {
    let exe = env::current_exe()
        .map_err(|err| format!("cannot resolve current exe: {err}"))?
        .to_string_lossy()
        .into_owned();
    let parameters = args
        .iter()
        .map(|arg| {
            if arg.as_bytes().iter().any(|b| *b == b'"' || *b == b' ') {
                return format!("\"{}\"", arg.replace('"', "\\\""));
            }
            return arg.clone();
        })
        .collect::<Vec<_>>()
        .join(" ");
    let verb = to_wide("runas");
    let file = to_wide(&exe);
    let param_wide = if parameters.is_empty() {
        None
    } else {
        Some(to_wide(&parameters))
    };
    let param_ptr = match param_wide.as_ref() {
        Some(wide) => wide.as_ptr(),
        None => std::ptr::null(),
    };
    unsafe {
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            param_ptr,
            std::ptr::null(),
            1,
        );
        if result as isize > 32 {
            return Ok(());
        }
        return Err(String::from("elevation was declined or failed"));
    }
}

pub fn run(program: &Path, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let child = Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot run {}: {err}", program.display()))?;
    return wait_captured(child, timeout, &program.display().to_string());
}

pub fn run_script(script: &str, timeout: Duration) -> Result<Output, String> {
    return run_script_args(script, &[], timeout);
}

pub fn run_script_args(
    script: &str,
    extra_args: &[&str],
    timeout: Duration,
) -> Result<Output, String> {
    let path = write_temp("winforge-ps", "ps1", script.as_bytes())?;
    let mut line = format!("& \"{}\"", path.to_string_lossy());
    for arg in extra_args {
        if is_switch(arg) {
            line.push(' ');
            line.push_str(arg);
            continue;
        }
        line.push_str(&format!(" \"{}\"", arg.replace('"', "")));
    }
    line.push('\n');
    let result = ps_host_run(&line, timeout);
    let _ = fs::remove_file(&path);
    return result;
}

fn is_switch(arg: &str) -> bool {
    let Some(name) = arg.strip_prefix('-') else {
        return false;
    };
    return !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric());
}

const PS_MARKER: &str = "__WF_RC__";
const PS_EOF: &str = "__WF_EOF__";
const PS_BOOTSTRAP: &str = "while ($true) { $p = [Console]::In.ReadLine(); if ($null -eq $p) { break }; $global:LASTEXITCODE = 0; try { Invoke-Expression $p 2>&1 | ForEach-Object { \"$_\" } } catch { $_ | Out-String; $global:LASTEXITCODE = 1 }; \"__WF_RC__$($global:LASTEXITCODE)\" }";

struct PsHost {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
    buf: Arc<Mutex<Vec<u8>>>,
}

enum PsErr {
    Dead,
    Msg(String),
}

impl Drop for PsHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl PsHost {
    fn spawn() -> Result<PsHost, String> {
        let mut child = Command::new(powershell_exe())
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                PS_BOOTSTRAP,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("cannot start powershell host: {err}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or(String::from("powershell host stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(String::from("powershell host stdout missing"))?;
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        let buf2 = buf.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(String::from(PS_EOF));
                        break;
                    }
                    Ok(_) => match line.trim_end_matches(['\r', '\n']).strip_prefix(PS_MARKER) {
                        Some(code) => {
                            let _ = tx.send(code.to_string());
                        }
                        None => {
                            if let Ok(mut guard) = buf2.lock() {
                                guard.extend_from_slice(line.as_bytes());
                            }
                        }
                    },
                }
            }
        });
        return Ok(PsHost {
            child,
            stdin,
            rx,
            buf,
        });
    }

    fn exec(&mut self, line: &str, timeout: Duration) -> Result<Output, PsErr> {
        if let Ok(mut guard) = self.buf.lock() {
            guard.clear();
        }
        if self.stdin.write_all(line.as_bytes()).is_err() || self.stdin.flush().is_err() {
            return Err(PsErr::Dead);
        }
        let code: i32 = match self.rx.recv_timeout(timeout) {
            Ok(marker) if marker == PS_EOF => return Err(PsErr::Dead),
            Ok(code) => code.parse().unwrap_or(-1),
            Err(RecvTimeoutError::Timeout) => {
                return Err(PsErr::Msg(format!(
                    "powershell script timed out after {timeout:?}"
                )));
            }
            Err(_) => return Err(PsErr::Dead),
        };
        let stdout = self
            .buf
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        return Ok(Output {
            status: ExitStatus::from_raw(code as u32),
            stdout,
            stderr: Vec::new(),
        });
    }
}

fn ps_host_run(line: &str, timeout: Duration) -> Result<Output, String> {
    static HOST: Mutex<Option<PsHost>> = Mutex::new(None);
    let mut guard = HOST.lock().unwrap();
    let mut retried = false;
    loop {
        if guard.is_none() {
            match PsHost::spawn() {
                Ok(host) => *guard = Some(host),
                Err(err) => return Err(err),
            }
        }
        let host = guard.as_mut().unwrap();
        return match host.exec(line, timeout) {
            Ok(out) => Ok(out),
            Err(PsErr::Dead) => {
                *guard = None;
                if retried {
                    Err(String::from("powershell host died"))
                } else {
                    retried = true;
                    continue;
                }
            }
            Err(PsErr::Msg(message)) => {
                *guard = None;
                Err(message)
            }
        };
    }
}

fn wait_captured(mut child: Child, timeout: Duration, program: &str) -> Result<Output, String> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = thread::spawn(move || drain(stdout));
    let stderr_reader = thread::spawn(move || drain(stderr));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(format!("{program} timed out after {timeout:?}"));
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("cannot wait for {program}: {err}"));
            }
        }
    };
    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    return Ok(Output {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
    });
}

fn drain<R: Read>(pipe: Option<R>) -> Vec<u8> {
    let Some(mut pipe) = pipe else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    let _ = pipe.read_to_end(&mut buf);
    return buf;
}

pub fn output_text(out: &Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut text = stdout.trim_end().to_owned();
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim_end());
    }
    return text;
}

pub fn write_temp(prefix: &str, ext: &str, data: &[u8]) -> Result<PathBuf, String> {
    let path = temp_file(&format!(
        "{prefix}-{}-{}.{ext}",
        std::process::id(),
        nanos()
    ));
    fs::write(&path, data).map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    return Ok(path);
}

pub fn temp_file(name: &str) -> PathBuf {
    return env::temp_dir().join(name);
}

fn nanos() -> u128 {
    return SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u128 * 1_000_000_000 + d.subsec_nanos() as u128)
        .unwrap_or(0);
}

pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    return format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        year,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    );
}

static LOG_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn log(line: &str) {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    let stamp = timestamp();
    if let Ok(mut guard) = LOG_LINES.lock() {
        guard.push(format!("{stamp} {line}"));
        if guard.len() > 500 {
            let overflow = guard.len() - 500;
            guard.drain(0..overflow);
        }
    }
    let path = PATH.get_or_init(|| {
        let base = env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        let dir = base.join("winforge").join("logs");
        if fs::create_dir_all(&dir).is_err() {
            return None;
        }
        return Some(dir.join(format!("{}.log", timestamp())));
    });
    let Some(path) = path.as_ref() else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{stamp} {line}");
    }
}

pub fn log_tail(max: usize) -> Vec<String> {
    let guard = match LOG_LINES.lock() {
        Ok(guard) => guard,
        Err(_) => return Vec::new(),
    };
    let start = guard.len().saturating_sub(max);
    return guard[start..].to_vec();
}

fn to_wide(value: &str) -> Vec<u16> {
    return value.encode_utf16().chain(std::iter::once(0)).collect();
}
