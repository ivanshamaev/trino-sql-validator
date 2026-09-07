from typing import TypeAlias

__version__: str

_Warning: TypeAlias = tuple[str, str, int | None, int | None]

def validate(
    sql: str,
    dialect: str = "trino",
) -> tuple[bool, int, str | None, int | None, int | None, tuple[_Warning, ...]]: ...

def validate_file(
    path: str,
    dialect: str = "trino",
) -> tuple[bool, int, str | None, int | None, int | None, tuple[_Warning, ...]]: ...