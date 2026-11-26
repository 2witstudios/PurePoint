//! WIN-1.1 ConPTY spike driver. Each subcommand is one experiment; results are
//! printed and screen fixtures land in `fixtures/`, raw byte dumps in `results/`.

mod pty;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use pty::{spawn, Pty, PSEUDOCONSOLE_PASSTHROUGH_MODE};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::*;
use windows_sys::Win32::System::Diagnostics::ToolHelp::*;

const S: Duration = Duration::from_secs(1);

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn redact(s: &str) -> String {
    let mut out = s.to_string();
    if let Ok(u) = std::env::var("USERNAME") {
        out = out.replace(&u, "<user>");
    }
    if let Ok(h) = std::env::var("COMPUTERNAME") {
        out = out.replace(&h, "<host>").replace(&h.to_lowercase(), "<host>");
    }
    // Emails.
    out.split_inclusive(|c: char| c.is_whitespace() || c == '│' || c == '(' || c == ')')
        .map(|w| {
            let t = w.trim_end_matches(|c: char| c.is_whitespace() || c == '│' || c == '(' || c == ')');
            if t.contains('@') && t.contains('.') && !t.starts_with('@') {
                w.replace(t, "<email>")
            } else {
                w.to_string()
            }
        })
        .collect()
}

pub fn screen_text(screen: &vt100::Screen) -> String {
    let (_, cols) = screen.size();
    let rows: Vec<String> = screen.rows(0, cols).map(|r| r.trim_end().to_string()).collect();
    let mut s = rows.join("\n");
    while s.ends_with('\n') {
        s.pop();
    }
    s.push('\n');
    redact(&s)
}

fn escape_raw(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\x1b' => out.push_str("\\e"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n\n"),
            '\x07' => out.push_str("\\a"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    redact(&out)
}

fn save_fixture(name: &str, screen: &vt100::Screen) {
    let p = root().join("fixtures").join(format!("{name}.txt"));
    std::fs::write(&p, screen_text(screen)).unwrap();
    println!("  wrote {}", p.display());
}

fn save_raw(name: &str, bytes: &[u8]) {
    let p = root().join("results").join(format!("{name}.raw.txt"));
    std::fs::write(&p, escape_raw(bytes)).unwrap();
}

// ---- copy of crates/pu-engine/src/screen.rs heuristics (unix-only crate) ----

fn is_prompt_marker(c: char) -> bool {
    c == '>' || c == '❯'
}
fn is_border(row: &str) -> bool {
    let t = row.trim_start();
    t.starts_with('╭') || t.starts_with('╰') || t.starts_with("──") || t.starts_with('─')
}
fn is_top_border(row: &str) -> bool {
    let t = row.trim_start();
    t.starts_with('╭') || t.starts_with('─')
}
fn strip_sides(row: &str) -> &str {
    let t = row.trim();
    let t = t.strip_prefix('│').unwrap_or(t);
    let t = t.strip_suffix('│').unwrap_or(t);
    t.trim()
}
fn find_input_box(rows: &[String]) -> Option<String> {
    for start in (0..rows.len()).rev() {
        let first = strip_sides(&rows[start]);
        let mut chars = first.chars();
        let Some(marker) = chars.next() else { continue };
        if !is_prompt_marker(marker) || start == 0 || !is_top_border(&rows[start - 1]) {
            continue;
        }
        let mut lines = vec![chars.as_str().trim().to_string()];
        let mut end = start + 1;
        while end < rows.len() && !is_border(&rows[end]) {
            lines.push(strip_sides(&rows[end]).to_string());
            end += 1;
        }
        if end >= rows.len() {
            continue;
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        return Some(lines.join("\n"));
    }
    None
}
pub fn input_phase(screen: &vt100::Screen) -> &'static str {
    let (height, width) = screen.size();
    let rows: Vec<String> = (0..height)
        .map(|r| {
            (0..width)
                .map(|c| match screen.cell(r, c) {
                    Some(cell) if cell.has_contents() && !cell.dim() => cell.contents().to_string(),
                    _ => " ".to_string(),
                })
                .collect()
        })
        .collect();
    let text = rows.join("\n").to_lowercase();
    if text.contains("esc to cancel")
        || text.contains("do you want to proceed")
        || text.contains("yes, allow")
        || text.contains("trust this folder")
    {
        "dialog"
    } else if let Some(b) = find_input_box(&rows) {
        if b.trim().is_empty() {
            "input_ready"
        } else {
            "has_draft"
        }
    } else {
        "busy"
    }
}
fn dim_cells(screen: &vt100::Screen) -> usize {
    let (h, w) = screen.size();
    (0..h)
        .flat_map(|r| (0..w).map(move |c| (r, c)))
        .filter(|&(r, c)| screen.cell(r, c).is_some_and(|x| x.has_contents() && x.dim()))
        .count()
}

fn report(label: &str, pty: &Pty, p: &vt100::Parser) {
    let raw = pty.snapshot();
    println!(
        "  [{label}] bytes={} input_phase={} dim_cells={} has_SGR2={} has_NBSP={} alt_screen={} answered={:?}",
        raw.len(),
        input_phase(p.screen()),
        dim_cells(p.screen()),
        sgr_has_dim(&raw),
        pty::contains(&raw, "\u{a0}".as_bytes()),
        p.screen().alternate_screen(),
        pty.answered.lock().unwrap()
    );
}

// ---- process snapshots ----

fn processes() -> Vec<(u32, u32, String)> {
    let mut v = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return v;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
                v.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..n])));
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    v
}

fn descendants(root_pid: u32) -> Vec<(u32, u32, String)> {
    let all = processes();
    let mut kids: HashMap<u32, Vec<&(u32, u32, String)>> = HashMap::new();
    for p in &all {
        kids.entry(p.1).or_default().push(p);
    }
    let mut out = Vec::new();
    let mut stack = vec![root_pid];
    let mut seen = HashSet::new();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        for k in kids.get(&pid).into_iter().flatten() {
            if k.0 != pid {
                out.push((*k).clone());
                stack.push(k.0);
            }
        }
    }
    out
}

// ---- cmd.exe shim quoting ----

/// Quotes one argument for `cmd.exe /d /s /c "<shim> <args>"` where the shim
/// forwards `%*` to an MSVC-CRT exe. Quotes are doubled (`""`), which both cmd
/// (quote state toggles twice, so specials stay quoted) and the CRT (a `""`
/// inside a quoted arg is a literal `"`) accept. Backslashes before a quote or
/// the closing quote are doubled per CRT rules. `%` is broken up so cmd cannot
/// expand `%VAR%` on the /c command line. Newlines cannot be passed.
pub fn quote_cmd_arg(arg: &str) -> Result<String, String> {
    if arg.contains(['\r', '\n', '\0']) {
        return Err("newline/NUL cannot pass through cmd.exe".into());
    }
    let needs = arg.is_empty() || arg.contains([' ', '\t', '"', '&', '|', '<', '>', '^', '(', ')', '%', '!', ',', ';', '=']);
    if !needs {
        return Ok(arg.to_string());
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2));
                backslashes = 0;
                out.push_str("\"\"");
            }
            '%' => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                // `%%cd:~,%` expands to empty in cmd, so `%PATH%` stays literal.
                out.push_str("%%cd:~,%");
            }
            c => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    Ok(out)
}

pub fn shim_cmdline(shim: &str, args: &[&str]) -> Result<String, String> {
    let mut inner = format!("\"{shim}\"");
    for a in args {
        inner.push(' ');
        inner.push_str(&quote_cmd_arg(a)?);
    }
    Ok(format!("cmd.exe /d /s /c \"{inner}\""))
}

fn quoting() {
    // Copy the shim to a directory with a space to cover the shim path too.
    let spaced = std::env::temp_dir().join("pu spike dir");
    let _ = std::fs::create_dir_all(&spaced);
    let target = root().join("target").join("debug").join("conpty-spike.exe");
    let shim_body = format!(
        "@echo off\r\nrem Mirrors an npm/winget shim: forwards %* verbatim to a real exe.\r\n\"{}\" argv %*\r\n",
        target.display()
    );
    let spaced_shim = spaced.join("echo args.cmd");
    std::fs::write(&spaced_shim, shim_body).unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["plain", "two words", ""],
        vec![r#"say "hi""#, r#"a"b"#, r#""quoted""#],
        vec!["amp & pipe | lt < gt >", "caret ^ paren ( )"],
        vec![r#"say "hi & bye""#, r#"x" & echo INJECTED & "#],
        vec!["100%", "%PATH%", "%USERNAME%x"],
        vec![r"C:\dir with space\", r"back\\slash", r#"bs\"q"#],
        vec!["bang!", "semi;colon,comma=eq", "tab\there"],
        vec!["--prompt", "multi word prompt with \"quotes\" and 'single'"],
    ];
    let mut all_ok = true;
    for case in cases {
        let line = shim_cmdline(spaced_shim.to_str().unwrap(), &case).unwrap();
        let out = std::process::Command::new("cmd.exe")
            .raw_arg_line(&line)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let got: Vec<String> = stdout
            .lines()
            .filter_map(|l| l.strip_prefix("ARGV "))
            .map(|l| l.to_string())
            .collect();
        let want: Vec<String> = case.iter().map(|a| format!("{a:?}")).collect();
        let ok = got == want;
        all_ok &= ok;
        println!("{} {line}", if ok { "OK  " } else { "FAIL" });
        if !ok {
            println!("     want {want:?}\n     got  {got:?}\n     raw  {stdout}");
        }
    }
    println!("all quoting cases ok: {all_ok}");
}

trait RawArgLine {
    fn raw_arg_line(&mut self, line: &str) -> &mut Self;
}
impl RawArgLine for std::process::Command {
    /// Runs exactly `line` (which starts with `cmd.exe`) as the command line.
    fn raw_arg_line(&mut self, line: &str) -> &mut Self {
        use std::os::windows::process::CommandExt;
        let rest = line.strip_prefix("cmd.exe ").unwrap();
        self.raw_arg(rest)
    }
}

// ---- experiments ----

fn first_screen(name: &str, cwd: Option<&str>, cmd: &str, cols: u16, rows: u16, max: u64, flags: u32) {
    println!("== {name}: {cmd} ({cols}x{rows}, flags={flags:#x})");
    let pty = match spawn(cmd, cwd, cols, rows, flags) {
        Ok(p) => p,
        Err(e) => {
            println!("  spawn error: {e}");
            return;
        }
    };
    pty.settle(2 * S, Duration::from_millis(2500), Duration::from_secs(max));
    let p = pty.parse();
    report(name, &pty, &p);
    save_fixture(name, p.screen());
    save_raw(name, &pty.snapshot());
    let exited = pty.wait_exit(Duration::ZERO);
    println!("  child exited during capture: {exited:?}");
    pty.finish();
}

fn resize(name: &str, cwd: Option<&str>, cmd: &str) {
    println!("== resize {name}: {cmd}");
    let mut pty = spawn(cmd, cwd, 120, 30, 0).unwrap();
    pty.settle(3 * S, Duration::from_millis(2500), Duration::from_secs(25));
    let before = pty.parse();
    report("before 120x30", &pty, &before);
    save_fixture(&format!("{name}-resize-before-120x30"), before.screen());
    let mark = pty.len();
    let hr = pty.resize(70, 30);
    println!("  ResizePseudoConsole(70x30) HRESULT={hr:#x}");
    pty.settle(S, Duration::from_millis(1500), Duration::from_secs(10));
    let after = pty.parse(); // parse at the new size from the start; the redraw wins
    let mut incremental = vt100::Parser::new(30, 120, 0);
    let raw = pty.snapshot();
    incremental.process(&raw[..mark]);
    incremental.screen_mut().set_size(30, 70);
    incremental.process(&raw[mark..]);
    println!("  bytes after resize: {}", raw.len() - mark);
    report("after 70x30", &pty, &incremental);
    save_fixture(&format!("{name}-resize-after-70x30"), incremental.screen());
    let _ = after;
    save_raw(&format!("{name}-resize"), &raw);
    pty.finish();
}

fn tree(custom: &str) {
    println!("== process tree: pwsh -> cmd -> ping -t, pwsh -> ping -t (Start-Process)");
    let default_cmd = r#"pwsh.exe -NoLogo -NoProfile -Command "Start-Process -NoNewWindow ping.exe -ArgumentList '-t','localhost'; cmd.exe /c ping -t 127.0.0.1""#;
    let cmd = if custom.is_empty() { default_cmd } else { custom };
    println!("  root cmdline: {cmd}");
    let pty = spawn(cmd, None, 100, 30, 0).unwrap();
    std::thread::sleep(4 * S);
    let before = descendants(pty.pid);
    let me = std::process::id();
    let conhosts: Vec<_> = descendants(me).into_iter().filter(|p| p.2.eq_ignore_ascii_case("conhost.exe") || p.2.eq_ignore_ascii_case("OpenConsole.exe")).collect();
    println!("  root pid {} descendants before: {:?}", pty.pid, before);
    println!("  pseudoconsole host processes under spike: {conhosts:?}");
    println!("  job ActiveProcesses before: {}", pty.job_active());
    for p in std::iter::once(&(pty.pid, 0, "root".to_string())).chain(before.iter()) {
        println!("    pid {} {} in our job: {:?}", p.0, p.2, in_job(p.0, pty.job));
    }
    let t = Instant::now();
    let ok = pty.terminate_job();
    let exit = pty.wait_exit(5 * S);
    println!("  TerminateJobObject ok={ok} root exit={exit:?} in {:?}", t.elapsed());
    std::thread::sleep(Duration::from_millis(500));
    let alive: HashSet<u32> = processes().iter().map(|p| p.0).collect();
    let survivors: Vec<_> = before.iter().filter(|p| alive.contains(&p.0)).collect();
    let pings: Vec<_> = processes().into_iter().filter(|p| p.2.eq_ignore_ascii_case("PING.EXE")).collect();
    println!("  job ActiveProcesses after: {}", pty.job_active());
    println!("  survivors among former descendants: {survivors:?}");
    println!("  any PING.EXE still running system-wide: {pings:?}");
    let mut pty = pty;
    let close = pty.close_pc();
    std::thread::sleep(Duration::from_millis(500));
    let alive: HashSet<u32> = processes().iter().map(|p| p.0).collect();
    let conhost_left: Vec<_> = conhosts.iter().filter(|p| alive.contains(&p.0)).collect();
    println!("  ClosePseudoConsole took {close:?}; conhost survivors: {conhost_left:?}");
    pty.finish();

    // Second variant: drop the job handle (KILL_ON_JOB_CLOSE) instead of TerminateJobObject.
    let pty = spawn(cmd, None, 100, 30, 0).unwrap();
    std::thread::sleep(4 * S);
    let before = descendants(pty.pid);
    unsafe { CloseHandle(pty.job) };
    std::thread::sleep(S);
    let alive: HashSet<u32> = processes().iter().map(|p| p.0).collect();
    let survivors: Vec<_> = before.iter().filter(|p| alive.contains(&p.0)).collect();
    println!("  [KILL_ON_JOB_CLOSE] descendants before={} survivors after CloseHandle(job)={survivors:?} root alive={}", before.len(), alive.contains(&pty.pid));
    let mut pty = pty;
    pty.job = std::ptr::null_mut();
    pty.close_pc();
}

fn send_ctrl_event(pid: u32) -> String {
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe).args(["ctrlc", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn stop(name: &str, cwd: Option<&str>, cmd: &str, method: &str) {
    println!("== stop {name} via {method}");
    let mut pty = spawn(cmd, cwd, 120, 30, 0).unwrap();
    let ready = pty.wait_screen(Duration::from_secs(30), |s| {
        let ph = input_phase(s);
        ph == "input_ready" || ph == "dialog" || s.contents().contains('›')
    });
    pty.settle(S, S, Duration::from_secs(5));
    let pre = pty.parse();
    println!("  ready={ready} phase={} job_active={} root_pid={}", input_phase(pre.screen()), pty.job_active(), pty.pid);
    let t = Instant::now();
    match method {
        "ctrlc-write" => pty.write(b"\x03"),
        "ctrlc-write-x2" => {
            pty.write(b"\x03");
            std::thread::sleep(Duration::from_millis(300));
            pty.write(b"\x03");
        }
        "generate-ctrl-event" => println!("  helper: {}", send_ctrl_event(pty.pid)),
        "close-pseudoconsole" => {
            let d = pty.close_pc();
            println!("  ClosePseudoConsole returned after {d:?}");
        }
        _ => panic!("method"),
    }
    let exit = pty.wait_exit(2 * S);
    let el = t.elapsed();
    std::thread::sleep(Duration::from_millis(200));
    println!(
        "  RESULT {name} {method}: root exited within 2s = {} (code {:?}, {:?}); job ActiveProcesses after = {}",
        exit.is_some(),
        exit,
        el,
        pty.job_active()
    );
    if pty.hpc.is_some() {
        let p = pty.parse();
        save_fixture(&format!("{name}-stop-{method}"), p.screen());
    }
    pty.finish();
}

fn passthrough(name: &str, cwd: Option<&str>, cmd: &str) {
    for (label, flags) in [("conpty", 0u32), ("passthrough", PSEUDOCONSOLE_PASSTHROUGH_MODE)] {
        first_screen(&format!("{name}-{label}"), cwd, cmd, 120, 30, 20, flags);
    }
}

/// Helper process: attach to the pseudoconsole of `pid` and raise CTRL_C_EVENT
/// for every process on it (process group 0).
fn ctrlc(pid: u32) {
    unsafe {
        FreeConsole();
        let a = AttachConsole(pid);
        let h = SetConsoleCtrlHandler(None, 1);
        let g = GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0);
        let e = windows_sys::Win32::Foundation::GetLastError();
        FreeConsole();
        println!("AttachConsole={a} SetConsoleCtrlHandler={h} GenerateConsoleCtrlEvent={g} lastError={e}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let cwd = |i: usize| if a(i) == "-" { None } else { Some(a(i)) };
    match a(0) {
        "screen" => first_screen(a(1), cwd(2), a(3), a(4).parse().unwrap_or(120), a(5).parse().unwrap_or(30), a(6).parse().unwrap_or(20), 0),
        "resize" => resize(a(1), cwd(2), a(3)),
        "tree" => { println!("spike job: {}", own_job()); tree(a(1)) }
        "job" => println!("{}", own_job()),
        // Re-runs a subcommand outside any inherited job (CREATE_BREAKAWAY_FROM_JOB),
        // as a daemon not started under a silent-breakaway job would be.
        "breakaway" => {
            use std::os::windows::process::CommandExt;
            let st = std::process::Command::new(std::env::current_exe().unwrap())
                .args(&args[1..])
                .creation_flags(windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB)
                .status()
                .unwrap();
            std::process::exit(st.code().unwrap_or(1));
        }
        "stop" => stop(a(1), cwd(2), a(3), a(4)),
        "passthrough" => passthrough(a(1), cwd(2), a(3)),
        "quoting" => quoting(),
        "typing" => typing(cwd(1), a(2)),
        "registry" => registry(cwd(1), a(2)),
        "argv" => {
            for x in &args[1..] {
                println!("ARGV {x:?}");
            }
            // Hook probe: also append argv + parent process to a log.
            if let Ok(log) = std::env::var("SPIKE_ARGV_LOG") {
                use std::io::Write;
                let me = std::process::id();
                let all = processes();
                let parent = all.iter().find(|p| p.0 == me).map(|p| p.1).unwrap_or(0);
                let pname = all.iter().find(|p| p.0 == parent).map(|p| p.2.clone()).unwrap_or_default();
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap();
                writeln!(f, "argv={:?} parent={pname}", &args[1..]).unwrap();
            }
        }
        "ctrlc" => ctrlc(a(1).parse().unwrap()),
        "flags" => {
            for f in [0u32, 0x1, 0x2, 0x4, PSEUDOCONSOLE_PASSTHROUGH_MODE] {
                match spawn("cmd.exe /c exit 0", None, 80, 25, f) {
                    Ok(p) => {
                        println!("CreatePseudoConsole flags={f:#x}: accepted");
                        p.finish();
                    }
                    Err(e) => println!("flags={f:#x}: {e}"),
                }
            }
        }
        _ => eprintln!("usage: conpty-spike <screen|resize|tree|stop|passthrough|quoting|flags> ..."),
    }
}

/// Reports whether this process already runs inside a job and that job's limit flags.
pub fn own_job() -> String {
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut injob = 0;
        IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut injob);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        let ok = QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of_val(&info) as u32,
            std::ptr::null_mut(),
        );
        format!("in_job={injob} query_ok={ok} limit_flags={:#x}", info.BasicLimitInformation.LimitFlags)
    }
}

fn in_job(pid: u32, job: windows_sys::Win32::Foundation::HANDLE) -> Option<bool> {
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::*;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut r = 0;
        IsProcessInJob(h, job, &mut r);
        CloseHandle(h);
        Some(r != 0)
    }
}

/// True when the raw stream has an SGR (CSI ... m, no private marker) with
/// parameter 2 (faint/dim). `CSI > 4 ; 2 m` (modifyOtherKeys) does not count.
pub fn sgr_has_dim(raw: &[u8]) -> bool {
    let s = String::from_utf8_lossy(raw);
    let mut rest = &s[..];
    while let Some(i) = rest.find("\x1b[") {
        rest = &rest[i + 2..];
        let end = rest.find(|c: char| ('@'..='~').contains(&c)).unwrap_or(rest.len());
        if end < rest.len() && rest.as_bytes()[end] == b'm' {
            let params = &rest[..end];
            if params.starts_with(['>', '<', '=', '?']) {
                continue;
            }
            let ps: Vec<&str> = params.split(';').collect();
            let mut i = 0;
            while i < ps.len() {
                match ps[i] {
                    // Extended colour: 38;5;n or 38;2;r;g;b.
                    "38" | "48" | "58" => {
                        i += if ps.get(i + 1) == Some(&"5") { 3 } else { 5 };
                        continue;
                    }
                    "2" => return true,
                    _ => {}
                }
                i += 1;
            }
        }
    }
    false
}

/// Types into Claude's input box (never presses Enter), then clears it, checking
/// the delivery.rs signals: input box text, Ctrl-U clear, Backspace.
fn typing(cwd: Option<&str>, cmd: &str) {
    println!("== typing into {cmd} (no Enter is ever sent)");
    let mut pty = spawn(cmd, cwd, 120, 30, 0).unwrap();
    let ready = pty.wait_screen(Duration::from_secs(30), |s| input_phase(s) == "input_ready");
    pty.settle(S, S, Duration::from_secs(5));
    println!("  ready={ready}");
    let text = "hello from conpty \"quoted\" ünïcødé ❯ end";
    for chunk in text.as_bytes().chunks(8) {
        pty.write(chunk);
        std::thread::sleep(Duration::from_millis(20));
    }
    let typed = pty.wait_screen(Duration::from_secs(5), |s| input_phase(s) == "has_draft");
    pty.settle(Duration::from_millis(300), Duration::from_millis(500), Duration::from_secs(3));
    let p = pty.parse();
    let rows: Vec<String> = (0..p.screen().size().0).map(|r| {
        (0..p.screen().size().1).map(|c| match p.screen().cell(r, c) {
            Some(cell) if cell.has_contents() && !cell.dim() => cell.contents().to_string(),
            _ => " ".into(),
        }).collect()
    }).collect();
    let boxed = find_input_box(&rows).unwrap_or_default();
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    println!("  typed has_draft={typed} box={boxed:?} matches={}", squash(&boxed) == squash(text));
    save_fixture("claude-typed", p.screen());
    pty.write(b"\x15");
    let cleared = pty.wait_screen(Duration::from_secs(2), |s| input_phase(s) == "input_ready");
    println!("  after Ctrl-U input_ready={cleared}");
    pty.write(b"abc");
    pty.wait_screen(Duration::from_secs(2), |s| input_phase(s) == "has_draft");
    pty.write(&[0x7f, 0x7f, 0x7f]);
    let bs = pty.wait_screen(Duration::from_secs(2), |s| input_phase(s) == "input_ready");
    println!("  after 3x Backspace(0x7f) input_ready={bs}");
    pty.write(b"\x03");
    std::thread::sleep(Duration::from_millis(300));
    pty.write(b"\x03");
    println!("  exit after ^C^C: {:?}", pty.wait_exit(3 * S));
    pty.finish();
}

/// Launches claude and prints the session registry row it writes for its pid.
fn registry(cwd: Option<&str>, cmd: &str) {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    let pty = spawn(cmd, cwd, 120, 30, 0).unwrap();
    pty.wait_screen(Duration::from_secs(30), |s| input_phase(s) == "input_ready");
    std::thread::sleep(2 * S);
    let home = std::env::var("USERPROFILE").unwrap();
    let path = PathBuf::from(home).join(".claude").join("sessions").join(format!("{}.json", pty.pid));
    let row = std::fs::read_to_string(&path).unwrap_or_else(|e| format!("<{e}>"));
    let (mut c, mut x, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = unsafe { std::mem::zeroed() };
    unsafe { GetProcessTimes(pty.process, &mut c, &mut x, &mut k, &mut u) };
    let created = ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64;
    println!("registry file: ~/.claude/sessions/{}.json", pty.pid);
    println!("row: {}", redact(&row));
    println!("GetProcessTimes creation FILETIME (100ns since 1601): {created}");
    println!("COMPUTERNAME lowercased equals pidDomain host part: {}", row.contains(&format!("\"win32:{}\"", std::env::var("COMPUTERNAME").unwrap_or_default().to_lowercase())));
    let mut pty = pty;
    pty.write(b"\x03");
    std::thread::sleep(Duration::from_millis(300));
    pty.write(b"\x03");
    pty.wait_exit(3 * S);
    println!("row file exists after exit: {}", path.exists());
    pty.finish();
}
