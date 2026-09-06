__version__: str

def validate(
    sql: str,
    dialect: str = "trino",
) -> tuple[bool, int, str | None, int | None, int | None]: ...

def validate_file(
    path: str,
    dialect: str = "trino",
) -> tuple[bool, int, str | None, int | None, int | None]: ...