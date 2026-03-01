import Foundation

enum TmuxCommandBuilder {
    enum Action {
        case hasSession(String)
        case newGroupedSession(target: String, viewSession: String)
        case setOption(target: String, option: String, value: String)
        case selectWindow(target: String, window: String)
        case listSessions
        case killSession(String)
    }

    /// Build the shell command string for a tmux action.
    /// The returned string is meant to be run via `/bin/zsh -c "..."`.
    static func shellCommand(for action: Action) -> String {
        let args = shellFragments(for: action)
        return args.joined(separator: " ")
    }

    /// Build shell command fragments for a tmux action.
    /// These contain shell syntax (redirects, semicolons) and must be
    /// joined into a string for `/bin/zsh -c "..."`, not passed as Process arguments.
    static func shellFragments(for action: Action) -> [String] {
        switch action {
        case .hasSession(let name):
            return ["tmux", "has-session", "-t", shellEscape(name), "2>/dev/null"]

        case .newGroupedSession(let target, let viewSession):
            return [
                "tmux", "new-session",
                "-t", shellEscape(target),
                "-s", shellEscape(viewSession),
                "\\;", "set-option", "destroy-unattached", "on",
                "\\;", "set-option", "status", "off",
                "\\;", "set-option", "mouse", "on",
            ]

        case .setOption(let target, let option, let value):
            return ["tmux", "set-option", "-t", shellEscape(target), shellEscape(option), shellEscape(value), "2>/dev/null"]

        case .selectWindow(let target, let window):
            return ["tmux", "select-window", "-t", shellEscape(target) + ":" + shellEscape(window)]

        case .listSessions:
            return ["tmux", "list-sessions", "-F", shellEscape("#{session_name}")]

        case .killSession(let name):
            return ["tmux", "kill-session", "-t", shellEscape(name)]
        }
    }

    /// Build the full shell command for starting a grouped tmux session
    /// suitable for use with SwiftTerm's startProcess.
    static func groupedSessionCommand(tmuxTarget: String, viewSession: String, windowSpec: String?) -> String {
        var cmd = "if [ -x /usr/libexec/path_helper ]; then eval $(/usr/libexec/path_helper -s); fi; "
        cmd += "[ -f ~/.zprofile ] && source ~/.zprofile; "
        cmd += "[ -f ~/.zshrc ] && source ~/.zshrc; "

        // Parse session from target
        let tmuxSession: String
        if let colonIdx = tmuxTarget.firstIndex(of: ":") {
            tmuxSession = String(tmuxTarget[..<colonIdx])
        } else {
            tmuxSession = tmuxTarget
        }

        cmd += "tmux set-option -t \(shellEscape(tmuxTarget)) status off 2>/dev/null; "
        cmd += "exec tmux new-session -t \(shellEscape(tmuxSession)) -s \(shellEscape(viewSession))"
        cmd += " \\; set-option destroy-unattached on"
        cmd += " \\; set-option status off"
        cmd += " \\; set-option mouse on"
        if let win = windowSpec {
            cmd += " \\; select-window -t :\(shellEscape(win))"
        }
        return cmd
    }
}
