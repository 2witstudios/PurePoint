<#
.SYNOPSIS
Install the PurePoint CLI (pu.exe) and daemon (pu-engine.exe) into %USERPROFILE%\.pu\bin.

.DESCRIPTION
  irm https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.ps1 | iex

The Windows counterpart of scripts/install.sh (DIST-009, REQ-WINAPP-038). Downloads are verified
before anything is installed: the release key below must have signed the archive (checked with
minisign when it is on PATH, otherwise with the verifier built into this script), and when the
GitHub CLI is installed and signed in, the archive's build provenance must verify with
`gh attestation verify`. At least one of the two must succeed, and any check that runs must pass.
The binaries then lose their Mark of the Web (Unblock-File) and are installed by rename, so a
running pu-engine.exe is renamed aside rather than overwritten. PATH is changed only with
-AddToPath.

Environment (used when the matching parameter is not given):
  PU_VERSION              release to install, e.g. 1.4.0 (default: latest)
  PU_INSTALL_DIR          install directory (default: %USERPROFILE%\.pu\bin)
  PU_ARCHIVE              install from a local purepoint-*-windows-msvc.zip instead of
                          downloading (not verified: you vouch for the file)
  PU_INSTALL_SKIP_VERIFY=1
                          install a download even if its authenticity can't be verified
                          (prints a warning; never overrides a check that failed)

Works in PowerShell 7 and Windows PowerShell 5.1.

.PARAMETER BaseUrl
Where the versioned releases live (default: the GitHub releases of 2witstudios/PurePoint). A
mirror is verified exactly like GitHub; plain http is accepted only for 127.0.0.1/localhost.
#>
[CmdletBinding()]
param(
    [string]$Version = $env:PU_VERSION,
    [string]$InstallDir = $env:PU_INSTALL_DIR,
    [string]$Archive = $env:PU_ARCHIVE,
    [ValidateSet('', 'x86_64', 'aarch64')]
    [string]$Arch = '',
    [string]$BaseUrl = 'https://github.com/2witstudios/PurePoint/releases/download',
    [switch]$AddToPath
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = '2witstudios/PurePoint'
# Minisign public key for release archives (.github/scripts/minisign.sh); the same key as
# scripts/install.sh. The release job refuses a signing key that doesn't match both.
$ReleasePublicKey = 'RWSEvitv5QIgchUQgebD93PuXamKznz5sMJMavmViuMEYSM8nVtcrRv6'

class PuInstallError : System.Exception {
    PuInstallError([string]$message) : base($message) {}
}

function Say([string]$Message) { Write-Host $Message }
function Warn([string]$Message) { [Console]::Error.WriteLine("install.ps1: warning: $Message") }
function Fail([string]$Message) { throw [PuInstallError]::new($Message) }

# Ed25519 (RFC 8032) and BLAKE2b-512 (RFC 7693) for minisign's prehashed signatures. .NET has
# neither, so they are compiled here; C# 5 keeps Windows PowerShell 5.1's compiler happy.
$VerifierSource = @'
using System;
using System.IO;
using System.Numerics;
using System.Security.Cryptography;

public static class PuMinisignVerifier
{
    static readonly ulong[] IV = {
        0x6a09e667f3bcc908UL, 0xbb67ae8584caa73bUL, 0x3c6ef372fe94f82bUL, 0xa54ff53a5f1d36f1UL,
        0x510e527fade682d1UL, 0x9b05688c2b3e6c1fUL, 0x1f83d9abfb41bd6bUL, 0x5be0cd19137e2179UL };
    static readonly byte[] Sigma = {
        0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15, 14,10,4,8,9,15,13,6,1,12,0,2,11,7,5,3,
        11,8,12,0,5,2,15,13,10,14,3,6,7,1,9,4, 7,9,3,1,13,12,11,14,2,6,5,10,4,0,15,8,
        9,0,5,7,2,4,10,15,14,1,11,12,6,8,3,13, 2,12,6,10,0,11,8,3,4,13,7,5,15,14,1,9,
        12,5,1,15,14,13,4,10,0,7,6,3,9,2,8,11, 13,11,7,14,12,1,3,9,5,0,15,4,8,6,2,10,
        6,15,14,9,11,3,0,8,12,2,13,7,1,4,10,5, 10,2,8,4,7,6,1,5,15,11,9,14,3,12,13,0 };

    static ulong Rotr(ulong x, int n) { return (x >> n) | (x << (64 - n)); }

    static void Compress(ulong[] h, byte[] block, ulong t0, ulong t1, bool last)
    {
        ulong[] m = new ulong[16];
        for (int i = 0; i < 16; i++) m[i] = BitConverter.ToUInt64(block, i * 8);
        ulong[] v = new ulong[16];
        for (int i = 0; i < 8; i++) { v[i] = h[i]; v[i + 8] = IV[i]; }
        v[12] ^= t0; v[13] ^= t1;
        if (last) v[14] = ~v[14];
        for (int r = 0; r < 12; r++)
        {
            int s = (r % 10) * 16;
            G(v, 0, 4, 8, 12, m[Sigma[s + 0]], m[Sigma[s + 1]]);
            G(v, 1, 5, 9, 13, m[Sigma[s + 2]], m[Sigma[s + 3]]);
            G(v, 2, 6, 10, 14, m[Sigma[s + 4]], m[Sigma[s + 5]]);
            G(v, 3, 7, 11, 15, m[Sigma[s + 6]], m[Sigma[s + 7]]);
            G(v, 0, 5, 10, 15, m[Sigma[s + 8]], m[Sigma[s + 9]]);
            G(v, 1, 6, 11, 12, m[Sigma[s + 10]], m[Sigma[s + 11]]);
            G(v, 2, 7, 8, 13, m[Sigma[s + 12]], m[Sigma[s + 13]]);
            G(v, 3, 4, 9, 14, m[Sigma[s + 14]], m[Sigma[s + 15]]);
        }
        for (int i = 0; i < 8; i++) h[i] ^= v[i] ^ v[i + 8];
    }

    static void G(ulong[] v, int a, int b, int c, int d, ulong x, ulong y)
    {
        unchecked
        {
            v[a] = v[a] + v[b] + x; v[d] = Rotr(v[d] ^ v[a], 32);
            v[c] = v[c] + v[d]; v[b] = Rotr(v[b] ^ v[c], 24);
            v[a] = v[a] + v[b] + y; v[d] = Rotr(v[d] ^ v[a], 16);
            v[c] = v[c] + v[d]; v[b] = Rotr(v[b] ^ v[c], 63);
        }
    }

    /// <summary>Unkeyed BLAKE2b-512 of a stream.</summary>
    public static byte[] Blake2b512(Stream input)
    {
        ulong[] h = (ulong[])IV.Clone();
        h[0] ^= 0x01010040UL;
        byte[] block = new byte[128];
        int filled = 0;
        ulong t0 = 0, t1 = 0;
        byte[] chunk = new byte[65536];
        int read;
        while ((read = input.Read(chunk, 0, chunk.Length)) > 0)
        {
            int offset = 0;
            while (offset < read)
            {
                // A full block is compressed only once more input follows: the last one is final.
                if (filled == 128)
                {
                    unchecked { t0 += 128; if (t0 < 128) t1++; }
                    Compress(h, block, t0, t1, false);
                    filled = 0;
                }
                int take = Math.Min(128 - filled, read - offset);
                Buffer.BlockCopy(chunk, offset, block, filled, take);
                filled += take;
                offset += take;
            }
        }
        unchecked { t0 += (ulong)filled; if (t0 < (ulong)filled) t1++; }
        for (int i = filled; i < 128; i++) block[i] = 0;
        Compress(h, block, t0, t1, true);
        byte[] output = new byte[64];
        for (int i = 0; i < 8; i++) Buffer.BlockCopy(BitConverter.GetBytes(h[i]), 0, output, i * 8, 8);
        return output;
    }

    static readonly BigInteger P = BigInteger.Pow(2, 255) - 19;
    static readonly BigInteger L = BigInteger.Pow(2, 252) + BigInteger.Parse("27742317777372353535851937790883648493");
    static readonly BigInteger D = Mod(-121665 * Inv(121666));
    static readonly BigInteger SqrtM1 = BigInteger.ModPow(2, (P - 1) / 4, P);

    static BigInteger Mod(BigInteger a) { BigInteger r = a % P; return r.Sign < 0 ? r + P : r; }
    static BigInteger Inv(BigInteger a) { return BigInteger.ModPow(Mod(a), P - 2, P); }

    static BigInteger FromLittleEndian(byte[] bytes, int offset, int count)
    {
        byte[] le = new byte[count + 1];
        Buffer.BlockCopy(bytes, offset, le, 0, count);
        return new BigInteger(le);
    }

    // Extended coordinates (X, Y, Z, T), x = X/Z, y = Y/Z, x*y = T/Z.
    static BigInteger[] Add(BigInteger[] p, BigInteger[] q)
    {
        BigInteger a = Mod((p[1] - p[0]) * (q[1] - q[0]));
        BigInteger b = Mod((p[1] + p[0]) * (q[1] + q[0]));
        BigInteger c = Mod(2 * p[3] * q[3] * D);
        BigInteger d = Mod(2 * p[2] * q[2]);
        BigInteger e = b - a, f = d - c, g = d + c, hh = b + a;
        return new BigInteger[] { Mod(e * f), Mod(g * hh), Mod(f * g), Mod(e * hh) };
    }

    static BigInteger[] Multiply(BigInteger s, BigInteger[] p)
    {
        BigInteger[] q = { BigInteger.Zero, BigInteger.One, BigInteger.One, BigInteger.Zero };
        while (s > 0)
        {
            if (!s.IsEven) q = Add(q, p);
            p = Add(p, p);
            s >>= 1;
        }
        return q;
    }

    static BigInteger RecoverX(BigInteger y, int sign)
    {
        if (y >= P) return -1;
        BigInteger x2 = Mod((y * y - 1) * Inv(D * y * y + 1));
        if (x2.IsZero) return sign == 0 ? BigInteger.Zero : -1;
        BigInteger x = BigInteger.ModPow(x2, (P + 3) / 8, P);
        if (!Mod(x * x - x2).IsZero) x = Mod(x * SqrtM1);
        if (!Mod(x * x - x2).IsZero) return -1;
        if ((int)(x & 1) != sign) x = P - x;
        return x;
    }

    static BigInteger[] Decompress(byte[] bytes)
    {
        if (bytes.Length != 32) return null;
        byte[] copy = (byte[])bytes.Clone();
        int sign = copy[31] >> 7;
        copy[31] &= 0x7f;
        BigInteger y = FromLittleEndian(copy, 0, 32);
        BigInteger x = RecoverX(y, sign);
        if (x.Sign < 0) return null;
        return new BigInteger[] { x, y, BigInteger.One, Mod(x * y) };
    }

    static byte[] Compress(BigInteger[] p)
    {
        BigInteger zi = Inv(p[2]);
        BigInteger x = Mod(p[0] * zi), y = Mod(p[1] * zi);
        byte[] le = y.ToByteArray();
        byte[] output = new byte[32];
        Buffer.BlockCopy(le, 0, output, 0, Math.Min(32, le.Length));
        output[31] |= (byte)((int)(x & 1) << 7);
        return output;
    }

    static readonly BigInteger[] Base = BasePoint();

    static BigInteger[] BasePoint()
    {
        BigInteger y = Mod(4 * Inv(5));
        BigInteger x = RecoverX(y, 0);
        return new BigInteger[] { x, y, BigInteger.One, Mod(x * y) };
    }

    /// <summary>RFC 8032 Ed25519 verification: [S]B = R + [k]A, with a canonical S and R.</summary>
    public static bool Ed25519Verify(byte[] publicKey, byte[] message, byte[] signature)
    {
        if (publicKey == null || publicKey.Length != 32 || signature == null || signature.Length != 64) return false;
        BigInteger[] a = Decompress(publicKey);
        if (a == null) return false;
        byte[] r = new byte[32];
        Buffer.BlockCopy(signature, 0, r, 0, 32);
        BigInteger s = FromLittleEndian(signature, 32, 32);
        if (s >= L) return false;
        byte[] kInput = new byte[64 + message.Length];
        Buffer.BlockCopy(r, 0, kInput, 0, 32);
        Buffer.BlockCopy(publicKey, 0, kInput, 32, 32);
        Buffer.BlockCopy(message, 0, kInput, 64, message.Length);
        byte[] digest;
        using (SHA512 sha = SHA512.Create()) digest = sha.ComputeHash(kInput);
        BigInteger k = FromLittleEndian(digest, 0, 64) % L;
        BigInteger[] negA = { Mod(-a[0]), a[1], a[2], Mod(-a[3]) };
        // [S]B - [k]A must encode exactly as R (so a non-canonical R never matches).
        byte[] check = Compress(Add(Multiply(s, Base), Multiply(k, negA)));
        int diff = 0;
        for (int i = 0; i < 32; i++) diff |= check[i] ^ r[i];
        return diff == 0;
    }
}
'@

function Initialize-Verifier {
    if ('PuMinisignVerifier' -as [type]) { return $true }
    try {
        if ($PSVersionTable.PSEdition -eq 'Desktop') {
            Add-Type -TypeDefinition $VerifierSource -Language CSharp -ReferencedAssemblies System.Numerics
        } else {
            Add-Type -TypeDefinition $VerifierSource -Language CSharp
        }
        return $true
    } catch {
        Warn "the built-in signature verifier is unavailable ($($_.Exception.Message))"
        return $false
    }
}

function ConvertFrom-Base64OrNull([string]$Text) {
    try { return [Convert]::FromBase64String($Text.Trim()) } catch { return $null }
}

function Test-BytesEqual([byte[]]$A, [int]$AOffset, [byte[]]$B, [int]$BOffset, [int]$Count) {
    for ($i = 0; $i -lt $Count; $i++) {
        if ($A[$AOffset + $i] -ne $B[$BOffset + $i]) { return $false }
    }
    return $true
}

function Get-Slice([byte[]]$Bytes, [int]$Offset, [int]$Count) {
    $slice = New-Object byte[] $Count
    [Array]::Copy($Bytes, $Offset, $slice, 0, $Count)
    return , $slice
}

# Checks a minisign signature (prehashed "ED" format, the minisign default) against
# $ReleasePublicKey, including the trusted comment. Mirrors install.sh's OpenSSL path.
function Test-MinisignSignature([string]$File, [string]$SignatureFile) {
    $pub = ConvertFrom-Base64OrNull $ReleasePublicKey
    if ($null -eq $pub -or $pub.Length -ne 42 -or $pub[0] -ne 0x45 -or $pub[1] -ne 0x64) { return $false } # "Ed"
    $key = Get-Slice $pub 10 32

    $lines = [IO.File]::ReadAllText($SignatureFile) -split "`n" | ForEach-Object { $_.TrimEnd("`r") }
    if ($lines.Count -lt 4) { return $false }
    if (-not $lines[0].StartsWith('untrusted comment: ', [StringComparison]::Ordinal)) { return $false }
    $sigLine = ConvertFrom-Base64OrNull $lines[1]
    if ($null -eq $sigLine -or $sigLine.Length -ne 74 -or $sigLine[0] -ne 0x45 -or $sigLine[1] -ne 0x44) { return $false } # "ED"
    if (-not (Test-BytesEqual $sigLine 2 $pub 2 8)) { return $false }
    $signature = Get-Slice $sigLine 10 64

    $prefix = 'trusted comment: '
    if (-not $lines[2].StartsWith($prefix, [StringComparison]::Ordinal)) { return $false }
    $trusted = [Text.Encoding]::UTF8.GetBytes($lines[2].Substring($prefix.Length))
    $global = ConvertFrom-Base64OrNull $lines[3]
    if ($null -eq $global -or $global.Length -ne 64) { return $false }

    $stream = [IO.File]::OpenRead($File)
    try { $hash = [PuMinisignVerifier]::Blake2b512($stream) } finally { $stream.Dispose() }
    if (-not [PuMinisignVerifier]::Ed25519Verify($key, $hash, $signature)) { return $false }
    $globalInput = New-Object byte[] (64 + $trusted.Length)
    [Array]::Copy($signature, 0, $globalInput, 0, 64)
    [Array]::Copy($trusted, 0, $globalInput, 64, $trusted.Length)
    return [PuMinisignVerifier]::Ed25519Verify($key, $globalInput, $global)
}

function Get-Download([string]$Url, [string]$OutFile) {
    $params = @{ Uri = $Url; OutFile = $OutFile; UseBasicParsing = $true }
    Invoke-WebRequest @params | Out-Null
}

function Test-Download([string]$Url, [string]$OutFile) {
    try { Get-Download $Url $OutFile; return $true } catch { return $false }
}

# "verified", "unavailable" (nothing to check with) or "failed".
function Get-SignatureStatus([string]$File, [string]$SignatureUrl, [string]$Work) {
    $sig = Join-Path $Work 'archive.minisig'
    if (-not (Test-Download $SignatureUrl $sig)) {
        Warn "the release has no signature ($SignatureUrl)"
        return 'unavailable'
    }
    $minisign = Get-Command minisign -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($minisign) {
        & $minisign.Source -Vm $File -x $sig -P $ReleasePublicKey *> $null
        if ($LASTEXITCODE -eq 0) { return 'verified' } else { return 'failed' }
    }
    if (Initialize-Verifier) {
        if (Test-MinisignSignature $File $sig) { return 'verified' } else { return 'failed' }
    }
    Warn 'install minisign to check the release signature'
    return 'unavailable'
}

# "verified", "unavailable" or "failed".
function Get-AttestationStatus([string]$File) {
    $gh = Get-Command gh -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $gh) { return 'unavailable' }
    & $gh.Source auth status *> $null
    if ($LASTEXITCODE -ne 0) { return 'unavailable' }
    & $gh.Source attestation verify $File --repo $Repo *> $null
    if ($LASTEXITCODE -eq 0) { return 'verified' } else { return 'failed' }
}

function Confirm-Download([string]$File, [string]$SignatureUrl, [string]$Work) {
    $name = Split-Path -Leaf $File
    $signature = Get-SignatureStatus $File $SignatureUrl $Work
    if ($signature -eq 'failed') { Fail "signature check failed for $name; not installing" }
    $attestation = Get-AttestationStatus $File
    if ($attestation -eq 'failed') { Fail "gh attestation verify failed for $name; not installing" }
    if ($signature -eq 'verified') { Say 'verified release signature' }
    if ($attestation -eq 'verified') { Say 'verified build provenance (gh attestation)' }
    if ($signature -eq 'verified' -or $attestation -eq 'verified') { return }
    if ($env:PU_INSTALL_SKIP_VERIFY -eq '1') {
        Warn "PU_INSTALL_SKIP_VERIFY=1: installing $name without verifying who built it"
        return
    }
    Fail "could not verify ${name}: install minisign, or sign in with the GitHub CLI (gh auth login). To install anyway, set PU_INSTALL_SKIP_VERIFY=1."
}

function Get-TargetArch {
    if ($Arch) { return $Arch }
    $os = $null
    try { $os = [string][Runtime.InteropServices.RuntimeInformation]::OSArchitecture } catch { }
    if (-not $os) {
        $os = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    }
    switch -Regex ($os) {
        '^(X64|AMD64)$' { return 'x86_64' }
        '^(Arm64|ARM64)$' { return 'aarch64' }
        default { Fail "unsupported architecture: $os" }
    }
}

function Get-ReleaseVersion {
    if ($Version) { $v = $Version } else {
        $latest = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
        $v = [string]$latest.tag_name
        if (-not $v) { Fail 'could not determine the latest release' }
    }
    $v = $v.Trim()
    if ($v.StartsWith('v')) { $v = $v.Substring(1) }
    if ($v -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { Fail "release version must be X.Y.Z, got: $v" }
    return $v
}

function Assert-BaseUrl([string]$Url) {
    $uri = $null
    if (-not [Uri]::TryCreate($Url, [UriKind]::Absolute, [ref]$uri)) { Fail "not a URL: $Url" }
    if ($uri.Scheme -eq 'https') { return }
    if ($uri.Scheme -eq 'http' -and ($uri.Host -eq '127.0.0.1' -or $uri.Host -eq 'localhost')) { return }
    Fail "refusing to download over $($uri.Scheme): $Url"
}

function Find-One([string]$Root, [string]$Name) {
    $found = @(Get-ChildItem -LiteralPath $Root -Recurse -File -Filter $Name)
    if ($found.Count -ne 1) { Fail "archive must contain exactly one $Name (found $($found.Count))" }
    return $found[0].FullName
}

# Replace by rename: a running pu-engine.exe can be renamed but not overwritten, so the old
# image moves aside and is deleted when nothing runs it any more.
function Install-Binary([string]$Source, [string]$Directory) {
    $name = Split-Path -Leaf $Source
    $dest = Join-Path $Directory $name
    $tag = "$PID.$([Guid]::NewGuid().ToString('N').Substring(0, 8))"
    $staging = "$dest.new.$tag"
    Copy-Item -LiteralPath $Source -Destination $staging
    Unblock-File -LiteralPath $staging
    $aside = $null
    if (Test-Path -LiteralPath $dest) {
        $aside = "$dest.old.$tag"
        Move-Item -LiteralPath $dest -Destination $aside
    }
    try {
        Move-Item -LiteralPath $staging -Destination $dest
    } catch {
        if ($aside) { Move-Item -LiteralPath $aside -Destination $dest -ErrorAction SilentlyContinue }
        Remove-Item -LiteralPath $staging -Force -ErrorAction SilentlyContinue
        throw
    }
    foreach ($old in @(Get-ChildItem -LiteralPath $Directory -File -Filter "$name.old.*")) {
        try { Remove-Item -LiteralPath $old.FullName -Force -ErrorAction Stop } catch {
            Say "note: $($old.Name) is still in use (a running daemon); it is removed by the next install"
        }
    }
}

function Test-PathContains([string]$PathValue, [string]$Directory) {
    if (-not $PathValue) { return $false }
    $want = $Directory.TrimEnd('\')
    foreach ($entry in $PathValue.Split(';')) {
        if ($entry -and [string]::Equals($entry.TrimEnd('\'), $want, [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
    return $false
}

function Invoke-Install {
    if ($PSVersionTable.PSEdition -eq 'Desktop') {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    }
    $installDir = if ($InstallDir) { $InstallDir } else { Join-Path $env:USERPROFILE '.pu\bin' }
    $work = Join-Path ([IO.Path]::GetTempPath()) ("pu-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
        if ($Archive) {
            if (-not (Test-Path -LiteralPath $Archive -PathType Leaf)) { Fail "PU_ARCHIVE not found: $Archive" }
            # A copy keeps the user's file as it is; Copy-Item keeps its Zone.Identifier stream.
            $zip = Join-Path $work (Split-Path -Leaf $Archive)
            Copy-Item -LiteralPath $Archive -Destination $zip
            Say "note: a local archive is not verified; check it with: gh attestation verify $Archive --repo $Repo"
        } else {
            Assert-BaseUrl $BaseUrl
            $target = "$(Get-TargetArch)-pc-windows-msvc"
            $v = Get-ReleaseVersion
            $name = "purepoint-$v-$target"
            $base = "$($BaseUrl.TrimEnd('/'))/v$v"
            $zip = Join-Path $work "$name.zip"
            Say "downloading $name"
            try { Get-Download "$base/$name.zip" $zip } catch { Fail "download failed: $base/$name.zip" }
            $sum = Join-Path $work 'sum'
            try { Get-Download "$base/$name.zip.sha256" $sum } catch { Fail 'checksum download failed' }
            $expected = ([IO.File]::ReadAllText($sum).Trim() -split '\s+')[0].ToLowerInvariant()
            $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($expected -ne $actual) { Fail "checksum mismatch for $name.zip" }
            Confirm-Download $zip "$base/$name.zip.minisig" $work
        }

        # Mark of the Web: unblock the archive and, below, each binary before it is installed.
        Unblock-File -LiteralPath $zip
        $unpacked = Join-Path $work 'x'
        Expand-Archive -LiteralPath $zip -DestinationPath $unpacked
        $pu = Find-One $unpacked 'pu.exe'
        $engine = Find-One $unpacked 'pu-engine.exe'

        New-Item -ItemType Directory -Force -Path $installDir | Out-Null
        foreach ($bin in @($pu, $engine)) { Install-Binary $bin $installDir }
        Say 'unblocked pu.exe and pu-engine.exe (Unblock-File)'
        Say "installed pu.exe and pu-engine.exe to $installDir"

        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ($AddToPath) {
            if (-not (Test-PathContains $userPath $installDir)) {
                $newPath = if ($userPath) { "$installDir;$userPath" } else { $installDir }
                [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
                Say "added $installDir to your user PATH; open a new terminal to pick it up"
            }
        } elseif (-not (Test-PathContains $userPath $installDir) -and -not (Test-PathContains $env:Path $installDir)) {
            Say ''
            Say 'add it to your PATH (or run this script again with -AddToPath), e.g.:'
            Say "  [Environment]::SetEnvironmentVariable('Path', `"$installDir;`" + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')"
        }

        Say ''
        Say 'next steps:'
        Say "  $installDir\pu.exe daemon install --now   # start the daemon at sign-in (HKCU Run, no admin)"
        Say "  cd <repo>; $installDir\pu.exe init        # register each project"
        Say 'If the daemon is already running, restart it to pick up the new version (pu daemon stop; pu daemon start).'
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}

try {
    Invoke-Install
} catch [PuInstallError] {
    [Console]::Error.WriteLine("install.ps1: $($_.Exception.Message)")
    # Run as a script, exit with a status; run through `irm | iex`, don't close the caller's shell.
    if ($PSCommandPath) { exit 1 }
    throw
}
