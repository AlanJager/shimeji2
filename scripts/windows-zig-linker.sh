#!/bin/sh
set -eu
# Zig 0.15 calls the legacy msvcrt import library msvcrt-os.
for arg do
    shift
    case "$arg" in
        -lmsvcrt) set -- "$@" -lmsvcrt-os ;;
        -l:libpthread.a) set -- "$@" -lpthread ;;
        *) set -- "$@" "$arg" ;;
    esac
done
exec zig cc -target x86_64-windows-gnu "$@"
