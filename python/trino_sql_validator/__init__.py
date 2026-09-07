"""trino_sql_validator — validate Trino SQL syntax.

The heavy lifting is done by the Rust core (``trino_sql_validator._native``);
this module wraps it in a small, typed public API.
"""

from __future__ import annotations

import os
from dataclasses import dataclass

from . import _native
from ._native import validate as _native_validate
from ._native import validate_file as _native_validate_file

__all__ = [
    "Error",
    "FunctionWarning",
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


def validate(sql: str, *, dialect: Dialect = "trino") -> ValidationResult:
    """Validate a SQL string containing one or more statements.

    Never raises for invalid SQL — errors are returned as a
    :class:`ValidationResult`. Raises :class:`ValueError` for an unknown
    dialect. For the ``trino`` dialect the result also carries advisory
    warnings for function calls and data types missing from the documented
    catalog; these never affect ``valid``.
    """
    return _validate(dialect, _native_validate(sql, dialect))


def validate_file(
    path: str | os.PathLike[str], *, dialect: Dialect = "trino"
) -> ValidationResult:
    """Validate a UTF-8 SQL file containing one or more statements.

    Raises :class:`ValueError` if the file cannot be read (missing file,
    decode failure) or the dialect is unknown. Invalid SQL is returned as a
    :class:`ValidationResult`.
    """
    return _validate(dialect, _native_validate_file(os.fspath(path), dialect))