# A quoted `[[agents]]` command line on Windows

An agent command that contains a double quote reaches the child mangled on Windows.
`term::shell_command` launches `cmd /C <command>`, and portable-pty escapes the argument by
`CommandLineToArgvW` rules, which `cmd.exe` does not parse.

## Not to be done

- Routing every platform through a POSIX shell: Windows does not have one as a given.

## Open

- Whether the fix is upstream (a raw-command-line constructor in portable-pty) or local
  (`cmd /S /C` with the argument pre-shaped for portable-pty's transform, which encodes another
  crate's escaping rules here).
- Whether `%COMSPEC%` or a per-entry shell in the config should name the launcher instead of a
  literal `cmd`.
