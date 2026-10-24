#!/usr/bin/env python3
"""Run a command on a pseudo-terminal of a fixed size, for parity/tui.mjs.

    pty-run.py COLS ROWS COMMAND [ARG...]

Bytes on our stdin go to the terminal (keys); bytes the command writes to the
terminal come out on our stdout. Exits with the command's exit code, or 128 +
the signal that ended it. Stdlib only, so the harness needs no native module.
"""
import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios


def main():
    cols, rows, argv = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3:]
    pid, master = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        os.execv(argv[0], argv)

    stdin, stdout = sys.stdin.fileno(), sys.stdout.fileno()
    signal.signal(signal.SIGTERM, lambda *_: os.kill(pid, signal.SIGKILL))
    readers = [master, stdin]
    while master in readers:
        for fd in select.select(readers, [], [])[0]:
            try:
                data = os.read(fd, 65536)
            except OSError:  # EIO: the command closed the terminal
                data = b""
            if fd == master:
                if data:
                    os.write(stdout, data)
                else:
                    readers.remove(master)
            elif data:
                os.write(master, data)
            else:
                readers.remove(stdin)

    _, status = os.waitpid(pid, 0)
    sys.exit(os.waitstatus_to_exitcode(status) if not os.WIFSIGNALED(status) else 128 + os.WTERMSIG(status))


if __name__ == "__main__":
    main()
