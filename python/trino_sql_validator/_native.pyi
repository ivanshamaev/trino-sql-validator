from typing import Literal, TypeAlias

__version__: str

_WarningKind: TypeAlias = Literal["function", "function_arguments", "type", "alias"]
_Warning: TypeAlias = tuple[
    _WarningKind,
    str,
    int | None,
    int | None,
    int | None,
    list[int] | None,
]
_Validation: TypeAlias = tuple[bool, int, str | None, int | None, int | None, tuple[_Warning, ...]]
_StatementInfo: TypeAlias = tuple[int, int, int, int, int, str, str | None]

def validate(
    sql: str,
    dialect: str = "trino",
    function_arguments: str = "warn",
) -> _Validation: ...
def validate_file(
    path: str,
    dialect: str = "trino",
    function_arguments: str = "warn",
) -> _Validation: ...
def analyze_statements(
    sql: str,
    dialect: str = "trino",
    function_arguments: str = "warn",
) -> tuple[_Validation, tuple[_StatementInfo, ...], int | None]: ...
