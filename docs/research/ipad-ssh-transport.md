# iPad SSH transport spike (IPAD-1.1)

**Researched: 2026-10-20.** Feeds `docs/product/ipad-app.md` (IPAD) and the iPad amendments in `docs/product/machines.md`.

## Question

Can the iPad reach a machine's daemon by opening SSH exec channels that run a stdio-to-socket relay (`pu daemon proxy`), using Citadel? This needs three things:

- stdin bytes have to stream into the channel, and stdout has to stream back as it arrives, not only when the command exits;
- several channels have to stay open at once on one SSH session;
- if `direct-streamlocal@openssh.com` (the protocol behind `ssh -L local.sock:remote.sock`) were available, it would make a remote relay unnecessary, so that needs checking too.

## Outcome

**Citadel streams exec stdin and stdout, so no fallback is needed.** The raw NIOSSH `ExecRequest` fallback was not implemented. Citadel's `withExec` already is that fallback, wrapped (see the API section below).

| Check | Result |
|---|---|
| Streamed round trip through `cat` | PASS. 5 chunks of 9–37 B were written 150 ms apart. Each one came back as its own stdout frame within 1–8 ms, before the next write. |
| 8-bit clean | PASS. All 256 byte values came back unchanged. |
| Bulk | PASS. 1 MiB of random bytes, written in 32 KiB pieces, came back intact in 34 frames. |
| Two concurrent channels on one `SSHClient` | PASS. Channels A and B ran 5 interleaved rounds each on one SSH session. |
| Ten simultaneous long-lived channels | PASS. 10 of 10 opened. |
| Twelve simultaneous channels | **10 opened and 2 were refused** with `NIOSSHError.channelSetupRejected` ("open failed"). sshd logged `no more sessions`. This is OpenSSH `MaxSessions`, which defaults to 10. |
| Remote command exits non-zero | `SSHClient.CommandFailed(exitCode: 3)` with stderr arrives inside `perform`. `withExec` then rethrows `ChannelError.alreadyClosed` from its own `close()` (see Caveats). |
| Remote binary missing | `CommandFailed(exitCode: 127)`, stderr `zsh:1: command not found: …`. |
| Host-key pin mismatch | The connection was refused before authentication by a custom `NIOSSHClientServerAuthenticationDelegate`. |
| `direct-streamlocal@openssh.com` | **Not available** in SwiftNIO SSH or in Citadel (evidence below). |

### Setup

- Throwaway package at `$TMPDIR/ipad-ssh-spike`, outside the repository and both app projects. It is a Swift 6.2.4 toolchain with Swift 5 language mode, on macOS 26 arm64.
- The server was an unprivileged `/usr/sbin/sshd -D -e -p 22022 -f <tmp>/sshd_config` (OpenSSH_10.0p2) listening on 127.0.0.1 only. It used its own Ed25519 `HostKey`, `AuthorizedKeysFile <tmp>/authorized_keys`, `PasswordAuthentication no` and `UsePAM no`.
- The client authenticated with an Ed25519 key generated in the same temporary directory. No system SSH configuration or `~/.ssh` file was touched, and the sshd was stopped afterwards.

### Versions

| Package | Version |
|---|---|
| Citadel (`github.com/orlandos-nl/Citadel`) | **0.12.1** (tag commit `ae8562f`, 2026-04-04), the latest release |
| swift-nio-ssh | **0.3.7 from the fork `github.com/Wellz26/swift-nio-ssh`**. Citadel 0.12.1 pins this fork (`"0.3.4" ..< "0.4.0"`), not `apple/swift-nio-ssh` (upstream is at 0.15.0). |
| swift-nio | 2.104.0 |
| swift-crypto | 3.15.1 |

Citadel declares `.iOS(.v17)` and `.macOS(.v14)`. `withExec` is `@available(macOS 15.0, *)`, with no iOS restriction, so it is available on the iPadOS 26 target (DEC-7).

## API used

`SSHClient.withExec(_:environment:perform:)` is in `Sources/Citadel/TTY/Client/TTY.swift`. It does the following:

1. Opens a `session` channel.
2. Sends `SSHChannelRequestEvent.ExecRequest(command:wantReply: true)`.
3. Hands `perform` two values:
   - `TTYOutput`, an `AsyncSequence` of `.stdout(ByteBuffer)` / `.stderr(ByteBuffer)` events;
   - `TTYStdinWriter`, whose `write(_:)` sends an `SSHChannelData(type: .channel)` frame with `writeAndFlush`.
4. Closes the channel when `perform` returns or throws.

Unlike `withTTY` and `withPTY`, no PTY is requested, so the channel is 8-bit clean.

The other candidates do not fit:

- `executeCommandStream` and `executeCommandPair` stream stdout and stderr but expose no stdin.
- `executeCommand` buffers all output until the command exits.

### Minimal sample (from the spike)

```swift
import Citadel
import Crypto
import NIOCore
import NIOSSH

let client = try await SSHClient.connect(
    host: "127.0.0.1", port: 22022,
    authenticationMethod: .ed25519(username: user, privateKey: try .init(sshEd25519: privateKeyOpenSSH)),
    hostKeyValidator: .custom(PinValidator(pinned: try NIOSSHPublicKey(openSSHPublicKey: pinnedLine))),
    reconnect: .never
)

try await client.withExec("cat") { inbound, outbound in
    var it = inbound.makeAsyncIterator()
    try await outbound.write(ByteBuffer(string: "chunk-0\n"))
    // Arrives as soon as the remote side writes it; the channel stays open.
    if case .stdout(let echoed)? = try await it.next() { print(String(buffer: echoed)) }
}

/// TOFU pin: the delegate sees the presented key before authentication.
final class PinValidator: NIOSSHClientServerAuthenticationDelegate, @unchecked Sendable {
    let pinned: NIOSSHPublicKey
    init(pinned: NIOSSHPublicKey) { self.pinned = pinned }
    func validateHostKey(hostKey: NIOSSHPublicKey, validationCompletePromise: EventLoopPromise<Void>) {
        hostKey == pinned ? validationCompletePromise.succeed(()) : validationCompletePromise.fail(HostKeyMismatch())
    }
}
```

Two concurrent channels are two `withExec` calls on the same `SSHClient` from separate tasks (`async let a = …; async let b = …`). The spike interleaved them round by round.

## Caveats the transport must handle

1. **Session-channel cap.** By default OpenSSH refuses the 11th concurrent session channel on one connection (`MaxSessions 10`, sshd_config(5)). Against a busy machine, the Mac app can hold more long-lived daemon connections than that:
   - one attach stream per cached terminal;
   - the grid and status subscriptions;
   - `subscribe_fs`;
   - plus a short-lived connection per request.

   One exec channel per daemon connection therefore cannot all live on one SSH connection. `SSHExecTransport` keeps one primary SSH connection per machine and opens an additional connection to the same machine only when a channel open is refused with `channelSetupRejected`. Each additional connection uses the same identity and pinned host key, and is closed once it carries no channels. `docs/product/ipad-app.md` specifies this as IPAD-005.
2. **`withExec` masks the exit status.** When the remote command exits by itself, any exit code included, `withExec`'s trailing `channel.close()` throws `ChannelError.alreadyClosed`. That replaces the `CommandFailed` thrown out of `perform`. The transport has to catch `CommandFailed` (and the stderr text) inside `perform`, record it, and treat a later `alreadyClosed` from `withExec` as normal. This is what tells "no daemon running" (the proxy exits non-zero) and "`pu` not found" (exit 127) apart from a dropped session.
3. **No stdin half-close.** `TTYStdinWriter` has `write` and `changeSize` but cannot send `CHANNEL_EOF`. The relay's end of stream is therefore closing the channel: the client closes it, and sshd closes the remote stdin. `pu daemon proxy` must exit when stdin reaches EOF.
4. **Login shell and `PATH`.** sshd runs the exec string through the user's login shell (the spike's 127 error came from `zsh`), and a non-interactive shell has a minimal `PATH`. The relay command uses the same `/bin/sh` wrapper as the Mac's Start Daemon command (`machines.md`): `/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu daemon proxy'`.
5. **Concurrency annotations.** `SSHClient` is not `Sendable`. Under Swift 6 language mode, capturing it in child tasks is an error unless it is wrapped, for example by an actor that owns the `SSHClient`, or imported with `@preconcurrency import Citadel`.
6. **Back-pressure.** `TTYOutput` is backed by an unbounded `AsyncThrowingStream`, and NIOSSH reads the child channel automatically. A slow consumer therefore buffers in memory rather than shrinking the SSH window. Daemon streams are bounded line-JSON (1 MB request lines, and attach output the app already consumes continuously), so this is acceptable. It is noted for the attach stream.
7. **Supply chain.** Citadel resolves SwiftNIO SSH from a third-party fork. The iPad target pins Citadel exactly (`exact: "0.12.1"`) and commits `Package.resolved`, so the fork revision cannot drift.

## Raw NIOSSH fallback

Not needed, because Citadel passed. For the record, Citadel's `withExec` is the fallback itself:

- it calls `NIOSSHHandler.createChannel(_:channelType: .session)`, adding a `ChannelDuplexHandler` that maps `SSHChannelData` `.channel` and `.stdErr` frames to output, and `SSHChannelRequestEvent.ExitStatus` to the exit code;
- it then triggers `SSHChannelRequestEvent.ExecRequest`.

A raw NIOSSH implementation would be the same ~60 lines (`_executeCommandStream` in `TTY.swift`). It would also gain `channel.close(mode: .output)` for a real stdin EOF. That remains the escape hatch if caveats 2 or 3 ever need fixing at the source.

## `direct-streamlocal@openssh.com`

**Not available.** Neither SwiftNIO SSH (the fork Citadel uses, or upstream) nor Citadel can open a `direct-streamlocal@openssh.com` channel:

- `SSHChannelType` is a closed public enum with exactly `.session`, `.directTCPIP` and `.forwardedTCPIP`. See `Sources/NIOSSH/Child Channels/SSHChannelType.swift:29` in fork 0.3.7, and `:29` in apple/swift-nio-ssh `main` at `149f32f` (2026-10-07, after 0.15.0).
- The wire codec knows only `"session"`, `"forwarded-tcpip"` and `"direct-tcpip"`. It throws `unknownPacketType("Channel request with …")` for any other type. See `Sources/NIOSSH/SSHMessages.swift` `ChannelType` (line 220), parse cases at lines 844, 865 and 884 in the fork, and line 906 upstream.
- Citadel builds on those types. Its only forwarding API is `createDirectTCPIPChannel` (`Sources/Citadel/DirectTCPIP/Client/DirectTCPIP+Client.swift:39`), and nothing in its sources mentions streamlocal.

Supporting it would mean forking SwiftNIO SSH to add a channel type and codec case. It would also depend on the server allowing `AllowStreamLocalForwarding`. The exec relay needs neither, and it works through any sshd that allows command execution. This confirms DEC-2: the iPad uses exec channels running `pu daemon proxy`, not `direct-streamlocal`.

## Sources

- Citadel 0.12.1: `Sources/Citadel/TTY/Client/TTY.swift` (`withExec`, `_executeCommandStream`, `TTYStdinWriter`), `Sources/Citadel/ClientSession.swift` (`SSHHostKeyValidator.custom`) and `Package.swift`.
- SwiftNIO SSH fork 0.3.7 and apple/swift-nio-ssh `main` `149f32f`, files as cited above.
- [sshd_config(5)](https://man.openbsd.org/sshd_config): `MaxSessions` (default 10) and `AllowStreamLocalForwarding`.
- [RFC 4254 §6.5](https://www.rfc-editor.org/rfc/rfc4254#section-6.5): exec requests.
