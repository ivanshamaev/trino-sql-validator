# Зарезервированные SQL-слова Trino, недопустимые как алиасы

Проверено 24 сентября 2026 года. В Trino **83 reserved keywords** нельзя
использовать как алиас без двойных кавычек. Ограничение одинаково для алиаса
выражения и relation/table alias: соответствующие grammar rules принимают
`identifier`, а reserved token не преобразуется в identifier.

```text
ALTER
AND
AS
AUTO
BETWEEN
BY
CASE
CAST
CONSTRAINT
CREATE
CROSS
CUBE
CURRENT_CATALOG
CURRENT_DATE
CURRENT_PATH
CURRENT_ROLE
CURRENT_SCHEMA
CURRENT_TIME
CURRENT_TIMESTAMP
CURRENT_USER
DEALLOCATE
DELETE
DESCRIBE
DISTINCT
DROP
ELSE
END
ESCAPE
EXCEPT
EXISTS
EXTRACT
FALSE
FOR
FROM
FULL
GROUP
GROUPING
HAVING
IN
INNER
INSERT
INTERSECT
INTO
IS
JOIN
JSON_ARRAY
JSON_EXISTS
JSON_OBJECT
JSON_QUERY
JSON_TABLE
JSON_VALUE
LEFT
LIKE
LISTAGG
LOCALTIME
LOCALTIMESTAMP
NATURAL
NORMALIZE
NOT
NULL
ON
OR
ORDER
OUTER
OVERLAPS
PREPARE
RECURSIVE
RIGHT
ROLLUP
SELECT
SKIP
TABLE
THEN
TRIM
TRUE
UESCAPE
UNION
UNNEST
USING
VALUES
WHEN
WHERE
WITH
```

## Практическое правило

Reserved word допустим как алиас только как delimited identifier в двойных
кавычках:

```sql
-- Не разбирается как alias: GROUP и ORDER зарезервированы.
SELECT total AS group FROM sales;
SELECT * FROM orders AS order;

-- Корректно.
SELECT total AS "group" FROM sales;
SELECT * FROM orders AS "order";
```

Регистр не меняет статус слова. Одинарные кавычки создают строковый литерал,
а не identifier; backticks Trino не поддерживает для quoting identifiers.
Незарезервированные tokens вроде `ANALYZE`, `FILTER`, `LIMIT`, `PIVOT`,
`ROW`, `UPDATE` и `WINDOW` входят в grammar как keywords, но правило
`nonReserved` разрешает использовать их как unquoted aliases. Поэтому список
всех lexer keywords значительно шире списка выше.

## Как проверена полнота

Проверка выполнена по опубликованной документации Trino 483 и checkout
официального repository `trinodb/trino` на commit
[`77792a9b435d591939724a38d35bef3bbd64f47b`](https://github.com/trinodb/trino/commit/77792a9b435d591939724a38d35bef3bbd64f47b)
(`484-SNAPSHOT`). Repository-wide поиск охватил 15 417 tracked files.

1. [Текущая документация](https://trino.io/docs/current/language/reserved.html)
   нормативно перечисляет 83 reserved keywords и требует double quoting при
   использовании как identifiers.
2. [`identifier` и `nonReserved`](https://github.com/trinodb/trino/blob/77792a9b435d591939724a38d35bef3bbd64f47b/core/trino-grammar/src/main/antlr4/io/trino/grammar/sql/SqlBase.g4#L1086-L1138),
   [`selectItem`](https://github.com/trinodb/trino/blob/77792a9b435d591939724a38d35bef3bbd64f47b/core/trino-grammar/src/main/antlr4/io/trino/grammar/sql/SqlBase.g4#L371-L375)
   и [`aliasedRelation`](https://github.com/trinodb/trino/blob/77792a9b435d591939724a38d35bef3bbd64f47b/core/trino-grammar/src/main/antlr4/io/trino/grammar/sql/SqlBase.g4#L498-L503)
   показывают, что и explicit `AS`, и implicit alias используют один и тот же
   `identifier` boundary.
3. [`SqlKeywords`](https://github.com/trinodb/trino/blob/77792a9b435d591939724a38d35bef3bbd64f47b/core/trino-grammar/src/main/java/io/trino/grammar/sql/SqlKeywords.java#L25-L58)
   извлекает bare-word tokens из lexer vocabulary.
4. [`ReservedIdentifiers`](https://github.com/trinodb/trino/blob/77792a9b435d591939724a38d35bef3bbd64f47b/core/trino-parser/src/main/java/io/trino/sql/ReservedIdentifiers.java#L44-L132)
   оставляет tokens, которые parser не принимает как `Identifier`, и содержит
   проверку полного равенства этого множества таблице документации.
5. [`docs/pom.xml`](https://github.com/trinodb/trino/blob/77792a9b435d591939724a38d35bef3bbd64f47b/docs/pom.xml#L75-L99)
   запускает `validateDocs` в test phase. Файл списка в release tag `483` и в
   указанном `master` commit совпадает побайтно (SHA-256
   `dccbfbe195e91b0e20a2aa55cfb550aff0608a690726b7320ab675ce4ed33453`).

Независимый статический аудит grammar нашёл 309 именованных lexer rules с
одиночным uppercase literal и implicit literal `SKIP`, то есть 310 bare-word
keyword literals по форме, которую использует `SqlKeywords`. Правило
`nonReserved` содержит 230 token alternatives; `UTF8`, `UTF16` и `UTF32` не
совпадают с regex `[A-Z_]+`, поэтому сопоставимое множество содержит 227
keyword literals. Разность содержит 83 элемента и точно совпадает с
документацией. В итоговом списке 83 уникальные строки, отсортированные по
алфавиту.

Connector implementations не определяют отдельный синтаксис aliases: SQL
сначала проходит core parser. Отдельная JSON Path grammar не относится к SQL
aliases. Список version-sensitive; при обновлении Trino его нужно повторно
сверять с `ReservedIdentifiers` и `language/reserved.md` выбранной версии.
