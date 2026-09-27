SELECT last_z OVER (MEASURES missing_measure(z) AS last_z ROWS CURRENT ROW PATTERN (A) DEFINE A AS missing_define(z)) FROM t
