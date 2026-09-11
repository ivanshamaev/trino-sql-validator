from typing import TypeAlias

__version__: str

_Warning: TypeAlias = tuple[str, str, int | None, int | None]
_Validation: TypeAlias = tuple[bool, int, str | None, int | None, int | None, tuple[_Warning, ...]]
_StatementInfo: TypeAlias = tuple[int, int, int, int, int, str, str | None]

def validate(
    sql: str,
    dialect: str = "trino",
) -> _Validation: ...
def validate_file(
    path: str,
    dialect: str = "trino",
) -> _Validation: ...
def analyze_statements(
    sql: str,
    dialect: str = "trino",
) -> tuple[_Validation, tuple[_StatementInfo, ...], int | None]: ...
