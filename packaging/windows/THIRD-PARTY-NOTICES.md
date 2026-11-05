# Windows preview notices

The shared Rust dependencies and their license texts are included in
THIRD-PARTY-NOTICES.md. That existing inventory conservatively also includes
macOS-only packages; those Apple bindings are not linked into this executable.

The Windows shell additionally uses Microsoft's windows-sys 0.61.2 and
windows-link 0.2.1 (MIT OR Apache-2.0). Their MIT license texts are in licenses/.
Windows system DLLs are dynamically linked and are not redistributed.

The GNU cross-build uses the MinGW-w64 runtime and Zig compiler support.
Their notices are included in licenses/. Rust standard library notices are also
included there. These notices do not grant rights to imported character art.
