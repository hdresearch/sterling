# Eagle Core Features Used in This Repository

> **Scope.** This document enumerates **Eagle-only** language/runtime features actually used by the scripts in this repository (e.g., `fileSystem.eagle`, `test.eagle`, `basic.eagle`). It is *not* a complete Eagle manual. When in doubt, prefer the upstream Eagle sources/docs for authoritative behavior.

Eagle is an implementation of Tcl for the .NET/CLR with additional commands, options, and datatypes. The items below are **not available in stock Tcl** or behave differently there. All examples reflect how this codebase uses them.

---

## Table of Contents

- [Language & Procedure System](#language--procedure-system)
  - [`nproc` — named-argument procedures](#nproc--named-argument-procedures)
- [Platform & Runtime Introspection](#platform--runtime-introspection)
  - [`hasRuntimeOption`](#hasruntimeoption)
  - [`isWindows`](#iswindows)
  - [`info binary`](#info-binary)
- [Extended Type Predicates](#extended-type-predicates)
- [Math & Randomness](#math--randomness)
- [Filesystem & Paths](#filesystem--paths)
  - [`file temppath`, `file tempname`](#file-temppath-file-tempname)
  - [`getShellExecutableName`](#getshellexecutablename)
- [URI & Encoding](#uri--encoding)
- [Hashing](#hashing)
- [Regular Expressions](#regular-expressions)
- [Glob Enhancements](#glob-enhancements)
- [Utility Helpers Shipped with Eagle](#utility-helpers-shipped-with-eagle)
- [Notes on Portability](#notes-on-portability)

---

## Language & Procedure System

### `nproc` — named-argument procedures

Define procs that accept **name/value pairs** in any order, with defaults per parameter.

```tcl
# Definition
nproc execCurlCommand {
  baseUri {apiKey ""} {method GET} {path ""} {data ""} {query ""}
} {
  # body...
}

# Call site (from this repo)
execCurlCommand \
  baseUri $baseUri apiKey $apiKey method $method path $path \
  data $data query $query
```

**Why Eagle-only?** Stock Tcl has only `proc` with positional parameters; there is no built-in “named parameters” facility.

**Tips**
- Defaults are provided via `{paramName defaultValue}` in the formal list.
- Callers may pass only the parameters they need.
- Argument names at the call site must **match the formal parameter names exactly** — the engine performs a literal dictionary lookup with no transformation. This codebase uses the dashless `name value` convention; the official Eagle documentation shows a `-name value` convention (with leading dashes). Both work as long as the call-site names match the declaration.

---

## Platform & Runtime Introspection

### `hasRuntimeOption`

Query process-wide options toggled via the Eagle shell/host. This repo uses:

- `cleanupDisabled`, `stopDisabled` — change teardown semantics.
- `auditExec` — enable `testExec -debug -trace`.
- `breakOnTestFailure` — drop into debugger on failure.
- `straceDaemon` — wrap daemon start with `strace`.

```tcl
if {[hasRuntimeOption straceDaemon]} { ... }
```

### `isWindows`

Predicate used for OS-specific behavior (e.g., owner/SID formats).

```tcl
if {[isWindows]} { set pattern $sidRegex } else { set pattern $userGroupRegex }
```

### `info binary`

Returns the path to the running **Eagle shell** executable. Used to locate side-by-side assemblies.

```tcl
set exeDir [file dirname [info binary]]
```

---

## Extended Type Predicates

Eagle extends `string is` with several predicates used throughout this repo:

```tcl
string is guid        -strict $value
string is inetaddr    -strict $ip
string is uri         -strict $uri
string is wideinteger -strict $n
string is file        -strict $path
string is directory   -strict $path
string is path        -strict $path
```

**Why Eagle-only?** These types (`guid`, `uri`, `inetaddr`, `file`, `directory`, `path`, `wideinteger`) are not provided by stock Tcl.

**Usage highlights**
- Input validation for IDs (`guid`) and network values (`inetaddr`).
- Strict path sanity checks before creating/mutating files.

---

## Math & Randomness

Eagle supports `random()` in `expr`, returning a cryptographically secure random 64-bit integer. The code formats random values as fixed-width hex:

```tcl
format %016lx [expr {random()}]
```

**Note:** Stock Tcl uses `rand()` (floating-point).

---

## Filesystem & Paths

### `file temppath`, `file tempname`

- `file temppath` → system temporary root.
- `file tempname` → unique temp path (file is not created by this call).

Used to carve out a *safe* test workspace (e.g., `/tmp/etd_<pid>`).

```tcl
set root [file temppath]
set tmp  [file tempname]
```

### `getShellExecutableName`

Returns the path to the hosting shell executable; in this repo it is used to build the path to native SQLite bits (e.g., `SQLite.Interop.dll`).

```tcl
set interop [file join [file dirname [getShellExecutableName]] SQLite.Interop.dll]
```

---

## URI & Encoding

`uri escape type string` URL-encodes a string; the `type` argument specifies the encoding context (e.g., `data`, `path`, `query`). This codebase passes `data` as the type:

```tcl
append result [uri escape data $name] = [uri escape data $value]
```

**Why Eagle-only?** Stock Tcl’s `uri::encode` (tcllib) has different APIs and is not core.

---

## Hashing

`hash normal` computes digests for strings, files, or in-memory objects. The official Eagle documentation lists `-encoding` and `-binary` as options; the `-filename` and `-object` flags below are also supported (confirmed by widespread use in the Eagle source tree).

```tcl
hash normal -filename -- sha1 $fileName
hash normal -object   -- sha1 $byteArrayHandle ;# see INTEROP for Byte[] handles
hash normal sha256 "Hello"                      ;# in-memory string
```

**Algorithms:** `md5`, `sha1`, `sha256`, `sha384`, `sha512`, and others (use `hash list` to enumerate).

**Why Eagle-only?** Tcllib provides hashing via different commands; `hash normal` is not part of Tcl core.

---

## Regular Expressions

`regexp` supports the Eagle-specific `-skip` switch, convenient with `-inline`/`-all` to return only capture groups:

```tcl
# Extract capture group 1 directly into 'result'
regexp -skip 1 -- {^Fetching kernel\s+(.*)$} $value result

# Return all capture groups across lines (omit full-match)
regexp -all -line -inline -skip 1 -- $pattern $text
```

**Why Eagle-only?** Stock Tcl lacks `-skip`.

---

## Glob Enhancements

`glob -types dotfiles` includes hidden dotfiles without specialized patterns:

```tcl
glob -nocomplain -types dotfiles -- [file join $dir $pattern]
```

**Why Eagle-only?** Tcl’s `glob` type filters differ; `dotfiles` is not a stock type name.

---

## Utility Helpers Shipped with Eagle

The repository relies on several small helpers that ship with Eagle distributions (core/toolkit), none of which are Tcl core commands:

- `appendArgs a b c ...` — concatenate arguments (no extra spaces) for messages/paths.
- `readFile path` / `writeFile path data` — whole-file I/O helpers.
- `findFilesRecursive pattern` — recursive discovery (used before bulk `chown`).
- `getTemporaryPath` — resolve a usable temporary directory from environment variables. (Placed in the test framework section of the official Eagle docs, but usable outside the harness.)

> These helpers are treated as *core utilities* here because they do not depend on the test harness. See **TEST_FEATURES.md** for harness-specific helpers.

---

## Notes on Portability

- Many of these features (e.g., extra `string is` types, `uri`, `hash normal`, `regexp -skip`) are **Eagle additions** intended to simplify common tasks in .NET‑hosted environments.
- Where this repository needs cross-platform behavior (e.g., SIDs vs `user:group`), it branches using `isWindows` and validates accordingly.
