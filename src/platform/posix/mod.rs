//! Shared POSIX backends. The pty used by the host terminal is the same on
//! Linux and macOS; this tree builds the Linux copy.
pub(crate) mod terminal;
