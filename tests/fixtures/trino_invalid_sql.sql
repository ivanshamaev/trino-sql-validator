-- =====================================================================
-- Trino SQL: набор ЗАВЕДОМО НЕВАЛИДНЫХ statements (236 штук)
-- для тестирования обработки ошибок парсером.
-- Каждый блок — своя категория синтаксической ошибки, с комментарием
-- перед запросом, что именно сломано. Все запросы гарантированно
-- не проходят парсинг в Trino (SYNTAX_ERROR / ParsingException),
-- либо содержат конструкции, которых нет в грамматике Trino вовсе.
-- Скрипт НЕ предназначен для выполнения — только для парсера.
-- =====================================================================

-- ---------------------------------------------------------------------
-- 1. Лишние / пропущенные запятые
-- ---------------------------------------------------------------------

-- лишняя запятая перед FROM
SELECT orderkey, custkey, FROM orders;

-- лишняя запятая в конце списка колонок
SELECT orderkey, custkey, totalprice, FROM orders WHERE orderkey = 1;

-- две запятые подряд
SELECT orderkey,, custkey FROM orders;

-- запятая перед первой колонкой
SELECT , orderkey, custkey FROM orders;

-- пропущена запятая между колонками
SELECT orderkey custkey totalprice FROM orders;

-- пропущена запятая в списке значений VALUES
INSERT INTO orders VALUES (1 'O' 100.0);

-- запятая после последнего значения в VALUES
INSERT INTO orders VALUES (1, 'O', 100.0,);

-- запятая перед закрывающей скобкой в списке колонок CREATE TABLE
CREATE TABLE t1 (id integer, name varchar,);

-- пропущена запятая между определениями колонок
CREATE TABLE t2 (id integer name varchar);

-- лишняя запятая в GROUP BY
SELECT custkey, count(*) FROM orders GROUP BY custkey,;

-- лишняя запятая в ORDER BY
SELECT * FROM orders ORDER BY orderkey, totalprice,;

-- запятая вместо AND в WHERE
SELECT * FROM orders WHERE orderkey > 1, totalprice > 100;

-- лишняя запятая в списке аргументов функции
SELECT coalesce(clerk,, 'n/a') FROM orders;

-- запятая между таблицами без выражения между ними (двойная)
SELECT * FROM orders,, customers;

-- запятая перед WHERE
SELECT * FROM orders, WHERE orderkey = 1;

-- ---------------------------------------------------------------------
-- 2. Alias из двух слов без кавычек и другие ошибки алиасов
-- ---------------------------------------------------------------------

-- alias из двух слов без кавычек
SELECT orderkey AS order key FROM orders;

-- alias из двух слов без AS
SELECT orderkey order key FROM orders;

-- alias таблицы из двух слов без кавычек
SELECT o.orderkey FROM orders order table o;

-- alias с дефисом без кавычек
SELECT orderkey AS order-key FROM orders;

-- alias начинается с цифры без кавычек
SELECT orderkey AS 1order FROM orders;

-- alias — зарезервированное слово без кавычек
SELECT custkey AS select FROM orders;

-- alias через два AS подряд
SELECT orderkey AS AS ok FROM orders;

-- alias таблицы с пробелом и без кавычек в JOIN
SELECT * FROM orders o JOIN customers cust key ON o.custkey = cust key.custkey;

-- пустой alias в кавычках без содержимого перед FROM
SELECT orderkey AS "" FROM orders;

-- alias без разделителя сразу после выражения из двух идентификаторов
SELECT totalprice total price FROM orders;

-- ---------------------------------------------------------------------
-- 3. "Лишний statement" — два оператора слиплись / дублирование ключевых слов
-- ---------------------------------------------------------------------

-- второй SELECT сразу после первого без разделителя
SELECT * FROM orders SELECT;

-- SELECT приклеен к WHERE следующего запроса
SELECT * FROM orders WHERE orderkey = 1 SELECT * FROM customers;

-- два FROM подряд
SELECT * FROM orders FROM customers;

-- ORDER BY встречается дважды
SELECT * FROM orders ORDER BY orderkey ORDER BY custkey;

-- WHERE дублируется
SELECT * FROM orders WHERE orderkey = 1 WHERE custkey = 2;

-- CREATE TABLE приклеен к предыдущему без ';'
CREATE TABLE t3 (id integer) CREATE TABLE t4 (id integer);

-- INSERT приклеен к SELECT без ';'
SELECT * FROM orders INSERT INTO customers VALUES (1);

-- DROP TABLE без завершения и сразу следующий CREATE
DROP TABLE t1 CREATE TABLE t1 (id integer);

-- два ключевых слова LIMIT
SELECT * FROM orders LIMIT 10 LIMIT 20;

-- GROUP BY встречается дважды
SELECT custkey, count(*) FROM orders GROUP BY custkey GROUP BY orderstatus;

-- ---------------------------------------------------------------------
-- 4. Пропущенные / переставленные ключевые слова, неверный порядок клауз
-- ---------------------------------------------------------------------

-- WHERE без условия (FROM для SELECT в Trino необязателен)
SELECT orderkey, custkey WHERE;

-- WHERE перед FROM
SELECT * WHERE orderkey = 1 FROM orders;

-- GROUP BY перед WHERE
SELECT custkey, count(*) FROM orders GROUP BY custkey WHERE totalprice > 100;

-- HAVING без GROUP BY, но написан как GROUP
SELECT custkey FROM orders HAVING GROUP BY custkey;

-- ORDER BY перед WHERE
SELECT * FROM orders ORDER BY orderkey WHERE custkey = 1;

-- LIMIT перед ORDER BY
SELECT * FROM orders LIMIT 10 ORDER BY orderkey;

-- JOIN без ON и без USING
SELECT * FROM orders o JOIN customers c;

-- BY без GROUP
SELECT custkey, count(*) FROM orders BY custkey;

-- ORDER без BY
SELECT * FROM orders ORDER orderkey;

-- GROUP без BY
SELECT custkey FROM orders GROUP custkey;

-- FROM написан как FORM
SELECT * FORM orders;

-- SELECT написан как SELET
SELET * FROM orders;

-- дублирование BY: GROUP BY BY
SELECT custkey, count(*) FROM orders GROUP BY BY custkey;

-- дублирование BY: ORDER BY BY
SELECT * FROM orders ORDER BY BY orderkey;

-- AND в начале WHERE
SELECT * FROM orders WHERE AND orderkey = 1;

-- ---------------------------------------------------------------------
-- 5. Несбалансированные скобки
-- ---------------------------------------------------------------------

-- незакрытая скобка в списке колонок CREATE TABLE
CREATE TABLE t5 (id integer, name varchar;

-- лишняя закрывающая скобка
SELECT * FROM orders WHERE (orderkey = 1));

-- незакрытая скобка в подзапросе
SELECT * FROM (SELECT * FROM orders WHERE orderkey = 1;

-- незакрытая скобка в вызове функции
SELECT coalesce(clerk, 'n/a' FROM orders;

-- незакрытая скобка в VALUES
INSERT INTO orders VALUES (1, 'O', 100.0;

-- две лишние открывающие скобки без закрытия
SELECT * FROM orders WHERE ((orderkey = 1;

-- незакрытая скобка в CAST
SELECT CAST(totalprice AS decimal(10, 2 FROM orders;

-- незакрытая скобка в MERGE ON
MERGE INTO orders t USING orders_stage s ON (t.orderkey = s.orderkey WHEN MATCHED THEN DELETE;

-- незакрытая скобка после WITH (CTE)
WITH big AS (SELECT * FROM orders WHERE totalprice > 1000 SELECT * FROM big;

-- незакрытая скобка в оконной функции
SELECT row_number() OVER (PARTITION BY custkey ORDER BY orderdate FROM orders;

-- ---------------------------------------------------------------------
-- 6. Незакрытые строковые литералы / некорректные литералы
-- ---------------------------------------------------------------------

-- незакрытая одинарная кавычка
SELECT * FROM orders WHERE orderstatus = 'O;

-- незакрытая двойная кавычка у идентификатора
SELECT * FROM "orders WHERE orderkey = 1;

-- некорректный экранированный литерал юникода без хвоста
SELECT U&'\0041\004' FROM orders;

-- шестнадцатеричный литерал с нечётным числом символов
SELECT X'ABC' FROM orders;

-- незакрытый строковый literal с переносом строки
SELECT 'unterminated
string FROM orders;

-- ---------------------------------------------------------------------
-- 7. Обратные кавычки вместо двойных (MySQL-стиль, в Trino не поддерживается)
-- ---------------------------------------------------------------------

SELECT * FROM `orders`;

SELECT `orderkey`, `custkey` FROM orders;

SELECT * FROM orders `o`;

CREATE TABLE `t6` (`id` integer);

SELECT * FROM orders WHERE `orderkey` = 1;

-- ---------------------------------------------------------------------
-- 8. Идентификаторы, начинающиеся с цифры (без кавычек)
-- ---------------------------------------------------------------------

SELECT 1orderkey FROM orders;

CREATE TABLE t7 (1id integer);

SELECT * FROM orders WHERE 2custkey = 1;

CREATE TABLE 1t8 (id integer);

SELECT orderkey AS 1st_key FROM orders;

-- ---------------------------------------------------------------------
-- 9. Зарезервированные слова как неэкранированные идентификаторы
-- ---------------------------------------------------------------------

SELECT * FROM select;

CREATE TABLE where (id integer);

SELECT from FROM orders;

CREATE TABLE group (id integer, order integer);

SELECT table FROM table;

-- ---------------------------------------------------------------------
-- 10. Ошибки в CTE (WITH)
-- ---------------------------------------------------------------------

-- пропущено AS после имени CTE
WITH big (SELECT * FROM orders WHERE totalprice > 1000) SELECT * FROM big;

-- пропущено WITH перед именем CTE
big AS (SELECT * FROM orders) SELECT * FROM big;

-- лишняя запятая после последнего CTE перед SELECT
WITH a AS (SELECT 1), b AS (SELECT 2), SELECT * FROM a, b;

-- CTE без основного запроса
WITH big AS (SELECT * FROM orders WHERE totalprice > 1000);

-- RECURSIVE без CTE-тела
WITH RECURSIVE SELECT 1;

-- две CTE с одинаковым именем и без запятой между ними
WITH a AS (SELECT 1) a AS (SELECT 2) SELECT * FROM a;

-- пропущены скобки вокруг тела CTE
WITH big AS SELECT * FROM orders SELECT * FROM big;

-- список колонок CTE с незакрытой скобкой
WITH t (a, b AS (VALUES (1, 2)) SELECT * FROM t;

-- FUNCTION без RETURN
WITH FUNCTION double_it(x integer) RETURNS integer SELECT double_it(1);

-- FUNCTION без RETURNS
WITH FUNCTION double_it(x integer) RETURN x * 2 SELECT double_it(1);

-- ---------------------------------------------------------------------
-- 11. Ошибки JOIN
-- ---------------------------------------------------------------------

-- USING с пустым списком колонок
SELECT * FROM orders o JOIN customers c USING ();

-- ON и USING одновременно
SELECT * FROM orders o JOIN customers c ON o.custkey = c.custkey USING (custkey);

-- NATURAL JOIN с ON (несовместимо)
SELECT * FROM orders o NATURAL JOIN customers c ON o.custkey = c.custkey;

-- CROSS JOIN с ON (несовместимо, CROSS JOIN не принимает условие)
SELECT * FROM orders o CROSS JOIN customers c ON o.custkey = c.custkey;

-- JOIN без указания таблицы справа
SELECT * FROM orders o JOIN ON o.custkey = 1;

-- LEFT JOIN написан как JOIN LEFT
SELECT * FROM orders o JOIN LEFT customers c ON o.custkey = c.custkey;

-- пропущено слово JOIN
SELECT * FROM orders o LEFT customers c ON o.custkey = c.custkey;

-- USING с точечной нотацией колонки (недопустимо)
SELECT * FROM orders o JOIN customers c USING (o.custkey);

-- дублирование ON
SELECT * FROM orders o JOIN customers c ON o.custkey = c.custkey ON o.orderkey > 0;

-- LATERAL без круглых скобок вокруг подзапроса
SELECT * FROM customers c CROSS JOIN LATERAL SELECT orderkey FROM orders WHERE orders.custkey = c.custkey;

-- ---------------------------------------------------------------------
-- 12. Ошибки оконных функций
-- ---------------------------------------------------------------------

-- OVER с пустым ORDER BY
SELECT row_number() OVER (ORDER BY) FROM orders;

-- PARTITION без BY
SELECT sum(totalprice) OVER (PARTITION custkey) FROM orders;

-- ORDER без BY внутри OVER
SELECT rank() OVER (ORDER totalprice) FROM orders;

-- граница рамки окна без PRECEDING/FOLLOWING
SELECT sum(totalprice) OVER (ORDER BY orderdate ROWS 2) FROM orders;

-- BETWEEN без AND в рамке окна
SELECT sum(totalprice) OVER (ORDER BY orderdate ROWS BETWEEN 2 PRECEDING 1 FOLLOWING) FROM orders;

-- WINDOW-клауза без имени окна
SELECT sum(totalprice) OVER w FROM orders WINDOW AS (PARTITION BY custkey);

-- пропущена скобка после OVER
SELECT row_number() OVER PARTITION BY custkey) FROM orders;

-- FILTER до WITHIN GROUP у listagg (неверный порядок клауз)
SELECT listagg(orderstatus, ',') FILTER (WHERE totalprice > 0) WITHIN GROUP (ORDER BY orderkey) FROM orders;

-- FILTER без скобок вокруг условия
SELECT sum(totalprice) FILTER WHERE orderstatus = 'O' FROM orders;

-- RANGE с числовым смещением без UNBOUNDED/CURRENT ROW и без единиц времени в неверном контексте
SELECT sum(totalprice) OVER (ORDER BY orderdate RANGE BETWEEN 2 AND 5) FROM orders;

-- ---------------------------------------------------------------------
-- 13. Ошибки в CASE / условных выражениях
-- ---------------------------------------------------------------------

-- CASE без END
SELECT CASE WHEN totalprice > 100 THEN 'big' ELSE 'small' FROM orders;

-- CASE без WHEN
SELECT CASE totalprice THEN 'x' END FROM orders;

-- ELSE перед WHEN
SELECT CASE WHEN totalprice > 100 THEN 'big' ELSE 'small' WHEN totalprice < 0 THEN 'neg' END FROM orders;

-- два ELSE в одном CASE
SELECT CASE WHEN totalprice > 100 THEN 'big' ELSE 'small' ELSE 'other' END FROM orders;

-- THEN без значения
SELECT CASE WHEN totalprice > 100 THEN END FROM orders;

-- IF с одним аргументом
SELECT IF(totalprice > 100) FROM orders;

-- COALESCE без аргументов
SELECT coalesce() FROM orders;

-- NULLIF с тремя аргументами (функция принимает два)
SELECT nullif(orderstatus, 'O', 'F') FROM orders;

-- ---------------------------------------------------------------------
-- 14. Ошибки CREATE TABLE / DDL
-- ---------------------------------------------------------------------

-- CREATE TABLE без списка колонок и без AS SELECT
CREATE TABLE t9;

-- колонка без типа
CREATE TABLE t10 (id, name varchar);

-- WITH-свойства без скобок
CREATE TABLE t11 (id integer) WITH format = 'PARQUET';

-- COMMENT после WITH вместо перед ним
CREATE TABLE t12 (id integer) WITH (format = 'PARQUET') COMMENT 'table';

-- IF NOT EXISTS написан в обратном порядке
CREATE TABLE NOT IF EXISTS t13 (id integer);

-- дублирование ключевого слова TABLE
CREATE TABLE TABLE t14 (id integer);

-- CREATE без объекта (TABLE/VIEW/SCHEMA и т.д.)
CREATE t15 (id integer);

-- LIKE без имени таблицы
CREATE TABLE t16 (LIKE);

-- ALTER TABLE без действия (ADD/DROP/RENAME/SET)
ALTER TABLE orders;

-- ALTER TABLE ADD COLUMN без типа
ALTER TABLE orders ADD COLUMN discount;

-- RENAME COLUMN без TO
ALTER TABLE orders RENAME COLUMN clerk newclerk;

-- DROP TABLE без имени таблицы
DROP TABLE;

-- DROP без объекта
DROP orders;

-- CREATE SCHEMA без имени
CREATE SCHEMA;

-- ---------------------------------------------------------------------
-- 15. Ошибки INSERT / UPDATE / DELETE / MERGE
-- ---------------------------------------------------------------------

-- INSERT без INTO
INSERT orders VALUES (1, 2, 'O', 100.0, DATE '2024-01-01');

-- INSERT INTO без VALUES/SELECT
INSERT INTO orders;

-- пропущена запятая между scalar rows в VALUES
INSERT INTO orders VALUES 1 'O', 100.0;

-- UPDATE без SET
UPDATE orders WHERE orderkey = 1;

-- SET без значения после =
UPDATE orders SET totalprice = WHERE orderkey = 1;

-- SET с двумя '=' подряд
UPDATE orders SET totalprice == 100 WHERE orderkey = 1;

-- DELETE без FROM
DELETE orders WHERE orderkey = 1;

-- MERGE без USING
MERGE INTO orders t ON t.orderkey = 1 WHEN MATCHED THEN DELETE;

-- MERGE без ON
MERGE INTO orders t USING orders_stage s WHEN MATCHED THEN DELETE;

-- MERGE с WHEN без MATCHED/NOT MATCHED
MERGE INTO orders t USING orders_stage s ON t.orderkey = s.orderkey WHEN THEN DELETE;

-- MERGE INSERT без VALUES
MERGE INTO orders t USING orders_stage s ON t.orderkey = s.orderkey WHEN NOT MATCHED THEN INSERT (orderkey);

-- TRUNCATE без TABLE
TRUNCATE orders;

-- ---------------------------------------------------------------------
-- 16. Ошибки VIEW / MATERIALIZED VIEW / SCHEMA
-- ---------------------------------------------------------------------

-- CREATE VIEW без AS
CREATE VIEW v1 SELECT * FROM orders;

-- CREATE VIEW с пустым телом
CREATE VIEW v2 AS;

-- CREATE MATERIALIZED VIEW без ключевого слова MATERIALIZED
CREATE VIEW v3 REFRESH EVERY '1' HOUR AS SELECT * FROM orders;

-- REFRESH MATERIALIZED VIEW без имени
REFRESH MATERIALIZED VIEW;

-- ALTER VIEW без действия
ALTER VIEW v1;

-- DROP VIEW без имени
DROP VIEW;

-- COMMENT ON без объекта
COMMENT ON 'orders' IS 'comment';

-- COMMENT ON TABLE без IS
COMMENT ON TABLE orders 'a comment';

-- ---------------------------------------------------------------------
-- 17. Ошибки множественных операций (UNION/INTERSECT/EXCEPT), ORDER BY, LIMIT
-- ---------------------------------------------------------------------

-- UNION с некорректным модификатором
SELECT custkey FROM orders UNION DISTINCT ALL SELECT custkey FROM customers;

-- INTERSECT ALL (в Trino не поддерживается для некоторых версий, ошибка грамматики в части конструкций)
SELECT custkey FROM orders INTERSECT ALL ALL SELECT custkey FROM customers;

-- ORDER BY между двумя SELECT в UNION без скобок
SELECT custkey FROM orders ORDER BY custkey UNION SELECT custkey FROM customers;

-- EXCEPT без второго запроса
SELECT custkey FROM orders EXCEPT;

-- LIMIT с нечисловым значением без CAST
SELECT * FROM orders LIMIT 'ten';

-- LIMIT ALL написан как ALL LIMIT
SELECT * FROM orders ALL LIMIT;

-- OFFSET без ROWS и без числа
SELECT * FROM orders OFFSET ROWS;

-- FETCH без ROWS ONLY
SELECT * FROM orders FETCH FIRST 5;

-- FETCH NEXT без ROW/ROWS
SELECT * FROM orders FETCH NEXT 5 ONLY;

-- LIMIT и FETCH одновременно (несовместимые клаузы в одном запросе)
SELECT * FROM orders LIMIT 10 FETCH FIRST 5 ROWS ONLY;

-- ---------------------------------------------------------------------
-- 18. Ошибки JSON-функций и JSON_TABLE
-- ---------------------------------------------------------------------

-- JSON_VALUE без пути
SELECT JSON_VALUE(payload) FROM events;

-- JSON_TABLE без COLUMNS
SELECT * FROM JSON_TABLE('{"a":1}', 'lax $');

-- JSON_EXISTS с некорректным ключевым словом вместо RETURNING
SELECT JSON_EXISTS(payload, 'lax $.page' RETURN boolean) FROM events;

-- JSON_OBJECT с VALUE без KEY-конструкции и без имени
SELECT JSON_OBJECT(VALUE 1) FROM events;

-- json_extract без пути в кавычках
SELECT json_extract(payload, $.page) FROM events;

-- ---------------------------------------------------------------------
-- 19. Ошибки MATCH_RECOGNIZE
-- ---------------------------------------------------------------------

-- отсутствует PATTERN
SELECT * FROM orders MATCH_RECOGNIZE (PARTITION BY custkey ORDER BY orderdate DEFINE b AS totalprice < PREV(totalprice));

-- отсутствует DEFINE при использовании переменных в PATTERN
SELECT * FROM orders MATCH_RECOGNIZE (PARTITION BY custkey ORDER BY orderdate MEASURES a.totalprice AS p PATTERN (a b+));

-- MEASURES без AS у алиаса
SELECT * FROM orders MATCH_RECOGNIZE (ORDER BY orderdate MEASURES match_number() mn PATTERN (a) DEFINE a AS true);

-- quantifier в PATTERN с тремя границами
SELECT * FROM orders MATCH_RECOGNIZE (ORDER BY orderdate PATTERN (a{1,2,3}) DEFINE a AS true);

-- AFTER MATCH без SKIP
SELECT * FROM orders MATCH_RECOGNIZE (ORDER BY orderdate AFTER MATCH PATTERN (a) DEFINE a AS true);

-- ---------------------------------------------------------------------
-- 20. Ошибки PREPARE / EXECUTE / транзакций / сессии
-- ---------------------------------------------------------------------

-- PREPARE без FROM
PREPARE stmt1 SELECT * FROM orders WHERE orderkey = ?;

-- EXECUTE без имени подготовленного запроса
EXECUTE USING 1;

-- DEALLOCATE без PREPARE
DEALLOCATE stmt1;

-- START TRANSACTION с несуществующим уровнем изоляции по синтаксису (лишнее слово)
START TRANSACTION ISOLATION LEVEL VERY SERIALIZABLE;

-- COMMIT WORK с лишним словом
COMMIT TRANSACTION WORK;

-- SET SESSION без значения
SET SESSION iceberg.compression_codec;

-- SET SESSION без '='
SET SESSION iceberg.compression_codec 'ZSTD';

-- RESET SESSION с '='
RESET SESSION join_distribution_type = 'BROADCAST';

-- ---------------------------------------------------------------------
-- 21. Ошибки GRANT / REVOKE / ROLE
-- ---------------------------------------------------------------------

-- GRANT без ON
GRANT SELECT orders TO USER alice;

-- GRANT без TO
GRANT SELECT ON orders USER alice;

-- REVOKE без FROM
REVOKE SELECT ON orders USER alice;

-- CREATE ROLE без имени
CREATE ROLE;

-- SET ROLE без имени роли
SET ROLE IN iceberg;

-- ---------------------------------------------------------------------
-- 22. Двойной ';' и пустые операторы, оборванные операторы
-- ---------------------------------------------------------------------

SELECT * FROM orders;;

SELECT * FROM orders WHERE orderkey = 1;;;

;

SELECT * FROM orders WHERE orderkey = ;

SELECT * FROM orders WHERE orderkey = 1 AND;

SELECT * FROM orders WHERE orderkey = 1 AND ;

SELECT * FROM orders WHERE ;

SELECT * FROM orders WHERE orderkey =;

-- ---------------------------------------------------------------------
-- 23. Незакрытые комментарии и прочие лексические ошибки
-- ---------------------------------------------------------------------

-- запрос с недопустимым спецсимволом в выражении
SELECT * FROM orders WHERE @orderkey = 1;

-- запрос с символом '#' вне комментария
SELECT * FROM orders # WHERE orderkey = 1;

-- пустое выражение между операторами
SELECT * FROM orders WHERE orderkey = () ;

-- двойной оператор сравнения
SELECT * FROM orders WHERE orderkey == 1;

-- оператор в начале выражения без операнда
SELECT * FROM orders WHERE > 1;

-- ---------------------------------------------------------------------
-- 24. Прочие разнообразные ошибки
-- ---------------------------------------------------------------------

-- CAST без AS
SELECT CAST(totalprice decimal(10,2)) FROM orders;

-- TRY_CAST без типа
SELECT TRY_CAST(totalprice AS) FROM orders;

-- SUBSTRING с неверным разделителем вместо FOR
SELECT substring(name FROM 1 TO 3) FROM customers;

-- EXTRACT без FROM
SELECT EXTRACT(YEAR current_date) FROM orders;

-- INTERVAL с TO без начальной единицы
SELECT INTERVAL '3' TO DAY FROM orders;

-- ARRAY-литерал с фигурными скобками вместо квадратных
SELECT {1, 2, 3} FROM orders;

-- MAP без запятой между массивами ключей и значений
SELECT MAP(ARRAY['a'] ARRAY[1]) FROM orders;

-- ROW без скобок
SELECT ROW 1, 2 FROM orders;

-- TABLESAMPLE без метода (BERNOULLI/SYSTEM)
SELECT * FROM orders TABLESAMPLE (10);

-- UNNEST без аргументов
SELECT * FROM UNNEST() AS t (a);

-- WITH ORDINALITY без UNNEST
SELECT * FROM orders WITH ORDINALITY;

-- EXPLAIN с неизвестным TYPE
EXPLAIN (TYPE UNKNOWN) SELECT * FROM orders;

-- SHOW без объекта
SHOW;

-- DESCRIBE без имени таблицы
DESCRIBE;

-- ORDER BY с NULLS без FIRST/LAST
SELECT * FROM orders ORDER BY orderkey NULLS;

-- GROUPING SETS без скобок вокруг набора
SELECT orderstatus, count(*) FROM orders GROUP BY GROUPING SETS orderstatus;

-- ROLLUP без скобок
SELECT orderstatus, count(*) FROM orders GROUP BY ROLLUP orderstatus;

-- VALUES без строк
INSERT INTO orders VALUES;

-- WHEN без MATCHED/NOT в MERGE (повтор другой формы)
MERGE INTO orders t USING orders_stage s ON t.orderkey = s.orderkey WHEN MATCHED AND THEN DELETE;

-- пустой список колонок в CREATE TABLE
CREATE TABLE t17 ();

-- LIKE INCLUDING без указания что включать
CREATE TABLE t18 (LIKE orders INCLUDING);

-- некорректная точка в имени схемы (двойная точка)
SELECT * FROM iceberg..orders;

-- некорректное завершение идентификатора точкой
SELECT * FROM iceberg.parser_test. WHERE orderkey = 1;

-- незакрытый вызов функции count
SELECT count( FROM orders;

-- DISTINCT после колонок вместо перед ними
SELECT orderkey DISTINCT FROM orders;

-- ALL и DISTINCT одновременно в SELECT
SELECT ALL DISTINCT orderkey FROM orders;

-- SELECT * с точкой без таблицы
SELECT *. FROM orders;

-- LIMIT с отрицательным числом без поддержки (грамматически LIMIT ожидает целое без минуса в некоторых контекстах)
SELECT * FROM orders LIMIT -10;

-- составной оператор внутри идентификатора без экранирования
SELECT order-key FROM orders;

-- запрос с одним открывающим апострофом в идентификаторе колонки
SELECT orderkey, 'unclosed FROM orders;

-- некорректный вызов CURRENT_DATE со скобками (это не функция, а keyword)
SELECT CURRENT_DATE() FROM orders;

-- некорректный вызов CURRENT_TIMESTAMP с аргументом
SELECT CURRENT_TIMESTAMP(3, 4) FROM orders;

-- дублирование WITH (RECURSIVE) внутри одного CTE-блока
WITH RECURSIVE RECURSIVE t (n) AS (SELECT 1) SELECT * FROM t;

-- некорректный порядок в CREATE TABLE ... WITH ... COMMENT ... AS SELECT
CREATE TABLE t19 WITH (format = 'PARQUET') AS SELECT * FROM orders COMMENT 'wrong place';

-- CREATE TABLE AS с ORDER BY без скобок вокруг запроса (запрещённое место для ORDER BY без LIMIT в некоторых контекстах CTAS)
CREATE TABLE t20 AS SELECT * FROM orders ORDER;

-- DROP SCHEMA с CASCADE и RESTRICT одновременно
DROP SCHEMA parser_test CASCADE RESTRICT;

-- END-of-file: незакрытая скобка последней функции без завершения запроса
SELECT max(totalprice FROM orders;

-- незакрытый блочный комментарий — проглатывает всё до конца файла (намеренно последний кейс)
/* незакрытый блочный комментарий, после него ничего не должно парситься
SELECT * FROM orders WHERE orderkey = 1;
