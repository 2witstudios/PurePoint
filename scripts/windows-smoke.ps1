#Requires -Version 7
<#
.SYNOPSIS
    Scripted end-to-end smoke run of the `pu` CLI against a real `pu-engine`
    on Windows (WIN-4.6).

.DESCRIPTION
    Creates a throwaway git repository and a throwaway HOME/USERPROFILE, then
    drives every headless row of docs/reference/cli-commands-map.csv (plus the
    newer commands: bench/play, grid, prompt/agent/swarm/schedule/trigger
    definitions, gate, channel, daemon, projects ...) through `pu ... --json`,
    asserting on the parsed JSON at every step. Midway it stops and restarts
    the daemon and checks that benched agents come back suspended and resume.

    At the end it asserts that no pu-engine or agent process started under the
    temp HOME is still alive, removes the temp directories, and prints a
    command -> pass/fail/skipped table. Exit code 0 only if nothing failed.

    The real user's %USERPROFILE%\.pu and HKCU Run key are never touched:
    every pu/pu-engine invocation runs with HOME and USERPROFILE (and the
    claude/codex config dirs) pointed into the temp directory, and
    `pu daemon install/uninstall` are not run.

.PARAMETER PuPath
    pu.exe to test. Default: <workspace>\target\debug\pu.exe.

.PARAMETER EnginePath
    pu-engine.exe to test (passed as PU_ENGINE_PATH). Default: beside the
    default pu.exe.

.PARAMETER Agent
    Agent type for the primary root and worktree agents: terminal (default),
    claude or codex. claude/codex agents are only spawned with no prompt,
    inspected and killed: they never receive a prompt, so they cannot edit
    files or spend tokens. (Under the temp HOME they are not signed in.)
    Prompts, templates and sends always go to terminal agents.
#>
[CmdletBinding()]
param(
    [string]$PuPath,
    [string]$EnginePath,
    [ValidateSet('terminal', 'claude', 'codex')]
    [string]$Agent = 'terminal'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

$workspace = Split-Path -Parent $PSScriptRoot
if (-not $PuPath) { $PuPath = Join-Path $workspace 'target\debug\pu.exe' }
if (-not $EnginePath) { $EnginePath = Join-Path (Split-Path -Parent $PuPath) 'pu-engine.exe' }
$PuPath = (Resolve-Path -LiteralPath $PuPath).ProviderPath
$EnginePath = (Resolve-Path -LiteralPath $EnginePath).ProviderPath
$csvPath = Join-Path $workspace 'docs\reference\cli-commands-map.csv'

# Rows that cannot run headless. Each is reported as skipped with its reason.
$SkippedRows = [ordered]@{
    'pu attach'           = 'interactive: puts the console in raw mode and relays keystrokes to the PTY until detach'
    'pu watch'            = 'interactive: full-screen dashboard that redraws until Ctrl-C'
    'pu phone'            = 'not on Windows yet (phone pairing/bridge is macOS/Linux only)'
    'pu machine'          = 'needs an sshd on a remote machine'
    'pu daemon install'   = 'writes the real HKCU Run key (not redirected by HOME); smoke runs must never touch it'
    'pu daemon uninstall' = 'deletes the real HKCU Run key (not redirected by HOME); smoke runs must never touch it'
    'agent login'         = 'interactive: claude /login and codex login need a browser'
}
# CSV rows that name planning-era commands this CLI does not have. The script
# verifies each is still absent (clap rejects it), so a new command fails the
# run until it is covered here.
$AbsentRows = @('pu aggregate', 'pu merge', 'pu list', 'pu restart', 'pu recover', 'pu pr',
    'pu reset', 'pu wait', 'pu worktree create', 'pu ui',
    'pu cron start', 'pu cron stop', 'pu cron list', 'pu cron add', 'pu cron remove')

# ---------------------------------------------------------------------------
# Results
# ---------------------------------------------------------------------------
$Results = [System.Collections.Generic.List[object]]::new()
function Add-Result([string]$Command, [string]$Status, [string]$Detail = '') {
    $Results.Add([pscustomobject]@{ Command = $Command; Status = $Status; Detail = $Detail })
    $color = switch ($Status) { 'pass' { 'Green' } 'fail' { 'Red' } default { 'Yellow' } }
    $line = "[$Status] $Command"
    if ($Detail) { $line += " - $Detail" }
    Write-Host $line -ForegroundColor $color
}

function Step([string]$Command, [scriptblock]$Body) {
    try {
        & $Body
        Add-Result $Command 'pass'
    } catch {
        $msg = $_.Exception.Message -replace '\s+', ' '
        if ($msg.Length -gt 300) { $msg = $msg.Substring(0, 300) + '...' }
        Add-Result $Command 'fail' $msg
    }
}

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "assertion failed: $Message" }
}

# ---------------------------------------------------------------------------
# Temp layout and isolated environment
# ---------------------------------------------------------------------------
Add-Type -Namespace PuSmoke -Name Native -MemberDefinition @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint size);
'@
function Get-LongPath([string]$Path) {
    $sb = [System.Text.StringBuilder]::new(1024)
    $n = [PuSmoke.Native]::GetLongPathName($Path, $sb, 1024)
    if ($n -gt 0 -and $n -lt 1024) { return $sb.ToString() }
    return $Path
}

$tempBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
$TempRoot = Join-Path (Get-LongPath $tempBase) ("pu-smoke-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
$HomeDir = Join-Path $TempRoot 'home'
$Repo = Join-Path $TempRoot 'repo'
$Repo2 = Join-Path $TempRoot 'repo2'
$FilesDir = Join-Path $TempRoot 'files'
$ScriptStart = Get-Date
$TrackedPids = [System.Collections.Generic.HashSet[int]]::new()

$savedEnv = @{}
foreach ($name in @('HOME', 'USERPROFILE', 'CLAUDE_CONFIG_DIR', 'CODEX_HOME') +
    @(Get-ChildItem Env: | Where-Object Name -like 'PU_*' | ForEach-Object Name)) {
    $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name)
}
function Set-IsolatedEnv {
    foreach ($name in @($savedEnv.Keys)) { [Environment]::SetEnvironmentVariable($name, $null) }
    $env:HOME = $HomeDir
    $env:USERPROFILE = $HomeDir
    $env:CLAUDE_CONFIG_DIR = Join-Path $HomeDir '.claude'
    $env:CODEX_HOME = Join-Path $HomeDir '.codex'
    $env:PU_ENGINE_PATH = $EnginePath
}
function Restore-Env {
    foreach ($name in @($savedEnv.Keys) + 'PU_ENGINE_PATH') {
        $value = if ($savedEnv.ContainsKey($name)) { $savedEnv[$name] } else { $null }
        [Environment]::SetEnvironmentVariable($name, $value)
    }
}

# ---------------------------------------------------------------------------
# Running pu
# ---------------------------------------------------------------------------
function Invoke-Pu {
    param(
        [Parameter(Mandatory, Position = 0)][string[]]$PuArgs,
        [string]$Cwd = $Repo,
        [switch]$AllowFail,
        [int]$TimeoutSec = 120,
        [string]$StdIn
    )
    $psi = [System.Diagnostics.ProcessStartInfo]::new($PuPath)
    foreach ($a in $PuArgs) { $psi.ArgumentList.Add($a) }
    $psi.WorkingDirectory = $Cwd
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    $psi.StandardOutputEncoding = [System.Text.Encoding]::UTF8
    $psi.StandardErrorEncoding = [System.Text.Encoding]::UTF8
    $p = [System.Diagnostics.Process]::Start($psi)
    try {
        $outTask = $p.StandardOutput.ReadToEndAsync()
        $errTask = $p.StandardError.ReadToEndAsync()
        if ($StdIn) { $p.StandardInput.Write($StdIn) }
        $p.StandardInput.Close()
        if (-not $p.WaitForExit($TimeoutSec * 1000)) {
            $p.Kill($true)
            throw "pu $($PuArgs -join ' ') timed out after ${TimeoutSec}s"
        }
        $p.WaitForExit()
        $r = [pscustomobject]@{ Code = $p.ExitCode; Out = $outTask.Result.Trim(); Err = $errTask.Result.Trim() }
    } finally {
        $p.Dispose()
    }
    if ($r.Code -ne 0 -and -not $AllowFail) {
        throw "pu $($PuArgs -join ' ') exited $($r.Code): $($r.Err) $($r.Out)"
    }
    return $r
}

# `pu ... --json`, parsed.
function Invoke-PuJson {
    param(
        [Parameter(Mandatory, Position = 0)][string[]]$PuArgs,
        [string]$Cwd = $Repo,
        [switch]$AllowFail
    )
    $r = Invoke-Pu -PuArgs ($PuArgs + '--json') -Cwd $Cwd -AllowFail:$AllowFail
    try {
        return ($r.Out | ConvertFrom-Json -Depth 64 -NoEnumerate)
    } catch {
        throw "pu $($PuArgs -join ' ') --json printed invalid JSON: $($r.Out)"
    }
}

function Get-Status { Invoke-PuJson @('status') }

# A property that may be absent (StrictMode throws on a missing property).
function Get-Prop($Object, [string]$Name) {
    if ($null -eq $Object) { return $null }
    $p = $Object.PSObject.Properties[$Name]
    if ($p) { return $p.Value }
    return $null
}

# Every agent in a status report (root and worktree), keyed by id, with
# snake_case fields for root agents and camelCase for worktree entries.
function Get-AllAgents($Status) {
    $all = @{}
    foreach ($a in @($Status.agents)) {
        $all[$a.id] = [pscustomobject]@{ id = $a.id; type = $a.agent_type; status = $a.status
            pid = $a.pid; suspended = [bool]$a.suspended; session_id = (Get-Prop $a 'session_id')
            worktree = $null }
    }
    foreach ($wt in @($Status.worktrees)) {
        foreach ($prop in $wt.agents.PSObject.Properties) {
            $a = $prop.Value
            $all[$a.id] = [pscustomobject]@{ id = $a.id; type = $a.agentType; status = $a.status
                pid = (Get-Prop $a 'pid'); suspended = [bool](Get-Prop $a 'suspended')
                session_id = (Get-Prop $a 'sessionId'); worktree = $wt.id }
        }
    }
    foreach ($a in $all.Values) { if ($a.pid) { [void]$TrackedPids.Add([int]$a.pid) } }
    return $all
}

function Get-AgentReport([string]$Id) {
    $r = Invoke-PuJson @('status', '--agent', $Id)
    Assert ($r.type -eq 'agent_status') "status --agent type is $($r.type)"
    Assert ($r.id -eq $Id) "status --agent returned $($r.id)"
    if ($r.pid) { [void]$TrackedPids.Add([int]$r.pid) }
    return $r
}

# Poll `pu read` until a terminal row equals $Marker (the command's output,
# not the echoed command line).
function Wait-ReadMarker([string]$Id, [string]$Marker, [int]$TimeoutSec = 45) {
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $last = $null
    while ((Get-Date) -lt $deadline) {
        $last = Invoke-PuJson @('read', $Id, '--limit', '200')
        Assert ($last.type -eq 'read_result') "read type is $($last.type)"
        Assert ($last.agent_id -eq $Id) "read agent_id is $($last.agent_id)"
        foreach ($rec in @($last.records)) {
            if ($rec.text.Trim() -eq $Marker) { return $last }
        }
        Start-Sleep -Milliseconds 500
    }
    $texts = (@($last.records) | ForEach-Object { $_.text }) -join ' | '
    throw "marker '$Marker' never appeared in pu read $Id (last rows: $texts)"
}

function Test-Alive([int]$ProcessId) {
    $p = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    return [bool]($p -and $p.StartTime -ge $ScriptStart.AddSeconds(-1))
}

function Invoke-Git([string]$Dir, [string[]]$GitArgs) {
    $out = & git -C $Dir @GitArgs 2>&1
    if ($LASTEXITCODE -ne 0) { throw "git $($GitArgs -join ' ') failed: $out" }
}

function Same-Path([string]$A, [string]$B) {
    return [string]::Equals($A.TrimEnd('\'), $B.TrimEnd('\'), [System.StringComparison]::OrdinalIgnoreCase)
}

# Processes started by this run: anything whose command line or executable
# references the temp root, every tracked pid (daemons, agents), and all of
# their descendants. Only processes created after the script started count,
# so a recycled pid is never mistaken for ours.
function Get-SmokeProcesses {
    $procs = @(Get-CimInstance Win32_Process | Where-Object { $_.CreationDate -ge $ScriptStart.AddSeconds(-1) })
    $roots = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($p in $procs) {
        $hay = "$($p.CommandLine) $($p.ExecutablePath)"
        if ($hay.IndexOf($TempRoot, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -or
            $TrackedPids.Contains([int]$p.ProcessId)) {
            [void]$roots.Add([int]$p.ProcessId)
        }
    }
    $selfPid = $PID
    $found = [System.Collections.Generic.HashSet[int]]::new()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    foreach ($r in $roots) { if ($r -ne $selfPid) { $queue.Enqueue($r) } }
    # Children of tracked pids that already exited are still ours.
    foreach ($t in $TrackedPids) { $queue.Enqueue($t) }
    while ($queue.Count -gt 0) {
        $cur = $queue.Dequeue()
        foreach ($p in $procs) {
            if ([int]$p.ParentProcessId -eq $cur -and [int]$p.ProcessId -ne $selfPid -and $found.Add([int]$p.ProcessId)) {
                $queue.Enqueue([int]$p.ProcessId)
            }
        }
    }
    foreach ($r in $roots) { if ($r -ne $selfPid) { [void]$found.Add($r) } }
    return @($procs | Where-Object { $found.Contains([int]$_.ProcessId) })
}

# ---------------------------------------------------------------------------
# Scenario
# ---------------------------------------------------------------------------
function Invoke-Scenario {
    New-Item -ItemType Directory -Force $HomeDir, $Repo, $Repo2, $FilesDir | Out-Null
    Set-IsolatedEnv
    # codex refuses a CODEX_HOME that does not exist.
    New-Item -ItemType Directory -Force $env:CLAUDE_CONFIG_DIR, $env:CODEX_HOME | Out-Null
    Invoke-Git $Repo @('init', '-q', '-b', 'main')
    Invoke-Git $Repo @('config', 'user.email', 'smoke@example.invalid')
    Invoke-Git $Repo @('config', 'user.name', 'pu smoke')
    Invoke-Git $Repo @('config', 'commit.gpgsign', 'false')
    Set-Content -LiteralPath (Join-Path $Repo 'README.md') -Value 'smoke repo' -Encoding utf8NoBOM
    Invoke-Git $Repo @('add', 'README.md')
    Invoke-Git $Repo @('commit', '-q', '-m', 'init')
    Invoke-Git $Repo2 @('init', '-q', '-b', 'main')

    $s = @{}  # shared state between steps

    Step 'pu daemon status (stopped)' {
        $r = Invoke-PuJson @('daemon', 'status')
        Assert ($r.daemon.running -eq $false) 'daemon should not be running yet'
        Assert ($r.socket.StartsWith($HomeDir, [System.StringComparison]::OrdinalIgnoreCase)) "socket $($r.socket) is not under the temp HOME"
    }

    Step 'pu init' {
        $r = Invoke-PuJson @('init')
        Assert ($r.type -eq 'init_result') "type is $($r.type)"
        Assert ($r.created -eq $true) 'created should be true'
        Assert (Test-Path -LiteralPath (Join-Path $Repo '.pu')) '.pu directory missing'
    }

    Step 'pu health' {
        $r = Invoke-PuJson @('health')
        Assert ($r.type -eq 'health_report') "type is $($r.type)"
        Assert ($r.protocol_version -gt 0) 'protocol_version missing'
        Assert (@($r.projects | Where-Object { Same-Path $_ $Repo }).Count -eq 1) "projects $($r.projects -join ',') lack the repo"
        $s.daemonPid = [int]$r.pid
        [void]$TrackedPids.Add($s.daemonPid)
        Assert (Test-Alive $s.daemonPid) "daemon pid $($s.daemonPid) is not alive"
    }

    Step 'pu daemon status' {
        $r = Invoke-PuJson @('daemon', 'status')
        Assert ($r.daemon.running -eq $true) 'daemon should be running'
        Assert ([int]$r.daemon.pid -eq $s.daemonPid) "pid $($r.daemon.pid) != health pid $($s.daemonPid)"
        Assert ($null -ne $r.service) 'service section missing for the default socket'
    }

    Step 'pu daemon proxy' {
        # A relay for SSH exec channels: it stays open until the daemon hangs
        # up (a named pipe has no half-close), so read one reply, then stop it.
        $psi = [System.Diagnostics.ProcessStartInfo]::new($PuPath)
        foreach ($a in @('daemon', 'proxy')) { $psi.ArgumentList.Add($a) }
        $psi.WorkingDirectory = $Repo
        $psi.UseShellExecute = $false
        $psi.RedirectStandardInput = $true
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $p = [System.Diagnostics.Process]::Start($psi)
        try {
            [void]$TrackedPids.Add($p.Id)
            $p.StandardInput.Write('{"type":"health"}' + "`n")
            $p.StandardInput.Flush()
            $lineTask = $p.StandardOutput.ReadLineAsync()
            Assert ($lineTask.Wait(20000)) 'no reply through the proxy within 20s'
            $resp = $lineTask.Result | ConvertFrom-Json
            Assert ($resp.type -eq 'health_report') "proxy relayed $($lineTask.Result)"
            Assert ([int]$resp.pid -eq $s.daemonPid) 'proxy reached a different daemon'
        } finally {
            if (-not $p.HasExited) { $p.Kill($true); [void]$p.WaitForExit(10000) }
            $p.Dispose()
        }
    }

    Step 'pu projects add' {
        $r = Invoke-PuJson @('projects', 'add', $Repo2)
        Assert ($r.type -eq 'init_result') "type is $($r.type)"
        Assert ($r.created -eq $true) 'created should be true'
    }

    Step 'pu projects list' {
        $r = Invoke-PuJson @('projects', 'list')
        Assert ($r.type -eq 'inventory') "type is $($r.type)"
        $roots = @($r.projects | ForEach-Object { $_.project_root })
        Assert (@($roots | Where-Object { Same-Path $_ $Repo2 }).Count -eq 1) "repo2 not listed: $($roots -join ',')"
        Assert ($r.summary.projects -ge 2) "summary.projects is $($r.summary.projects)"
    }

    Step "pu spawn --root --agent $Agent" {
        $r = Invoke-PuJson @('spawn', '--root', '--agent', $Agent)
        Assert ($r.type -eq 'spawn_result') "type is $($r.type)"
        Assert ($null -eq $r.worktree_id) 'root agent got a worktree'
        Assert ($r.status -eq 'running') "status is $($r.status)"
        $s.rootAgent = $r.agent_id
    }

    Step "pu spawn --name smoke-wt --agent $Agent (worktree)" {
        $r = Invoke-PuJson @('spawn', '--agent', $Agent, '--name', 'smoke-wt')
        Assert ($r.type -eq 'spawn_result') "type is $($r.type)"
        Assert ($r.worktree_id -like 'wt-*') "worktree_id is $($r.worktree_id)"
        $s.wtAgent = $r.agent_id
        $s.wtId = $r.worktree_id
    }

    if ($Agent -eq 'terminal') {
        $s.term = $s.rootAgent
    } else {
        Step 'pu spawn --root --agent terminal (send target)' {
            $r = Invoke-PuJson @('spawn', '--root', '--agent', 'terminal')
            Assert ($r.type -eq 'spawn_result' -and $r.status -eq 'running') "spawn returned $($r.type)/$($r.status)"
            $s.term = $r.agent_id
        }
    }

    Step 'pu status' {
        $r = Get-Status
        Assert ($r.type -eq 'status_report') "type is $($r.type)"
        $all = Get-AllAgents $r
        $wt = @($r.worktrees | Where-Object id -eq $s.wtId)
        Assert ($wt.Count -eq 1) "worktree $($s.wtId) missing"
        $s.wtPath = $wt[0].path
        foreach ($id in @($s.rootAgent, $s.wtAgent, $s.term)) {
            Assert ($all.ContainsKey($id)) "agent $id missing from status"
            Assert ($all[$id].status -eq 'running') "agent $id is $($all[$id].status)"
            Assert ([bool]$all[$id].pid) "agent $id has no pid"
            Assert (Test-Alive ([int]$all[$id].pid)) "agent $id pid $($all[$id].pid) is not alive"
        }
        Assert ($all[$s.rootAgent].type -eq $Agent) "root agent type is $($all[$s.rootAgent].type)"
        Assert ($wt[0].branch -eq 'pu/smoke-wt') "branch is $($wt[0].branch)"
        Assert (Test-Path -LiteralPath $wt[0].path) "worktree path $($wt[0].path) missing"
        Assert ($all[$s.wtAgent].worktree -eq $s.wtId) 'worktree agent not under its worktree'
    }

    Step 'pu status --agent' {
        $r = Get-AgentReport $s.rootAgent
        Assert ($r.agent_type -eq $Agent) "agent_type is $($r.agent_type)"
        Assert ($r.status -eq 'running') "status is $($r.status)"
        if ($Agent -eq 'claude') { Assert ([bool](Get-Prop $r 'session_id')) 'claude agent has no session_id' }
    }

    Step 'pu send' {
        $r = Invoke-PuJson @('send', $s.term, 'echo smoke-send-ok')
        Assert ($r.status -eq 'ok') "status is $($r.status)"
        Assert ($r.agent_id -eq $s.term) "agent_id is $($r.agent_id)"
    }

    Step 'pu read' {
        $r = Wait-ReadMarker $s.term 'smoke-send-ok'
        Assert ($r.runtime -eq 'terminal') "runtime is $($r.runtime)"
        Assert ($r.source_kind -eq 'terminal_text') "source_kind is $($r.source_kind)"
        Assert ([bool]$r.next_cursor) 'next_cursor missing'
    }

    if ($Agent -ne 'terminal') {
        Step "pu read ($Agent, no conversation yet)" {
            # The agent was never prompted, so it has no native transcript:
            # either an empty native read or a structured READ_* error.
            $r = Invoke-PuJson @('read', $s.rootAgent) -AllowFail
            if ($r.type -eq 'read_result') {
                Assert ($r.agent_id -eq $s.rootAgent) "agent_id is $($r.agent_id)"
            } else {
                Assert ($r.type -eq 'error') "type is $($r.type)"
                Assert ($r.code -like 'READ_*') "error code is $($r.code)"
            }
        }
    }

    Step 'pu logs' {
        $r = Invoke-PuJson @('logs', $s.term, '--tail', '20000')
        Assert ($r.type -eq 'logs_result') "type is $($r.type)"
        Assert ($r.agent_id -eq $s.term) "agent_id is $($r.agent_id)"
        Assert ($r.data -match 'smoke-send-ok') 'raw terminal bytes lack the sent command'
        if ($Agent -ne 'terminal') {
            $deadline = (Get-Date).AddSeconds(30)
            do {
                $ai = Invoke-PuJson @('logs', $s.rootAgent, '--tail', '20000')
                Assert ($ai.type -eq 'logs_result') "type is $($ai.type)"
                if ($ai.data.Length -gt 0) { break }
                Start-Sleep -Milliseconds 500
            } while ((Get-Date) -lt $deadline)
            Assert ($ai.data.Length -gt 0) "$Agent agent produced no terminal output"
        }
    }

    Step 'pu prompt create' {
        $r = Invoke-PuJson @('prompt', 'create', 'smoke-prompt', '--body', 'echo smoke-tmpl-{{word}}',
            '--description', 'smoke template', '--agent', 'terminal')
        Assert ($r.type -eq 'ok') "type is $($r.type)"
    }

    Step 'pu prompt list' {
        $r = Invoke-PuJson @('prompt', 'list')
        Assert ($r.type -eq 'template_list') "type is $($r.type)"
        $t = @($r.templates | Where-Object name -eq 'smoke-prompt')
        Assert ($t.Count -eq 1) 'smoke-prompt not listed'
        Assert ($t[0].agent -eq 'terminal' -and $t[0].source -eq 'local') "agent/source is $($t[0].agent)/$($t[0].source)"
        Assert ((@($t[0].variables) -join ',') -eq 'word') "variables are $($t[0].variables -join ',')"
    }

    Step 'pu prompt show' {
        $r = Invoke-PuJson @('prompt', 'show', 'smoke-prompt')
        Assert ($r.type -eq 'template_detail') "type is $($r.type)"
        Assert ($r.body -eq 'echo smoke-tmpl-{{word}}') "body is $($r.body)"
        Assert ($r.description -eq 'smoke template') "description is $($r.description)"
    }

    Step 'pu spawn --template' {
        $r = Invoke-PuJson @('spawn', '--root', '--template', 'smoke-prompt', '--var', 'word=ok')
        Assert ($r.type -eq 'spawn_result' -and $r.status -eq 'running') "spawn returned $($r.type)/$($r.status)"
        $s.tmplAgent = $r.agent_id
        $a = Get-AgentReport $s.tmplAgent
        Assert ($a.agent_type -eq 'terminal') "template agent type is $($a.agent_type)"
        Assert ((Get-Prop $a 'prompt') -eq 'echo smoke-tmpl-ok') "prompt is $(Get-Prop $a 'prompt')"
        [void](Wait-ReadMarker $s.tmplAgent 'smoke-tmpl-ok')
    }

    Step 'pu spawn --file' {
        $file = Join-Path $FilesDir 'prompt.md'
        Set-Content -LiteralPath $file -Value 'echo smoke-file-ok' -Encoding utf8NoBOM
        $r = Invoke-PuJson @('spawn', '--root', '--agent', 'terminal', '--file', $file)
        Assert ($r.type -eq 'spawn_result' -and $r.status -eq 'running') "spawn returned $($r.type)/$($r.status)"
        $s.fileAgent = $r.agent_id
        [void](Wait-ReadMarker $s.fileAgent 'smoke-file-ok')
    }

    Step 'pu diff' {
        $r = Invoke-PuJson @('diff')
        Assert ($r.type -eq 'diff_result') "type is $($r.type)"
        $d = @($r.diffs | Where-Object worktree_id -eq $s.wtId)
        Assert ($d.Count -eq 1) 'worktree missing from diff'
        Assert ($d[0].files_changed -eq 0) "clean worktree reports $($d[0].files_changed) files"
        Add-Content -LiteralPath (Join-Path $s.wtPath 'README.md') -Value 'smoke change' -Encoding utf8NoBOM
        $r = Invoke-PuJson @('diff', '--worktree', $s.wtId)
        $d = @($r.diffs | Where-Object worktree_id -eq $s.wtId)
        Assert ($d.Count -eq 1) 'worktree missing from diff --worktree'
        Assert ($d[0].files_changed -eq 1 -and $d[0].insertions -eq 1) "diff is $($d[0].files_changed) files/+$($d[0].insertions)"
        Assert ($d[0].diff_output -match 'smoke change') 'diff_output lacks the change'
        Assert ($d[0].branch -eq 'pu/smoke-wt' -and $d[0].base_branch -eq 'main') "branch $($d[0].branch) base $($d[0].base_branch)"
    }

    Step 'pu pulse' {
        $r = Invoke-PuJson @('pulse')
        Assert ($r.type -eq 'pulse_report') "type is $($r.type)"
        $w = @($r.worktrees | Where-Object worktree_id -eq $s.wtId)
        Assert ($w.Count -eq 1) 'worktree missing from pulse'
        Assert (@($w[0].agents | Where-Object id -eq $s.wtAgent).Count -eq 1) 'worktree agent missing from pulse'
        Assert (@($r.root_agents | Where-Object id -eq $s.rootAgent).Count -eq 1) 'root agent missing from pulse'
    }

    Step 'pu agents list' {
        $r = Invoke-PuJson @('agents', 'list')
        Assert ($r.type -eq 'inventory') "type is $($r.type)"
        $ids = @($r.agents | ForEach-Object { $_.id })
        foreach ($id in @($s.rootAgent, $s.wtAgent, $s.term)) { Assert ($ids -contains $id) "agent $id missing from inventory" }
        Assert ($r.summary.running -ge 3) "summary.running is $($r.summary.running)"
    }

    Step 'pu worktrees list' {
        $r = Invoke-PuJson @('worktrees', 'list')
        Assert ($r.type -eq 'inventory') "type is $($r.type)"
        Assert ($r.summary.worktrees -ge 1) "summary.worktrees is $($r.summary.worktrees)"
        $wtIds = @($r.worktrees | ForEach-Object { (Get-Prop $_ 'worktree_id') ?? (Get-Prop $_ 'id') })
        Assert ($wtIds -contains $s.wtId) "worktree $($s.wtId) missing: $($wtIds -join ',')"
    }

    Step 'pu grid show' {
        $r = Invoke-Pu @('grid', 'show', '--json')
        Assert ($r.Out -eq 'null') "no layout should print null, got $($r.Out)"
        # The engine serves the app's saved workspaces file verbatim.
        $layoutFile = Join-Path $Repo '.pu\workspaces.json'
        Set-Content -LiteralPath $layoutFile -Value '{"smoke":"grid","workspaces":[]}' -Encoding utf8NoBOM
        try {
            $r = Invoke-PuJson @('grid', 'show')
            Assert ($r.smoke -eq 'grid') 'grid show did not return the saved layout'
        } finally {
            Remove-Item -LiteralPath $layoutFile -Force
        }
    }
    # Mutations are broadcast to a subscribed app; headless they must succeed
    # and confirm (they have no --json).
    foreach ($g in @(
            @{ Name = 'pu grid split'; Args = @('grid', 'split', '--axis', 'h'); Says = 'Split' },
            @{ Name = 'pu grid focus'; Args = @('grid', 'focus', '--direction', 'right'); Says = 'Focus' },
            @{ Name = 'pu grid assign'; Args = @('grid', 'assign', 'AGENT', '--leaf', '1'); Says = 'assigned' },
            @{ Name = 'pu grid tab new'; Args = @('grid', 'tab', 'new'); Says = '' },
            @{ Name = 'pu grid close'; Args = @('grid', 'close', '--leaf', '1'); Says = 'Close' })) {
        Step $g.Name {
            $gridArgs = @($g.Args | ForEach-Object { if ($_ -eq 'AGENT') { $s.term } else { $_ } })
            $r = Invoke-Pu $gridArgs
            Assert ($r.Out.Length -gt 0) 'no confirmation printed'
            if ($g.Says) { Assert ($r.Out -match $g.Says) "confirmation was '$($r.Out)'" }
        }
    }

    Step 'pu agent create' {
        $r = Invoke-PuJson @('agent', 'create', 'smoke-agent', '--agent-type', 'terminal',
            '--inline-prompt', 'echo smoke-agentdef-ok', '--tags', 'smoke,ci')
        Assert ($r.type -eq 'ok') "type is $($r.type)"
    }

    Step 'pu agent list' {
        $r = Invoke-PuJson @('agent', 'list')
        Assert ($r.type -eq 'agent_def_list') "type is $($r.type)"
        $d = @($r.agent_defs | Where-Object name -eq 'smoke-agent')
        Assert ($d.Count -eq 1) 'smoke-agent not listed'
        Assert ((@($d[0].tags) -join ',') -eq 'smoke,ci') "tags are $($d[0].tags -join ',')"
    }

    Step 'pu agent show' {
        $r = Invoke-PuJson @('agent', 'show', 'smoke-agent')
        Assert ($r.type -eq 'agent_def_detail') "type is $($r.type)"
        Assert ($r.agent_type -eq 'terminal' -and $r.inline_prompt -eq 'echo smoke-agentdef-ok') "detail is $($r.agent_type)/$($r.inline_prompt)"
    }

    Step 'pu swarm create' {
        $r = Invoke-PuJson @('swarm', 'create', 'smoke-swarm', '--worktrees', '1', '--roster', 'smoke-agent:worker:1')
        Assert ($r.type -eq 'ok') "type is $($r.type)"
    }

    Step 'pu swarm list' {
        $r = Invoke-PuJson @('swarm', 'list')
        Assert ($r.type -eq 'swarm_def_list') "type is $($r.type)"
        Assert (@($r.swarm_defs | Where-Object name -eq 'smoke-swarm').Count -eq 1) 'smoke-swarm not listed'
    }

    Step 'pu swarm show' {
        $r = Invoke-PuJson @('swarm', 'show', 'smoke-swarm')
        Assert ($r.type -eq 'swarm_def_detail') "type is $($r.type)"
        Assert ($r.worktree_count -eq 1) "worktree_count is $($r.worktree_count)"
        $e = @($r.roster)[0]
        Assert ($e.agent_def -eq 'smoke-agent' -and $e.role -eq 'worker' -and $e.quantity -eq 1) "roster is $($e | ConvertTo-Json -Compress)"
    }

    Step 'pu swarm run' {
        $r = Invoke-PuJson @('swarm', 'run', 'smoke-swarm')
        Assert ($r.type -eq 'run_swarm_result') "type is $($r.type)"
        $spawned = @($r.spawned_agents)
        Assert ($spawned.Count -eq 1) "spawned $($spawned.Count) agents"
        $s.swarmAgent = $spawned[0]
        $a = Get-AgentReport $s.swarmAgent
        Assert ($a.agent_type -eq 'terminal' -and [bool]$a.worktree_id -and $a.worktree_id -ne $s.wtId) "swarm agent is $($a.agent_type) in $($a.worktree_id)"
        [void](Wait-ReadMarker $s.swarmAgent 'smoke-agentdef-ok')
    }

    Step 'pu schedule create' {
        # Far in the future: it must never fire during the run.
        $r = Invoke-PuJson @('schedule', 'create', 'smoke-schedule', '--start-at', '2099-01-01T00:00:00Z',
            '--trigger', 'inline-prompt', '--trigger-prompt', 'echo never', '--agent', 'terminal', '--root')
        Assert ($r.type -eq 'ok') "type is $($r.type)"
    }

    Step 'pu schedule list' {
        $r = Invoke-PuJson @('schedule', 'list')
        Assert ($r.type -eq 'schedule_list') "type is $($r.type)"
        $d = @($r.schedules | Where-Object name -eq 'smoke-schedule')
        Assert ($d.Count -eq 1) 'smoke-schedule not listed'
        Assert ($d[0].enabled -eq $true -and $d[0].root -eq $true) "enabled/root is $($d[0].enabled)/$($d[0].root)"
        Assert ($d[0].trigger.type -eq 'inline_prompt' -and $d[0].trigger.agent -eq 'terminal') "trigger is $($d[0].trigger | ConvertTo-Json -Compress)"
    }

    Step 'pu schedule disable' {
        $r = Invoke-PuJson @('schedule', 'disable', 'smoke-schedule')
        Assert ($r.type -eq 'ok') "type is $($r.type)"
        Assert ((Invoke-PuJson @('schedule', 'show', 'smoke-schedule')).enabled -eq $false) 'still enabled'
    }

    Step 'pu schedule enable' {
        $r = Invoke-PuJson @('schedule', 'enable', 'smoke-schedule')
        Assert ($r.type -eq 'ok') "type is $($r.type)"
        Assert ((Invoke-PuJson @('schedule', 'show', 'smoke-schedule')).enabled -eq $true) 'still disabled'
    }

    Step 'pu schedule show' {
        $r = Invoke-PuJson @('schedule', 'show', 'smoke-schedule')
        Assert ($r.type -eq 'schedule_detail') "type is $($r.type)"
        Assert ($r.trigger.prompt -eq 'echo never') "prompt is $($r.trigger.prompt)"
        Assert (Same-Path $r.project_root $Repo) "project_root is $($r.project_root)"
        Assert (([datetime]$r.next_run).Year -eq 2099) "next_run is $($r.next_run)"
    }

    Step 'pu trigger create' {
        $r = Invoke-PuJson @('trigger', 'create', 'smoke-gate-pass', '--on', 'pre_push', '--gate', 'exit 0', '--description', 'passes')
        Assert ($r.type -eq 'ok') "type is $($r.type)"
        $r = Invoke-PuJson @('trigger', 'create', 'smoke-gate-fail', '--on', 'pre_commit', '--gate', 'exit 3')
        Assert ($r.type -eq 'ok') "type is $($r.type)"
    }

    Step 'pu trigger list' {
        $r = Invoke-PuJson @('trigger', 'list')
        Assert ($r.type -eq 'trigger_list') "type is $($r.type)"
        $names = @($r.triggers | ForEach-Object { $_.name })
        Assert ($names -contains 'smoke-gate-pass' -and $names -contains 'smoke-gate-fail') "triggers are $($names -join ',')"
    }

    Step 'pu trigger show' {
        $r = Invoke-PuJson @('trigger', 'show', 'smoke-gate-pass')
        Assert ($r.type -eq 'trigger_detail') "type is $($r.type)"
        Assert ($r.on -eq 'pre_push' -and $r.description -eq 'passes') "on/description is $($r.on)/$($r.description)"
        Assert (@($r.sequence)[0].gate.run -eq 'exit 0') 'gate step missing'
    }

    Step 'pu gate' {
        $pass = Invoke-Pu @('gate', 'pre_push') -AllowFail
        Assert ($pass.Code -eq 0) "passing gate exited $($pass.Code): $($pass.Err)"
        $fail = Invoke-Pu @('gate', 'pre_commit') -AllowFail
        Assert ($fail.Code -ne 0) 'failing gate (exit 3) passed'
        Assert ($fail.Err -match 'gate check failed') "failing gate said: $($fail.Err)"
    }

    Step 'pu channel send' {
        $r = Invoke-PuJson @('channel', 'send', 'hello from the smoke run')
        Assert ($r.type -eq 'channel_message') "type is $($r.type)"
        Assert ($r.message.text -eq 'hello from the smoke run') "text is $($r.message.text)"
        Assert ($r.message.author.kind -eq 'human') "author kind is $($r.message.author.kind)"
        $s.msgId = $r.message.id
        $reply = Invoke-PuJson @('channel', 'send', 'smoke reply', '--reply-to', $s.msgId)
        Assert ($reply.message.parent_id -eq $s.msgId) "reply parent_id is $($reply.message.parent_id)"
    }

    Step 'pu channel edit' {
        $r = Invoke-PuJson @('channel', 'edit', $s.msgId, 'edited by the smoke run')
        Assert ($r.type -eq 'channel_message') "type is $($r.type)"
        Assert ($r.message.text -eq 'edited by the smoke run') "text is $($r.message.text)"
        Assert ($null -ne $r.message.edited_at) 'edited_at not set'
    }

    Step 'pu channel react' {
        $r = Invoke-PuJson @('channel', 'react', $s.msgId)
        Assert (@($r.message.reactions).Count -eq 1) 'reaction missing'
        $r = Invoke-PuJson @('channel', 'react', $s.msgId, '--remove')
        Assert (@($r.message.reactions).Count -eq 0) 'reaction not removed'
    }

    Step 'pu channel read' {
        $r = Invoke-PuJson @('channel', 'read')
        Assert ($r.type -eq 'channel_history') "type is $($r.type)"
        $m = @($r.messages | Where-Object id -eq $s.msgId)
        Assert ($m.Count -eq 1 -and $m[0].text -eq 'edited by the smoke run') 'edited message missing from history'
        Assert ($r.reply_counts.($s.msgId) -eq 1) "reply count is $($r.reply_counts.($s.msgId))"
        $t = Invoke-PuJson @('channel', 'read', '--thread', $s.msgId)
        Assert (@($t.messages | Where-Object text -eq 'smoke reply').Count -eq 1) 'reply missing from thread'
    }

    # --- bench / play and a daemon restart -------------------------------
    Step 'pu bench' {
        $r = Invoke-PuJson @('bench', $s.term)
        Assert ($r.type -eq 'suspend_result') "type is $($r.type)"
        Assert (@($r.suspended) -contains $s.term) "suspended is $($r.suspended -join ',')"
        $a = Get-AgentReport $s.term
        Assert ($a.suspended -eq $true) 'benched agent not suspended'
        Assert ($null -eq $a.pid) "benched agent still has pid $($a.pid)"
    }

    Step 'pu daemon stop' {
        $s.beforeStop = Get-AllAgents (Get-Status)
        $r = Invoke-Pu @('daemon', 'stop')
        Assert ($r.Out -match 'daemon stopped') "stop said: $($r.Out)"
        Assert (-not (Test-Alive $s.daemonPid)) "daemon pid $($s.daemonPid) still alive after stop"
        $st = Invoke-PuJson @('daemon', 'status')
        Assert ($st.daemon.running -eq $false) 'daemon status still reports running'
        foreach ($a in $s.beforeStop.Values) {
            if ($a.pid) { Assert (-not (Test-Alive ([int]$a.pid))) "agent $($a.id) pid $($a.pid) survived daemon stop" }
        }
    }

    Step 'pu daemon start' {
        $r = Invoke-Pu @('daemon', 'start')
        Assert ($r.Out -match 'daemon running') "start said: $($r.Out)"
        $h = Invoke-PuJson @('health')
        Assert ([int]$h.pid -ne $s.daemonPid) 'daemon pid unchanged after restart'
        $s.daemonPid = [int]$h.pid
        [void]$TrackedPids.Add($s.daemonPid)
    }

    Step 'daemon restart: benched agents stay suspended (status)' {
        $after = Get-AllAgents (Get-Status)
        foreach ($a in $s.beforeStop.Values) {
            if ($a.status -ne 'running') { continue }
            Assert ($after.ContainsKey($a.id)) "agent $($a.id) vanished across the restart"
            $now = $after[$a.id]
            # As on Unix: benched agents and resumable AI agents (a stored
            # session id) come back suspended; other live agents are Broken.
            $resumable = $a.suspended -or ($a.session_id -and $a.type -in @('claude', 'codex', 'opencode'))
            if ($resumable) {
                Assert ($now.suspended -eq $true) "agent $($a.id) ($($a.type)) is not suspended after restart"
                Assert ($null -eq $now.pid) "suspended agent $($a.id) has pid $($now.pid)"
            } else {
                Assert ($now.status -eq 'broken') "agent $($a.id) ($($a.type)) is $($now.status), expected broken"
            }
        }
        Assert ($after[$s.term].suspended -eq $true) 'benched terminal agent lost its suspension'
    }

    Step 'pu play' {
        $r = Invoke-PuJson @('play', $s.term)
        Assert ($r.type -eq 'resume_result') "type is $($r.type)"
        Assert ($r.agent_id -eq $s.term -and $r.status -eq 'running') "resume is $($r.agent_id)/$($r.status)"
        $a = Get-AgentReport $s.term
        Assert ($a.suspended -eq $false -and $a.status -eq 'running') "after play: suspended=$($a.suspended) status=$($a.status)"
        Assert (Test-Alive ([int]$a.pid)) "resumed agent pid $($a.pid) not alive"
        $null = Invoke-PuJson @('send', $s.term, 'echo smoke-resumed-ok')
        [void](Wait-ReadMarker $s.term 'smoke-resumed-ok')
        if ($Agent -eq 'claude') {
            # The never-prompted claude agent is resumable too.
            $c = Invoke-PuJson @('play', $s.rootAgent)
            Assert ($c.type -eq 'resume_result' -and $c.agent_id -eq $s.rootAgent) "claude resume returned $($c.type)"
        }
    }

    # --- teardown commands ---------------------------------------------------
    Step 'pu kill --agent' {
        $r = Invoke-PuJson @('kill', '--agent', $s.fileAgent)
        Assert ($r.type -eq 'kill_result') "type is $($r.type)"
        Assert (@($r.killed) -contains $s.fileAgent) "killed is $($r.killed -join ',')"
        $all = Get-AllAgents (Get-Status)
        Assert (-not $all.ContainsKey($s.fileAgent) -or $all[$s.fileAgent].status -ne 'running') 'killed agent still running'
        Assert ($all.ContainsKey($s.term) -and $all[$s.term].status -eq 'running') 'kill --agent hit another agent'

    }

    Step 'pu kill --worktree' {
        $r = Invoke-PuJson @('kill', '--worktree', $s.wtId)
        Assert ($r.type -eq 'kill_result') "type is $($r.type)"
        $all = Get-AllAgents (Get-Status)
        Assert (-not $all.ContainsKey($s.wtAgent) -or $all[$s.wtAgent].status -ne 'running') 'worktree agent still running'

    }

    Step 'pu clean --worktree' {
        $r = Invoke-PuJson @('clean', '--worktree', $s.wtId)
        Assert ($r.type -eq 'delete_worktree_result') "type is $($r.type)"
        Assert ($r.worktree_id -eq $s.wtId -and $r.directory_removed -eq $true -and $r.branch_deleted -eq $true) "result is $($r | ConvertTo-Json -Compress)"
        Assert (-not (Test-Path -LiteralPath $s.wtPath)) "worktree dir $($s.wtPath) still exists"
    }

    Step 'pu clean --all' {
        # A second worktree so --all has more than one result to print.
        $null = Invoke-PuJson @('spawn', '--agent', 'terminal', '--name', 'smoke-wt2')
        $r = Invoke-PuJson @('clean', '--all')
        $items = @($r)
        Assert ($r -is [array]) "clean --all --json is not an array: $($r | ConvertTo-Json -Compress -Depth 5)"
        Assert ($items.Count -eq 2) "cleaned $($items.Count) worktrees, expected 2 (swarm + smoke-wt2)"
        foreach ($i in $items) {
            Assert ($i.type -eq 'delete_worktree_result' -and $i.directory_removed -eq $true) "result is $($i | ConvertTo-Json -Compress)"
        }
        Assert (@((Get-Status).worktrees).Count -eq 0) 'worktrees remain after clean --all'
    }

    Step 'pu kill --all --include-root' {
        $r = Invoke-PuJson @('kill', '--all', '--include-root')
        Assert ($r.type -eq 'kill_result') "type is $($r.type)"
        $all = Get-AllAgents (Get-Status)
        $running = @($all.Values | Where-Object { $_.status -eq 'running' -and -not $_.suspended })
        Assert ($running.Count -eq 0) "still running: $(($running | ForEach-Object { $_.id }) -join ',')"
    }

    foreach ($d in @(
            @{ Name = 'pu schedule delete'; Del = @('schedule', 'delete', 'smoke-schedule'); List = @('schedule', 'list'); Field = 'schedules'; Item = 'smoke-schedule' },
            @{ Name = 'pu trigger delete'; Del = @('trigger', 'delete', 'smoke-gate-pass'); List = @('trigger', 'list'); Field = 'triggers'; Item = 'smoke-gate-pass' },
            @{ Name = 'pu swarm delete'; Del = @('swarm', 'delete', 'smoke-swarm'); List = @('swarm', 'list'); Field = 'swarm_defs'; Item = 'smoke-swarm' },
            @{ Name = 'pu agent delete'; Del = @('agent', 'delete', 'smoke-agent'); List = @('agent', 'list'); Field = 'agent_defs'; Item = 'smoke-agent' },
            @{ Name = 'pu prompt delete'; Del = @('prompt', 'delete', 'smoke-prompt'); List = @('prompt', 'list'); Field = 'templates'; Item = 'smoke-prompt' })) {
        Step $d.Name {
            $r = Invoke-PuJson $d.Del
            Assert ($r.type -eq 'ok') "type is $($r.type)"
            $l = Invoke-PuJson $d.List
            Assert (@($l.($d.Field) | Where-Object name -eq $d.Item).Count -eq 0) "$($d.Item) still listed"
        }
    }

    Step 'pu projects remove' {
        $r = Invoke-PuJson @('projects', 'remove', $Repo2)
        Assert ($r.type -eq 'project_removed') "type is $($r.type)"
        Assert ($r.removed -eq $true -and (Same-Path $r.project_root $Repo2)) "removed=$($r.removed) root=$($r.project_root)"
        $l = Invoke-PuJson @('projects', 'list')
        Assert (@($l.projects | Where-Object { Same-Path $_.project_root $Repo2 }).Count -eq 0) 'repo2 still listed'
        Assert (Test-Path -LiteralPath $Repo2) 'projects remove deleted the folder'
    }

    Step 'pu health (final)' {
        $r = Invoke-PuJson @('health')
        Assert ([int]$r.pid -eq $s.daemonPid) 'daemon changed unexpectedly'
    }

    Step 'pu daemon stop (final)' {
        $r = Invoke-Pu @('daemon', 'stop')
        Assert ($r.Out -match 'daemon stopped') "stop said: $($r.Out)"
        Assert (-not (Test-Alive $s.daemonPid)) "daemon pid $($s.daemonPid) still alive"
    }

    foreach ($row in $AbsentRows) {
        $words = @(($row -split ' ') | Select-Object -Skip 1)
        $r = Invoke-Pu ($words + '--help') -AllowFail
        if ($r.Code -ne 0) {
            Add-Result $row 'skipped' 'CSV planning row: no such pu subcommand (verified absent)'
        } else {
            Add-Result $row 'fail' "'$row' exists now: cover it in scripts/windows-smoke.ps1"
        }
    }
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
Write-Host "pu:        $PuPath"
Write-Host "pu-engine: $EnginePath"
Write-Host "agent:     $Agent"
Write-Host "temp root: $TempRoot"
$originalLocation = Get-Location
try {
    Invoke-Scenario
} catch {
    Add-Result 'scenario setup' 'fail' ($_.Exception.Message -replace '\s+', ' ')
} finally {
    # Best-effort stop of a daemon still running under the temp HOME.
    try {
        if (Test-Path -LiteralPath $Repo) { $null = Invoke-Pu @('daemon', 'stop') -AllowFail -TimeoutSec 30 }
    } catch { Write-Warning "daemon stop during cleanup: $_" }

    $leftover = @()
    for ($i = 0; $i -lt 20; $i++) {
        $leftover = @(Get-SmokeProcesses)
        if ($leftover.Count -eq 0) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($leftover.Count -eq 0) {
        Add-Result 'no leftover pu-engine/agent processes' 'pass'
    } else {
        $desc = ($leftover | ForEach-Object { "$($_.Name)#$($_.ProcessId)" }) -join ', '
        Add-Result 'no leftover pu-engine/agent processes' 'fail' "still alive: $desc"
        foreach ($p in $leftover) { Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }
    }

    Set-Location $originalLocation
    Restore-Env
    for ($i = 0; $i -lt 10 -and (Test-Path -LiteralPath $TempRoot); $i++) {
        try { Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction Stop } catch { Start-Sleep -Milliseconds 500 }
    }
    if (Test-Path -LiteralPath $TempRoot) {
        Add-Result 'temp dirs removed' 'fail' "$TempRoot still exists"
    } else {
        Add-Result 'temp dirs removed' 'pass'
    }
}

foreach ($row in $SkippedRows.Keys) { Add-Result $row 'skipped' $SkippedRows[$row] }

# Every CSV row must be accounted for by at least one result.
$csvRows = @(Import-Csv -LiteralPath $csvPath | ForEach-Object { $_.Command })
foreach ($cmd in $csvRows) {
    $covered = @($Results | Where-Object { $_.Command -eq $cmd -or $_.Command.StartsWith("$cmd ") -or $_.Command.StartsWith("$cmd (") })
    if ($covered.Count -eq 0) { Add-Result $cmd 'fail' 'CSV row not exercised or listed as skipped by the smoke script' }
}

Write-Host ''
Write-Host "Windows CLI smoke run (-Agent $Agent)"
$Results | Format-Table -AutoSize -Wrap Command, Status, Detail | Out-String -Width 220 | Write-Host
$pass = @($Results | Where-Object Status -eq 'pass').Count
$fail = @($Results | Where-Object Status -eq 'fail').Count
$skip = @($Results | Where-Object Status -eq 'skipped').Count
Write-Host "pass: $pass  fail: $fail  skipped: $skip"
exit ([int]($fail -gt 0))
