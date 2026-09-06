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
    "ValidationResult",
    "__version__",
    "validate",
    "validate_file",
]

_NativeResult = tuple[bool, int, str | None, int | None, int | None]
"""Tuple shape returned by the Rust core: (valid, statement_count, error,
message, line, column)."""

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
class ValidationResult:
    """Structured outcome of validating one or more SQL statements."""

    valid: bool
    statement_count: int
    error: Error | None = None

    def __bool__(self) -> bool:
        return self.valid

    def __repr__(self) -> str:
        if self.valid:
            return f"<ValidationResult valid=True statements={self.statement_count}>"
        return f"<ValidationResult valid=False error={self.error!r}>"


_SUPPORTED_DIALECTS = ("trino", "hive", "generic")


def _validate(dialect: Dialect, call: _NativeResult) -> ValidationResult:
    if dialect.lower() not in _SUPPORTED_DIALECTS:
        raise ValueError(
            f"unknown dialect {dialect!r}; expected one of {_SUPPORTED_DIALECTS}"
        )
    valid, statement_count, message, line, column = call
    return ValidationResult(
        valid=bool(valid),
        statement_count=int(statement_count),
        error=Error(message=message, line=line, column=column) if message else None,
    )


def validate(sql: str, *, dialect: Dialect = "trino") -> ValidationResult:
    """Validate a SQL string containing one or more statements.

    Never raises for invalid SQL — errors are returned as a
    :class:`ValidationResult`. Raises :class:`ValueError` for an unknown
    dialect.
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