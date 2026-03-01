import Foundation

/// Shell-escape a string by wrapping in single quotes and escaping embedded single quotes.
nonisolated func shellEscape(_ s: String) -> String {
    "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
}

/// Returns a shell init script that sources the appropriate profile files for the given shell.
nonisolated func shellProfileScript(for shellPath: String) -> String {
    let name = (shellPath as NSString).lastPathComponent
    switch name {
    case "zsh":
        return """
        if [ -x /usr/libexec/path_helper ]; then eval $(/usr/libexec/path_helper -s); fi; \
        [ -f ~/.zprofile ] && source ~/.zprofile; \
        [ -f ~/.zshrc ] && source ~/.zshrc;
        """
    case "bash":
        return """
        if [ -x /usr/libexec/path_helper ]; then eval $(/usr/libexec/path_helper -s); fi; \
        [ -f ~/.bash_profile ] && source ~/.bash_profile; \
        [ -f ~/.bashrc ] && source ~/.bashrc;
        """
    case "fish":
        return ""
    default:
        return ""
    }
}
