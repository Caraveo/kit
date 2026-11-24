---
title: x language reference
description: Every keyword, type, operator and built-in.
---

## Keywords

| Keyword | Meaning |
|---|---|
| `TAKE` / `BRING` | Use a module (`TAKE Transform`) or another file (`TAKE "lib/a.x"`) |
| `CREATE` | Declare a class (`CREATE Name[ … ]`) or function (`TYPE CREATE Name(params)[ … ]`) |
| `RIDE` | Inherit: `CREATE Child RIDE Parent[ … ]` |
| `NEW` | Make an instance: `NEW Name Class(args)` or `NEW Class(args)` |
| `CALL` | Optional before a call: `CALL Jump()` |
| `GIVE` | Return value(s) |
| `IF` / `ELSE` | Conditions |
| `LOOP` | While loop |
| `GRAB … IN` | For-each loop |
| `EXIT` / `SKIP` | Break / continue |
| `DISPLAY` | Print |
| `THIS` | The current instance |
| `TRUE` `FALSE` `NULL` | Literals |
| `AND` `OR` `NOT` | Logic (also `&&` `\|\|` `!`) |
| `EXIST` | `EXIST x` is `TRUE` when `x` is not `NULL` |

## Types

`STRING`, `INTEGER`, `FLOAT`, `BOOL`, `INDEX`, `OBJECT`, `VECTOR`, `ANY`, `VOID` (function results), or a class name.

## Operators

`+ - * / %`, `== != < > <= >=`. Integer division truncates. `+` joins strings. `VECTOR` supports `+ -` with vectors, `* /` with numbers, and `*` per component with another vector.

## Built-in functions

`RANGE(n)`, `RANGE(a, b)`, `LENGTH(x)`, `TYPEOF(x)`; conversions `INTEGER(x)`, `FLOAT(x)`, `STRING(x)`, `BOOL(x)`, `VECTOR(x, y, z)`.

## Built-in methods

| Type | Methods |
|---|---|
| `INDEX` | `Add`, `Insert`, `Remove`, `Pop`, `Clear`, `Contains`, `Find`, `Join`, `Reverse`, `Copy`, `Length` |
| `STRING` | `Length`, `Upper`, `Lower`, `Trim`, `Contains`, `StartsWith`, `Split`, `Replace` |
| `OBJECT` | `Keys`, `Has`, `Remove`, `Length` |
| `VECTOR` | `X`, `Y`, `Z`, `Length`, `Normalize`, `Dot`, `Cross`, `Distance` |

Negative list indexes count from the end: `Items[-1]`.

## Safety

A script that runs a very long time without returning (for example an endless `LOOP`) is stopped with an error instead of freezing the game. Deep recursion is limited the same way.
