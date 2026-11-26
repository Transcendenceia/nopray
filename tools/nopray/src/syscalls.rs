//! Linux syscall numbers per ABI (curated subset; extend when a program needs more).
//!
//! Sources: `arch/x86/entry/syscalls/syscall_64.tbl` and `include/uapi/asm-generic/unistd.h`
//! (used by arm64 and riscv). Names follow systemd's `SystemCallFilter=` spelling.

use crate::manifest::Isa;

const X86_64: &[(u64, &str)] = &[
    (0, "read"),
    (1, "write"),
    (2, "open"),
    (3, "close"),
    (4, "stat"),
    (5, "fstat"),
    (6, "lstat"),
    (7, "poll"),
    (8, "lseek"),
    (9, "mmap"),
    (10, "mprotect"),
    (11, "munmap"),
    (12, "brk"),
    (13, "rt_sigaction"),
    (14, "rt_sigprocmask"),
    (15, "rt_sigreturn"),
    (16, "ioctl"),
    (17, "pread64"),
    (18, "pwrite64"),
    (19, "readv"),
    (20, "writev"),
    (21, "access"),
    (22, "pipe"),
    (23, "select"),
    (24, "sched_yield"),
    (25, "mremap"),
    (26, "msync"),
    (28, "madvise"),
    (32, "dup"),
    (33, "dup2"),
    (34, "pause"),
    (35, "nanosleep"),
    (37, "alarm"),
    (38, "setitimer"),
    (39, "getpid"),
    (40, "sendfile"),
    (41, "socket"),
    (42, "connect"),
    (43, "accept"),
    (44, "sendto"),
    (45, "recvfrom"),
    (46, "sendmsg"),
    (47, "recvmsg"),
    (48, "shutdown"),
    (49, "bind"),
    (50, "listen"),
    (51, "getsockname"),
    (52, "getpeername"),
    (53, "socketpair"),
    (54, "setsockopt"),
    (55, "getsockopt"),
    (56, "clone"),
    (57, "fork"),
    (58, "vfork"),
    (59, "execve"),
    (60, "exit"),
    (61, "wait4"),
    (62, "kill"),
    (63, "uname"),
    (72, "fcntl"),
    (73, "flock"),
    (74, "fsync"),
    (75, "fdatasync"),
    (76, "truncate"),
    (77, "ftruncate"),
    (78, "getdents"),
    (79, "getcwd"),
    (80, "chdir"),
    (81, "fchdir"),
    (82, "rename"),
    (83, "mkdir"),
    (84, "rmdir"),
    (85, "creat"),
    (86, "link"),
    (87, "unlink"),
    (88, "symlink"),
    (89, "readlink"),
    (90, "chmod"),
    (91, "fchmod"),
    (92, "chown"),
    (95, "umask"),
    (96, "gettimeofday"),
    (97, "getrlimit"),
    (98, "getrusage"),
    (99, "sysinfo"),
    (102, "getuid"),
    (104, "getgid"),
    (105, "setuid"),
    (106, "setgid"),
    (107, "geteuid"),
    (108, "getegid"),
    (110, "getppid"),
    (131, "sigaltstack"),
    (157, "prctl"),
    (158, "arch_prctl"),
    (186, "gettid"),
    (200, "tkill"),
    (201, "time"),
    (202, "futex"),
    (217, "getdents64"),
    (218, "set_tid_address"),
    (228, "clock_gettime"),
    (229, "clock_getres"),
    (230, "clock_nanosleep"),
    (231, "exit_group"),
    (232, "epoll_wait"),
    (233, "epoll_ctl"),
    (234, "tgkill"),
    (254, "inotify_add_watch"),
    (255, "inotify_rm_watch"),
    (257, "openat"),
    (262, "newfstatat"),
    (270, "pselect6"),
    (271, "ppoll"),
    (281, "epoll_pwait"),
    (283, "timerfd_create"),
    (286, "timerfd_settime"),
    (288, "accept4"),
    (289, "signalfd4"),
    (290, "eventfd2"),
    (291, "epoll_create1"),
    (292, "dup3"),
    (293, "pipe2"),
    (294, "inotify_init1"),
    (302, "prlimit64"),
    (318, "getrandom"),
    (319, "memfd_create"),
    (332, "statx"),
];

const GENERIC: &[(u64, &str)] = &[
    (17, "getcwd"),
    (19, "eventfd2"),
    (20, "epoll_create1"),
    (21, "epoll_ctl"),
    (22, "epoll_pwait"),
    (23, "dup"),
    (24, "dup3"),
    (25, "fcntl"),
    (26, "inotify_init1"),
    (27, "inotify_add_watch"),
    (28, "inotify_rm_watch"),
    (29, "ioctl"),
    (32, "flock"),
    (33, "mknodat"),
    (34, "mkdirat"),
    (35, "unlinkat"),
    (37, "linkat"),
    (46, "ftruncate"),
    (48, "faccessat"),
    (49, "chdir"),
    (52, "fchmod"),
    (56, "openat"),
    (57, "close"),
    (59, "pipe2"),
    (61, "getdents64"),
    (62, "lseek"),
    (63, "read"),
    (64, "write"),
    (65, "readv"),
    (66, "writev"),
    (67, "pread64"),
    (68, "pwrite64"),
    (71, "sendfile"),
    (72, "pselect6"),
    (73, "ppoll"),
    (74, "signalfd4"),
    (78, "readlinkat"),
    (79, "newfstatat"),
    (80, "fstat"),
    (82, "fsync"),
    (85, "timerfd_create"),
    (86, "timerfd_settime"),
    (93, "exit"),
    (94, "exit_group"),
    (96, "set_tid_address"),
    (98, "futex"),
    (101, "nanosleep"),
    (113, "clock_gettime"),
    (115, "clock_nanosleep"),
    (124, "sched_yield"),
    (129, "kill"),
    (130, "tkill"),
    (131, "tgkill"),
    (132, "sigaltstack"),
    (134, "rt_sigaction"),
    (135, "rt_sigprocmask"),
    (139, "rt_sigreturn"),
    (160, "uname"),
    (163, "getrlimit"),
    (167, "prctl"),
    (169, "gettimeofday"),
    (172, "getpid"),
    (173, "getppid"),
    (174, "getuid"),
    (175, "geteuid"),
    (176, "getgid"),
    (177, "getegid"),
    (178, "gettid"),
    (198, "socket"),
    (199, "socketpair"),
    (200, "bind"),
    (201, "listen"),
    (202, "accept"),
    (203, "connect"),
    (204, "getsockname"),
    (205, "getpeername"),
    (206, "sendto"),
    (207, "recvfrom"),
    (208, "setsockopt"),
    (209, "getsockopt"),
    (210, "shutdown"),
    (211, "sendmsg"),
    (212, "recvmsg"),
    (214, "brk"),
    (215, "munmap"),
    (216, "mremap"),
    (220, "clone"),
    (221, "execve"),
    (222, "mmap"),
    (226, "mprotect"),
    (233, "madvise"),
    (242, "accept4"),
    (260, "wait4"),
    (261, "prlimit64"),
    (278, "getrandom"),
    (279, "memfd_create"),
    (291, "statx"),
];

fn table(isa: Isa) -> Option<&'static [(u64, &'static str)]> {
    match isa {
        Isa::X86_64 => Some(X86_64),
        Isa::Aarch64 | Isa::Riscv64 | Isa::Riscv32 => Some(GENERIC),
        Isa::Thumb | Isa::Xtensa | Isa::Avr => None,
    }
}

/// Syscall name for `nr` on the Linux ABI of `isa`.
#[must_use]
pub fn name(isa: Isa, nr: u64) -> Option<&'static str> {
    table(isa)?.iter().find(|(n, _)| *n == nr).map(|(_, s)| *s)
}

/// Syscall number for `name` on the Linux ABI of `isa`.
#[must_use]
pub fn number(isa: Isa, name: &str) -> Option<u64> {
    table(isa)?
        .iter()
        .find(|(_, s)| *s == name)
        .map(|(n, _)| *n)
}

/// Whether the syscall never returns.
#[must_use]
pub fn is_noreturn(name: &str) -> bool {
    matches!(name, "exit" | "exit_group" | "rt_sigreturn" | "execve")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_consistent() {
        for isa in [Isa::X86_64, Isa::Aarch64] {
            let t = table(isa).expect("linux table");
            for (n, s) in t {
                assert_eq!(number(isa, s), Some(*n), "{isa} {s}");
                assert_eq!(
                    t.iter().filter(|(m, _)| m == n).count(),
                    1,
                    "duplicate number {n} on {isa}"
                );
            }
        }
        assert_eq!(name(Isa::Aarch64, 64), Some("write"));
        assert_eq!(name(Isa::X86_64, 60), Some("exit"));
    }
}
