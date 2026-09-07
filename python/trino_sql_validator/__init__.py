"""trino_sql_validator — validate Trino SQL syntax.

The heavy lifting is done by the Rust core (``trino_sql_validator._native``);
this module wraps it in a small, typed public API.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Literal

from . import _native
from ._native import validate as _native_validate
from ._native import validate_file as _native_validate_file

__all__ = [
    "Error",
    "FunctionWarning",
    "JinjaMode",
    "TypeWarning",
    "ValidationResult",
    "__version__",
    "validate",
    "validate_file",
]

_NativeWarning = tuple[str, str, int | None, int | None]
"""A single analytical warning from the Rust core: (kind, name, line, column)
where kind is ``"function"`` or ``"type"``."""

_NativeResult = tuple[
    bool,
    int,
    str | None,
    int | None,
    int | None,
    tuple[_NativeWarning, ...],
]
"""Tuple shape returned by the Rust core: (valid, statement_count, error
message, line, column, warnings)."""

__version__ = _native.__version__

Dialect = str
JinjaMode = Literal["auto", "mask", "reject"]


def _mask_jinja(sql: str) -> str:
    """Mask Jinja tags while preserving SQL length and line breaks."""
    output = list(sql)
    position = 0
    quote: str | None = None
    tags = (("{{", "}}"), ("{%", "%}"), ("{#", "#}"))
    while position < len(sql):
        character = sql[position]
        if quote is not None:
            if character == quote:
                if position + 1 < len(sql) and sql[position + 1] == quote:
                    position += 2
                    continue
                quote = None
            elif character == "\\":
                position += 2
                continue
            position += 1
            continue
        if character in ("'", '"'):
            quote = character
            position += 1
            continue
        tag = next((tag for tag in tags if sql.startswith(tag[0], position)), None)
        if tag is None:
            position += 1
            continue
        start, end = tag
        close = sql.find(end, position + len(start))
        if close == -1:
            position += len(start)
            continue
        content = sql[position + len(start) : close]
        expression = start == "{{" and not content.lstrip("- ").startswith("config")
        for index in range(position, close + len(end)):
            if output[index] not in "\r\n":
                output[index] = " "
        if expression:
            output[position] = "j"
        position = close + len(end)
    return "".join(output)


def _prepare_sql(sql: str, jinja: JinjaMode) -> str:
    if jinja not in ("auto", "mask", "reject"):
        raise ValueError("unknown jinja mode; expected 'auto', 'mask', or 'reject'")
    if jinja == "reject":
        return sql
    return _mask_jinja(sql)


@dataclass(frozen=True)
class Error:
    """A single parse error found during validation."""

    message: str
    line: int | None = None
    column: int | None = None

    def __str__(self) -> str:
        if self.line is not None and self.column is not None:
            return f"{self.message} at line {self.line}, column {self.column}"
        return self.message


@dataclass(frozen=True)
class FunctionWarning:
    """A call to a function that is not in the documented Trino catalog.

    ``valid`` stays ``True`` for such statements — name checks are advisory,
    not syntax errors (a deployed Trino may still offer plugin functions that
    the docs do not list).
    """

    name: str
    line: int | None = None
    column: int | None = None

    def __str__(self) -> str:
        if self.line is not None and self.column is not None:
            return f"unknown function '{self.name}' at line {self.line}, column {self.column}"
        return f"unknown function '{self.name}'"


@dataclass(frozen=True)
class TypeWarning:
    """A use of a data type that is not in the documented Trino catalog.

    ``valid`` stays ``True`` for such statements — type checks are advisory,
    not syntax errors (a deployed Trino may still offer plugin types that the
    docs do not list).
    """

    name: str
    line: int | None = None
    column: int | None = None

    def __str__(self) -> str:
        if self.line is not None and self.column is not None:
            return f"unknown type '{self.name}' at line {self.line}, column {self.column}"
        return f"unknown type '{self.name}'"


@dataclass(frozen=True)
class ValidationResult:
    """Structured outcome of validating one or more SQL statements."""

    valid: bool
    statement_count: int
    error: Error | None = None
    warnings: tuple[FunctionWarning | TypeWarning, ...] = ()

    @property
    def unknown_functions(self) -> list[str]:
        """Function names used in the SQL that have no Trino documentation
        entry, in order of appearance."""
        return [warning.name for warning in self.warnings if isinstance(warning, FunctionWarning)]

    @property
    def unknown_types(self) -> list[str]:
        """Data types used in the SQL that have no Trino documentation entry,
        in order of appearance."""
        return [warning.name for warning in self.warnings if isinstance(warning, TypeWarning)]

    def __bool__(self) -> bool:
        return self.valid

    def __repr__(self) -> str:
        if not self.valid:
            return f"<ValidationResult valid=False error={self.error!r}>"
        if self.warnings:
            return (
                f"<ValidationResult valid=True statements={self.statement_count} "
                f"warnings={len(self.warnings)}>"
            )
        return f"<ValidationResult valid=True statements={self.statement_count}>"


_SUPPORTED_DIALECTS = ("trino", "hive", "generic")


def _validate(dialect: Dialect, call: _NativeResult) -> ValidationResult:
    if dialect.lower() not in _SUPPORTED_DIALECTS:
        raise ValueError(
            f"unknown dialect {dialect!r}; expected one of {_SUPPORTED_DIALECTS}"
        )
    valid, statement_count, message, line, column, warnings = call
    converted = []
    for kind, name, wl, wc in warnings:
        cls = TypeWarning if kind == "type" else FunctionWarning
        converted.append(cls(name=name, line=wl, column=wc))
    return ValidationResult(
        valid=bool(valid),
        statement_count=int(statement_count),
        error=Error(message=message, line=line, column=column) if message else None,
        warnings=tuple(converted),
    )


def validate(
    sql: str, *, dialect: Dialect = "trino", jinja: JinjaMode = "auto"
) -> ValidationResult:
    """Validate a SQL string containing one or more statements.

    Never raises for invalid SQL — errors are returned as a
    :class:`ValidationResult`. Raises :class:`ValueError` for an unknown
    dialect or Jinja mode. By default, Jinja/dbt tags are masked before
    parsing; use ``jinja="reject"`` to parse the original template strictly.
    For the ``trino`` dialect the result also carries advisory warnings for
    function calls and data types missing from the documented catalog; these
    never affect ``valid``.
    """
    return _validate(dialect, _native_validate(_prepare_sql(sql, jinja), dialect))


def validate_file(
    path: str | os.PathLike[str], *, dialect: Dialect = "trino", jinja: JinjaMode = "auto"
) -> ValidationResult:
    """Validate a UTF-8 SQL file containing one or more statements.

    Raises :class:`ValueError` if the file cannot be read (missing file,
    decode failure), the dialect is unknown, or the Jinja mode is invalid.
    Invalid SQL is returned as a :class:`ValidationResult`.
    """
    _prepare_sql("", jinja)
    if jinja == "reject":
        return _validate(dialect, _native_validate_file(os.fspath(path), dialect))
    try:
        with open(path, encoding="utf-8") as sql_file:
            return validate(sql_file.read(), dialect=dialect, jinja=jinja)
    except (OSError, UnicodeError) as error:
        raise ValueError(f"failed to read SQL file {path!r}: {error}") from error