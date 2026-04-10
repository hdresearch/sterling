# Eagle Interop Features Used in This Repository

> **Scope.** This document covers **.NET/ADO.NET integration** features provided by Eagle and exercised by this codebase. It highlights invocation styles, object lifetimes/aliases, and database behaviors that are **not available in stock Tcl**.

---

## Table of Contents

- [.NET Interop via `object`](#net-interop-via-object)
  - [Constructors & Invocation](#constructors--invocation)
  - [Aliases & Lifetime](#aliases--lifetime)
  - [Non-public access & object flags](#non-public-access--object-flags)
  - [Working with Byte[]](#working-with-byte)
- [ADO.NET via `sql`](#adonet-via-sql)
  - [Opening & closing](#opening--closing)
  - [Executing queries](#executing-queries)
  - [Result shaping options](#result-shaping-options)
  - [SQLite extension loading](#sqlite-extension-loading)
- [Patterns from This Repository](#patterns-from-this-repository)

---

## .NET Interop via `object`

Eagle exposes the CLR directly via a single **`object`** command. This repo uses it to handle files/bytes, manipulate `Guid`, and call ADO.NET APIs on native connection objects.

### Constructors & Invocation

```tcl
# Invoke static/instance members
object invoke -create System.IO.File ReadAllBytes $path       ;# -> Byte[] alias
object invoke $byteArrayHandle Length                         ;# -> Int32
object invoke Utility ToHexadecimalString $byteArrayHandle

# Create a managed object (alias is returned)
set guidObj [object create -alias Guid $byteArrayHandle]
$guidObj ToString                                             ;# canonical string
```

### Aliases & Lifetime

- `-alias` registers the returned object with an **alias** string (e.g., `System#Byte[]#123`), which you pass to subsequent `object`/`sql` commands.
- `object dispose <alias>` releases the object.

### Non-public access & object flags

The repo occasionally reaches **non-public** members and disables automatic disposal for an object reference:

```tcl
object invoke \
  -flags +NonPublic \
  -objectflags +NoDispose \
  -objectname $connection \
  -alias \
  Interpreter.GetActive.connections Item $connection
```

- `-flags +NonPublic` — permit non-public reflection.
- `-objectflags +NoDispose` — do not auto-dispose the target.
- `-objectname` — refer to an **existing** object by alias. (The official Eagle docs list this option for `object create`, `library call`, and `debug exception`; its use with `object invoke` as shown above is also supported.)
- `-alias` — assign an alias to any new object returned.

### Working with Byte[]

Large binary values (BLOBs) are handled as **.NET `System.Byte[]`** and passed by their Eagle alias:

```tcl
# Read/write files using .NET Byte[] as buffer
set bytes  [object invoke -create System.IO.File ReadAllBytes $fileName]
object invoke System.IO.File WriteAllBytes $fileName $bytes
```

---

## ADO.NET via `sql`

Eagle maps ADO.NET onto the script surface via a **`sql`** command.

### Opening & closing

```tcl
set connection [sql open -type SQLite $connectionString]
# … use the connection …
sql close $connection
```

- `-type SQLite` selects the provider.

### Executing queries

```tcl
# Scalar result (single value)
set value [sql execute -execute scalar -- $connection {SELECT COUNT(*) FROM T;}]

# Reader result (tabular) with array formatting
sql execute \
  -execute reader \
  -format array \
  -datetimebehavior seconds \
  -blobbehavior object \
  -alias -- $connection $selectSql
```

### Result shaping options

This repo relies on the following options. The official Eagle documentation explicitly covers `-execute` and `-format nestedlist`; the remaining options below (`-format array`, `-datetimebehavior`, `-blobbehavior`, `-alias`) are additional `sql execute` capabilities used by this codebase.

- `-execute {reader|scalar}` — rowset vs single value.
- `-format array` — produces a keyed array layout (e.g., `rows(count)`, `rows(1)`, …) convenient for scripted data access.
- `-datetimebehavior seconds` — map date/times to Unix epoch seconds.
- `-blobbehavior object` — return BLOBs as **.NET Byte[]** (by alias), for use with `object`.
- `-alias` — treat `$connection` as an **object alias** (not a connection string).

### SQLite extension loading

This repo enables SQLite extensions and loads a custom function (e.g., regexp):

```tcl
# Acquire the underlying ADO.NET connection object
set connObj [getConnectionObject connection]  ;# project helper

# Enable & load extension module
$connObj EnableExtensions true
$connObj LoadExtension $sqliteInteropPath sqlite3_regexp_init
```

---

## Patterns from This Repository

- **BLOBs ↔ files.** Use `Byte[]` from `sql` (`-blobbehavior object`) and marshal to/from files using `System.IO.File` via `object invoke`.
- **Alias discipline.** Always pass/receive object **aliases** (strings). Dispose when done.
- **Connection bridging.** Use helper procs to bridge from a *script-level connection token* to the underlying **ADO.NET object** when needed for non-standard operations (e.g., extension loading).
